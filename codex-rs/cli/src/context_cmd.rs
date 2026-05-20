use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use codex_context_harness::BuildPacketOptions;
use codex_context_harness::ContextPacketRenderer;
use codex_context_harness::RunMemory;
use codex_context_harness::TokenBudget;
use codex_context_harness::build_context_packet;
use codex_context_harness::build_post_failure_context_packet;
use codex_context_harness::estimate_tokens_from_prompt_json;
use codex_context_harness::extract_paths_from_prompt_json;
use codex_context_harness::load_eval_fixtures;
use codex_context_harness::render_eval_human;
use codex_context_harness::render_eval_summary;
use codex_context_harness::render_post_failure_prompt_fragment;
use codex_context_harness::run_eval;
use codex_core::config::Config;
use codex_core::config::ConfigBuilder;
use codex_core::config::ConfigOverrides;
use codex_protocol::user_input::UserInput;
use codex_repo_index::RepoIndexCache;
use codex_repo_index::RepoMapBuilder;
use codex_repo_index::RepoMapBuilderOptions;
use codex_utils_home_dir::find_codex_home;
use codex_verification::PlanRequest;
use codex_verification::VerificationPlanner;
use codex_verification::load_verification_run_report;
use codex_verification::post_failure_context_from_report;

#[derive(Debug, Parser)]
pub struct ContextCli {
    #[command(subcommand)]
    pub subcommand: ContextSubcommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum ContextSubcommand {
    /// Build a harness context packet for a task (JSON).
    Build(ContextBuildCommand),
    /// Compare harness packet paths against vanilla prompt-input.
    DiffPrompt(ContextDiffPromptCommand),
    /// Run fixture-based harness metrics (recall, waste, test accuracy).
    Eval(ContextEvalCommand),
}

#[derive(Debug, Parser)]
pub struct ContextBuildCommand {
    /// Task description.
    pub task: String,
    /// Repository working directory.
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Write JSON output to a file instead of stdout.
    #[arg(long)]
    pub json_out: Option<PathBuf>,
    /// Rebuild the repo index instead of using cache.
    #[arg(long)]
    pub refresh_index: bool,
    /// Print human-readable debug output.
    #[arg(long)]
    pub human: bool,
    /// Print only the model-visible prompt fragment.
    #[arg(long)]
    pub prompt_fragment: bool,
    /// Token budget limit for context packing.
    #[arg(long, default_value_t = 12_000)]
    pub token_budget: u32,
    /// Changed file paths used when attaching a verification plan.
    #[arg(long = "changed")]
    pub changed: Vec<String>,
    /// Include a deterministic verification plan in JSON output.
    #[arg(long)]
    pub with_verification_plan: bool,
    /// Build a post-failure packet from a verification run report JSON file.
    #[arg(long = "with-verification-report", alias = "failure-packet")]
    pub verification_report: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub struct ContextDiffPromptCommand {
    /// Task description.
    pub task: String,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long)]
    pub refresh_index: bool,
    #[arg(long)]
    pub token_budget: Option<u32>,
    /// Print a short human-readable summary instead of JSON only.
    #[arg(long)]
    pub human: bool,
}

#[derive(Debug, Parser)]
pub struct ContextEvalCommand {
    /// JSON fixture with labeled tasks (see context-harness/tests/fixtures/tasks.json).
    #[arg(long)]
    pub fixture: PathBuf,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long)]
    pub refresh_index: bool,
    /// Use a static RepoMap JSON for all tasks (skips repo indexing).
    #[arg(long)]
    pub map_fixture: Option<PathBuf>,
    /// Emit per-task JSON instead of summary lines.
    #[arg(long)]
    pub json: bool,
    /// Per-task metrics with missed gold paths and extras.
    #[arg(long)]
    pub human: bool,
    #[arg(long, default_value_t = 12_000)]
    pub token_budget: u32,
}

pub async fn run_context_command(command: ContextCli) -> Result<()> {
    match command.subcommand {
        ContextSubcommand::Build(cmd) => run_context_build(cmd).await,
        ContextSubcommand::DiffPrompt(cmd) => run_context_diff_prompt(cmd).await,
        ContextSubcommand::Eval(cmd) => run_context_eval(cmd).await,
    }
}

pub async fn run_context_build(cmd: ContextBuildCommand) -> Result<()> {
    let cwd = resolve_cwd(cmd.cwd)?;
    let map = load_or_build_map(&cwd, cmd.refresh_index)?;
    let build_options = BuildPacketOptions {
        token_budget: TokenBudget {
            limit: cmd.token_budget,
        },
        ..BuildPacketOptions::default()
    };

    let post_failure = cmd
        .verification_report
        .as_ref()
        .map(|path| {
            let report = load_verification_run_report(path)?;
            post_failure_context_from_report(&report, &cmd.task, &cmd.changed)
        })
        .transpose()?;

    let packet = if let Some(failure) = &post_failure {
        build_post_failure_context_packet(&map, failure, build_options)
    } else {
        build_context_packet(&cmd.task, &map, &RunMemory::default(), build_options)
    };

    let output = if cmd.prompt_fragment {
        if let Some(failure) = &post_failure {
            render_post_failure_prompt_fragment(&packet, failure)
        } else {
            ContextPacketRenderer::render_prompt_fragment(&packet)
        }
    } else if cmd.human {
        ContextPacketRenderer::render_human_debug(&packet)
    } else if post_failure.is_some() {
        let failure = post_failure.expect("post_failure checked above");
        serde_json::to_string_pretty(&serde_json::json!({
            "packet": packet,
            "post_failure_context": failure,
            "prompt_fragment": render_post_failure_prompt_fragment(&packet, &failure),
        }))?
    } else if cmd.with_verification_plan {
        let plan = if cmd.changed.is_empty() {
            VerificationPlanner::plan_with_request(
                &map,
                &PlanRequest {
                    task: Some(cmd.task.clone()),
                    changed_paths: Vec::new(),
                },
            )
        } else {
            VerificationPlanner::plan_with_context(
                &map,
                &PlanRequest {
                    task: Some(cmd.task.clone()),
                    changed_paths: cmd.changed.clone(),
                },
                &packet,
            )
        };
        serde_json::to_string_pretty(&serde_json::json!({
            "packet": packet,
            "verification_plan": plan,
        }))?
    } else {
        ContextPacketRenderer::render_json(&packet)?
    };

    if let Some(path) = cmd.json_out {
        std::fs::write(&path, &output).with_context(|| format!("write {}", path.display()))?;
    } else {
        println!("{output}");
    }
    Ok(())
}

pub async fn run_context_diff_prompt(cmd: ContextDiffPromptCommand) -> Result<()> {
    let cwd = resolve_cwd(cmd.cwd)?;
    let map = load_or_build_map(&cwd, cmd.refresh_index)?;
    let options = BuildPacketOptions {
        token_budget: TokenBudget {
            limit: cmd.token_budget.unwrap_or(12_000),
        },
        ..BuildPacketOptions::default()
    };
    let packet = build_context_packet(&cmd.task, &map, &RunMemory::default(), options);
    let harness_paths: std::collections::BTreeSet<String> = packet
        .included_paths()
        .into_iter()
        .map(str::to_string)
        .collect();

    let vanilla_json = build_vanilla_prompt_json(&cmd.task, &cwd).await?;
    let vanilla_paths = extract_paths_from_prompt_json(&vanilla_json);
    let vanilla_token_estimate = estimate_tokens_from_prompt_json(&vanilla_json);

    let harness_fragment = ContextPacketRenderer::render_prompt_fragment(&packet);
    let overlap = harness_paths.intersection(&vanilla_paths).count();
    let report = serde_json::json!({
        "task": cmd.task,
        "harness_included_paths": harness_paths.iter().collect::<Vec<_>>(),
        "vanilla_paths": vanilla_paths.iter().collect::<Vec<_>>(),
        "overlap": harness_paths.intersection(&vanilla_paths).collect::<Vec<_>>(),
        "harness_only": harness_paths.difference(&vanilla_paths).collect::<Vec<_>>(),
        "vanilla_only": vanilla_paths.difference(&harness_paths).collect::<Vec<_>>(),
        "harness_token_estimate": packet.token_budget.used_estimate,
        "vanilla_token_estimate": vanilla_token_estimate,
        "dropped_count": packet.decision_log.dropped.len(),
        "budget_exhausted_count": packet.decision_log.budget_exhausted.len(),
        "harness_prompt_fragment": harness_fragment,
    });

    if cmd.human {
        println!(
            "task: {}\nharness paths: {} | vanilla paths: {} | overlap: {}\nharness tokens: {} | vanilla tokens: {}\nharness-only: {}\nvanilla-only: {}",
            cmd.task,
            harness_paths.len(),
            vanilla_paths.len(),
            overlap,
            packet.token_budget.used_estimate,
            vanilla_token_estimate,
            harness_paths.difference(&vanilla_paths).count(),
            vanilla_paths.difference(&harness_paths).count(),
        );
        println!("\n--- harness prompt fragment ---\n{harness_fragment}");
    } else {
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    Ok(())
}

pub async fn run_context_eval(cmd: ContextEvalCommand) -> Result<()> {
    let map = if let Some(path) = cmd.map_fixture {
        load_map_fixture(&path)?
    } else {
        let cwd = resolve_cwd(cmd.cwd)?;
        load_or_build_map(&cwd, cmd.refresh_index)?
    };
    let fixtures = load_eval_fixtures(&cmd.fixture)?;
    let report = run_eval(
        &fixtures,
        &map,
        BuildPacketOptions {
            token_budget: TokenBudget {
                limit: cmd.token_budget,
            },
            ..BuildPacketOptions::default()
        },
    );

    if cmd.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if cmd.human {
        println!("{}", render_eval_human(&report));
    } else {
        println!("{}", render_eval_summary(&report));
    }
    Ok(())
}

async fn build_vanilla_prompt_json(task: &str, cwd: &Path) -> Result<String> {
    let overrides = ConfigOverrides {
        cwd: Some(cwd.to_path_buf()),
        ephemeral: Some(true),
        ..Default::default()
    };
    let config = ConfigBuilder::default()
        .harness_overrides(overrides)
        .build()
        .await?;
    let input = vec![UserInput::Text {
        text: task.replace("\r\n", "\n").replace('\r', "\n"),
        text_elements: Vec::new(),
    }];
    let prompt_input = codex_core::build_prompt_input(config, input, None).await?;
    Ok(serde_json::to_string(&prompt_input)?)
}

fn resolve_cwd(cwd: Option<PathBuf>) -> Result<PathBuf> {
    Ok(cwd.unwrap_or_else(|| std::env::current_dir().expect("cwd")))
}

fn load_map_fixture(path: &Path) -> Result<codex_repo_index::RepoMap> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn load_or_build_map(cwd: &Path, refresh: bool) -> Result<codex_repo_index::RepoMap> {
    let cache = RepoIndexCache::new(find_codex_home()?.as_path());
    RepoMapBuilder::build_with_options(
        cwd,
        RepoMapBuilderOptions {
            refresh,
            cache: Some(cache),
        },
    )
}
