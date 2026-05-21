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
        }
    }

    pub fn display_label(self) -> &'static str {
        self.artifact_dir()
    }
}

/// Per-run scores on the five comparison dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRunScore {
    pub correct_file_touched: bool,
    pub tests_passed: bool,
    pub turn_count: Option<u32>,
    pub unnecessary_files_changed: Vec<String>,
    pub harness_context_visible: bool,
    pub bridge_files_touched: Vec<String>,
    pub run_valid: bool,
    pub invalid_reason: Option<AgentRunInvalidReason>,
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
    let changed: BTreeSet<String> = filter_scoring_changed_files(&record.changed_files)
        .into_iter()
        .map(|path| normalize_agent_eval_path(&path))
        .filter(|path| !path.is_empty())
        .collect();
    let correct_file_touched = task
        .relevant_files
        .iter()
        .any(|path| changed.contains(path));
    let unnecessary_files_changed: Vec<String> = changed.difference(&gold).cloned().collect();

    let bridge_files_touched: Vec<String> = task
        .bridge_files
        .iter()
        .filter(|path| changed.contains(*path))
        .cloned()
        .collect();

    AgentRunScore {
        correct_file_touched,
        tests_passed: record.tests_passed && record.run_valid,
        turn_count: record.turn_count,
        unnecessary_files_changed,
        harness_context_visible: record.harness_context_visible,
        bridge_files_touched,
        run_valid: record.run_valid,
        invalid_reason: record.invalid_reason,
    }
}

pub fn compare_task(
    task: &AgentEvalTask,
    vanilla: &AgentRunRecord,
    treatment: &AgentRunRecord,
) -> AgentEvalComparison {
    let excluded_reason = pair_excluded_reason(vanilla, treatment);
    let valid_for_comparison = excluded_reason.is_none();
    AgentEvalComparison {
        task_id: task.id.clone(),
        task: task.task.clone(),
        vanilla: score_run(vanilla, task),
        treatment: score_run(treatment, task),
        treatment_arm: treatment.arm,
        valid_for_comparison,
        excluded_reason,
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
        }
    }
}

pub fn render_agent_eval_human(report: &AgentEvalReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Valid comparisons: {}/{}",
        report.summary.valid_pairs, report.summary.total_pairs
    ));
    lines.push(format!(
        "Invalid comparisons: {}/{}",
        report.summary.invalid_pairs, report.summary.total_pairs
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
    lines.push(String::new());
    for row in &report.comparisons {
        let treatment = row.treatment_arm.display_label();
        lines.push(format!("Task: {} ({})", row.task_id, row.task));
        lines.push(format!(
            "  treatment_arm: {}",
            row.treatment_arm.display_label()
        ));
        lines.push(format!(
            "  valid_for_comparison: {}",
            row.valid_for_comparison
        ));
        if let Some(reason) = &row.excluded_reason {
            lines.push(format!("  excluded_reason: {reason}"));
        }
        lines.push(format_dimension_row(
            "correct_file_touched",
            row.vanilla.correct_file_touched,
            row.treatment.correct_file_touched,
            treatment,
        ));
        lines.push(format_dimension_row(
            "tests_passed",
            row.vanilla.tests_passed,
            row.treatment.tests_passed,
            treatment,
        ));
        lines.push(format_dimension_row(
            "harness_context_visible",
            row.vanilla.harness_context_visible,
            row.treatment.harness_context_visible,
            treatment,
        ));
        lines.push(format!(
            "  turn_count: vanilla {:?} | {treatment} {:?}",
            row.vanilla.turn_count, row.treatment.turn_count
        ));
        lines.push(format!(
            "  unnecessary_files: vanilla {:?} | {treatment} {:?}",
            row.vanilla.unnecessary_files_changed, row.treatment.unnecessary_files_changed
        ));
        lines.push(format!(
            "  bridge_files_touched: vanilla {:?} | {treatment} {:?}",
            row.vanilla.bridge_files_touched, row.treatment.bridge_files_touched
        ));
        lines.push(format!(
            "  run_valid: vanilla {} | {treatment} {}",
            row.vanilla.run_valid, row.treatment.run_valid
        ));
        lines.push(format!(
            "  invalid_reason: vanilla {:?} | {treatment} {:?}",
            row.vanilla.invalid_reason, row.treatment.invalid_reason
        ));
        lines.push(String::new());
    }
    lines.join("\n")
}

fn format_dimension_row(
    name: &str,
    vanilla: bool,
    treatment: bool,
    treatment_label: &str,
) -> String {
    format!("  {name}: vanilla {vanilla} | {treatment_label} {treatment}")
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
        }
    }

    #[test]
    fn scores_correct_fix() {
        let task = calculator_task();
        let record = AgentRunRecord {
            arm: AgentArm::Harness,
            task_id: task.id.clone(),
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(2),
            exec_exit_code: Some(0),
            repo_intelligence_enabled: false,
            harness_context_visible: false,
            run_valid: true,
            invalid_reason: None,
        };
        let score = score_run(&record, &task);
        assert_eq!(
            score,
            AgentRunScore {
                correct_file_touched: true,
                tests_passed: true,
                turn_count: Some(2),
                unnecessary_files_changed: Vec::new(),
                harness_context_visible: false,
                bridge_files_touched: Vec::new(),
                run_valid: true,
                invalid_reason: None,
            }
        );
    }

    #[test]
    fn ignores_python_cache_artifacts_in_scoring() {
        let task = calculator_task();
        let record = AgentRunRecord {
            arm: AgentArm::Harness,
            task_id: task.id.clone(),
            changed_files: vec![
                "src/__pycache__/calculator.cpython-313.pyc".to_string(),
                "tests/__pycache__/test_calculator.cpython-313-pytest-9.0.0.pyc".to_string(),
                ".pytest_cache/v/cache/nodeids".to_string(),
            ],
            tests_passed: false,
            turn_count: Some(1),
            exec_exit_code: Some(0),
            repo_intelligence_enabled: false,
            harness_context_visible: false,
            run_valid: true,
            invalid_reason: None,
        };
        let score = score_run(&record, &task);
        assert_eq!(score.correct_file_touched, false);
        assert_eq!(score.unnecessary_files_changed, Vec::<String>::new());
    }

    #[test]
    fn scores_unnecessary_files_and_no_touch() {
        let task = calculator_task();
        let record = AgentRunRecord {
            arm: AgentArm::Vanilla,
            task_id: task.id.clone(),
            changed_files: vec!["README.md".to_string()],
            tests_passed: false,
            turn_count: Some(5),
            exec_exit_code: Some(1),
            repo_intelligence_enabled: false,
            harness_context_visible: false,
            run_valid: true,
            invalid_reason: None,
        };
        let score = score_run(&record, &task);
        assert_eq!(score.correct_file_touched, false);
        assert_eq!(
            score.unnecessary_files_changed,
            vec!["README.md".to_string()]
        );
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
    fn repo_intelligence_arm_round_trips() {
        let json = r#"{"arm":"repo_intelligence","task_id":"t","changed_files":[],"tests_passed":false,"turn_count":null,"exec_exit_code":null,"harness_context_visible":true}"#;
        let record: AgentRunRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.arm, AgentArm::RepoIntelligence);
        assert!(record.harness_context_visible);
        assert!(record.run_valid);
        assert_eq!(record.invalid_reason, None);
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
        };
        let record = AgentRunRecord {
            arm: AgentArm::Vanilla,
            task_id: task.id.clone(),
            changed_files: vec!["codex-rs/cli/src/context_cmd.rs".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            exec_exit_code: Some(0),
            repo_intelligence_enabled: false,
            harness_context_visible: false,
            run_valid: true,
            invalid_reason: None,
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
        };
        let record = AgentRunRecord {
            arm: AgentArm::RepoIntelligence,
            task_id: task.id.clone(),
            changed_files: vec!["codex-rs/cli/src/main.rs".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            exec_exit_code: Some(0),
            repo_intelligence_enabled: true,
            harness_context_visible: true,
            run_valid: true,
            invalid_reason: None,
        };
        let score = score_run(&record, &task);
        assert_eq!(
            score.bridge_files_touched,
            vec!["cli/src/main.rs".to_string()]
        );
        assert_eq!(
            score.unnecessary_files_changed,
            vec!["cli/src/main.rs".to_string()]
        );
    }

    #[test]
    fn invalid_pairs_are_excluded_from_behavioral_comparison() {
        let task = calculator_task();
        let vanilla = AgentRunRecord {
            arm: AgentArm::Vanilla,
            task_id: task.id.clone(),
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            exec_exit_code: Some(0),
            repo_intelligence_enabled: false,
            harness_context_visible: false,
            run_valid: false,
            invalid_reason: Some(AgentRunInvalidReason::ProviderUsageLimit),
        };
        let treatment = AgentRunRecord {
            arm: AgentArm::RepoIntelligence,
            task_id: task.id.clone(),
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(1),
            exec_exit_code: Some(0),
            repo_intelligence_enabled: true,
            harness_context_visible: true,
            run_valid: true,
            invalid_reason: None,
        };
        let row = compare_task(&task, &vanilla, &treatment);
        assert!(!row.valid_for_comparison);
        assert_eq!(
            row.excluded_reason.as_deref(),
            Some("pair_invalid:provider_usage_limit|invalid")
        );
        // tests_passed is ignored for invalid runs
        assert!(!row.vanilla.tests_passed);
    }
}
