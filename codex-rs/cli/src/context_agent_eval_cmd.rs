//! Offline artifact scoring for vanilla vs harness-context agent runs.
//!
//! `agent-eval score` reads fixture labels and per-arm `record.json` files only.
//! It does not start Codex, mutate a repo, or use the network.

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Parser;
use codex_context_harness::AgentArm;
use codex_context_harness::AgentEvalReport;
use codex_context_harness::AgentRunRecord;
use codex_context_harness::SearchProxyReportRow;
use codex_context_harness::build_report;
use codex_context_harness::compare_task;
use codex_context_harness::load_agent_eval_tasks;
use codex_context_harness::render_agent_eval_human;
use codex_context_harness::render_search_proxy_table;
use codex_context_harness::search_proxy_report_row;

#[derive(Debug, Parser)]
pub struct ContextAgentEvalCli {
    #[command(subcommand)]
    pub subcommand: ContextAgentEvalSubcommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum ContextAgentEvalSubcommand {
    /// Score vanilla vs harness run records under an artifacts directory.
    Score(ContextAgentEvalScoreCommand),
}

#[derive(Debug, Parser)]
pub struct ContextAgentEvalScoreCommand {
    /// Task fixture (same tasks for both arms).
    #[arg(long)]
    pub fixture: PathBuf,
    /// Directory with `{task_id}/vanilla/record.json` and treatment arm `record.json`.
    #[arg(long)]
    pub artifacts_dir: PathBuf,
    /// Treatment arm: `harness` (manual prefix) or `repo_intelligence` (session injection).
    #[arg(long)]
    pub treatment_arm: Option<String>,
    #[arg(long)]
    pub human: bool,
    #[arg(long)]
    pub json_out: Option<PathBuf>,
}

pub async fn run_context_agent_eval(command: ContextAgentEvalCli) -> Result<()> {
    match command.subcommand {
        ContextAgentEvalSubcommand::Score(cmd) => run_agent_eval_score(cmd).await,
    }
}

async fn run_agent_eval_score(cmd: ContextAgentEvalScoreCommand) -> Result<()> {
    let tasks = load_agent_eval_tasks(&cmd.fixture)?;
    let task_ids: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let treatment_arm = resolve_treatment_arm(&cmd, &task_ids)?;
    validate_artifacts_layout(&cmd.artifacts_dir, &task_ids, treatment_arm)?;
    let mut comparisons = Vec::new();
    let mut sp_rows: Vec<SearchProxyReportRow> = Vec::new();
    for task in &tasks {
        let vanilla = load_run_record(&cmd.artifacts_dir, &task.id, AgentArm::Vanilla)?;
        let treatment = load_run_record(&cmd.artifacts_dir, &task.id, treatment_arm)?;
        let comparison = compare_task(task, &vanilla, &treatment);
        if treatment_arm == AgentArm::SearchProxy {
            sp_rows.push(search_proxy_report_row(task, &vanilla, &treatment, &comparison));
        }
        comparisons.push(comparison);
    }
    let report = build_report(comparisons);
    emit_report(&cmd, &report)?;

    // Search-proxy runs carry per-arm interception metrics that the generic
    // Main/Cost tables don't surface. Print the focused table (human mode)
    // so reviewers don't have to dig through record.json by hand.
    if cmd.human && treatment_arm == AgentArm::SearchProxy && !sp_rows.is_empty() {
        println!("{}", render_search_proxy_table(&sp_rows));
    }
    Ok(())
}

fn resolve_treatment_arm(
    cmd: &ContextAgentEvalScoreCommand,
    task_ids: &[String],
) -> Result<AgentArm> {
    if let Some(ref name) = cmd.treatment_arm {
        return parse_treatment_arm(name);
    }
    detect_treatment_arm(&cmd.artifacts_dir, task_ids)
}

fn parse_treatment_arm(name: &str) -> Result<AgentArm> {
    match name {
        "harness" => Ok(AgentArm::Harness),
        "repo_intelligence" => Ok(AgentArm::RepoIntelligence),
        "search_proxy" => Ok(AgentArm::SearchProxy),
        other => {
            bail!(
                "unknown treatment arm {other:?}; expected \"harness\", \"repo_intelligence\", or \"search_proxy\""
            )
        }
    }
}

fn detect_treatment_arm(artifacts_dir: &Path, task_ids: &[String]) -> Result<AgentArm> {
    let Some(task_id) = task_ids.first() else {
        bail!("fixture has no tasks");
    };
    // Detection order is the set of possible treatment arms; the first
    // one with a record.json on disk wins. Vanilla is never a
    // treatment arm so it's excluded here.
    for arm in [
        AgentArm::RepoIntelligence,
        AgentArm::SearchProxy,
        AgentArm::Harness,
    ] {
        let candidate = artifacts_dir
            .join(task_id)
            .join(arm.artifact_dir())
            .join("record.json");
        if candidate.is_file() {
            return Ok(arm);
        }
    }
    bail!(
        "could not detect treatment arm under {}; expected {}, {}, or {}",
        artifacts_dir.display(),
        AgentArm::RepoIntelligence.artifact_dir(),
        AgentArm::SearchProxy.artifact_dir(),
        AgentArm::Harness.artifact_dir()
    );
}

fn load_run_record(base: &Path, task_id: &str, arm: AgentArm) -> Result<AgentRunRecord> {
    let path = base
        .join(task_id)
        .join(arm.artifact_dir())
        .join("record.json");
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

fn emit_report(cmd: &ContextAgentEvalScoreCommand, report: &AgentEvalReport) -> Result<()> {
    let payload = if cmd.human {
        render_agent_eval_human(report)
    } else {
        serde_json::to_string_pretty(report)?
    };
    if let Some(path) = &cmd.json_out {
        std::fs::write(path, &payload).with_context(|| format!("write {}", path.display()))?;
    } else {
        println!("{payload}");
    }
    Ok(())
}

pub fn validate_artifacts_layout(
    artifacts_dir: &Path,
    task_ids: &[String],
    treatment_arm: AgentArm,
) -> Result<()> {
    for task_id in task_ids {
        for arm in [AgentArm::Vanilla, treatment_arm] {
            let path = artifacts_dir
                .join(task_id)
                .join(arm.artifact_dir())
                .join("record.json");
            if !path.is_file() {
                bail!("missing artifact: {}", path.display());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_treatment_arm_accepts_all_three_arms() {
        assert_eq!(parse_treatment_arm("harness").unwrap(), AgentArm::Harness);
        assert_eq!(
            parse_treatment_arm("repo_intelligence").unwrap(),
            AgentArm::RepoIntelligence
        );
        assert_eq!(
            parse_treatment_arm("search_proxy").unwrap(),
            AgentArm::SearchProxy
        );
    }

    #[test]
    fn parse_treatment_arm_rejects_unknown() {
        let err = parse_treatment_arm("nonsense").unwrap_err().to_string();
        assert!(err.contains("search_proxy"), "error should list the arms: {err}");
    }

    #[test]
    fn search_proxy_addendum_reports_metrics_and_savings() {
        let mut record = sample_search_proxy_record();
        record.search_proxy_substitutions = 3;
        record.search_proxy_escape_hatch_repeats = 3;
        record.search_proxy_compact_bytes = 3_389;
        record.search_proxy_raw_bytes_estimated = 41_218;
        record.search_proxy_top_files = vec!["context-harness/src/agent_eval.rs".to_string()];
        let out = render_search_proxy_addendum(&[record]).expect("addendum");
        assert!(out.contains("==== Search proxy ===="), "{out}");
        assert!(out.contains("subs=3"), "{out}");
        assert!(out.contains("escape_hatch_repeats=3"), "{out}");
        assert!(out.contains("saves 92% vs raw"), "{out}");
        assert!(out.contains("context-harness/src/agent_eval.rs"), "{out}");
    }

    #[test]
    fn search_proxy_addendum_warns_when_feature_never_enabled() {
        let mut record = sample_search_proxy_record();
        record.search_proxy_enabled = false;
        let out = render_search_proxy_addendum(&[record]).expect("addendum");
        assert!(
            out.contains("WARNING: search_proxy feature not enabled"),
            "{out}"
        );
    }

    fn sample_search_proxy_record() -> AgentRunRecord {
        serde_json::from_str(
            r#"{
                "arm": "search_proxy",
                "task_id": "t",
                "changed_files": [],
                "tests_passed": false,
                "turn_count": null,
                "search_proxy_enabled": true
            }"#,
        )
        .expect("sample record")
    }
}
