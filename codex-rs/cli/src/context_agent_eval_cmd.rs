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
use codex_context_harness::build_report;
use codex_context_harness::compare_task;
use codex_context_harness::load_agent_eval_tasks;
use codex_context_harness::render_agent_eval_human;

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
    /// Directory with `{task_id}/vanilla/record.json` and `{task_id}/harness/record.json`.
    #[arg(long)]
    pub artifacts_dir: PathBuf,
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
    validate_artifacts_layout(&cmd.artifacts_dir, &task_ids)?;
    let mut comparisons = Vec::new();
    for task in &tasks {
        let vanilla = load_run_record(&cmd.artifacts_dir, &task.id, AgentArm::Vanilla)?;
        let harness = load_run_record(&cmd.artifacts_dir, &task.id, AgentArm::Harness)?;
        comparisons.push(compare_task(task, &vanilla, &harness));
    }
    let report = build_report(comparisons);
    emit_report(&cmd, &report)
}

fn load_run_record(base: &Path, task_id: &str, arm: AgentArm) -> Result<AgentRunRecord> {
    let arm_dir = match arm {
        AgentArm::Vanilla => "vanilla",
        AgentArm::Harness => "harness",
    };
    let path = base.join(task_id).join(arm_dir).join("record.json");
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

pub fn validate_artifacts_layout(artifacts_dir: &Path, task_ids: &[String]) -> Result<()> {
    for task_id in task_ids {
        for arm in ["vanilla", "harness"] {
            let path = artifacts_dir.join(task_id).join(arm).join("record.json");
            if !path.is_file() {
                bail!("missing artifact: {}", path.display());
            }
        }
    }
    Ok(())
}
