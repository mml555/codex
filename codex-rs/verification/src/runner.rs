use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use serde::Deserialize;
use serde::Serialize;

use crate::command_exec::spawn_narrow_command;
use crate::output::DEFAULT_MAX_RELEVANT_OUTPUT_CHARS;
use crate::output::summarize_failure_output;
use crate::output::truncate_text;
use crate::planner::PlanScope;
use crate::planner::PlannedCommand;
use crate::planner::VerificationPlan;

/// Overall result of a guarded verification run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationRunStatus {
    Passed,
    Failed,
    Cancelled,
}

/// Compact failure context for later harness / model turns (not a full [`ContextPacket`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailurePacket {
    pub stage: String,
    pub summary: String,
    pub relevant_output: String,
}

/// Result of executing one planned command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandRunResult {
    pub command: String,
    pub reason: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

/// Report emitted after a guarded verification run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationRunReport {
    pub status: VerificationRunStatus,
    pub plan_risk: crate::planner::VerificationRisk,
    pub commands: Vec<CommandRunResult>,
    pub skipped_not_run: Vec<String>,
    pub failure_packet: Option<FailurePacket>,
    #[serde(default)]
    pub changed_files: Vec<String>,
}

/// Options controlling guarded execution.
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub cwd: std::path::PathBuf,
    pub timeout_per_command: Duration,
    pub max_stream_chars: usize,
    pub max_relevant_output_chars: usize,
    pub changed_files: Vec<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            cwd: std::path::PathBuf::from("."),
            timeout_per_command: Duration::from_secs(600),
            max_stream_chars: crate::output::DEFAULT_MAX_STREAM_CHARS,
            max_relevant_output_chars: DEFAULT_MAX_RELEVANT_OUTPUT_CHARS,
            changed_files: Vec::new(),
        }
    }
}

/// Commands eligible for M8.2 execution (narrow scope only, never workspace-wide).
pub fn runnable_narrow_commands(plan: &VerificationPlan) -> Vec<&PlannedCommand> {
    plan.commands
        .iter()
        .filter(|cmd| cmd.scope == PlanScope::Narrow && is_safe_to_run(&cmd.command))
        .collect()
}

pub fn is_safe_to_run(command: &str) -> bool {
    crate::command_exec::is_safe_to_run(command)
}

pub fn run_verification_plan(
    plan: &VerificationPlan,
    options: &RunOptions,
) -> VerificationRunReport {
    let runnable: Vec<PlannedCommand> = runnable_narrow_commands(plan)
        .into_iter()
        .cloned()
        .collect();

    let skipped_not_run: Vec<String> = plan
        .commands
        .iter()
        .filter(|cmd| cmd.scope != PlanScope::Narrow || !is_safe_to_run(&cmd.command))
        .map(|cmd| cmd.command.clone())
        .chain(plan.skipped.iter().map(|s| s.command.clone()))
        .collect();

    let mut results = Vec::new();
    let mut failure_packet = None;

    for planned in runnable {
        let started = Instant::now();
        let run = execute_command(&planned.command, &options.cwd, options.timeout_per_command);
        let duration_ms = started.elapsed().as_millis() as u64;

        let stdout = truncate_text(&run.stdout, options.max_stream_chars);
        let stderr = truncate_text(&run.stderr, options.max_stream_chars);

        let success = run.error.is_none() && run.exit_code == Some(0);
        if !success && failure_packet.is_none() {
            let relevant = summarize_failure_output(
                &run.stdout,
                &run.stderr,
                options.max_relevant_output_chars,
            );
            failure_packet = Some(FailurePacket {
                stage: "post_failure".to_string(),
                summary: format!("{} failed", planned.command),
                relevant_output: relevant,
            });
        }

        results.push(CommandRunResult {
            command: planned.command.clone(),
            reason: planned.reason.clone(),
            exit_code: run.exit_code,
            duration_ms,
            stdout,
            stderr,
            error: run.error,
        });

        if !success {
            break;
        }
    }

    let status = if results.is_empty() {
        VerificationRunStatus::Passed
    } else if results
        .iter()
        .all(|r| r.exit_code == Some(0) && r.error.is_none())
    {
        VerificationRunStatus::Passed
    } else {
        VerificationRunStatus::Failed
    };

    VerificationRunReport {
        status,
        plan_risk: plan.risk,
        commands: results,
        skipped_not_run,
        failure_packet,
        changed_files: options.changed_files.clone(),
    }
}

#[derive(Debug)]
struct RawRunOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    error: Option<String>,
}

fn execute_command(command: &str, cwd: &Path, timeout: Duration) -> RawRunOutput {
    if !is_safe_to_run(command) {
        return RawRunOutput {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(format!("refusing to run unsafe command: {command}")),
        };
    }

    let mut child = match spawn_narrow_command(command, cwd) {
        Ok(child) => child,
        Err(err) => {
            return RawRunOutput {
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(err.to_string()),
            };
        }
    };

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child
                    .wait_with_output()
                    .unwrap_or_else(|_| std::process::Output {
                        status,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                    });
                return RawRunOutput {
                    exit_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    error: None,
                };
            }
            Ok(None) => {}
            Err(err) => {
                return RawRunOutput {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error: Some(err.to_string()),
                };
            }
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return RawRunOutput {
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!(
                    "command timed out after {}s: {command}",
                    timeout.as_secs()
                )),
            };
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn cancelled_run_report(
    plan: &VerificationPlan,
    changed_files: &[String],
) -> VerificationRunReport {
    VerificationRunReport {
        status: VerificationRunStatus::Cancelled,
        plan_risk: plan.risk,
        commands: Vec::new(),
        skipped_not_run: plan
            .commands
            .iter()
            .map(|c| c.command.clone())
            .chain(plan.skipped.iter().map(|s| s.command.clone()))
            .collect(),
        failure_packet: None,
        changed_files: changed_files.to_vec(),
    }
}

pub fn verification_exit_code(report: &VerificationRunReport) -> i32 {
    match report.status {
        VerificationRunStatus::Passed => 0,
        VerificationRunStatus::Failed => 1,
        VerificationRunStatus::Cancelled => 0,
    }
}

pub fn render_run_human(report: &VerificationRunReport) -> String {
    let status_label = match report.status {
        VerificationRunStatus::Passed => "passed",
        VerificationRunStatus::Failed => "failed",
        VerificationRunStatus::Cancelled => "cancelled",
    };
    let mut lines = vec![format!("status: {status_label}")];
    if !report.commands.is_empty() {
        lines.push("commands:".to_string());
        for cmd in &report.commands {
            let code = cmd
                .exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".to_string());
            lines.push(format!(
                "  - {} (exit {code}, {}ms)",
                cmd.command, cmd.duration_ms
            ));
            lines.push(format!("    reason: {}", cmd.reason));
            if let Some(err) = &cmd.error {
                lines.push(format!("    error: {err}"));
            }
        }
    }
    if !report.skipped_not_run.is_empty() {
        lines.push(format!(
            "skipped (not run): {}",
            report.skipped_not_run.join(", ")
        ));
    }
    if let Some(failure) = &report.failure_packet {
        lines.push("failure_packet:".to_string());
        lines.push(format!("  stage: {}", failure.stage));
        lines.push(format!("  summary: {}", failure.summary));
        lines.push("  relevant_output:".to_string());
        for line in failure.relevant_output.lines() {
            lines.push(format!("    {line}"));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::VerificationRisk;

    #[test]
    fn refuses_workspace_commands() {
        assert!(!is_safe_to_run("cargo test --workspace"));
    }

    #[test]
    fn runnable_filters_broad_scope() {
        let plan = VerificationPlan {
            commands: vec![
                PlannedCommand {
                    command: "cargo test -p codex-context-harness".to_string(),
                    reason: "narrow".to_string(),
                    scope: PlanScope::Narrow,
                    confidence: 0.9,
                },
                PlannedCommand {
                    command: "cargo test --workspace".to_string(),
                    reason: "broad".to_string(),
                    scope: PlanScope::Broad,
                    confidence: 0.5,
                },
            ],
            skipped: Vec::new(),
            risk: VerificationRisk::Low,
        };
        let runnable = runnable_narrow_commands(&plan);
        assert_eq!(runnable.len(), 1);
        assert!(runnable[0].command.contains("context-harness"));
    }

    #[test]
    fn narrow_cargo_command_run_succeeds() {
        let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let plan = VerificationPlan {
            commands: vec![PlannedCommand {
                command: "cargo test -p codex-repo-index".to_string(),
                reason: "test".to_string(),
                scope: PlanScope::Narrow,
                confidence: 1.0,
            }],
            skipped: Vec::new(),
            risk: VerificationRisk::Low,
        };
        let report = run_verification_plan(
            &plan,
            &RunOptions {
                cwd: workspace_root,
                ..RunOptions::default()
            },
        );
        assert_eq!(report.status, VerificationRunStatus::Passed);
        assert_eq!(report.commands[0].exit_code, Some(0));
    }

    #[test]
    fn failing_pytest_produces_failure_packet() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../context-harness/tests/fixtures/e2e_python_calculator");
        let plan = VerificationPlan {
            commands: vec![PlannedCommand {
                command: "python -m pytest tests/test_calculator.py".to_string(),
                reason: "test".to_string(),
                scope: PlanScope::Narrow,
                confidence: 1.0,
            }],
            skipped: Vec::new(),
            risk: VerificationRisk::Low,
        };
        let report = run_verification_plan(
            &plan,
            &RunOptions {
                cwd: fixture,
                ..RunOptions::default()
            },
        );
        assert_eq!(report.status, VerificationRunStatus::Failed);
        assert_eq!(report.commands[0].exit_code, Some(1));
        assert!(report.failure_packet.is_some());
    }
}
