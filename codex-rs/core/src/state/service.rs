use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use crate::SkillsManager;
use crate::agent::AgentControl;
use crate::attestation::AttestationProvider;
use crate::client::ModelClient;
use crate::config::NetworkProxyAuditMetadata;
use crate::config::StartedNetworkProxy;
use crate::exec_policy::ExecPolicyManager;
use crate::guardian::GuardianRejection;
use crate::guardian::GuardianRejectionCircuitBreaker;
use crate::mcp::McpManager;
use crate::tools::code_mode::CodeModeService;
use crate::tools::network_approval::NetworkApprovalService;
use crate::tools::sandboxing::ApprovalStore;
use crate::unified_exec::UnifiedExecProcessManager;
use arc_swap::ArcSwap;
use arc_swap::ArcSwapOption;
use codex_analytics::AnalyticsEventsClient;
use codex_core_plugins::PluginsManager;
use codex_exec_server::EnvironmentManager;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistry;
use codex_hooks::Hooks;
use codex_login::AuthManager;
use codex_mcp::McpConnectionManager;
use codex_models_manager::manager::SharedModelsManager;
use codex_otel::SessionTelemetry;
use codex_rollout::state_db::StateDbHandle;
use codex_rollout_trace::ThreadTraceContext;
use codex_thread_store::LiveThread;
use codex_thread_store::ThreadStore;
use std::path::PathBuf;
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub(crate) struct SessionServices {
    pub(crate) mcp_connection_manager: Arc<RwLock<McpConnectionManager>>,
    pub(crate) mcp_startup_cancellation_token: Mutex<CancellationToken>,
    pub(crate) unified_exec_manager: UnifiedExecProcessManager,
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) shell_zsh_path: Option<PathBuf>,
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) main_execve_wrapper_exe: Option<PathBuf>,
    pub(crate) analytics_events_client: AnalyticsEventsClient,
    pub(crate) hooks: ArcSwap<Hooks>,
    pub(crate) rollout_thread_trace: ThreadTraceContext,
    pub(crate) user_shell: Arc<crate::shell::Shell>,
    pub(crate) shell_snapshot_tx: watch::Sender<Option<Arc<crate::shell_snapshot::ShellSnapshot>>>,
    pub(crate) show_raw_agent_reasoning: bool,
    pub(crate) exec_policy: Arc<ExecPolicyManager>,
    pub(crate) auth_manager: Arc<AuthManager>,
    pub(crate) models_manager: SharedModelsManager,
    pub(crate) session_telemetry: SessionTelemetry,
    pub(crate) tool_approvals: Mutex<ApprovalStore>,
    pub(crate) guardian_rejections: Mutex<HashMap<String, GuardianRejection>>,
    pub(crate) guardian_rejection_circuit_breaker: Mutex<GuardianRejectionCircuitBreaker>,
    pub(crate) runtime_handle: Handle,
    pub(crate) skills_manager: Arc<SkillsManager>,
    pub(crate) plugins_manager: Arc<PluginsManager>,
    pub(crate) mcp_manager: Arc<McpManager>,
    pub(crate) extensions: Arc<ExtensionRegistry<crate::config::Config>>,
    pub(crate) session_extension_data: ExtensionData,
    pub(crate) thread_extension_data: ExtensionData,
    pub(crate) agent_control: AgentControl,
    pub(crate) network_proxy: ArcSwapOption<StartedNetworkProxy>,
    pub(crate) network_proxy_audit_metadata: NetworkProxyAuditMetadata,
    pub(crate) managed_network_requirements_configured: bool,
    pub(crate) network_approval: Arc<NetworkApprovalService>,
    pub(crate) state_db: Option<StateDbHandle>,
    pub(crate) live_thread: Option<LiveThread>,
    pub(crate) thread_store: Arc<dyn ThreadStore>,
    pub(crate) attestation_provider: Option<Arc<dyn AttestationProvider>>,
    /// Session-scoped model client shared across turns.
    pub(crate) model_client: ModelClient,
    pub(crate) code_mode_service: CodeModeService,
    /// Shared process-level environment registry. Sessions carry an `Arc` handle so they can pass
    /// the same manager through child-thread spawn paths without reconstructing it.
    pub(crate) environment_manager: Arc<EnvironmentManager>,

    /// Set of `ClassifiedRg::normalized` strings the search-proxy
    /// hook has already substituted in this session. Used as the
    /// repeat-command escape hatch: if the model re-sends the same
    /// rg invocation, the second call is passed through to raw rg.
    /// Only populated when `Feature::SearchProxy` is enabled.
    pub(crate) search_proxy_intercepts: Mutex<HashSet<String>>,

    /// Set of `ClassifiedRead::normalized` strings the large-read-proxy
    /// hook has already substituted in this session. Repeat-command escape
    /// hatch: a re-sent `cat`/`sed` read is passed through to raw output.
    /// Only populated when `Feature::LargeReadProxy` is enabled.
    pub(crate) large_read_proxy_intercepts: Mutex<HashSet<String>>,

    /// Per-session running counters for the reactive-mediation proxies.
    /// Updated by [`crate::tools::handlers::search_proxy::intercept_search_proxy`]
    /// and [`crate::tools::handlers::large_read_proxy::intercept_large_read_proxy`]
    /// each time they fire; consumed at session end to emit a human-readable
    /// telemetry summary ("did proxy fire? what did it save?" — Track C1).
    pub(crate) proxy_telemetry: Mutex<ProxyTelemetry>,
}

/// Telemetry accumulator for the search-proxy and large-read-proxy.
/// Mirrors the tracing event fields (`event = "substitute" | ...`) so the
/// session-end summary can describe what the operator saw without a
/// separate event-stream consumer. Counters are u32 (commands per session
/// fits) and byte sums are u64. All fields default to 0.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ProxyTelemetry {
    pub(crate) search_proxy_enabled: bool,
    pub(crate) search_proxy_substitutions: u32,
    pub(crate) search_proxy_escape_hatch_repeats: u32,
    pub(crate) search_proxy_build_pass_throughs: u32,
    pub(crate) search_proxy_compact_bytes: u64,
    pub(crate) search_proxy_raw_bytes: u64,
    pub(crate) large_read_proxy_enabled: bool,
    pub(crate) large_read_proxy_substitutions: u32,
    pub(crate) large_read_proxy_escape_hatch_repeats: u32,
    pub(crate) large_read_proxy_build_pass_throughs: u32,
    pub(crate) large_read_proxy_compact_bytes: u64,
    pub(crate) large_read_proxy_raw_bytes: u64,
}

impl ProxyTelemetry {
    /// True if either proxy fired (substituted, pass-through, or bypassed)
    /// in this session.
    pub(crate) fn any_event(&self) -> bool {
        self.search_proxy_substitutions
            + self.search_proxy_escape_hatch_repeats
            + self.search_proxy_build_pass_throughs
            + self.large_read_proxy_substitutions
            + self.large_read_proxy_escape_hatch_repeats
            + self.large_read_proxy_build_pass_throughs
            > 0
    }

    /// Human-readable end-of-session summary. Returns `None` when no proxy
    /// was enabled — opting out should mean zero noise. When a proxy was
    /// enabled but no event fired, returns a brief "inert" line so the
    /// opted-in operator gets confirmation the feature was wired correctly.
    pub(crate) fn summary(&self) -> Option<String> {
        if !self.search_proxy_enabled && !self.large_read_proxy_enabled {
            return None;
        }
        let mut lines = Vec::new();
        lines.push("[reactive mediation] session summary:".to_string());
        if self.search_proxy_enabled {
            let saved = self
                .search_proxy_raw_bytes
                .saturating_sub(self.search_proxy_compact_bytes);
            if self.search_proxy_substitutions
                + self.search_proxy_escape_hatch_repeats
                + self.search_proxy_build_pass_throughs
                == 0
            {
                lines.push("  search-proxy: inert (no eligible rg occurred)".to_string());
            } else {
                lines.push(format!(
                    "  search-proxy: {} substituted, {} bypassed (model re-ran), {} pass-through; saved ~{} (compact {} vs raw {})",
                    self.search_proxy_substitutions,
                    self.search_proxy_escape_hatch_repeats,
                    self.search_proxy_build_pass_throughs,
                    format_bytes(saved),
                    format_bytes(self.search_proxy_compact_bytes),
                    format_bytes(self.search_proxy_raw_bytes),
                ));
            }
        }
        if self.large_read_proxy_enabled {
            let saved = self
                .large_read_proxy_raw_bytes
                .saturating_sub(self.large_read_proxy_compact_bytes);
            if self.large_read_proxy_substitutions
                + self.large_read_proxy_escape_hatch_repeats
                + self.large_read_proxy_build_pass_throughs
                == 0
            {
                lines.push("  large-read-proxy: inert (no eligible cat/sed occurred)".to_string());
            } else {
                lines.push(format!(
                    "  large-read-proxy: {} substituted, {} bypassed (model re-ran), {} pass-through; saved ~{} (compact {} vs raw {})",
                    self.large_read_proxy_substitutions,
                    self.large_read_proxy_escape_hatch_repeats,
                    self.large_read_proxy_build_pass_throughs,
                    format_bytes(saved),
                    format_bytes(self.large_read_proxy_compact_bytes),
                    format_bytes(self.large_read_proxy_raw_bytes),
                ));
            }
        }
        Some(lines.join("\n"))
    }
}

/// Format a byte count as a short human-readable string.
fn format_bytes(b: u64) -> String {
    if b >= 1024 * 1024 {
        format!("{:.1} MB", b as f64 / (1024.0 * 1024.0))
    } else if b >= 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}

#[cfg(test)]
mod proxy_telemetry_tests {
    use super::*;

    #[test]
    fn summary_none_when_no_proxy_enabled() {
        let t = ProxyTelemetry::default();
        assert!(t.summary().is_none());
    }

    #[test]
    fn summary_inert_when_enabled_but_no_events() {
        let t = ProxyTelemetry {
            search_proxy_enabled: true,
            large_read_proxy_enabled: true,
            ..Default::default()
        };
        let s = t.summary().unwrap();
        assert!(s.contains("[reactive mediation] session summary:"));
        assert!(s.contains("search-proxy: inert"));
        assert!(s.contains("large-read-proxy: inert"));
    }

    #[test]
    fn summary_reports_substitutions_and_savings_in_human_units() {
        let t = ProxyTelemetry {
            search_proxy_enabled: true,
            search_proxy_substitutions: 3,
            search_proxy_escape_hatch_repeats: 1,
            search_proxy_compact_bytes: 2_048,
            search_proxy_raw_bytes: 200_000,
            large_read_proxy_enabled: false,
            ..Default::default()
        };
        let s = t.summary().unwrap();
        assert!(s.contains("search-proxy: 3 substituted, 1 bypassed"));
        // raw 200kB → compact 2kB ≈ 193 KB saved
        assert!(s.contains("KB"));
        // LRP not enabled → not mentioned.
        assert!(!s.contains("large-read-proxy"));
    }

    #[test]
    fn any_event_detects_partial_activity() {
        let mut t = ProxyTelemetry::default();
        assert!(!t.any_event());
        t.search_proxy_build_pass_throughs = 1;
        assert!(t.any_event());
    }
}
