use std::sync::Arc;

use codex_exec_server::EnvironmentManager;
use codex_exec_server::ExecServerRuntimePaths;
use codex_login::AuthManager;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SessionSource;
use codex_protocol::user_input::UserInput;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::resolve_installation_id;
use crate::session::session::Session;
use crate::session::turn::build_prompt;
use crate::session::turn::built_tools;
use crate::state_db_bridge::StateDbHandle;
use crate::thread_manager::ThreadManager;
use crate::thread_manager::thread_store_from_config;
use codex_extension_api::ExtensionRegistry;
use codex_extension_api::empty_extension_registry;

/// Build the model-visible `input` list for a single debug turn.
#[doc(hidden)]
pub async fn build_prompt_input(
    config: Config,
    input: Vec<UserInput>,
    state_db: Option<StateDbHandle>,
) -> CodexResult<Vec<ResponseItem>> {
    build_prompt_input_with_extensions(config, input, state_db, empty_extension_registry()).await
}

/// Build the model-visible `input` list for a single debug turn with extensions.
#[doc(hidden)]
pub async fn build_prompt_input_with_extensions(
    mut config: Config,
    input: Vec<UserInput>,
    state_db: Option<StateDbHandle>,
    extensions: Arc<ExtensionRegistry<Config>>,
) -> CodexResult<Vec<ResponseItem>> {
    config.ephemeral = true;

    let local_runtime_paths = ExecServerRuntimePaths::from_optional_paths(
        config.codex_self_exe.clone(),
        config.codex_linux_sandbox_exe.clone(),
    )?;
    let environment_manager = Arc::new(
        EnvironmentManager::from_codex_home(config.codex_home.clone(), Some(local_runtime_paths))
            .await
            .map_err(|err| CodexErr::Fatal(err.to_string()))?,
    );

    build_prompt_input_with_environment_manager(
        config,
        input,
        state_db,
        environment_manager,
        extensions,
    )
    .await
}

/// Build prompt input without reading configured execution environments or API key env vars.
#[doc(hidden)]
pub async fn build_prompt_input_self_contained(
    mut config: Config,
    input: Vec<UserInput>,
    state_db: Option<StateDbHandle>,
) -> CodexResult<Vec<ResponseItem>> {
    config.ephemeral = true;
    let environment_manager = Arc::new(EnvironmentManager::default_for_tests());

    build_prompt_input_with_environment_manager(
        config,
        input,
        state_db,
        environment_manager,
        empty_extension_registry(),
    )
    .await
}

async fn build_prompt_input_with_environment_manager(
    config: Config,
    input: Vec<UserInput>,
    state_db: Option<StateDbHandle>,
    environment_manager: Arc<EnvironmentManager>,
    extensions: Arc<ExtensionRegistry<Config>>,
) -> CodexResult<Vec<ResponseItem>> {
    let auth_manager =
        AuthManager::shared_from_config(&config, /*enable_codex_api_key_env*/ false).await;

    let thread_store = thread_store_from_config(&config, state_db.clone());
    let installation_id = resolve_installation_id(&config.codex_home).await?;
    let thread_manager = ThreadManager::new(
        &config,
        Arc::clone(&auth_manager),
        SessionSource::Exec,
        environment_manager,
        extensions,
        /*analytics_events_client*/ None,
        thread_store,
        state_db.clone(),
        installation_id,
        /*attestation_provider*/ None,
    );
    let thread = thread_manager.start_thread(config).await?;

    let output = build_prompt_input_from_session(thread.thread.codex.session.as_ref(), input).await;
    let shutdown = thread.thread.shutdown_and_wait().await;
    let _removed = thread_manager.remove_thread(&thread.thread_id).await;

    shutdown?;
    output
}

pub(crate) async fn build_prompt_input_from_session(
    sess: &Session,
    input: Vec<UserInput>,
) -> CodexResult<Vec<ResponseItem>> {
    let turn_context = sess.new_default_turn().await;
    sess.services
        .extensions
        .prepare_turn_input(&sess.services.thread_extension_data, &input);
    sess.record_context_updates_and_set_reference_context_item(turn_context.as_ref())
        .await;

    if !input.is_empty() {
        let input_item = ResponseInputItem::from(input);
        let response_item = ResponseItem::from(input_item);
        sess.record_conversation_items(turn_context.as_ref(), std::slice::from_ref(&response_item))
            .await;
    }

    let prompt_input = sess
        .clone_history()
        .await
        .for_prompt(&turn_context.model_info.input_modalities);
    let router = built_tools(sess, turn_context.as_ref(), &CancellationToken::new()).await?;
    let base_instructions = sess.get_base_instructions().await;
    let prompt = build_prompt(
        prompt_input,
        router.as_ref(),
        turn_context.as_ref(),
        base_instructions,
    );

    Ok(prompt.get_formatted_input())
}
