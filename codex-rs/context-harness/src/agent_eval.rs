//! Scoring for vanilla vs harness-context agent runs on the same tasks.
//!
//! Consumes per-run artifacts (git diff, test exit code, optional exec JSONL) and
//! fixture gold labels. Does not invoke models.

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
    /// When true, harness arm should attach post-failure context before the agent run.
    #[serde(default)]
    pub requires_post_failure: bool,
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
    pub used_post_failure: bool,
    #[serde(default)]
    pub exec_exit_code: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentArm {
    Vanilla,
    Harness,
}

/// Per-run scores on the five comparison dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRunScore {
    pub correct_file_touched: bool,
    pub tests_passed: bool,
    pub turn_count: Option<u32>,
    pub unnecessary_files_changed: Vec<String>,
    pub failure_recovery_quality: FailureRecoveryQuality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureRecoveryQuality {
    NotApplicable,
    Failed,
    Partial,
    Good,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentEvalComparison {
    pub task_id: String,
    pub task: String,
    pub vanilla: AgentRunScore,
    pub harness: AgentRunScore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentEvalReport {
    pub comparisons: Vec<AgentEvalComparison>,
}

pub fn load_agent_eval_tasks(path: &Path) -> anyhow::Result<Vec<AgentEvalTask>> {
    let bytes = std::fs::read(path)?;
    let mut tasks: Vec<AgentEvalTask> = serde_json::from_slice(&bytes)?;
    for (index, task) in tasks.iter_mut().enumerate() {
        if task.id.is_empty() {
            task.id = format!("task_{index}");
        }
    }
    Ok(tasks)
}

pub fn score_run(record: &AgentRunRecord, task: &AgentEvalTask) -> AgentRunScore {
    let gold: BTreeSet<String> = task.relevant_files.iter().cloned().collect();
    let changed: BTreeSet<String> = record.changed_files.iter().cloned().collect();
    let correct_file_touched = task
        .relevant_files
        .iter()
        .any(|path| changed.contains(path));
    let unnecessary_files_changed: Vec<String> = changed
        .difference(&gold)
        .cloned()
        .collect();
    let failure_recovery_quality = score_failure_recovery(record, task, correct_file_touched, &unnecessary_files_changed);

    AgentRunScore {
        correct_file_touched,
        tests_passed: record.tests_passed,
        turn_count: record.turn_count,
        unnecessary_files_changed,
        failure_recovery_quality,
    }
}

fn score_failure_recovery(
    record: &AgentRunRecord,
    task: &AgentEvalTask,
    correct_file_touched: bool,
    unnecessary: &[String],
) -> FailureRecoveryQuality {
    if !task.requires_post_failure {
        return FailureRecoveryQuality::NotApplicable;
    }
    if !record.used_post_failure {
        return FailureRecoveryQuality::Failed;
    }
    if !record.tests_passed {
        return FailureRecoveryQuality::Failed;
    }
    if correct_file_touched && unnecessary.is_empty() {
        FailureRecoveryQuality::Good
    } else if correct_file_touched {
        FailureRecoveryQuality::Partial
    } else {
        FailureRecoveryQuality::Failed
    }
}

pub fn compare_task(
    task: &AgentEvalTask,
    vanilla: &AgentRunRecord,
    harness: &AgentRunRecord,
) -> AgentEvalComparison {
    AgentEvalComparison {
        task_id: task.id.clone(),
        task: task.task.clone(),
        vanilla: score_run(vanilla, task),
        harness: score_run(harness, task),
    }
}

pub fn build_report(comparisons: Vec<AgentEvalComparison>) -> AgentEvalReport {
    AgentEvalReport { comparisons }
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
            requires_post_failure: false,
        }
    }
}

pub fn render_agent_eval_human(report: &AgentEvalReport) -> String {
    let mut lines = Vec::new();
    for row in &report.comparisons {
        lines.push(format!("Task: {} ({})", row.task_id, row.task));
        lines.push(format_dimension_row(
            "correct_file_touched",
            row.vanilla.correct_file_touched,
            row.harness.correct_file_touched,
        ));
        lines.push(format_dimension_row(
            "tests_passed",
            row.vanilla.tests_passed,
            row.harness.tests_passed,
        ));
        lines.push(format!(
            "  turn_count: vanilla {:?} | harness {:?}",
            row.vanilla.turn_count, row.harness.turn_count
        ));
        lines.push(format!(
            "  unnecessary_files: vanilla {:?} | harness {:?}",
            row.vanilla.unnecessary_files_changed, row.harness.unnecessary_files_changed
        ));
        lines.push(format!(
            "  failure_recovery: vanilla {:?} | harness {:?}",
            row.vanilla.failure_recovery_quality, row.harness.failure_recovery_quality
        ));
        lines.push(String::new());
    }
    lines.join("\n")
}

fn format_dimension_row(name: &str, vanilla: bool, harness: bool) -> String {
    format!("  {name}: vanilla {vanilla} | harness {harness}")
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
            requires_post_failure: false,
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
            used_post_failure: false,
            exec_exit_code: Some(0),
        };
        let score = score_run(&record, &task);
        assert_eq!(
            score,
            AgentRunScore {
                correct_file_touched: true,
                tests_passed: true,
                turn_count: Some(2),
                unnecessary_files_changed: Vec::new(),
                failure_recovery_quality: FailureRecoveryQuality::NotApplicable,
            }
        );
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
            used_post_failure: false,
            exec_exit_code: Some(1),
        };
        let score = score_run(&record, &task);
        assert_eq!(score.correct_file_touched, false);
        assert_eq!(score.unnecessary_files_changed, vec!["README.md".to_string()]);
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
    fn recovery_good_requires_post_failure_pass_and_minimal_diff() {
        let mut task = calculator_task();
        task.requires_post_failure = true;
        let record = AgentRunRecord {
            arm: AgentArm::Harness,
            task_id: task.id.clone(),
            changed_files: vec!["src/calculator.py".to_string()],
            tests_passed: true,
            turn_count: Some(3),
            used_post_failure: true,
            exec_exit_code: Some(0),
        };
        assert_eq!(
            score_run(&record, &task).failure_recovery_quality,
            FailureRecoveryQuality::Good
        );
    }
}
