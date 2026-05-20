use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use codex_repo_index::RepoIndexCache;
use codex_repo_index::RepoMapBuilder;
use codex_repo_index::RepoMapBuilderOptions;
use codex_utils_home_dir::find_codex_home;
use codex_verification::PlanRequest;
use codex_verification::RunOptions;
use codex_verification::VerificationPlanner;
use codex_verification::cancelled_run_report;
use codex_verification::load_plan_fixtures;
use codex_verification::render_plan_eval_summary;
use codex_verification::render_run_human;
use codex_verification::run_plan_eval;
use codex_verification::run_verification_plan;
use codex_verification::runnable_narrow_commands;
use codex_verification::verification_exit_code;

#[derive(Debug, Parser)]
pub struct VerificationCli {
    #[command(subcommand)]
    pub subcommand: VerificationSubcommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum VerificationSubcommand {
    /// Build a deterministic verification plan from changed paths (no execution).
    Plan(VerificationPlanCommand),
    /// Build a plan and run narrow commands with explicit approval.
    Run(VerificationRunCommand),
    /// Score verification plan fixtures against a repo map.
    Eval(VerificationEvalCommand),
}

#[derive(Debug, Parser)]
pub struct VerificationPlanCommand {
    /// Changed file paths (repeatable).
    #[arg(long = "changed", required = true)]
    pub changed: Vec<String>,
    /// Optional task description (improves bridge/core scope rules).
    #[arg(long)]
    pub task: Option<String>,
    /// Repository working directory.
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Rebuild the repo index instead of using cache.
    #[arg(long)]
    pub refresh_index: bool,
    /// Write JSON output to a file instead of stdout.
    #[arg(long)]
    pub json_out: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub struct VerificationRunCommand {
    /// Changed file paths (repeatable).
    #[arg(long = "changed", required = true)]
    pub changed: Vec<String>,
    /// Optional task description (improves bridge/core scope rules).
    #[arg(long)]
    pub task: Option<String>,
    /// Repository working directory.
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Rebuild the repo index instead of using cache.
    #[arg(long)]
    pub refresh_index: bool,
    /// Run planned commands without an interactive approval prompt.
    #[arg(long)]
    pub yes: bool,
    /// Human-readable report instead of JSON.
    #[arg(long)]
    pub human: bool,
    /// Per-command timeout in seconds.
    #[arg(long, default_value_t = 600)]
    pub timeout_secs: u64,
    /// Max characters retained per stdout/stderr stream in the report.
    #[arg(long, default_value_t = 24_000)]
    pub max_output_chars: usize,
    /// Write JSON output to a file instead of stdout.
    #[arg(long)]
    pub json_out: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub struct VerificationEvalCommand {
    /// Fixture JSON path.
    #[arg(long)]
    pub fixture: PathBuf,
    /// Optional repo map fixture; otherwise index from cwd.
    #[arg(long)]
    pub map_fixture: Option<PathBuf>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long)]
    pub refresh_index: bool,
}

pub async fn run_verification_command(command: VerificationCli) -> Result<()> {
    match command.subcommand {
        VerificationSubcommand::Plan(cmd) => run_verification_plan_cmd(cmd).await,
        VerificationSubcommand::Run(cmd) => run_verification_run(cmd).await,
        VerificationSubcommand::Eval(cmd) => run_verification_eval(cmd).await,
    }
}

pub async fn run_verification_plan_cmd(cmd: VerificationPlanCommand) -> Result<()> {
    let cwd = resolve_cwd(cmd.cwd)?;
    let map = load_or_build_map(&cwd, cmd.refresh_index)?;
    let request = PlanRequest {
        task: cmd.task,
        changed_paths: cmd.changed,
    };
    let plan = VerificationPlanner::plan_with_request(&map, &request);
    let output = serde_json::to_string_pretty(&plan)?;
    if let Some(path) = cmd.json_out {
        std::fs::write(&path, &output).with_context(|| format!("write {}", path.display()))?;
    } else {
        println!("{output}");
    }
    Ok(())
}

pub async fn run_verification_run(cmd: VerificationRunCommand) -> Result<()> {
    let cwd = resolve_cwd(cmd.cwd)?;
    let map = load_or_build_map(&cwd, cmd.refresh_index)?;
    let changed_files = cmd.changed.clone();
    let request = PlanRequest {
        task: cmd.task,
        changed_paths: changed_files.clone(),
    };
    let plan = VerificationPlanner::plan_with_request(&map, &request);
    let runnable = runnable_narrow_commands(&plan);

    print_planned_commands(&plan, runnable.len());

    if runnable.is_empty() {
        anyhow::bail!(
            "no narrow, safe commands to run; use `codex verification plan` to inspect skipped commands"
        );
    }

    if !cmd.yes && !prompt_run_approval(runnable.len())? {
        let report = cancelled_run_report(&plan, &changed_files);
        emit_run_report(&report, cmd.human, cmd.json_out.as_deref())?;
        std::process::exit(verification_exit_code(&report));
    }

    let options = RunOptions {
        cwd: cwd.clone(),
        timeout_per_command: Duration::from_secs(cmd.timeout_secs),
        max_stream_chars: cmd.max_output_chars,
        max_relevant_output_chars: codex_verification::DEFAULT_MAX_RELEVANT_OUTPUT_CHARS,
        changed_files,
    };
    let report = run_verification_plan(&plan, &options);
    emit_run_report(&report, cmd.human, cmd.json_out.as_deref())?;
    std::process::exit(verification_exit_code(&report));
}

fn print_planned_commands(plan: &codex_verification::VerificationPlan, runnable_count: usize) {
    println!(
        "Verification plan ({} runnable narrow command(s)):",
        runnable_count
    );
    for (index, cmd) in plan.commands.iter().enumerate() {
        let run_marker = if codex_verification::is_safe_to_run(&cmd.command)
            && cmd.scope == codex_verification::PlanScope::Narrow
        {
            "[run]"
        } else {
            "[skip]"
        };
        println!(
            "  {}. {run_marker} {} — {} (scope {:?}, confidence {:.2})",
            index + 1,
            cmd.command,
            cmd.reason,
            cmd.scope,
            cmd.confidence
        );
    }
    if !plan.skipped.is_empty() {
        println!("Skipped by planner:");
        for skipped in &plan.skipped {
            println!("  - {} — {}", skipped.command, skipped.reason);
        }
    }
}

fn prompt_run_approval(command_count: usize) -> Result<bool> {
    print!("Run {command_count} narrow command(s)? [y/N] ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let answer = line.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

fn emit_run_report(
    report: &codex_verification::VerificationRunReport,
    human: bool,
    json_out: Option<&Path>,
) -> Result<()> {
    let output = if human {
        render_run_human(report)
    } else {
        serde_json::to_string_pretty(report)?
    };
    if let Some(path) = json_out {
        std::fs::write(path, &output).with_context(|| format!("write {}", path.display()))?;
    } else {
        println!("{output}");
    }
    Ok(())
}

pub async fn run_verification_eval(cmd: VerificationEvalCommand) -> Result<()> {
    let map = if let Some(path) = cmd.map_fixture {
        load_map_fixture(&path)?
    } else {
        let cwd = resolve_cwd(cmd.cwd)?;
        load_or_build_map(&cwd, cmd.refresh_index)?
    };
    let fixtures = load_plan_fixtures(&cmd.fixture)?;
    let report = run_plan_eval(&fixtures, &map);
    println!("{}", render_plan_eval_summary(&report));
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn resolve_cwd(cwd: Option<PathBuf>) -> Result<PathBuf> {
    Ok(cwd.unwrap_or_else(|| std::env::current_dir().expect("cwd")))
}

fn load_map_fixture(path: &Path) -> Result<codex_repo_index::RepoMap> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn load_or_build_map(cwd: &Path, refresh: bool) -> Result<codex_repo_index::RepoMap> {
    let map = RepoMapBuilder::build_with_options(cwd, RepoMapBuilderOptions { refresh })?;
    let cache = RepoIndexCache::new(find_codex_home()?.as_path());
    let _ = cache.store(&map);
    Ok(map)
}
