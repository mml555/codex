use std::sync::Arc;

use codex_context_harness::BuildPacketOptions;
use codex_context_harness::ContextPacketRenderer;
use codex_context_harness::build_context_packet;
use codex_core::config::Config;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ContextContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::PromptFragment;
use codex_extension_api::PromptSlot;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::TurnInputContributor;
use codex_features::Feature;
use codex_protocol::user_input::UserInput;
use codex_repo_index::RepoMap;
use codex_repo_index::RepoMapBuilder;
use codex_utils_absolute_path::AbsolutePathBuf;

use crate::run_memory_bridge::RunMemoryBridge;
use crate::user_input::task_text_from_user_input;

#[derive(Clone, Copy, Debug, Default)]
pub struct RepoIntelligenceExtension;

#[derive(Clone, Debug)]
pub struct RepoIntelligenceExtensionConfig {
    pub enabled: bool,
    pub cwd: AbsolutePathBuf,
    pub cached_map: Option<RepoMap>,
    /// When set (e.g. by `codex context smoke`), overrides the default task string.
    pub task_override: Option<String>,
}

impl RepoIntelligenceExtensionConfig {
    fn from_config(config: &Config) -> Self {
        Self {
            enabled: config.features.enabled(Feature::RepoIntelligence),
            cwd: config.cwd.clone(),
            cached_map: None,
            task_override: None,
        }
    }
}

impl ContextContributor for RepoIntelligenceExtension {
    fn contribute<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<PromptFragment>> + Send + 'a>> {
        Box::pin(async move {
            let Some(config) = thread_store.get::<RepoIntelligenceExtensionConfig>() else {
                return Vec::new();
            };
            if !config.enabled {
                return Vec::new();
            }

            let map = match &config.cached_map {
                Some(map) => map.clone(),
                None => match RepoMapBuilder::build(config.cwd.as_path()) {
                    Ok(map) => map,
                    Err(err) => {
                        tracing::warn!("repo intelligence map build failed: {err}");
                        return Vec::new();
                    }
                },
            };

            let run_memory = thread_store
                .get::<RunMemoryBridge>()
                .map(|bridge| bridge.memory.clone())
                .unwrap_or_default();

            let task = config
                .task_override
                .as_deref()
                .filter(|text| !text.is_empty())
                .unwrap_or("continue current task");
            let packet =
                build_context_packet(task, &map, &run_memory, BuildPacketOptions::default());
            let text = ContextPacketRenderer::render_prompt_fragment(&packet);
            vec![PromptFragment::new(PromptSlot::ContextualUser, text)]
        })
    }
}

#[async_trait::async_trait]
impl ThreadLifecycleContributor<Config> for RepoIntelligenceExtension {
    async fn on_thread_start(&self, input: ThreadStartInput<'_, Config>) {
        input
            .thread_store
            .insert(RepoIntelligenceExtensionConfig::from_config(input.config));
        input.thread_store.insert(RunMemoryBridge::default());
    }
}

impl TurnInputContributor for RepoIntelligenceExtension {
    fn prepare_turn_input(&self, thread_store: &ExtensionData, input: &[UserInput]) {
        let Some(task) = task_text_from_user_input(input) else {
            return;
        };
        let Some(config) = thread_store.get::<RepoIntelligenceExtensionConfig>() else {
            return;
        };
        if !config.enabled {
            return;
        }
        let mut updated = (*config).clone();
        updated.task_override = Some(task);
        thread_store.insert(updated);
    }
}

impl ConfigContributor<Config> for RepoIntelligenceExtension {
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &Config,
        new_config: &Config,
    ) {
        thread_store.insert(RepoIntelligenceExtensionConfig::from_config(new_config));
    }
}

pub fn install(registry: &mut ExtensionRegistryBuilder<Config>) {
    let extension = Arc::new(RepoIntelligenceExtension);
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.turn_input_contributor(extension.clone());
    registry.prompt_contributor(extension);
}
