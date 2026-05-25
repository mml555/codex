use std::sync::Arc;

use codex_context_harness::BuildPacketOptions;
use codex_context_harness::ContextPacket;
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
use codex_verification::PlanRequest;
use codex_verification::PlanScope;
use codex_verification::VerificationPlanner;
use codex_verification::is_safe_to_run;

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

/// Env var the extension reads to load a pre-built `RepoMap` from
/// disk instead of indexing the worktree at session start. Set by
/// the eval runner (`scripts/harness-agent-eval.sh`) to a temp JSON
/// path. The file is `serde_json::to_string(&RepoMap)`; failures to
/// open or deserialize are logged and the extension falls back to
/// building the index from `config.cwd` as if the env var were unset.
///
/// The original cost we're amortizing: in two consecutive isolated-
/// worktree pairs of `convention_add_area_package_alias`, the RI arm
/// spent 170+ seconds in `RepoMapBuilder::build(...)` walking the
/// codex-rs tree before the model produced its first token. Vanilla
/// skipped this entirely. Caching the map across arms in a batch
/// erases that gap.
pub const CACHED_MAP_ENV_VAR: &str = "CODEX_REPO_INTELLIGENCE_CACHED_MAP";

impl RepoIntelligenceExtensionConfig {
    fn from_config(config: &Config) -> Self {
        Self {
            enabled: config.features.enabled(Feature::RepoIntelligence),
            cwd: config.cwd.clone(),
            cached_map: load_cached_map_from_env(),
            task_override: None,
        }
    }
}

/// Read `CODEX_REPO_INTELLIGENCE_CACHED_MAP` and attempt to load the
/// referenced JSON file as a `RepoMap`. Returns `None` on any error
/// (missing var, missing file, invalid JSON) — the extension will
/// fall back to building the index in-process. Logs at `warn!` so
/// reviewers can see why the cache didn't help.
fn load_cached_map_from_env() -> Option<RepoMap> {
    let path = std::env::var(CACHED_MAP_ENV_VAR).ok()?;
    if path.is_empty() {
        tracing::info!("repo intelligence cached_map: env var empty, will index in-arm");
        return None;
    }
    let read_start = std::time::Instant::now();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!(
                "repo intelligence cached_map: failed to read {path}: {err}"
            );
            return None;
        }
    };
    let read_ms = read_start.elapsed().as_millis();
    let parse_start = std::time::Instant::now();
    match serde_json::from_slice::<RepoMap>(&bytes) {
        Ok(map) => {
            let parse_ms = parse_start.elapsed().as_millis();
            tracing::info!(
                "repo intelligence cached_map: loaded {path} ({} files, read={}ms, parse={}ms)",
                map.files.len(),
                read_ms,
                parse_ms
            );
            Some(map)
        }
        Err(err) => {
            tracing::warn!(
                "repo intelligence cached_map: failed to deserialize {path}: {err}"
            );
            None
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
            let contribute_start = std::time::Instant::now();
            let Some(config) = thread_store.get::<RepoIntelligenceExtensionConfig>() else {
                return Vec::new();
            };
            if !config.enabled {
                return Vec::new();
            }

            let map_start = std::time::Instant::now();
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
            let map_ms = map_start.elapsed().as_millis();

            let run_memory = thread_store
                .get::<RunMemoryBridge>()
                .map(|bridge| bridge.memory.clone())
                .unwrap_or_default();

            let task = config
                .task_override
                .as_deref()
                .filter(|text| !text.is_empty())
                .unwrap_or("continue current task");
            let packet_start = std::time::Instant::now();
            let packet =
                build_context_packet(task, &map, &run_memory, BuildPacketOptions::default());
            let packet_ms = packet_start.elapsed().as_millis();
            let render_start = std::time::Instant::now();
            let mut text = ContextPacketRenderer::render_prompt_fragment(&packet);
            let render_ms = render_start.elapsed().as_millis();
            let hint_start = std::time::Instant::now();
            if let Some(hint) = narrow_verification_hint(task, &map, &packet) {
                text.push_str("\n\n");
                text.push_str(&hint);
            }
            let hint_ms = hint_start.elapsed().as_millis();
            let total_ms = contribute_start.elapsed().as_millis();
            tracing::info!(
                "repo intelligence contribute(): total={total_ms}ms (map={map_ms}ms, packet={packet_ms}ms, render={render_ms}ms, verification_hint={hint_ms}ms)"
            );
            vec![PromptFragment::new(PromptSlot::ContextualUser, text)]
        })
    }
}

/// Append a single-line directive that names the deterministic narrow test
/// command the planner would run for this task. Returns `None` if the planner
/// emits no narrow command that passes `is_safe_to_run`.
///
/// The hint is rendered as a post-edit checklist line ("after editing"), not a
/// pre-edit imperative — the main behavioral lift being tested is file
/// routing before first edit, not test-first execution.
pub fn narrow_verification_hint(
    task: &str,
    map: &RepoMap,
    packet: &ContextPacket,
) -> Option<String> {
    // Treat the inspect list as the prospective change set so the planner has
    // enough signal to emit area-specific commands before any files have been
    // edited.
    let prospective: Vec<String> = packet
        .included_paths()
        .into_iter()
        .map(str::to_string)
        .collect();
    let plan = VerificationPlanner::plan_with_context(
        map,
        &PlanRequest {
            task: Some(task.to_string()),
            changed_paths: prospective,
        },
        packet,
    );
    let command = plan
        .commands
        .iter()
        .filter(|cmd| cmd.scope == PlanScope::Narrow)
        .filter(|cmd| is_safe_to_run(&cmd.command))
        .max_by(|a, b| {
            a.confidence
                .partial_cmp(&b.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
    Some(format!(
        "After editing, likely narrow verification:\n- {}",
        command.command
    ))
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
