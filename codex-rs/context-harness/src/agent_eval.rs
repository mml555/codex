//! Scoring for vanilla vs harness-context agent runs on the same tasks.
//!
//! Consumes per-run artifacts (git diff, test exit code, optional exec JSONL) and
//! fixture gold labels. Does not invoke models.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

use crate::eval::EvalTaskFixture;

/// Task fixture for agent A/B evals (extends packet-eval labels with run metadata).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentEvalTask {
    #[serde(default)]
    pub id: String,
    pub task: String,
    #[serde(alias = "gold_files")]
    pub relevant_files: Vec<String>,
    #[serde(alias = "gold_tests", default)]
    pub relevant_tests: Vec<String>,
    #[serde(default)]
    pub danger_zones: Vec<String>,
    /// Shell command that must exit 0 for `tests_passed` (e.g. narrow pytest).
    #[serde(default)]
    pub verify_command: Option<String>,
    /// Paths that connect areas (CLI ↔ core ↔ harness); scored as `bridge_files_touched`.
    #[serde(default)]
    pub bridge_files: Vec<String>,
    /// `calculator` copies the Python E2E fixture; `codex_rs` runs in the codex-rs tree.
    #[serde(default)]
    pub workdir: AgentEvalWorkdir,
    /// Coarse-grained category for grouping the report. None tasks render under
    /// a final "uncategorized" group.
    #[serde(default)]
    pub category: Option<TaskCategory>,
    /// Whether this task naturally REQUIRES the model to verify its
    /// change (e.g. compile + run targeted tests). The first set of
    /// release-mode pairs revealed that "should the model run
    /// `just test`?" was a much bigger driver of wall-clock and token
    /// cost than the RI directive itself — vanilla skipping
    /// verification on a simple doc-comment task produced an
    /// 11x wall-clock difference vs RI. Tagging fixtures lets the
    /// report split metrics by task class so verification-optional
    /// and verification-required pairs are compared separately.
    ///
    /// `None` for pre-instrumentation fixtures (default). Use
    /// `Some(true)` when the task adds assertions, behavior changes,
    /// or new tests where both arms naturally need to confirm the
    /// result. Use `Some(false)` for pure routing / doc-only tasks
    /// where verification is optional.
    #[serde(default)]
    pub verification_required: Option<bool>,
}

/// What aspect of repo intelligence a task is meant to exercise. Used purely
/// for report grouping — scoring is unchanged across categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    /// Multiple files share the same keyword; RI should route to the right
    /// one before the first edit.
    FileRouting,
    /// Task names an owner concept; RI should also surface the bridge file
    /// the agent has to touch to wire it.
    BridgeWiring,
    /// Task names a behavior to verify; RI should point at the matching test
    /// file rather than a same-named file in src/.
    TestTargeting,
    /// Task asks to follow a convention that exists in one specific file
    /// (e.g. a feature-flag entry); RI should surface that file.
    LocalConvention,
    /// Task implicitly requires edits in two or more crates; RI should
    /// surface both owner and dependent.
    CrossModuleOwnership,
}

impl TaskCategory {
    pub fn slug(self) -> &'static str {
        match self {
            Self::FileRouting => "file_routing",
            Self::BridgeWiring => "bridge_wiring",
            Self::TestTargeting => "test_targeting",
            Self::LocalConvention => "local_convention",
            Self::CrossModuleOwnership => "cross_module_ownership",
        }
    }
}

/// Recorded outcome of one agent run (vanilla or harness-context).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRunRecord {
    pub arm: AgentArm,
    pub task_id: String,
    pub changed_files: Vec<String>,
    pub tests_passed: bool,
    pub turn_count: Option<u32>,
    #[serde(default)]
    pub exec_exit_code: Option<i32>,
    #[serde(default)]
    pub repo_intelligence_enabled: bool,
    #[serde(default)]
    pub harness_context_visible: bool,
    #[serde(default = "default_true")]
    pub run_valid: bool,
    #[serde(default)]
    pub invalid_reason: Option<AgentRunInvalidReason>,
    /// Sum of `turn.completed.usage.input_tokens` across all turns. `None` if
    /// no `turn.completed` events were parsed (e.g. missing/empty events.jsonl).
    #[serde(default)]
    pub tokens_input: Option<u64>,
    #[serde(default)]
    pub tokens_output: Option<u64>,
    #[serde(default)]
    pub tokens_total: Option<u64>,
    /// Wall-clock duration of the `codex exec` invocation in milliseconds.
    /// `None` for pre-duration records (`serde(default)`). The point of
    /// recording this is that *time* is part of the RI claim — if RI adds
    /// prompt tokens but avoids file-search turns, it can still be a net win
    /// on the metric users actually feel.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Local prewarm wall-clock time, in milliseconds, spent by the
    /// eval runner building the shared `RepoMap` ONCE for the batch
    /// before any arm started. Threaded into every arm's record (not
    /// per-arm cost) so reviewers can separate three numbers:
    ///   - prewarm_ms: local one-time index build (amortized)
    ///   - duration_ms: per-arm `codex exec` wall-clock
    ///   - model_loop = duration_ms minus the in-session init gap
    /// The area-package-alias gated pairs showed the RI arm's
    /// `duration_ms` included ~170s of in-session repo-index build
    /// that this prewarm path is designed to eliminate.
    #[serde(default)]
    pub harness_prewarm_ms: Option<u64>,
    /// Build profile of the codex binary that produced this record
    /// — "release" or "debug". The cached-pair diagnosis (Run 3)
    /// showed `build_context_packet` runs ~7× slower in debug
    /// (~85s vs ~12s for the codex-rs map), enough to dominate any
    /// model-loop comparison. Recording the profile prevents future
    /// reviewers from comparing wall-clock numbers across mixed
    /// builds. `None` for pre-instrumentation records.
    #[serde(default)]
    pub codex_build_profile: Option<String>,
    /// Count of `item.completed` events of any item type. A coarse proxy
    /// for "how many tool calls did the model make this turn".
    #[serde(default)]
    pub tool_call_count: Option<u32>,
    /// Count of `item.completed` events with `item.type == "command_execution"`.
    /// Direct measure of how many shell commands the agent fired.
    #[serde(default)]
    pub shell_command_count: Option<u32>,
    /// Heuristic count of file-read shell commands (`cat`/`head`/`tail`/
    /// `less`/`more`). Diagnostic — captures "stupid file-finding" but not
    /// loads via codex's structured file-read tool, if any.
    #[serde(default)]
    pub file_read_count: Option<u32>,
    /// Phase-classified shell command counts. See [`classify_shell_phase`]
    /// for the heuristic. Sum of all four phase counts (discover + read +
    /// edit + verify) plus an unrecorded "other" residual equals
    /// `shell_command_count`. None for pre-phase-instrumentation records.
    #[serde(default)]
    pub discover_command_count: Option<u32>,
    #[serde(default)]
    pub edit_command_count: Option<u32>,
    #[serde(default)]
    pub verify_command_count: Option<u32>,
    /// Non-fatal observations about this run that do NOT invalidate it.
    /// Populated by the bash runner when, e.g., a `provider_network_error`
    /// event appears mid-stream but codex auto-reconnects and the turn
    /// still completes. Reviewer-facing flag, not used by the classifier.
    ///
    /// Pre-warning records load with an empty Vec via `serde(default)`.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Paths the RI extension rendered under the "Likely edit targets:"
    /// section of its directive prompt for THIS run. Recovered from the
    /// rollout's user/developer message via
    /// `ContextPacketRenderer::parse_directive_file_lists`. Empty for
    /// vanilla arms (no RI directive in their prompt) and for pre-split
    /// records (the legacy `Before editing, inspect these files first:`
    /// single section is intentionally not back-parsed; see the
    /// `parse_directive_file_lists_handles_legacy_single_section_fragment`
    /// test).
    #[serde(default)]
    pub ri_surfaced_edit_targets: Vec<String>,
    /// Paths the RI extension rendered under the "Orientation only:"
    /// section. Same provenance as `ri_surfaced_edit_targets`. Feeds
    /// the new `orientation_files_touched` metric: a file the model
    /// edited despite being labeled orientation is direct evidence
    /// that the directive failed to constrain scope.
    #[serde(default)]
    pub ri_surfaced_orientation: Vec<String>,
    /// Paths the model INTENTIONALLY edited, extracted from
    /// `file_change` items in `events.jsonl`. The authoritative answer
    /// to "what did the model decide to touch?" — strictly better than
    /// `git diff --name-only HEAD`, which also picks up `cargo fmt`
    /// collateral, build artifacts, and any setup-time worktree writes.
    /// Empty for pre-instrumentation records; in that case the scorer
    /// falls back to `changed_files`.
    #[serde(default)]
    pub intent_changed_files: Vec<String>,
    /// Raw `git diff --name-only HEAD` ∪ untracked-files set, captured
    /// for diagnostic purposes. The `formatter_changed_files` field
    /// stores `diff_changed_files − intent_changed_files` so a reviewer
    /// can see at a glance how much collateral a run accumulated. Both
    /// new fields are diagnostic-only — `score_run` uses
    /// `intent_changed_files` (with `changed_files` as fallback) when
    /// computing target/orientation/extra metrics.
    #[serde(default)]
    pub diff_changed_files: Vec<String>,
    /// `diff_changed_files − intent_changed_files`. Files the diff saw
    /// but the model didn't author via `file_change` / apply_patch.
    /// Typical sources: `cargo fmt --all` reformatting drift,
    /// `__pycache__` artifacts, build-side outputs. NOT used for
    /// scoring; surfaced so reviewers can sanity-check what the eval
    /// is and isn't crediting.
    #[serde(default)]
    pub formatter_changed_files: Vec<String>,
    /// True when this arm ran in a worktree that was not shared with any other
    /// arm or task. For `codex_rs` workdirs that means `--isolated-worktrees`
    /// was set; for `calculator` workdirs the arm always gets a fresh
    /// `mktemp -d` + `git init`, so this is always true.
    #[serde(default)]
    pub worktree_isolated: bool,
    /// Resolved git SHA the arm's worktree started from. `None` for
    /// `calculator` workdirs (no shared base ref) and for non-isolated
    /// `codex_rs` runs.
    #[serde(default)]
    pub base_ref: Option<String>,
    /// Absolute path of the cwd the arm ran in. Recorded so a reviewer can
    /// later distinguish two runs that nominally shared the same checkout
    /// from two runs that were genuinely isolated.
    #[serde(default)]
    pub worktree_path: Option<String>,
    /// True when the codex binary was invoked with
    /// `features.search_proxy=true` for this run. False for vanilla
    /// arms and for any treatment whose runner forgot to flip the
    /// flag. Recorded explicitly so reviewers can confirm the
    /// treatment was actually applied (the rg-interception MVP
    /// silently no-ops when the feature is off).
    #[serde(default)]
    pub search_proxy_enabled: bool,
    /// Number of search-proxy `event=substitute` lines in the
    /// codex_exec stderr. Each line corresponds to a model-issued
    /// `rg` call where the proxy returned compact evidence instead
    /// of raw output.
    #[serde(default)]
    pub search_proxy_substitutions: u32,
    /// Number of `event=escape_hatch_repeat` lines — the model
    /// resent a previously-substituted normalized command and got
    /// raw `rg` output the second time.
    #[serde(default)]
    pub search_proxy_escape_hatch_repeats: u32,
    /// Number of `event=build_pass_through` lines — the proxy ran
    /// internal `rg` but declined to substitute (no matches, rg
    /// error, runner spawn failure, or raw output already smaller
    /// than the compact form would be).
    #[serde(default)]
    pub search_proxy_build_pass_throughs: u32,
    /// Sum of `compact_bytes` across all substitutions. Total bytes
    /// the proxy actually sent back to the model.
    #[serde(default)]
    pub search_proxy_compact_bytes: u64,
    /// Sum of `raw_bytes` across all substitutions. Total bytes of
    /// `rg --json` output the proxy consumed internally — a rough
    /// upper bound on what the model would have seen without the
    /// proxy.
    #[serde(default)]
    pub search_proxy_raw_bytes_estimated: u64,
    /// Top-ranked file from each substitution, in invocation order.
    /// For Run-8-style symbol searches the success signal is whether
    /// the gold owner file appears here.
    #[serde(default)]
    pub search_proxy_top_files: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEvalWorkdir {
    #[default]
    Calculator,
    CodexRs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentArm {
    Vanilla,
    Harness,
    RepoIntelligence,
    /// Treatment arm for the search-proxy MVP. Runs codex with
    /// `features.search_proxy=true`; vanilla arm runs without.
    SearchProxy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunInvalidReason {
    ProviderUsageLimit,
    ProviderAuthError,
    ProviderNetworkError,
    TurnFailed,
    RunnerError,
    MissingEvents,
    UnknownFailure,
}

impl AgentArm {
    pub fn artifact_dir(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Harness => "harness",
            Self::RepoIntelligence => "repo_intelligence",
            Self::SearchProxy => "search_proxy",
        }
    }

    pub fn display_label(self) -> &'static str {
        self.artifact_dir()
    }
}

/// Per-run scores for one arm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRunScore {
    /// True if any gold (`relevant_files`) path was touched.
    pub correct_file_touched: bool,
    /// Count of distinct paths from (gold ∪ bridge) that were touched.
    pub target_files_hit: usize,
    /// Size of the (gold ∪ bridge) set, i.e. denominator for target_files_hit.
    pub target_files_total: usize,
    pub tests_passed: bool,
    pub turn_count: Option<u32>,
    pub unnecessary_files_changed: Vec<String>,
    pub harness_context_visible: bool,
    pub bridge_files_touched: Vec<String>,
    pub run_valid: bool,
    pub invalid_reason: Option<AgentRunInvalidReason>,
    pub tokens_input: Option<u64>,
    pub tokens_output: Option<u64>,
    pub tokens_total: Option<u64>,
    /// Wall-clock duration in ms (from the script-side bracket around
    /// `codex exec`). Used by the classifier between `turns` and `tokens`.
    pub duration_ms: Option<u64>,
    /// Diagnostic: total `item.completed` events the agent emitted.
    pub tool_call_count: Option<u32>,
    /// Diagnostic: `item.completed` events with `command_execution` items.
    pub shell_command_count: Option<u32>,
    /// Diagnostic: shell commands whose first executable is `cat`/`head`/
    /// `tail`/`less`/`more`. Coarse signal for "model wandering the repo".
    pub file_read_count: Option<u32>,
    /// Phase-classified counts, propagated from `AgentRunRecord`. The
    /// cost-table renderer surfaces these separately so a reviewer can
    /// see, e.g., "RI cut discovery -2 but added +4 verify" instead of
    /// just the aggregate.
    ///
    /// `edit_command_count` includes both shell-based edits (sed -i,
    /// perl -i, etc.) AND structured `file_change` / apply_patch items
    /// from the events stream — otherwise frontier models that edit via
    /// apply_patch render as `e=0` even though they made changes.
    pub discover_command_count: Option<u32>,
    pub edit_command_count: Option<u32>,
    pub verify_command_count: Option<u32>,
    /// Non-fatal observations carried over from `AgentRunRecord`.
    /// Surfaced in the Main table's Valid? cell as `valid (warning)`.
    pub warnings: Vec<String>,
    /// Count of gold (`relevant_files`) paths that were touched. New
    /// stricter cut than `target_files_hit`: this excludes bridge files,
    /// which the first cloud batch showed were sometimes mistakenly
    /// counted as wins even when the model only updated wiring.
    pub edit_target_files_hit: usize,
    /// Size of the gold set, denominator for `edit_target_files_hit`.
    pub edit_target_files_total: usize,
    /// Count of files in (bridge ∪ ri_surfaced_orientation − gold) that
    /// the model TOUCHED. Captures scope-broadening: a non-zero value
    /// means the model edited something the directive explicitly
    /// labeled orientation (or a bridge file that the new directive
    /// rendering should classify as orientation by default).
    /// Pre-split records (no `ri_surfaced_*` data) fall back to
    /// `bridge − gold ∩ changed_files`, so the metric is meaningful
    /// for backfilled artifacts too.
    pub orientation_files_touched: usize,
    /// Size of (bridge ∪ ri_surfaced_orientation − gold). The
    /// denominator the model COULD have over-touched.
    pub orientation_files_total: usize,
}

/// Comparison verdict for a (vanilla, treatment) pair on one task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvalResult {
    /// Treatment improved on vanilla on the named dimension.
    RiBetter { reason: ResultReason },
    /// Treatment regressed against vanilla on the named dimension.
    RiWorse { reason: ResultReason },
    /// All comparison dimensions tied.
    Tie,
    /// Comparison was not made because the pair was invalid.
    Excluded { reason: String },
}

/// First-dimension-of-divergence label. Priority order (highest first):
/// file_targeting → fewer_extra_files → fewer_turns → faster_wall_clock →
/// fewer_tokens.
///
/// The `faster_wall_clock` tier was added when the project thesis sharpened
/// to "move file-discovery work out of the paid model loop into the
/// harness." Time is what users feel; tokens are what they pay. Both
/// matter, but a cheaper wrong/slower answer is never preferred over a
/// faster correct one — hence time sits below turns/waste/targeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultReason {
    FileTargeting,
    FewerExtraFiles,
    FewerTurns,
    FasterWallClock,
    FewerTokens,
}

impl ResultReason {
    pub fn slug(self) -> &'static str {
        match self {
            Self::FileTargeting => "file_targeting",
            Self::FewerExtraFiles => "fewer_extra_files",
            Self::FewerTurns => "fewer_turns",
            Self::FasterWallClock => "faster_wall_clock",
            Self::FewerTokens => "fewer_tokens",
        }
    }
}

impl AgentEvalResult {
    /// Render as the machine+human label, e.g. `ri_better:file_targeting`.
    pub fn slug(&self) -> String {
        match self {
            Self::RiBetter { reason } => format!("ri_better:{}", reason.slug()),
            Self::RiWorse { reason } => format!("ri_worse:{}", reason.slug()),
            Self::Tie => "tie".to_string(),
            Self::Excluded { reason } => format!("excluded:{reason}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentEvalComparison {
    pub task_id: String,
    pub task: String,
    pub vanilla: AgentRunScore,
    #[serde(alias = "harness")]
    pub treatment: AgentRunScore,
    pub treatment_arm: AgentArm,
    #[serde(default = "default_true")]
    pub valid_for_comparison: bool,
    #[serde(default)]
    pub excluded_reason: Option<String>,
    pub result: AgentEvalResult,
    #[serde(default)]
    pub category: Option<TaskCategory>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentEvalSummary {
    pub total_pairs: usize,
    pub valid_pairs: usize,
    pub invalid_pairs: usize,
    pub invalid_reason_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentEvalReport {
    pub comparisons: Vec<AgentEvalComparison>,
    pub summary: AgentEvalSummary,
}

pub fn load_agent_eval_tasks(path: &Path) -> anyhow::Result<Vec<AgentEvalTask>> {
    let bytes = std::fs::read(path)?;
    let mut tasks: Vec<AgentEvalTask> = serde_json::from_slice(&bytes)?;
    for (index, task) in tasks.iter_mut().enumerate() {
        if task.id.is_empty() {
            task.id = format!("task_{index}");
        }
        normalize_task_paths(task);
    }
    Ok(tasks)
}

/// Normalize repo-relative paths so fixture gold/bridge labels match `git diff` output.
///
/// Examples:
/// - `codex-rs/cli/src/foo.rs` → `cli/src/foo.rs`
/// - `./cli/src/foo.rs` → `cli/src/foo.rs`
/// - `/abs/.../codex-rs/cli/src/foo.rs` → `cli/src/foo.rs`
pub fn normalize_agent_eval_path(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    if path.is_empty() {
        return String::new();
    }
    let path = path.trim_start_matches("./");
    if let Some(idx) = path.find("/codex-rs/") {
        return path[idx + "/codex-rs/".len()..].to_string();
    }
    let mut rest = path;
    while let Some(stripped) = rest.strip_prefix("codex-rs/") {
        rest = stripped;
    }
    rest.to_string()
}

fn normalize_agent_eval_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .map(|path| normalize_agent_eval_path(path))
        .filter(|path| !path.is_empty())
        .collect()
}

fn normalize_task_paths(task: &mut AgentEvalTask) {
    task.relevant_files = normalize_agent_eval_paths(&task.relevant_files);
    task.bridge_files = normalize_agent_eval_paths(&task.bridge_files);
    task.danger_zones = normalize_agent_eval_paths(&task.danger_zones);
}

/// Paths produced by verification/pytest side effects, not meaningful agent edits.
pub fn is_agent_eval_noise_path(path: &str) -> bool {
    let path = path.trim();
    if path.is_empty() {
        return true;
    }
    if path.ends_with(".pyc") {
        return true;
    }
    path.split('/')
        .any(|segment| segment == "__pycache__" || segment == ".pytest_cache")
}

/// Filter `changed_files` before scoring agent-quality metrics.
pub fn filter_scoring_changed_files(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| !is_agent_eval_noise_path(path))
        .cloned()
        .collect()
}

pub fn score_run(record: &AgentRunRecord, task: &AgentEvalTask) -> AgentRunScore {
    let gold: BTreeSet<String> = task.relevant_files.iter().cloned().collect();
    let bridge: BTreeSet<String> = task.bridge_files.iter().cloned().collect();
    let target: BTreeSet<String> = gold.union(&bridge).cloned().collect();
    // Source of truth for "what did the model edit?" — prefer
    // `intent_changed_files` (from authoritative `file_change` events),
    // fall back to `changed_files` (raw git diff) for pre-instrumented
    // records. The rate_limit-v2 rerun showed `git diff` over-counts
    // by 4-7x when the model runs `just fmt`, so anchoring on intent
    // is required for `extra_files` and `orientation_files_touched`
    // to mean what their names say.
    let source = if record.intent_changed_files.is_empty() {
        &record.changed_files
    } else {
        &record.intent_changed_files
    };
    let changed: BTreeSet<String> = filter_scoring_changed_files(source)
        .into_iter()
        .map(|path| normalize_agent_eval_path(&path))
        .filter(|path| !path.is_empty())
        .collect();
    let correct_file_touched = gold.iter().any(|path| changed.contains(path));
    let target_files_hit = target.intersection(&changed).count();
    let target_files_total = target.len();
    let unnecessary_files_changed: Vec<String> = changed.difference(&gold).cloned().collect();
    let bridge_files_touched: Vec<String> = bridge.intersection(&changed).cloned().collect();

    // Orientation set: every file the directive labeled "orientation
    // only" for this arm, plus the fixture's bridge_files (which the
    // new directive rendering treats as orientation by default). Then
    // subtract the gold so an edit to a gold path doesn't double-count
    // as an over-broaden. Vanilla arms have no `ri_surfaced_*` data,
    // so their orientation_set collapses to (bridge − gold).
    let ri_orientation: BTreeSet<String> = record
        .ri_surfaced_orientation
        .iter()
        .map(|p| normalize_agent_eval_path(p))
        .filter(|p| !p.is_empty())
        .collect();
    let orientation_set: BTreeSet<String> = bridge
        .union(&ri_orientation)
        .filter(|p| !gold.contains(*p))
        .cloned()
        .collect();
    let edit_target_files_hit = gold.intersection(&changed).count();
    let edit_target_files_total = gold.len();
    let orientation_files_touched = orientation_set.intersection(&changed).count();
    let orientation_files_total = orientation_set.len();

    AgentRunScore {
        correct_file_touched,
        target_files_hit,
        target_files_total,
        tests_passed: record.tests_passed && record.run_valid,
        turn_count: record.turn_count,
        unnecessary_files_changed,
        harness_context_visible: record.harness_context_visible,
        bridge_files_touched,
        run_valid: record.run_valid,
        invalid_reason: record.invalid_reason,
        tokens_input: record.tokens_input,
        tokens_output: record.tokens_output,
        tokens_total: record.tokens_total,
        duration_ms: record.duration_ms,
        tool_call_count: record.tool_call_count,
        shell_command_count: record.shell_command_count,
        file_read_count: record.file_read_count,
        discover_command_count: record.discover_command_count,
        edit_command_count: record.edit_command_count,
        verify_command_count: record.verify_command_count,
        warnings: record.warnings.clone(),
        edit_target_files_hit,
        edit_target_files_total,
        orientation_files_touched,
        orientation_files_total,
    }
}

/// Decide the comparison verdict using the priority order
/// `file_targeting > fewer_extra_files > fewer_turns > fewer_tokens`. Cheaper
/// is never preferred over correctness — token comparison runs last.
pub fn classify_result(
    vanilla: &AgentRunScore,
    treatment: &AgentRunScore,
    valid_for_comparison: bool,
    excluded_reason: Option<&str>,
) -> AgentEvalResult {
    if !valid_for_comparison {
        let reason = excluded_reason.unwrap_or("invalid").to_string();
        return AgentEvalResult::Excluded { reason };
    }

    if treatment.target_files_hit > vanilla.target_files_hit {
        return AgentEvalResult::RiBetter {
            reason: ResultReason::FileTargeting,
        };
    }
    if treatment.target_files_hit < vanilla.target_files_hit {
        return AgentEvalResult::RiWorse {
            reason: ResultReason::FileTargeting,
        };
    }

    let v_extra = vanilla.unnecessary_files_changed.len();
    let t_extra = treatment.unnecessary_files_changed.len();
    if t_extra < v_extra {
        return AgentEvalResult::RiBetter {
            reason: ResultReason::FewerExtraFiles,
        };
    }
    if t_extra > v_extra {
        return AgentEvalResult::RiWorse {
            reason: ResultReason::FewerExtraFiles,
        };
    }

    if let (Some(vt), Some(tt)) = (vanilla.turn_count, treatment.turn_count) {
        if tt < vt {
            return AgentEvalResult::RiBetter {
                reason: ResultReason::FewerTurns,
            };
        }
        if tt > vt {
            return AgentEvalResult::RiWorse {
                reason: ResultReason::FewerTurns,
            };
        }
    }

    if let (Some(vd), Some(td)) = (vanilla.duration_ms, treatment.duration_ms) {
        if td < vd {
            return AgentEvalResult::RiBetter {
                reason: ResultReason::FasterWallClock,
            };
        }
        if td > vd {
            return AgentEvalResult::RiWorse {
                reason: ResultReason::FasterWallClock,
            };
        }
    }

    if let (Some(vt), Some(tt)) = (vanilla.tokens_total, treatment.tokens_total) {
        if tt < vt {
            return AgentEvalResult::RiBetter {
                reason: ResultReason::FewerTokens,
            };
        }
        if tt > vt {
            return AgentEvalResult::RiWorse {
                reason: ResultReason::FewerTokens,
            };
        }
    }

    AgentEvalResult::Tie
}

pub fn compare_task(
    task: &AgentEvalTask,
    vanilla: &AgentRunRecord,
    treatment: &AgentRunRecord,
) -> AgentEvalComparison {
    let excluded_reason = pair_excluded_reason(vanilla, treatment);
    let valid_for_comparison = excluded_reason.is_none();
    let vanilla_score = score_run(vanilla, task);
    let treatment_score = score_run(treatment, task);
    let result = classify_result(
        &vanilla_score,
        &treatment_score,
        valid_for_comparison,
        excluded_reason.as_deref(),
    );
    AgentEvalComparison {
        task_id: task.id.clone(),
        task: task.task.clone(),
        vanilla: vanilla_score,
        treatment: treatment_score,
        treatment_arm: treatment.arm,
        valid_for_comparison,
        excluded_reason,
        result,
        category: task.category,
    }
}

pub fn build_report(comparisons: Vec<AgentEvalComparison>) -> AgentEvalReport {
    let mut invalid_reason_counts: BTreeMap<String, usize> = BTreeMap::new();
    let total_pairs = comparisons.len();
    let mut invalid_pairs = 0usize;
    for row in &comparisons {
        if !row.valid_for_comparison {
            invalid_pairs += 1;
            if let Some(reason) = &row.excluded_reason {
                *invalid_reason_counts.entry(reason.clone()).or_default() += 1;
            }
        }
    }
    let valid_pairs = total_pairs.saturating_sub(invalid_pairs);
    AgentEvalReport {
        comparisons,
        summary: AgentEvalSummary {
            total_pairs,
            valid_pairs,
            invalid_pairs,
            invalid_reason_counts,
        },
    }
}

fn pair_excluded_reason(vanilla: &AgentRunRecord, treatment: &AgentRunRecord) -> Option<String> {
    if vanilla.run_valid && treatment.run_valid {
        return None;
    }
    let left = vanilla
        .invalid_reason
        .map(invalid_reason_slug)
        .unwrap_or("invalid");
    let right = treatment
        .invalid_reason
        .map(invalid_reason_slug)
        .unwrap_or("invalid");
    Some(format!("pair_invalid:{left}|{right}"))
}

fn invalid_reason_slug(reason: AgentRunInvalidReason) -> &'static str {
    match reason {
        AgentRunInvalidReason::ProviderUsageLimit => "provider_usage_limit",
        AgentRunInvalidReason::ProviderAuthError => "provider_auth_error",
        AgentRunInvalidReason::ProviderNetworkError => "provider_network_error",
        AgentRunInvalidReason::TurnFailed => "turn_failed",
        AgentRunInvalidReason::RunnerError => "runner_error",
        AgentRunInvalidReason::MissingEvents => "missing_events",
        AgentRunInvalidReason::UnknownFailure => "unknown_failure",
    }
}

/// Count model turns from `codex exec --json` JSONL (`turn.completed` / `turn.failed`).
pub fn count_turns_from_exec_jsonl(bytes: &[u8]) -> anyhow::Result<u32> {
    let mut count = 0u32;
    for line in std::str::from_utf8(bytes)?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)?;
        let Some(kind) = value.get("type").and_then(|v| v.as_str()) else {
            continue;
        };
        if matches!(kind, "turn.completed" | "turn.failed") {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

/// Per-arm token totals summed across all `turn.completed.usage` events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenUsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Sum `usage.input_tokens` and `usage.output_tokens` across `turn.completed`
/// events. Returns `None` if no `turn.completed` event is present (e.g. the
/// run crashed before completing a turn) — distinct from `Some(0)` for a
/// completed turn that reported zero usage.
pub fn token_usage_from_exec_jsonl(bytes: &[u8]) -> anyhow::Result<Option<TokenUsageTotals>> {
    let mut seen_completed = false;
    let mut input: u64 = 0;
    let mut output: u64 = 0;
    for line in std::str::from_utf8(bytes)?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)?;
        let Some(kind) = value.get("type").and_then(|v| v.as_str()) else {
            continue;
        };
        if kind != "turn.completed" {
            continue;
        }
        seen_completed = true;
        let Some(usage) = value.get("usage") else {
            continue;
        };
        if let Some(n) = usage.get("input_tokens").and_then(serde_json::Value::as_i64) {
            input = input.saturating_add(n.max(0) as u64);
        }
        if let Some(n) = usage.get("output_tokens").and_then(serde_json::Value::as_i64) {
            output = output.saturating_add(n.max(0) as u64);
        }
    }
    if !seen_completed {
        return Ok(None);
    }
    Ok(Some(TokenUsageTotals {
        input_tokens: input,
        output_tokens: output,
        total_tokens: input.saturating_add(output),
    }))
}

/// Detect whether the harness directive marker appears in the model's
/// prompt input (NOT in tool outputs the agent shell-grepped back).
///
/// Codex's session rollout (`~/.codex/sessions/.../rollout-*.jsonl`) emits
/// `response_item` entries whose `payload.type` distinguishes:
///   - `message`            (with `role` in {user, developer, assistant})
///   - `function_call`      (tool the model invoked)
///   - `function_call_output` (tool's output echoed back as context)
///   - `reasoning`, `custom_tool_call`, ...
///
/// The harness directive packet appears as a `message` with `role` ==
/// `user` or `developer`. It can ALSO appear as text inside
/// `function_call_output` payloads whenever the agent grepped a source
/// file containing the `HARNESS_MARKER` constant — that's a false
/// positive. This helper only inspects message-role payloads.
///
/// Returns `true` iff the literal marker appears in at least one
/// `payload.type == "message"` entry whose role is in `{user, developer}`.
pub fn rollout_carries_harness_directive(rollout_jsonl: &str) -> bool {
    const MARKER: &str = "Harness repo intelligence:";
    for line in rollout_jsonl.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(ev): Result<serde_json::Value, _> = serde_json::from_str(line) else {
            continue;
        };
        // Only response_item entries.
        if ev.get("type").and_then(|v| v.as_str()) != Some("response_item") {
            continue;
        }
        let Some(payload) = ev.get("payload") else {
            continue;
        };
        // Only message-typed payloads with user/system role.
        if payload.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        let role = payload.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if !matches!(role, "user" | "developer" | "system") {
            continue;
        }
        // Search ONLY the message content, not the full payload (which
        // could include metadata fields that happen to mention the
        // marker). content is typically a string or an array of parts.
        let needle_found = match payload.get("content") {
            Some(serde_json::Value::String(s)) => s.contains(MARKER),
            Some(serde_json::Value::Array(parts)) => parts.iter().any(|p| {
                p.as_str().map(|s| s.contains(MARKER)).unwrap_or_else(|| {
                    p.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.contains(MARKER))
                        .unwrap_or(false)
                })
            }),
            _ => false,
        };
        if needle_found {
            return true;
        }
    }
    false
}

/// Per-arm activity counts derived from a `codex exec --json` events stream.
/// All counters are coarse, post-hoc, and meant for visibility — not
/// scoring. The point is to surface "did the model wander the repo?" so a
/// reviewer can read the cost table next to the result table and tell
/// whether RI moved work out of the model loop.
///
/// Q8 rehearsal (2026-05-25) showed that the aggregate shell-command count
/// can MISLEAD: RI may save 2 discovery commands but lose 4 to edit/verify
/// churn, and the aggregate just shows "+2 total" without exposing which
/// phase moved. Hence the per-phase classification below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentActivityCounts {
    /// Every `item.completed` event the run emitted, regardless of item type.
    /// Coarse proxy for "how many tool turns did the model take".
    pub tool_calls: u32,
    /// `item.completed` events whose `item.type == "command_execution"`.
    /// Equal to `discover + read + edit + verify + other`.
    pub shell_commands: u32,
    /// Repo navigation: `find`, `ls`, `tree`, `locate`, `fd`, plain `grep`
    /// (no context flags), `git status`.
    pub discover_commands: u32,
    /// File reading: `cat`, `head`, `tail`, `less`, `more`. (Same set as
    /// the existing `file_read_count` to keep the bash↔Rust mirror simple.)
    pub file_reads: u32,
    /// Code modification: `sed -i`, `perl -i`, `awk` with `inplace`,
    /// `echo` with `>`/`>>` redirects, `mv`, `cp`, `rm`, `mkdir`, `touch`.
    pub edit_commands: u32,
    /// Post-edit confirmation: `grep` with `-A`/`-B`/`-C`/`-c` context
    /// flags, `diff`, `git diff`, `awk` with print-only patterns.
    pub verify_commands: u32,
}

/// Coarse classification of a single shell command into one of five phases.
/// Returned as a static slug for both Rust scoring and the bash mirror.
///
/// Limitations: stateless (can't tell "cat before edit" from "cat after"),
/// and a single command can serve multiple purposes (`grep -n` is discover;
/// `grep -A 3` is verify). The heuristic favors the most common case for
/// each shell tool as observed in q8 / qwen3-coder traces. Two refinements
/// from the 2026-05-25 rehearsal:
///
/// 1. `cd <dir> && <real_cmd>` chains: agents often prefix every shell with
///    `cd $WORKTREE && ...`. We strip every leading `cd ... &&` segment and
///    classify by the actual command that follows.
/// 2. `grep -A5` (no space between flag and number) counts as verify, same
///    as `grep -A 5`.
pub fn classify_shell_phase(command: &str) -> &'static str {
    let normalized = strip_leading_cd_chains(command);
    let first = first_executable_token(&normalized);
    match first {
        "find" | "ls" | "tree" | "locate" | "fd" => "discover",
        "cat" | "head" | "tail" | "less" | "more" => "read",
        "grep" | "rg" | "ack" => {
            // Context flags indicate post-edit verify, not discovery.
            // Match BOTH spaced (`-A 3`) and joined (`-A3`) variants, plus
            // long forms. Use lowercase for case-insensitivity.
            let lc = normalized.to_ascii_lowercase();
            let has_context = lc.contains(" -a ")
                || lc.contains(" -b ")
                || lc.contains(" -c ")
                || lc.contains(" -a=")
                || lc.contains(" -b=")
                || lc.contains(" -c=")
                || lc.contains(" --after-context")
                || lc.contains(" --before-context")
                || lc.contains(" --context")
                // -A5 / -B5 / -C5 (joined form). Single-digit suffix matched
                // by checking " -A<d>" / " -B<d>" / " -C<d>" where d is a
                // digit. Cheap byte scan; no regex dependency.
                || has_joined_context_flag(&lc);
            if has_context { "verify" } else { "discover" }
        }
        "sed" => {
            if command.contains("-i") {
                "edit"
            } else {
                "other"
            }
        }
        "perl" => {
            if command.contains("-i") {
                "edit"
            } else {
                "other"
            }
        }
        "awk" => {
            if command.contains("inplace") {
                "edit"
            } else if command.contains("print") {
                // Bare `awk '... {print ...}'` is almost always used to
                // print a specific line for verify in agent traces.
                "verify"
            } else {
                "other"
            }
        }
        "echo" | "printf" => {
            // `echo X > file` / `echo X >> file` is an edit.
            if command.contains('>') {
                "edit"
            } else {
                "other"
            }
        }
        "mv" | "cp" | "rm" | "mkdir" | "touch" | "rmdir" | "ln" => "edit",
        "diff" => "verify",
        "git" => {
            // `git diff`, `git log -p` etc. are verify; `git status` /
            // `git ls-files` etc. are discover.
            let lc = command.to_ascii_lowercase();
            if lc.contains(" diff") || lc.contains(" log") || lc.contains(" show") {
                "verify"
            } else if lc.contains(" status") || lc.contains(" ls-files") || lc.contains(" branch") {
                "discover"
            } else {
                "other"
            }
        }
        _ => "other",
    }
}

/// Extract the set of file paths the model INTENTIONALLY edited during
/// a run, sourced from authoritative `file_change` / apply_patch items
/// in the `events.jsonl` stream. Returns repo-relative paths (the
/// `codex-rs/` and absolute-worktree prefixes are stripped) so the set
/// can be compared directly against fixture `relevant_files` /
/// `bridge_files`.
///
/// This is the right source of truth for "what did the model decide to
/// touch?" — strictly better than `git diff --name-only HEAD`, which
/// also picks up:
///   - `cargo fmt --all` collateral (any pre-existing format drift in
///     the worktree),
///   - test-artifact dirs (`__pycache__`, `.pytest_cache`),
///   - build artifacts an agent's shell happens to drop,
///   - any setup-time writes the runner itself performs.
///
/// The gated `rate_limit` rerun demonstrated this failure mode: both
/// arms intentionally edited 2 files (gold + bridge) per the
/// `file_change` events, but `git diff` reported 9 files because the
/// model ran `just fmt` which reformatted 7 unrelated files.
///
/// Order is stable (BTreeSet under the hood) so test assertions can
/// compare by equality. Duplicates collapse — if the model edited the
/// same path twice, it appears once. Paths are returned LEXICALLY
/// sorted.
pub fn intent_changed_files_from_exec_jsonl(bytes: &[u8]) -> anyhow::Result<Vec<String>> {
    let mut paths: BTreeSet<String> = BTreeSet::new();
    for line in std::str::from_utf8(bytes)?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Both `item.completed` and `item.started` carry the path; we
        // only want `completed` so partial/failed patches don't count.
        if value.get("type").and_then(|v| v.as_str()) != Some("item.completed") {
            continue;
        }
        let Some(item) = value.get("item") else {
            continue;
        };
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        // Two shapes:
        //   - `file_change` items carry an array `item.changes` of
        //     `{path, kind}` (codex's structured editor).
        //   - `custom_tool_call` items with `name == "apply_patch"`
        //     carry the raw patch in `arguments`; we don't parse those
        //     here. If a deployment goes back to apply_patch shell
        //     wrappers we'd extend this; current frontier models emit
        //     `file_change` consistently.
        if item_type != "file_change" {
            continue;
        }
        let Some(changes) = item.get("changes").and_then(|v| v.as_array()) else {
            continue;
        };
        for ch in changes {
            let Some(raw_path) = ch.get("path").and_then(|v| v.as_str()) else {
                continue;
            };
            let normalized = normalize_agent_eval_path(raw_path);
            if normalized.is_empty() {
                continue;
            }
            paths.insert(normalized);
        }
    }
    Ok(paths.into_iter().collect())
}

/// Parse `events.jsonl` and count tool/shell/per-phase activity.
pub fn count_activity_from_exec_jsonl(bytes: &[u8]) -> anyhow::Result<AgentActivityCounts> {
    let mut counts = AgentActivityCounts::default();
    for line in std::str::from_utf8(bytes)?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)?;
        let Some(kind) = value.get("type").and_then(|v| v.as_str()) else {
            continue;
        };
        if kind != "item.completed" {
            continue;
        }
        counts.tool_calls = counts.tool_calls.saturating_add(1);
        let Some(item) = value.get("item") else {
            continue;
        };
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

        // Frontier models edit via structured tools (`file_change`,
        // `apply_patch` via custom_tool_call), NOT shell commands. Without
        // this branch the cost-table edit column reads `e=0` even though
        // multiple files changed. Count them as edits but NOT as shell
        // commands — shell_commands is reserved for command_execution.
        if item_type == "file_change" {
            counts.edit_commands = counts.edit_commands.saturating_add(1);
            continue;
        }

        if item_type != "command_execution" {
            continue;
        }
        counts.shell_commands = counts.shell_commands.saturating_add(1);
        let raw = item.get("command").and_then(|v| v.as_str()).unwrap_or("");
        // Derive ALL shell phase counts (including file_reads) from
        // `classify_shell_phase` so they agree on `cd $X && <real_cmd>`
        // and other chained shapes. Doing the read-check separately via
        // `first_executable_token` was misclassifying every chained
        // `cd && cat ...` as a non-read.
        match classify_shell_phase(raw) {
            "discover" => counts.discover_commands = counts.discover_commands.saturating_add(1),
            "read" => counts.file_reads = counts.file_reads.saturating_add(1),
            "edit" => counts.edit_commands = counts.edit_commands.saturating_add(1),
            "verify" => counts.verify_commands = counts.verify_commands.saturating_add(1),
            // "other" — implicit; equals shell - (d + r + e + v).
            _ => {}
        }
    }
    Ok(counts)
}

fn first_executable_token(cmd: &str) -> &str {
    let trimmed = cmd.trim();
    // Strip the `/bin/zsh -lc '...'` / `bash -c "..."` shell-wrapping if
    // present so we see the actual user command.
    let inner = if let Some(rest) = trimmed.strip_prefix("/bin/zsh -lc ") {
        rest.trim_start()
    } else if let Some(rest) = trimmed.strip_prefix("/bin/bash -c ") {
        rest.trim_start()
    } else if let Some(rest) = trimmed.strip_prefix("zsh -lc ") {
        rest.trim_start()
    } else if let Some(rest) = trimmed.strip_prefix("bash -c ") {
        rest.trim_start()
    } else {
        trimmed
    };
    let unquoted = inner.trim_start_matches(['\'', '"']);
    unquoted.split_ascii_whitespace().next().unwrap_or("")
}

/// Skip every leading `cd <something> && ` segment. Agents commonly prefix
/// every shell command with `cd $WORKTREE && <real_cmd>`; classifying by
/// the leading `cd` mislabels everything as "other". Returns a slice
/// starting at the first non-`cd` real command.
fn strip_leading_cd_chains(command: &str) -> String {
    // Unpeel any shell wrapper first so we see the actual command body.
    let trimmed = command.trim();
    let body = if let Some(rest) = trimmed.strip_prefix("/bin/zsh -lc ") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("/bin/bash -c ") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("zsh -lc ") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("bash -c ") {
        rest
    } else {
        trimmed
    };
    let body = body
        .trim_start_matches(['\'', '"'])
        .trim_end_matches(['\'', '"']);

    let mut remaining = body.to_string();
    loop {
        let lead = remaining.trim_start();
        if !lead.starts_with("cd ") {
            break;
        }
        let Some(idx) = lead.find("&&") else { break };
        remaining = lead[idx + 2..].trim_start().to_string();
    }
    remaining
}

/// Returns true if the lowercased command contains `-A<digit>`, `-B<digit>`,
/// or `-C<digit>` (the joined-flag form of grep context options). Used by
/// the verify-phase classifier when no space-separated flag is found.
fn has_joined_context_flag(lc: &str) -> bool {
    let bytes = lc.as_bytes();
    for i in 0..bytes.len().saturating_sub(3) {
        // Look for " -X<d>" with X in {a,b,c} and d a digit.
        if bytes[i] == b' ' && bytes[i + 1] == b'-' {
            let flag = bytes[i + 2];
            let digit = bytes[i + 3];
            if matches!(flag, b'a' | b'b' | b'c') && digit.is_ascii_digit() {
                return true;
            }
        }
    }
    false
}

/// Parse changed paths from `git diff --name-only` output.
pub fn changed_files_from_git_diff(diff_output: &str) -> Vec<String> {
    diff_output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn agent_labels_from_task(task: &AgentEvalTask) -> crate::metrics::EvalLabels {
    crate::metrics::EvalLabels {
        relevant_files: task.relevant_files.clone(),
        relevant_tests: task.relevant_tests.clone(),
        bridge_files: Vec::new(),
    }
}

impl AgentEvalTask {
    pub fn from_packet_fixture(fixture: &EvalTaskFixture) -> Self {
        Self {
            id: String::new(),
            task: fixture.task.clone(),
            relevant_files: fixture.relevant_files.clone(),
            relevant_tests: fixture.relevant_tests.clone(),
            danger_zones: fixture.danger_zones.clone(),
            verify_command: None,
            bridge_files: fixture.bridge_files.clone(),
            workdir: AgentEvalWorkdir::Calculator,
            category: None,
            verification_required: None,
        }
    }
}

/// Render the eval report as two grouped tables:
///
/// **Main** (one row per task — the "did RI help" answer):
///   `Task | Valid? | RI visible? | Target files V/RI | Extra files V/RI |
///    Turns V/RI | Time V/RI | Result`
///
/// **Cost** (one row per task — the "at what price" answer):
///   `Task | Tokens V/RI | Token Δ | Tool calls V/RI | Shell V/RI |
///    File reads V/RI`
///
/// Both tables group rows by [`TaskCategory`] with `== <category> ==`
/// headers; the Main table adds a per-group `Group: <category> — N
/// ri_better / ...` summary line. Column widths within each table are
/// computed once across all rows so columns line up vertically across
/// groups; widths are independent across the two tables. Missing
/// dimensions render as `—`. Excluded pairs render `—` across every
/// data column.
pub fn render_agent_eval_human(report: &AgentEvalReport) -> String {
    const MAIN_HEADERS: [&str; 9] = [
        "Task",
        "Valid?",
        "RI visible?",
        "Edit targets V/RI",
        "Orient. touched V/RI",
        "Extra files V/RI",
        "Turns V/RI",
        "Time V/RI",
        "Result",
    ];
    const COST_HEADERS: [&str; 8] = [
        "Task",
        "Tokens V/RI",
        "Token Δ",
        "Tool calls V/RI",
        "Discover V/RI",
        "Read V/RI",
        "Edit V/RI",
        "Verify V/RI",
    ];

    let comparisons: Vec<(Option<TaskCategory>, &AgentEvalComparison)> = report
        .comparisons
        .iter()
        .map(|row| (row.category, row))
        .collect();

    // Sort once; both tables walk the same ordering. None category goes last.
    let category_order: Vec<Option<TaskCategory>> = {
        let mut seen: Vec<Option<TaskCategory>> = Vec::new();
        for (cat, _) in &comparisons {
            if !seen.contains(cat) {
                seen.push(*cat);
            }
        }
        seen.sort_by_key(|cat| match cat {
            Some(c) => (0, c.slug()),
            None => (1, ""),
        });
        seen
    };

    let mut lines = Vec::new();
    lines.push(format!(
        "Valid comparisons: {}/{}",
        report.summary.valid_pairs, report.summary.total_pairs
    ));
    if !report.summary.invalid_reason_counts.is_empty() {
        let reasons: Vec<String> = report
            .summary
            .invalid_reason_counts
            .iter()
            .map(|(reason, count)| format!("{reason}={count}"))
            .collect();
        lines.push(format!("Invalid reasons: {}", reasons.join(", ")));
    }

    // ----- Main table -----
    let main_rows: Vec<(Option<TaskCategory>, &AgentEvalComparison, Vec<String>)> = comparisons
        .iter()
        .map(|(cat, row)| (*cat, *row, format_main_row(row).to_vec()))
        .collect();
    lines.push(String::new());
    lines.push("==== Main ====".to_string());
    render_grouped_table(
        &MAIN_HEADERS,
        &main_rows,
        &category_order,
        /* include_group_summary */ true,
        &mut lines,
    );

    // ----- Cost table -----
    let cost_rows: Vec<(Option<TaskCategory>, &AgentEvalComparison, Vec<String>)> = comparisons
        .iter()
        .map(|(cat, row)| (*cat, *row, format_cost_row(row).to_vec()))
        .collect();
    lines.push(String::new());
    lines.push("==== Cost ====".to_string());
    render_grouped_table(
        &COST_HEADERS,
        &cost_rows,
        &category_order,
        /* include_group_summary */ false,
        &mut lines,
    );

    lines.join("\n")
}

/// Render one grouped table into `out`. Column widths are computed across
/// every row (so headers line up across category groups within the table)
/// but independent of any other table. When `include_group_summary` is
/// true, append a `Group: <category> — N ri_better / ...` line under each
/// category's rows.
fn render_grouped_table(
    headers: &[&str],
    rows: &[(Option<TaskCategory>, &AgentEvalComparison, Vec<String>)],
    category_order: &[Option<TaskCategory>],
    include_group_summary: bool,
    out: &mut Vec<String>,
) {
    let n = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for (_, _, cells) in rows {
        for (i, cell) in cells.iter().enumerate() {
            if i < n {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
    }
    let render = |cells: &[String]| -> String {
        cells
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let pad = widths.get(i).copied().unwrap_or(0);
                let cell_len = cell.chars().count();
                let extra = pad.saturating_sub(cell_len);
                format!("{cell}{}", " ".repeat(extra))
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let header_cells: Vec<String> = headers.iter().map(std::string::ToString::to_string).collect();
    let separator_cells: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();

    for category in category_order {
        let group: Vec<&(Option<TaskCategory>, &AgentEvalComparison, Vec<String>)> =
            rows.iter().filter(|(c, _, _)| c == category).collect();
        if group.is_empty() {
            continue;
        }
        let category_label = category.map_or("uncategorized", TaskCategory::slug);
        out.push(String::new());
        out.push(format!("== {category_label} =="));
        out.push(render(&header_cells));
        out.push(render(&separator_cells));
        for (_, _, cells) in &group {
            out.push(render(cells));
        }
        if include_group_summary {
            out.push(format!(
                "Group: {category_label} — {}",
                group_summary(group.iter().map(|(_, row, _)| *row)),
            ));
        }
    }
}

/// Count `result` outcomes across a slice of comparisons and emit a one-line
/// summary like `2 ri_better / 0 ri_worse / 1 tie / 0 excluded`.
fn group_summary<'a>(rows: impl IntoIterator<Item = &'a AgentEvalComparison>) -> String {
    let mut better = 0u32;
    let mut worse = 0u32;
    let mut tie = 0u32;
    let mut excluded = 0u32;
    for row in rows {
        match row.result {
            AgentEvalResult::RiBetter { .. } => better += 1,
            AgentEvalResult::RiWorse { .. } => worse += 1,
            AgentEvalResult::Tie => tie += 1,
            AgentEvalResult::Excluded { .. } => excluded += 1,
        }
    }
    format!("{better} ri_better / {worse} ri_worse / {tie} tie / {excluded} excluded")
}

const DASH: &str = "—";

fn format_main_row(row: &AgentEvalComparison) -> [String; 9] {
    let result = row.result.slug();
    if !row.valid_for_comparison {
        let valid_cell = format_invalid_cell(row);
        let ri_visible = if row.treatment.harness_context_visible {
            "yes".to_string()
        } else {
            DASH.to_string()
        };
        return [
            row.task_id.clone(),
            valid_cell,
            ri_visible,
            DASH.to_string(),
            DASH.to_string(),
            DASH.to_string(),
            DASH.to_string(),
            DASH.to_string(),
            result,
        ];
    }

    // Edit targets: gold-only hit/total. Stricter than the old
    // `target_files_hit` column, which lumped bridge in with gold.
    let edit_targets = format!(
        "{} vs {}",
        ratio_cell(
            row.vanilla.edit_target_files_hit,
            row.vanilla.edit_target_files_total,
        ),
        ratio_cell(
            row.treatment.edit_target_files_hit,
            row.treatment.edit_target_files_total,
        ),
    );
    // Orientation touched: count of files in (bridge ∪ ri_surfaced
    // orientation − gold) that were edited. Per-arm, since the RI arm
    // may have a strictly larger orientation set.
    let orientation = format!(
        "{}/{}",
        row.vanilla.orientation_files_touched, row.treatment.orientation_files_touched,
    );
    let extra = format!(
        "{}/{}",
        row.vanilla.unnecessary_files_changed.len(),
        row.treatment.unnecessary_files_changed.len(),
    );
    let turns = format!(
        "{}/{}",
        option_cell(row.vanilla.turn_count),
        option_cell(row.treatment.turn_count),
    );
    let time = format!(
        "{}/{}",
        duration_cell(row.vanilla.duration_ms),
        duration_cell(row.treatment.duration_ms),
    );
    let ri_visible = if row.treatment.harness_context_visible {
        "yes".to_string()
    } else {
        "no".to_string()
    };

    [
        row.task_id.clone(),
        format_valid_cell(row),
        ri_visible,
        edit_targets,
        orientation,
        extra,
        turns,
        time,
        result,
    ]
}

/// Render the Main table's Valid? cell for a comparison row that
/// classified as `valid_for_comparison`. When either arm carries
/// `warnings` (e.g. `provider_network_error_recovered`) we surface them
/// inline so reviewers can tell a clean turn from one that auto-reconnected
/// mid-stream. Format: `valid` for clean pairs, `valid (warning1; warning2)`
/// otherwise. Arm prefix `V:` / `RI:` only when warnings differ between arms.
fn format_valid_cell(row: &AgentEvalComparison) -> String {
    let v = &row.vanilla.warnings;
    let t = &row.treatment.warnings;
    if v.is_empty() && t.is_empty() {
        return "valid".to_string();
    }
    if v == t {
        return format!("valid ({})", v.join("; "));
    }
    let mut parts: Vec<String> = Vec::new();
    if !v.is_empty() {
        parts.push(format!("V: {}", v.join("; ")));
    }
    if !t.is_empty() {
        parts.push(format!("RI: {}", t.join("; ")));
    }
    format!("valid ({})", parts.join(" / "))
}

fn format_cost_row(row: &AgentEvalComparison) -> [String; 8] {
    if !row.valid_for_comparison {
        return [
            row.task_id.clone(),
            DASH.to_string(),
            DASH.to_string(),
            DASH.to_string(),
            DASH.to_string(),
            DASH.to_string(),
            DASH.to_string(),
            DASH.to_string(),
        ];
    }

    let tokens = format!(
        "{}/{}",
        option_cell(row.vanilla.tokens_total),
        option_cell(row.treatment.tokens_total),
    );
    // Token Δ = treatment - vanilla (signed). Positive means RI cost MORE
    // tokens than vanilla, negative means RI saved tokens. Missing on
    // either side → dash.
    let token_delta = match (row.vanilla.tokens_total, row.treatment.tokens_total) {
        (Some(v), Some(t)) => {
            let v = v as i128;
            let t = t as i128;
            let delta = t - v;
            if delta > 0 {
                format!("+{delta}")
            } else {
                delta.to_string()
            }
        }
        _ => DASH.to_string(),
    };
    let tool_calls = format!(
        "{}/{}",
        option_cell(row.vanilla.tool_call_count),
        option_cell(row.treatment.tool_call_count),
    );
    let discover = format!(
        "{}/{}",
        option_cell(row.vanilla.discover_command_count),
        option_cell(row.treatment.discover_command_count),
    );
    let reads = format!(
        "{}/{}",
        option_cell(row.vanilla.file_read_count),
        option_cell(row.treatment.file_read_count),
    );
    let edits = format!(
        "{}/{}",
        option_cell(row.vanilla.edit_command_count),
        option_cell(row.treatment.edit_command_count),
    );
    let verifies = format!(
        "{}/{}",
        option_cell(row.vanilla.verify_command_count),
        option_cell(row.treatment.verify_command_count),
    );
    [
        row.task_id.clone(),
        tokens,
        token_delta,
        tool_calls,
        discover,
        reads,
        edits,
        verifies,
    ]
}

/// Format a `duration_ms` value as a compact human cell — seconds if
/// under 100s, minute+second mm:ss otherwise. `—` for None.
fn duration_cell(value: Option<u64>) -> String {
    match value {
        None => DASH.to_string(),
        Some(ms) => {
            let seconds = ms / 1000;
            if seconds < 100 {
                let frac = (ms % 1000) / 100;
                format!("{seconds}.{frac}s")
            } else {
                let minutes = seconds / 60;
                let remainder = seconds % 60;
                format!("{minutes}m{remainder:02}s")
            }
        }
    }
}

fn format_invalid_cell(row: &AgentEvalComparison) -> String {
    let v = row.vanilla.invalid_reason.map(invalid_reason_slug);
    let t = row.treatment.invalid_reason.map(invalid_reason_slug);
    match (v, t) {
        (Some(v), Some(t)) if v == t => format!("invalid: {v}"),
        (Some(v), Some(t)) => format!("invalid: vanilla {v}, RI {t}"),
        (Some(v), None) => format!("invalid: vanilla {v}"),
        (None, Some(t)) => format!("invalid: RI {t}"),
        (None, None) => "invalid".to_string(),
    }
}

fn ratio_cell(hit: usize, total: usize) -> String {
    if total == 0 {
        DASH.to_string()
    } else {
        format!("{hit}/{total}")
    }
}

fn option_cell<T: std::fmt::Display>(value: Option<T>) -> String {
    match value {
        Some(v) => v.to_string(),
        None => DASH.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn calculator_task() -> AgentEvalTask {
        AgentEvalTask {
            id: "calculator_fix".to_string(),
            task: "Fix the failing calculator test.".to_string(),
            relevant_files: vec!["src/calculator.py".to_string()],
            relevant_tests: vec!["tests/test_calculator.py".to_string()],
            danger_zones: Vec::new(),
            verify_command: Some("python -m pytest tests/test_calculator.py".to_string()),
            bridge_files: Vec::new(),
            workdir: AgentEvalWorkdir::Calculator,
            category: None,
            verification_required: None,
        }
    }

    fn synthetic_record(arm: AgentArm, task_id: &str) -> AgentRunRecord {
        AgentRunRecord {
            arm,
            task_id: task_id.to_string(),
            changed_files: Vec::new(),
            tests_passed: false,
            turn_count: None,
            exec_exit_code: None,
            repo_intelligence_enabled: matches!(arm, AgentArm::RepoIntelligence),
            harness_context_visible: false,
            run_valid: true,
            invalid_reason: None,
            tokens_input: None,
            tokens_output: None,
            tokens_total: None,
            duration_ms: None,
            tool_call_count: None,
            shell_command_count: None,
            file_read_count: None,
            discover_command_count: None,
            edit_command_count: None,
            verify_command_count: None,
            warnings: Vec::new(),
            ri_surfaced_edit_targets: Vec::new(),
            ri_surfaced_orientation: Vec::new(),
            intent_changed_files: Vec::new(),
            diff_changed_files: Vec::new(),
            formatter_changed_files: Vec::new(),
            harness_prewarm_ms: None,
            codex_build_profile: None,
            search_proxy_enabled: matches!(arm, AgentArm::SearchProxy),
            search_proxy_substitutions: 0,
            search_proxy_escape_hatch_repeats: 0,
            search_proxy_build_pass_throughs: 0,
            search_proxy_compact_bytes: 0,
            search_proxy_raw_bytes_estimated: 0,
            search_proxy_top_files: Vec::new(),
            worktree_isolated: false,
            base_ref: None,
            worktree_path: None,
        }
    }

    #[test]
    fn scores_correct_fix() {
        let task = calculator_task();
        let record = AgentRunRecord {
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(2),
            exec_exit_code: Some(0),
            tokens_input: Some(100),
            tokens_output: Some(50),
            tokens_total: Some(150),
            duration_ms: Some(42_000),
            tool_call_count: Some(3),
            shell_command_count: Some(1),
            file_read_count: Some(0),
            ..synthetic_record(AgentArm::Harness, &task.id)
        };
        let score = score_run(&record, &task);
        assert_eq!(
            score,
            AgentRunScore {
                correct_file_touched: true,
                target_files_hit: 1,
                target_files_total: 1,
                tests_passed: true,
                turn_count: Some(2),
                unnecessary_files_changed: Vec::new(),
                harness_context_visible: false,
                bridge_files_touched: Vec::new(),
                run_valid: true,
                invalid_reason: None,
                tokens_input: Some(100),
                tokens_output: Some(50),
                tokens_total: Some(150),
                duration_ms: Some(42_000),
                tool_call_count: Some(3),
                shell_command_count: Some(1),
                file_read_count: Some(0),
                discover_command_count: None,
                edit_command_count: None,
                verify_command_count: None,
                warnings: Vec::new(),
                edit_target_files_hit: 1,
                edit_target_files_total: 1,
                orientation_files_touched: 0,
                orientation_files_total: 0,
            }
        );
    }

    #[test]
    fn ignores_python_cache_artifacts_in_scoring() {
        let task = calculator_task();
        let record = AgentRunRecord {
            changed_files: vec![
                "src/__pycache__/calculator.cpython-313.pyc".to_string(),
                "tests/__pycache__/test_calculator.cpython-313-pytest-9.0.0.pyc".to_string(),
                ".pytest_cache/v/cache/nodeids".to_string(),
            ],
            tests_passed: false,
            turn_count: Some(1),
            ..synthetic_record(AgentArm::Harness, &task.id)
        };
        let score = score_run(&record, &task);
        assert!(!score.correct_file_touched);
        assert_eq!(score.unnecessary_files_changed, Vec::<String>::new());
    }

    #[test]
    fn scores_unnecessary_files_and_no_touch() {
        let task = calculator_task();
        let record = AgentRunRecord {
            changed_files: vec!["README.md".to_string()],
            turn_count: Some(5),
            exec_exit_code: Some(1),
            ..synthetic_record(AgentArm::Vanilla, &task.id)
        };
        let score = score_run(&record, &task);
        assert!(!score.correct_file_touched);
        assert_eq!(
            score.unnecessary_files_changed,
            vec!["README.md".to_string()]
        );
        assert_eq!(score.target_files_hit, 0);
        assert_eq!(score.target_files_total, 1);
    }

    #[test]
    fn counts_turns_from_jsonl() {
        let jsonl = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"turn.started"}
{"type":"turn.completed","usage":{}}
{"type":"turn.started"}
{"type":"turn.failed","error":{"message":"x"}}"#;
        assert_eq!(count_turns_from_exec_jsonl(jsonl.as_bytes()).unwrap(), 2);
    }

    #[test]
    fn token_usage_sums_input_and_output_across_turns() {
        let jsonl = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"turn.completed","usage":{"input_tokens":1200,"cached_input_tokens":200,"output_tokens":300,"reasoning_output_tokens":40}}
{"type":"turn.completed","usage":{"input_tokens":150,"output_tokens":50}}"#;
        let totals = token_usage_from_exec_jsonl(jsonl.as_bytes())
            .unwrap()
            .expect("expected token totals when turn.completed events are present");
        assert_eq!(totals.input_tokens, 1350);
        assert_eq!(totals.output_tokens, 350);
        assert_eq!(totals.total_tokens, 1700);
    }

    #[test]
    fn token_usage_is_none_when_no_turn_completed_event() {
        let jsonl = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"turn.started"}
{"type":"turn.failed","error":{"message":"provider unavailable"}}"#;
        assert_eq!(
            token_usage_from_exec_jsonl(jsonl.as_bytes()).unwrap(),
            None,
            "missing turn.completed must surface as None, not Some(0)"
        );
    }

    #[test]
    fn repo_intelligence_arm_round_trips() {
        let json = r#"{"arm":"repo_intelligence","task_id":"t","changed_files":[],"tests_passed":false,"turn_count":null,"exec_exit_code":null,"harness_context_visible":true}"#;
        let record: AgentRunRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.arm, AgentArm::RepoIntelligence);
        assert!(record.harness_context_visible);
        assert!(record.run_valid);
        assert_eq!(record.invalid_reason, None);
        assert_eq!(record.tokens_total, None);
        // Pre-isolation artifacts must keep loading (serde(default)).
        assert!(!record.worktree_isolated);
        assert_eq!(record.base_ref, None);
        assert_eq!(record.worktree_path, None);
    }

    #[test]
    fn isolated_worktree_metadata_round_trips() {
        let json = r#"{"arm":"repo_intelligence","task_id":"t","changed_files":[],"tests_passed":false,"turn_count":null,"exec_exit_code":null,"harness_context_visible":true,"worktree_isolated":true,"base_ref":"abc123","worktree_path":"/tmp/codex-arm-XXXX/codex-rs"}"#;
        let record: AgentRunRecord = serde_json::from_str(json).unwrap();
        assert!(record.worktree_isolated);
        assert_eq!(record.base_ref.as_deref(), Some("abc123"));
        assert_eq!(
            record.worktree_path.as_deref(),
            Some("/tmp/codex-arm-XXXX/codex-rs")
        );
    }

    #[test]
    fn normalize_strips_codex_rs_prefix() {
        assert_eq!(
            normalize_agent_eval_path("codex-rs/cli/src/context_cmd.rs"),
            "cli/src/context_cmd.rs"
        );
        assert_eq!(
            normalize_agent_eval_path("./cli/src/context_cmd.rs"),
            "cli/src/context_cmd.rs"
        );
        assert_eq!(
            normalize_agent_eval_path("/Users/me/codex/codex-rs/cli/src/context_cmd.rs"),
            "cli/src/context_cmd.rs"
        );
    }

    #[test]
    fn scores_codex_rs_prefixed_changed_paths_against_fixture_gold() {
        let task = AgentEvalTask {
            id: "path_norm".to_string(),
            task: "touch context cmd".to_string(),
            relevant_files: vec!["cli/src/context_cmd.rs".to_string()],
            relevant_tests: Vec::new(),
            danger_zones: Vec::new(),
            verify_command: None,
            bridge_files: Vec::new(),
            workdir: AgentEvalWorkdir::CodexRs,
            category: None,
            verification_required: None,
        };
        let record = AgentRunRecord {
            changed_files: vec!["codex-rs/cli/src/context_cmd.rs".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            exec_exit_code: Some(0),
            ..synthetic_record(AgentArm::Vanilla, &task.id)
        };
        let score = score_run(&record, &task);
        assert!(score.correct_file_touched);
        assert_eq!(score.unnecessary_files_changed, Vec::<String>::new());
    }

    #[test]
    fn scores_codex_rs_prefixed_bridge_paths() {
        let task = AgentEvalTask {
            id: "bridge_norm".to_string(),
            task: "touch bridge".to_string(),
            relevant_files: vec!["other/src/lib.rs".to_string()],
            relevant_tests: Vec::new(),
            danger_zones: Vec::new(),
            verify_command: None,
            bridge_files: vec!["cli/src/main.rs".to_string()],
            workdir: AgentEvalWorkdir::CodexRs,
            category: None,
            verification_required: None,
        };
        let record = AgentRunRecord {
            changed_files: vec!["codex-rs/cli/src/main.rs".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            exec_exit_code: Some(0),
            harness_context_visible: true,
            ..synthetic_record(AgentArm::RepoIntelligence, &task.id)
        };
        let score = score_run(&record, &task);
        assert_eq!(
            score.bridge_files_touched,
            vec!["cli/src/main.rs".to_string()]
        );
        // bridge files aren't gold; they still count as unnecessary against gold targets.
        assert_eq!(
            score.unnecessary_files_changed,
            vec!["cli/src/main.rs".to_string()]
        );
        // But they DO count toward target_files_hit (gold ∪ bridge).
        assert_eq!(score.target_files_hit, 1);
        assert_eq!(score.target_files_total, 2);
    }

    #[test]
    fn invalid_pairs_are_excluded_from_behavioral_comparison() {
        let task = calculator_task();
        let vanilla = AgentRunRecord {
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            run_valid: false,
            invalid_reason: Some(AgentRunInvalidReason::ProviderUsageLimit),
            ..synthetic_record(AgentArm::Vanilla, &task.id)
        };
        let treatment = AgentRunRecord {
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            harness_context_visible: true,
            ..synthetic_record(AgentArm::RepoIntelligence, &task.id)
        };
        let row = compare_task(&task, &vanilla, &treatment);
        assert!(!row.valid_for_comparison);
        assert_eq!(
            row.excluded_reason.as_deref(),
            Some("pair_invalid:provider_usage_limit|invalid")
        );
        // tests_passed is ignored for invalid runs
        assert!(!row.vanilla.tests_passed);
        // Excluded results carry the pair_invalid reason verbatim.
        assert_eq!(
            row.result.slug(),
            "excluded:pair_invalid:provider_usage_limit|invalid"
        );
    }

    fn classify(vanilla: AgentRunScore, treatment: AgentRunScore) -> AgentEvalResult {
        classify_result(&vanilla, &treatment, true, None)
    }

    fn zero_score() -> AgentRunScore {
        AgentRunScore {
            correct_file_touched: false,
            target_files_hit: 0,
            target_files_total: 2,
            tests_passed: false,
            turn_count: None,
            unnecessary_files_changed: Vec::new(),
            harness_context_visible: false,
            bridge_files_touched: Vec::new(),
            run_valid: true,
            invalid_reason: None,
            tokens_input: None,
            tokens_output: None,
            tokens_total: None,
            duration_ms: None,
            tool_call_count: None,
            shell_command_count: None,
            file_read_count: None,
            discover_command_count: None,
            edit_command_count: None,
            verify_command_count: None,
            warnings: Vec::new(),
            edit_target_files_hit: 0,
            edit_target_files_total: 0,
            orientation_files_touched: 0,
            orientation_files_total: 0,
        }
    }

    #[test]
    fn classify_ri_better_by_file_targeting() {
        let vanilla = zero_score();
        let treatment = AgentRunScore {
            target_files_hit: 2,
            ..zero_score()
        };
        assert_eq!(
            classify(vanilla, treatment),
            AgentEvalResult::RiBetter {
                reason: ResultReason::FileTargeting,
            }
        );
    }

    #[test]
    fn classify_ri_better_by_fewer_extra_files_when_targets_tie() {
        let vanilla = AgentRunScore {
            target_files_hit: 2,
            unnecessary_files_changed: vec!["a".into(), "b".into(), "c".into()],
            ..zero_score()
        };
        let treatment = AgentRunScore {
            target_files_hit: 2,
            unnecessary_files_changed: vec!["a".into()],
            ..zero_score()
        };
        assert_eq!(
            classify(vanilla, treatment),
            AgentEvalResult::RiBetter {
                reason: ResultReason::FewerExtraFiles,
            }
        );
    }

    #[test]
    fn classify_ri_better_by_fewer_turns_when_targets_and_extras_tie() {
        let vanilla = AgentRunScore {
            target_files_hit: 2,
            turn_count: Some(8),
            ..zero_score()
        };
        let treatment = AgentRunScore {
            target_files_hit: 2,
            turn_count: Some(3),
            ..zero_score()
        };
        assert_eq!(
            classify(vanilla, treatment),
            AgentEvalResult::RiBetter {
                reason: ResultReason::FewerTurns,
            }
        );
    }

    #[test]
    fn classify_ri_better_by_fewer_tokens_only_after_other_tiebreakers_tie() {
        let vanilla = AgentRunScore {
            target_files_hit: 2,
            turn_count: Some(4),
            tokens_total: Some(2000),
            ..zero_score()
        };
        let treatment = AgentRunScore {
            target_files_hit: 2,
            turn_count: Some(4),
            tokens_total: Some(1500),
            ..zero_score()
        };
        assert_eq!(
            classify(vanilla, treatment),
            AgentEvalResult::RiBetter {
                reason: ResultReason::FewerTokens,
            }
        );
    }

    #[test]
    fn classify_tokens_never_override_correctness() {
        // RI is cheaper but wrong; result must be ri_worse:file_targeting, not ri_better.
        let vanilla = AgentRunScore {
            target_files_hit: 2,
            tokens_total: Some(5000),
            ..zero_score()
        };
        let treatment = AgentRunScore {
            target_files_hit: 0,
            tokens_total: Some(500),
            ..zero_score()
        };
        assert_eq!(
            classify(vanilla, treatment),
            AgentEvalResult::RiWorse {
                reason: ResultReason::FileTargeting,
            }
        );
    }

    #[test]
    fn classify_tie_when_all_dimensions_match() {
        let vanilla = AgentRunScore {
            target_files_hit: 2,
            turn_count: Some(3),
            tokens_total: Some(1000),
            ..zero_score()
        };
        let treatment = vanilla.clone();
        assert_eq!(classify(vanilla, treatment), AgentEvalResult::Tie);
    }

    #[test]
    fn render_human_emits_dash_for_missing_tokens() {
        let task = calculator_task();
        let vanilla = AgentRunRecord {
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: false,
            turn_count: Some(4),
            ..synthetic_record(AgentArm::Vanilla, &task.id)
        };
        let treatment = AgentRunRecord {
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(2),
            harness_context_visible: true,
            ..synthetic_record(AgentArm::RepoIntelligence, &task.id)
        };
        let report = build_report(vec![compare_task(&task, &vanilla, &treatment)]);
        let text = render_agent_eval_human(&report);
        assert!(
            text.contains("—/—"),
            "expected token cell to render as `—/—` when both arms lack usage data\n{text}"
        );
        assert!(text.contains("ri_better:fewer_turns"), "{text}");
    }

    #[test]
    fn task_category_serializes_as_snake_case() {
        let cat = TaskCategory::CrossModuleOwnership;
        assert_eq!(
            serde_json::to_string(&cat).unwrap(),
            "\"cross_module_ownership\""
        );
        let back: TaskCategory = serde_json::from_str("\"bridge_wiring\"").unwrap();
        assert_eq!(back, TaskCategory::BridgeWiring);
    }

    #[test]
    fn category_flows_through_compare_task() {
        let mut task = calculator_task();
        task.category = Some(TaskCategory::FileRouting);
        let vanilla = synthetic_record(AgentArm::Vanilla, &task.id);
        let treatment = synthetic_record(AgentArm::RepoIntelligence, &task.id);
        let row = compare_task(&task, &vanilla, &treatment);
        assert_eq!(row.category, Some(TaskCategory::FileRouting));
    }

    #[test]
    fn render_human_groups_by_category_with_summary_lines() {
        let mut t_routing = calculator_task();
        t_routing.id = "fr_1".to_string();
        t_routing.category = Some(TaskCategory::FileRouting);
        let mut t_bridge = calculator_task();
        t_bridge.id = "bw_1".to_string();
        t_bridge.category = Some(TaskCategory::BridgeWiring);
        let mut t_uncat = calculator_task();
        t_uncat.id = "uc_1".to_string();
        t_uncat.category = None;

        // For fr_1: RI hits gold, vanilla doesn't → ri_better:file_targeting.
        let mk_records = |task: &AgentEvalTask, ri_hits: bool| {
            let vanilla = AgentRunRecord {
                changed_files: vec!["README.md".to_string()],
                tests_passed: false,
                turn_count: Some(4),
                ..synthetic_record(AgentArm::Vanilla, &task.id)
            };
            let treatment = AgentRunRecord {
                changed_files: if ri_hits {
                    vec!["src/calculator.py".to_string()]
                } else {
                    vec!["README.md".to_string()]
                },
                tests_passed: ri_hits,
                turn_count: Some(2),
                harness_context_visible: true,
                ..synthetic_record(AgentArm::RepoIntelligence, &task.id)
            };
            (vanilla, treatment)
        };

        let (v1, t1) = mk_records(&t_routing, true); // ri_better
        let (v2, t2) = mk_records(&t_bridge, false); // ri_worse (both miss but vanilla touches same wrong file; classifier picks Tie or another path)
        let (v3, t3) = mk_records(&t_uncat, true); // ri_better

        let report = build_report(vec![
            compare_task(&t_routing, &v1, &t1),
            compare_task(&t_bridge, &v2, &t2),
            compare_task(&t_uncat, &v3, &t3),
        ]);
        let text = render_agent_eval_human(&report);

        // Each category gets a header.
        assert!(
            text.contains("== bridge_wiring =="),
            "missing bw header:\n{text}"
        );
        assert!(
            text.contains("== file_routing =="),
            "missing fr header:\n{text}"
        );
        assert!(
            text.contains("== uncategorized =="),
            "missing uncat header:\n{text}"
        );

        // Each group has a per-group summary line.
        let group_summaries: Vec<&str> =
            text.lines().filter(|l| l.starts_with("Group: ")).collect();
        assert_eq!(
            group_summaries.len(),
            3,
            "expected exactly 3 group summaries, got {}:\n{text}",
            group_summaries.len()
        );

        // None group renders last.
        let bw_idx = text.find("== bridge_wiring ==").unwrap();
        let fr_idx = text.find("== file_routing ==").unwrap();
        let uc_idx = text.find("== uncategorized ==").unwrap();
        assert!(bw_idx < fr_idx, "categories should be alphabetic; bw < fr");
        assert!(fr_idx < uc_idx, "uncategorized must come last");

        // Column widths shared within each table across category groups.
        // Two tables × 3 categories = 6 "Task " header lines, with three
        // identical Main headers and three identical Cost headers (each
        // table has its own width set).
        let header_lines: Vec<&str> = text.lines().filter(|l| l.starts_with("Task ")).collect();
        assert_eq!(
            header_lines.len(),
            6,
            "expected 6 'Task ' headers (3 Main + 3 Cost), got {}:\n{text}",
            header_lines.len()
        );
        // Both tables must be present.
        assert!(
            text.contains("==== Main ===="),
            "Main section missing:\n{text}"
        );
        assert!(
            text.contains("==== Cost ===="),
            "Cost section missing:\n{text}"
        );
    }

    #[test]
    fn classify_ri_better_by_faster_wall_clock_only_after_turns_tie() {
        // Targets / extras / turns all tie; RI is faster by wall clock.
        let vanilla = AgentRunScore {
            target_files_hit: 2,
            turn_count: Some(3),
            duration_ms: Some(90_000),
            ..zero_score()
        };
        let treatment = AgentRunScore {
            target_files_hit: 2,
            turn_count: Some(3),
            duration_ms: Some(45_000),
            ..zero_score()
        };
        assert_eq!(
            classify(vanilla, treatment),
            AgentEvalResult::RiBetter {
                reason: ResultReason::FasterWallClock,
            }
        );
    }

    #[test]
    fn classify_time_sits_above_tokens_in_priority_order() {
        // RI is slower BUT cheaper. Time has higher priority than tokens,
        // so the verdict must be ri_worse:faster_wall_clock (slower), NOT
        // ri_better:fewer_tokens.
        let vanilla = AgentRunScore {
            target_files_hit: 2,
            turn_count: Some(3),
            duration_ms: Some(40_000),
            tokens_total: Some(5000),
            ..zero_score()
        };
        let treatment = AgentRunScore {
            target_files_hit: 2,
            turn_count: Some(3),
            duration_ms: Some(80_000),
            tokens_total: Some(3000),
            ..zero_score()
        };
        assert_eq!(
            classify(vanilla, treatment),
            AgentEvalResult::RiWorse {
                reason: ResultReason::FasterWallClock,
            }
        );
    }

    #[test]
    fn classify_skips_time_tier_when_either_duration_missing() {
        // Only vanilla has duration; classifier must skip to tokens.
        let vanilla = AgentRunScore {
            target_files_hit: 2,
            turn_count: Some(3),
            duration_ms: Some(40_000),
            tokens_total: Some(5000),
            ..zero_score()
        };
        let treatment = AgentRunScore {
            target_files_hit: 2,
            turn_count: Some(3),
            duration_ms: None,
            tokens_total: Some(3000),
            ..zero_score()
        };
        assert_eq!(
            classify(vanilla, treatment),
            AgentEvalResult::RiBetter {
                reason: ResultReason::FewerTokens,
            }
        );
    }

    #[test]
    fn count_activity_from_exec_jsonl_tallies_tool_shell_phases_and_reads() {
        let jsonl = r#"{"type":"thread.started","thread_id":"t"}
{"type":"item.completed","item":{"type":"command_execution","command":"/bin/zsh -lc 'find . -name *.rs'"}}
{"type":"item.completed","item":{"type":"command_execution","command":"/bin/zsh -lc 'grep -n pattern foo.rs'"}}
{"type":"item.completed","item":{"type":"command_execution","command":"/bin/zsh -lc 'cat README.md'"}}
{"type":"item.completed","item":{"type":"command_execution","command":"/bin/zsh -lc 'sed -i \"\" 1d foo.rs'"}}
{"type":"item.completed","item":{"type":"agent_message","text":"done"}}
{"type":"item.completed","item":{"type":"command_execution","command":"/bin/zsh -lc 'head -n 5 bar.rs'"}}
{"type":"item.completed","item":{"type":"command_execution","command":"/bin/zsh -lc 'grep -A 3 pattern foo.rs'"}}"#;
        let counts = count_activity_from_exec_jsonl(jsonl.as_bytes()).unwrap();
        assert_eq!(counts.tool_calls, 7, "every item.completed counts");
        assert_eq!(counts.shell_commands, 6, "command_execution items only");
        assert_eq!(counts.file_reads, 2, "cat + head");
        assert_eq!(counts.discover_commands, 2, "find + plain grep");
        assert_eq!(counts.edit_commands, 1, "sed -i");
        assert_eq!(counts.verify_commands, 1, "grep -A is verify");
        // discover + read + edit + verify = 6, equal to shell_commands → no `other`.
    }

    #[test]
    fn classify_shell_phase_unpeels_cd_and_chained_commands() {
        // The q8 rehearsal showed agents prefix every shell with
        // `cd $WORKTREE && <real_cmd>`. The classifier must look past the
        // `cd` prefix to the actual command after `&&`.
        assert_eq!(
            classify_shell_phase("/bin/zsh -lc 'cd /tmp/foo && sed -i \"\" 1d bar.rs'"),
            "edit"
        );
        assert_eq!(
            classify_shell_phase("/bin/zsh -lc 'cd /tmp/foo && cat baz.rs'"),
            "read"
        );
        assert_eq!(
            classify_shell_phase("/bin/zsh -lc 'cd /tmp/foo && grep -A 3 pattern bar.rs'"),
            "verify"
        );
        // Chained: cd && cd && real cmd — both `cd ... &&` prefixes stripped.
        assert_eq!(
            classify_shell_phase("/bin/zsh -lc 'cd /tmp && cd /tmp/foo && find . -name *.rs'"),
            "discover"
        );
        // Bare `cd` with no `&&` chain is still "other".
        assert_eq!(classify_shell_phase("cd /tmp/foo"), "other");
    }

    #[test]
    fn classify_shell_phase_detects_joined_grep_context_flags() {
        // Q8 rehearsal: `grep -A5 -B5` (no space) was misclassified as
        // discover. Joined flag forms must count as verify.
        assert_eq!(classify_shell_phase("grep -A5 pattern foo.rs"), "verify");
        assert_eq!(classify_shell_phase("grep -B5 pattern foo.rs"), "verify");
        assert_eq!(classify_shell_phase("grep -C3 pattern foo.rs"), "verify");
        assert_eq!(
            classify_shell_phase("/bin/zsh -lc 'grep -n -A5 -B5 thing foo.rs'"),
            "verify"
        );
        // No context flag → discover.
        assert_eq!(classify_shell_phase("grep -n pattern foo.rs"), "discover");
    }

    #[test]
    fn classify_shell_phase_covers_common_cases() {
        // Wrapped form (`/bin/zsh -lc '<inner>'`) and bare form should agree.
        assert_eq!(
            classify_shell_phase("/bin/zsh -lc 'find . -name *.rs'"),
            "discover"
        );
        assert_eq!(classify_shell_phase("find . -name '*.rs'"), "discover");
        assert_eq!(classify_shell_phase("ls -la"), "discover");
        assert_eq!(
            classify_shell_phase("/bin/zsh -lc 'grep -n install foo.rs'"),
            "discover"
        );
        assert_eq!(classify_shell_phase("grep -A 3 pattern foo.rs"), "verify");
        assert_eq!(classify_shell_phase("grep -B 2 pattern foo.rs"), "verify");
        assert_eq!(classify_shell_phase("cat README.md"), "read");
        assert_eq!(classify_shell_phase("head -n 5 foo.rs"), "read");
        assert_eq!(classify_shell_phase("sed -i '' '1d' foo.rs"), "edit");
        assert_eq!(
            classify_shell_phase("perl -i -pe 's/old/new/' foo.rs"),
            "edit"
        );
        assert_eq!(
            classify_shell_phase("awk 'NR==180{print}' foo.rs"),
            "verify"
        );
        assert_eq!(classify_shell_phase("echo content > newfile.rs"), "edit");
        assert_eq!(classify_shell_phase("mv old.rs new.rs"), "edit");
        assert_eq!(classify_shell_phase("git status"), "discover");
        assert_eq!(classify_shell_phase("git diff HEAD"), "verify");
        assert_eq!(
            classify_shell_phase("python -m pytest tests/foo.py"),
            "other"
        );
    }

    #[test]
    fn duration_cell_formatting() {
        assert_eq!(duration_cell(None), "—");
        assert_eq!(duration_cell(Some(0)), "0.0s");
        assert_eq!(duration_cell(Some(4_200)), "4.2s");
        assert_eq!(duration_cell(Some(99_900)), "99.9s");
        assert_eq!(duration_cell(Some(100_000)), "1m40s");
        assert_eq!(duration_cell(Some(605_000)), "10m05s");
    }

    #[test]
    fn cost_row_emits_signed_token_delta_and_diagnostic_counts() {
        let task = calculator_task();
        let vanilla = AgentRunRecord {
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            tokens_input: Some(400_000),
            tokens_output: Some(1_000),
            tokens_total: Some(401_000),
            duration_ms: Some(60_000),
            tool_call_count: Some(15),
            shell_command_count: Some(10),
            file_read_count: Some(5),
            discover_command_count: Some(2),
            edit_command_count: Some(1),
            verify_command_count: Some(2),
            ..synthetic_record(AgentArm::Vanilla, &task.id)
        };
        let treatment = AgentRunRecord {
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            harness_context_visible: true,
            tokens_input: Some(405_000),
            tokens_output: Some(800),
            tokens_total: Some(405_800),
            duration_ms: Some(50_000),
            tool_call_count: Some(7),
            shell_command_count: Some(4),
            file_read_count: Some(1),
            discover_command_count: Some(0),
            edit_command_count: Some(2),
            verify_command_count: Some(1),
            ..synthetic_record(AgentArm::RepoIntelligence, &task.id)
        };
        let row = compare_task(&task, &vanilla, &treatment);
        let cells = format_cost_row(&row);
        assert_eq!(cells[0], "calculator_fix");
        assert_eq!(cells[1], "401000/405800");
        assert_eq!(
            cells[2], "+4800",
            "Token Δ should be signed; RI +4800 tokens"
        );
        assert_eq!(cells[3], "15/7", "tool calls V/RI");
        assert_eq!(cells[4], "2/0", "discover V/RI");
        assert_eq!(cells[5], "5/1", "read V/RI");
        assert_eq!(cells[6], "1/2", "edit V/RI");
        assert_eq!(cells[7], "2/1", "verify V/RI");
    }

    #[test]
    fn count_activity_counts_file_change_as_edit_not_shell() {
        // Frontier (gpt-5.x-codex) edits via `apply_patch` / `file_change`
        // structured tool output rather than running `sed`/`perl` shells.
        // Without this branch, the cost-table `edit` column reads e=0 for
        // a session that touched three files. Verify file_change items
        // increment edit_commands but NOT shell_commands.
        let jsonl = r#"{"type":"thread.started","thread_id":"t"}
{"type":"item.completed","item":{"type":"file_change","path":"a.rs"}}
{"type":"item.completed","item":{"type":"file_change","path":"b.rs"}}
{"type":"item.completed","item":{"type":"command_execution","command":"/bin/zsh -lc 'ls'"}}
{"type":"item.completed","item":{"type":"file_change","path":"c.rs"}}"#;
        let counts = count_activity_from_exec_jsonl(jsonl.as_bytes()).unwrap();
        assert_eq!(counts.tool_calls, 4, "every item.completed counts");
        assert_eq!(
            counts.shell_commands, 1,
            "file_change must NOT inflate shell_commands"
        );
        assert_eq!(counts.edit_commands, 3, "3 file_change items → 3 edits");
        assert_eq!(counts.discover_commands, 1, "the lone `ls` is discover");
        assert_eq!(counts.file_reads, 0);
        assert_eq!(counts.verify_commands, 0);
    }

    #[test]
    fn rollout_visibility_detects_marker_in_user_message_only() {
        // The directive packet rides in as a `message` payload with role
        // `user` or `developer`. That's the only place that should count.
        let rollout = r#"{"type":"session_meta","payload":{}}
{"type":"response_item","payload":{"type":"message","role":"user","content":"Harness repo intelligence: inspect a.rs"}}
{"type":"response_item","payload":{"type":"message","role":"assistant","content":"OK"}}"#;
        assert!(rollout_carries_harness_directive(rollout));
    }

    #[test]
    fn rollout_visibility_ignores_marker_in_function_call_output() {
        // Source-code grep leaks: an agent ran `grep HARNESS_MARKER
        // renderer.rs`, codex echoed the file content back as a
        // function_call_output payload. That is NOT the model receiving
        // the directive — it's the model SEARCHING the codebase for the
        // string. Pre-fix, the legacy text-scan-based detector returned
        // `vis=True` here; we now require message-role context.
        let rollout = r#"{"type":"session_meta","payload":{}}
{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{}"}}
{"type":"response_item","payload":{"type":"function_call_output","output":"renderer.rs:42: const HARNESS_MARKER: &str = \"Harness repo intelligence:\";"}}
{"type":"response_item","payload":{"type":"message","role":"assistant","content":"Found it in renderer.rs"}}"#;
        assert!(
            !rollout_carries_harness_directive(rollout),
            "marker appearing only in function_call_output must NOT count as visibility"
        );
    }

    #[test]
    fn rollout_visibility_accepts_developer_role_and_content_array() {
        // Codex emits the directive as a `developer` (system-style)
        // message in some configurations, and content may be an array of
        // structured parts rather than a flat string. Both forms must
        // count.
        let rollout = r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"Harness repo intelligence: prelude"}]}}"#;
        assert!(rollout_carries_harness_directive(rollout));

        let rollout_string = r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":"Harness repo intelligence: prelude"}}"#;
        assert!(rollout_carries_harness_directive(rollout_string));
    }

    #[test]
    fn intent_changed_files_from_exec_jsonl_extracts_file_change_paths() {
        // Two `file_change` items for distinct files + one unrelated
        // `command_execution` + one item.started (not completed). The
        // extractor returns repo-relative paths sorted; only completed
        // file_change items count.
        let jsonl = r#"{"type":"thread.started","thread_id":"t"}
{"type":"item.started","item":{"id":"i1","type":"file_change","changes":[{"path":"/tmp/wt/codex-rs/should/not/appear.rs","kind":"update"}]}}
{"type":"item.completed","item":{"id":"i2","type":"command_execution","command":"ls"}}
{"type":"item.completed","item":{"id":"i3","type":"file_change","changes":[{"path":"/tmp/wt/codex-rs/context-harness/src/agent_eval.rs","kind":"update"}]}}
{"type":"item.completed","item":{"id":"i4","type":"file_change","changes":[{"path":"/tmp/wt/codex-rs/scripts/harness-agent-eval.sh","kind":"update"}]}}
{"type":"item.completed","item":{"id":"i5","type":"file_change","changes":[{"path":"/tmp/wt/codex-rs/context-harness/src/agent_eval.rs","kind":"update"}]}}"#;
        let intent = intent_changed_files_from_exec_jsonl(jsonl.as_bytes()).unwrap();
        // Duplicates collapse, paths normalized to repo-relative,
        // lexically sorted.
        assert_eq!(
            intent,
            vec![
                "context-harness/src/agent_eval.rs".to_string(),
                "scripts/harness-agent-eval.sh".to_string(),
            ]
        );
        // item.started is NOT counted (only completed) — `should/not/appear.rs`
        // must be absent.
        assert!(!intent.iter().any(|p| p.contains("should/not/appear")));
    }

    #[test]
    fn score_run_uses_intent_changed_files_and_ignores_formatter_collateral() {
        // The rate_limit-v2 measurement bug in concrete form: the
        // model intentionally edited only the gold + bridge, but
        // `git diff` saw 9 files because `cargo fmt --all` ran. With
        // intent_changed_files populated, score_run must ignore the
        // 7 formatter files entirely.
        let task = AgentEvalTask {
            id: "fmt_collateral".to_string(),
            task: "test".to_string(),
            relevant_files: vec!["src/agent_eval.rs".to_string()],
            relevant_tests: Vec::new(),
            bridge_files: vec!["scripts/runner.sh".to_string()],
            danger_zones: Vec::new(),
            verify_command: None,
            workdir: AgentEvalWorkdir::Calculator,
            category: None,
            verification_required: None,
        };
        let mut record = synthetic_record(AgentArm::RepoIntelligence, &task.id);
        // Diff sees: gold + bridge + 7 fmt-drift files.
        record.changed_files = vec![
            "src/agent_eval.rs".to_string(),
            "scripts/runner.sh".to_string(),
            "src/lib.rs".to_string(),
            "src/renderer.rs".to_string(),
            "src/task_terms.rs".to_string(),
            "tests/agent_eval.rs".to_string(),
            "core/src/lib.rs".to_string(),
            "ext/repo-intelligence/tests/contributor_injection.rs".to_string(),
            "verification/src/planner.rs".to_string(),
        ];
        // Intent: only the two files the model authored via apply_patch.
        record.intent_changed_files = vec![
            "src/agent_eval.rs".to_string(),
            "scripts/runner.sh".to_string(),
        ];
        let score = score_run(&record, &task);
        assert_eq!(score.edit_target_files_hit, 1, "gold hit via intent");
        assert_eq!(
            score.unnecessary_files_changed.len(),
            1,
            "extras = intent − gold = {{bridge}}, NOT the 7 formatter files. \
             Got: {:?}",
            score.unnecessary_files_changed
        );
        assert!(
            score
                .unnecessary_files_changed
                .iter()
                .all(|p| !p.contains("renderer.rs") && !p.contains("planner.rs")),
            "formatter collateral leaked into extras: {:?}",
            score.unnecessary_files_changed
        );
    }

    #[test]
    fn score_run_falls_back_to_changed_files_when_intent_is_empty() {
        // Pre-instrumentation records (no intent_changed_files
        // captured) must still score via `changed_files`. Without
        // this fallback every backfilled artifact would zero out
        // edit_target_files_hit.
        let task = AgentEvalTask {
            id: "legacy".to_string(),
            task: "test".to_string(),
            relevant_files: vec!["gold.rs".to_string()],
            relevant_tests: Vec::new(),
            bridge_files: Vec::new(),
            danger_zones: Vec::new(),
            verify_command: None,
            workdir: AgentEvalWorkdir::Calculator,
            category: None,
            verification_required: None,
        };
        let mut record = synthetic_record(AgentArm::Vanilla, &task.id);
        record.changed_files = vec!["gold.rs".to_string(), "other.rs".to_string()];
        // intent left empty → fallback path.
        let score = score_run(&record, &task);
        assert_eq!(score.edit_target_files_hit, 1);
        assert_eq!(score.unnecessary_files_changed.len(), 1);
    }

    #[test]
    fn score_run_counts_bridge_and_ri_surfaced_as_orientation_excluding_gold() {
        // Build a task where: gold=[a.rs], bridge=[b.rs], and the RI
        // arm surfaced [a.rs, b.rs, c.rs, d.rs] in its directive. The
        // model touched a.rs (gold), b.rs (bridge → orientation), and
        // c.rs (RI-surfaced extra → orientation). d.rs was NOT edited.
        let task = AgentEvalTask {
            id: "orient_test".to_string(),
            task: "test orientation".to_string(),
            relevant_files: vec!["a.rs".to_string()],
            relevant_tests: Vec::new(),
            bridge_files: vec!["b.rs".to_string()],
            danger_zones: Vec::new(),
            verify_command: None,
            workdir: AgentEvalWorkdir::Calculator,
            category: None,
            verification_required: None,
        };
        let record = AgentRunRecord {
            arm: AgentArm::RepoIntelligence,
            task_id: task.id.clone(),
            changed_files: vec!["a.rs".to_string(), "b.rs".to_string(), "c.rs".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            exec_exit_code: Some(0),
            repo_intelligence_enabled: true,
            harness_context_visible: true,
            run_valid: true,
            invalid_reason: None,
            tokens_input: None,
            tokens_output: None,
            tokens_total: None,
            duration_ms: None,
            tool_call_count: None,
            shell_command_count: None,
            file_read_count: None,
            discover_command_count: None,
            edit_command_count: None,
            verify_command_count: None,
            warnings: Vec::new(),
            // RI surfaced 1 edit target (a.rs) + 3 orientation files
            // (b.rs, c.rs, d.rs). a.rs as an edit target overlaps with
            // gold and is therefore NOT in the orientation set; the
            // formula `bridge ∪ ri_orient − gold` keeps that invariant.
            ri_surfaced_edit_targets: vec!["a.rs".to_string()],
            ri_surfaced_orientation: vec![
                "b.rs".to_string(),
                "c.rs".to_string(),
                "d.rs".to_string(),
            ],
            intent_changed_files: Vec::new(),
            diff_changed_files: Vec::new(),
            formatter_changed_files: Vec::new(),
            harness_prewarm_ms: None,
            codex_build_profile: None,
            search_proxy_enabled: false,
            search_proxy_substitutions: 0,
            search_proxy_escape_hatch_repeats: 0,
            search_proxy_build_pass_throughs: 0,
            search_proxy_compact_bytes: 0,
            search_proxy_raw_bytes_estimated: 0,
            search_proxy_top_files: Vec::new(),
            worktree_isolated: false,
            base_ref: None,
            worktree_path: None,
        };
        let score = score_run(&record, &task);
        assert_eq!(
            score.edit_target_files_hit, 1,
            "a.rs is in gold and touched"
        );
        assert_eq!(score.edit_target_files_total, 1);
        // Orientation set: bridge {b} ∪ ri_orient {b,c,d} − gold {a}
        //                 = {b, c, d}
        // Touched: {a, b, c}. Intersection with orientation_set = {b, c}.
        assert_eq!(
            score.orientation_files_touched, 2,
            "b (bridge) + c (RI-surfaced extra) edited; d was surfaced but not edited"
        );
        assert_eq!(score.orientation_files_total, 3, "{{b, c, d}}");
    }

    #[test]
    fn score_run_falls_back_to_bridge_only_for_vanilla_with_no_ri_surfaced() {
        // Vanilla arm has empty ri_surfaced_*. The orientation set
        // collapses to (bridge − gold). This keeps backfilled / legacy
        // records meaningful: orientation_files_touched still measures
        // "bridge file edited" even without the RI packet.
        let task = AgentEvalTask {
            id: "vanilla_orient".to_string(),
            task: "test".to_string(),
            relevant_files: vec!["gold.rs".to_string()],
            relevant_tests: Vec::new(),
            bridge_files: vec!["wiring.rs".to_string()],
            danger_zones: Vec::new(),
            verify_command: None,
            workdir: AgentEvalWorkdir::Calculator,
            category: None,
            verification_required: None,
        };
        let record = AgentRunRecord {
            arm: AgentArm::Vanilla,
            task_id: task.id.clone(),
            changed_files: vec!["gold.rs".to_string(), "wiring.rs".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            exec_exit_code: Some(0),
            repo_intelligence_enabled: false,
            harness_context_visible: false,
            run_valid: true,
            invalid_reason: None,
            tokens_input: None,
            tokens_output: None,
            tokens_total: None,
            duration_ms: None,
            tool_call_count: None,
            shell_command_count: None,
            file_read_count: None,
            discover_command_count: None,
            edit_command_count: None,
            verify_command_count: None,
            warnings: Vec::new(),
            ri_surfaced_edit_targets: Vec::new(),
            ri_surfaced_orientation: Vec::new(),
            intent_changed_files: Vec::new(),
            diff_changed_files: Vec::new(),
            formatter_changed_files: Vec::new(),
            harness_prewarm_ms: None,
            codex_build_profile: None,
            search_proxy_enabled: false,
            search_proxy_substitutions: 0,
            search_proxy_escape_hatch_repeats: 0,
            search_proxy_build_pass_throughs: 0,
            search_proxy_compact_bytes: 0,
            search_proxy_raw_bytes_estimated: 0,
            search_proxy_top_files: Vec::new(),
            worktree_isolated: false,
            base_ref: None,
            worktree_path: None,
        };
        let score = score_run(&record, &task);
        assert_eq!(score.edit_target_files_hit, 1);
        assert_eq!(
            score.orientation_files_touched, 1,
            "bridge wiring.rs edited"
        );
        assert_eq!(score.orientation_files_total, 1);
    }

    #[test]
    fn valid_cell_renders_warnings_when_present_and_plain_when_absent() {
        // Clean pair: just "valid".
        let task = calculator_task();
        let vanilla = AgentRunRecord {
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            ..synthetic_record(AgentArm::Vanilla, &task.id)
        };
        let treatment = AgentRunRecord {
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            ..synthetic_record(AgentArm::RepoIntelligence, &task.id)
        };
        let row = compare_task(&task, &vanilla, &treatment);
        assert!(row.valid_for_comparison);
        assert_eq!(format_valid_cell(&row), "valid");

        // Both arms carry the same recovered-network warning: collapse.
        let v_warn = AgentRunRecord {
            warnings: vec!["provider_network_error_recovered".to_string()],
            ..vanilla.clone()
        };
        let t_warn = AgentRunRecord {
            warnings: vec!["provider_network_error_recovered".to_string()],
            ..treatment.clone()
        };
        let row = compare_task(&task, &v_warn, &t_warn);
        assert_eq!(
            format_valid_cell(&row),
            "valid (provider_network_error_recovered)"
        );

        // Only RI arm carries a warning: prefix the arm.
        let t_only = AgentRunRecord {
            warnings: vec!["provider_network_error_recovered".to_string()],
            ..treatment
        };
        let row = compare_task(&task, &vanilla, &t_only);
        assert_eq!(
            format_valid_cell(&row),
            "valid (RI: provider_network_error_recovered)"
        );
    }

    #[test]
    fn search_proxy_record_round_trip_serde() {
        // A record populated by the search-proxy treatment arm should
        // serialize and deserialize without losing any of the seven
        // new fields, and the round-trip should equal the source.
        let original = AgentRunRecord {
            search_proxy_enabled: true,
            search_proxy_substitutions: 4,
            search_proxy_escape_hatch_repeats: 1,
            search_proxy_build_pass_throughs: 2,
            search_proxy_compact_bytes: 3_840,
            search_proxy_raw_bytes_estimated: 9_120,
            search_proxy_top_files: vec![
                "context-harness/src/agent_eval.rs".to_string(),
                "context-harness/src/renderer.rs".to_string(),
            ],
            ..synthetic_record(AgentArm::SearchProxy, "round_trip_task")
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let round_tripped: AgentRunRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(round_tripped, original);
    }

    #[test]
    fn search_proxy_record_loads_legacy_records_without_proxy_fields() {
        // Pre-Commit-4 record.json files (the four cloud pairs from
        // the RI experiment) have none of the search_proxy_* keys.
        // `#[serde(default)]` should make them load with zeros / empty
        // vec.
        let legacy = r#"{
            "arm": "vanilla",
            "task_id": "legacy",
            "changed_files": [],
            "tests_passed": false,
            "turn_count": null
        }"#;
        let rec: AgentRunRecord = serde_json::from_str(legacy).expect("legacy parse");
        assert!(!rec.search_proxy_enabled);
        assert_eq!(rec.search_proxy_substitutions, 0);
        assert_eq!(rec.search_proxy_escape_hatch_repeats, 0);
        assert_eq!(rec.search_proxy_build_pass_throughs, 0);
        assert_eq!(rec.search_proxy_compact_bytes, 0);
        assert_eq!(rec.search_proxy_raw_bytes_estimated, 0);
        assert!(rec.search_proxy_top_files.is_empty());
    }
}
