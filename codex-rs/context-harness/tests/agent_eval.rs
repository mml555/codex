use std::path::PathBuf;

use codex_context_harness::AgentArm;
use codex_context_harness::AgentRunRecord;
use codex_context_harness::build_report;
use codex_context_harness::compare_task;
use codex_context_harness::load_agent_eval_tasks;
use codex_context_harness::render_agent_eval_human;
use pretty_assertions::assert_eq;

fn fixture_tasks() -> Vec<codex_context_harness::AgentEvalTask> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent_eval_tasks.json");
    load_agent_eval_tasks(&path).expect("fixture tasks")
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
fn fixture_tasks_load_with_ids() {
    let tasks = fixture_tasks();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "calculator_fix");
}

#[test]
fn compare_vanilla_vs_harness_on_synthetic_records() {
    let tasks = fixture_tasks();
    let task = &tasks[0];
    let vanilla = AgentRunRecord {
        tests_passed: false,
        turn_count: Some(4),
        exec_exit_code: Some(1),
        ..synthetic_record(AgentArm::Vanilla, &task.id)
    };
    let harness = AgentRunRecord {
        changed_files: vec!["src/calculator.py".to_string()],
        tests_passed: true,
        turn_count: Some(2),
        exec_exit_code: Some(0),
        ..synthetic_record(AgentArm::Harness, &task.id)
    };
    let row = compare_task(task, &vanilla, &harness);
    assert_eq!(row.treatment_arm, AgentArm::Harness);
    assert_eq!(row.vanilla.tests_passed, false);
    assert_eq!(row.treatment.tests_passed, true);
    assert_eq!(
        row.treatment.unnecessary_files_changed,
        Vec::<String>::new()
    );
    assert_eq!(row.result.slug(), "ri_better:file_targeting");
}

#[test]
fn report_renders_human_summary_with_main_and_cost_tables() {
    let tasks = fixture_tasks();
    let task = &tasks[0];
    let vanilla = AgentRunRecord {
        tests_passed: false,
        turn_count: Some(3),
        exec_exit_code: Some(1),
        tokens_input: Some(800),
        tokens_output: Some(200),
        tokens_total: Some(1000),
        ..synthetic_record(AgentArm::Vanilla, &task.id)
    };
    let harness = AgentRunRecord {
        changed_files: vec!["src/calculator.py".to_string()],
        tests_passed: true,
        turn_count: Some(2),
        exec_exit_code: Some(0),
        harness_context_visible: true,
        tokens_input: Some(900),
        tokens_output: Some(150),
        tokens_total: Some(1050),
        ..synthetic_record(AgentArm::Harness, &task.id)
    };
    let report = build_report(vec![compare_task(task, &vanilla, &harness)]);
    let text = render_agent_eval_human(&report);
    // Header columns are present in the rendered tables. Main table now
    // separates "Edit targets" (gold only) from "Orient. touched"
    // (bridge ∪ ri_surfaced − gold). Tokens stayed in the Cost table.
    for column in [
        "Task",
        "Valid?",
        "RI visible?",
        "Edit targets V/RI",
        "Orient. touched V/RI",
        "Extra files V/RI",
        "Turns V/RI",
        "Tokens V/RI",
        "Result",
    ] {
        assert!(text.contains(column), "missing column `{column}`:\n{text}");
    }
    // The task row contains the canonical V vs RI values.
    assert!(text.contains("calculator_fix"), "{text}");
    assert!(
        text.contains("0/1 vs 1/1"),
        "edit targets column missing:\n{text}"
    );
    assert!(text.contains("3/2"), "turns column missing:\n{text}");
    assert!(text.contains("1000/1050"), "tokens column missing:\n{text}");
    assert!(
        text.contains("ri_better:file_targeting"),
        "result column missing:\n{text}"
    );
}

#[test]
fn codex_session_fixture_loads() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agent_eval_tasks_codex_session.json");
    let tasks = load_agent_eval_tasks(&path).expect("codex session fixture");
    assert!(tasks.len() >= 5);
    assert!(
        tasks
            .iter()
            .all(|t| matches!(t.workdir, codex_context_harness::AgentEvalWorkdir::CodexRs))
    );
}

#[test]
fn ri_hard_v1_fixture_loads_with_no_prompt_naming_the_gold_file() {
    use codex_context_harness::TaskCategory;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agent_eval_tasks_ri_hard_v1.json");
    let tasks = load_agent_eval_tasks(&path).expect("ri_hard_v1 fixture loads");

    // 5 tasks, all codex_rs workdir, all categorized.
    assert_eq!(tasks.len(), 5, "ri_hard_v1 fixture should hold 5 tasks");
    for t in &tasks {
        assert!(
            matches!(t.workdir, codex_context_harness::AgentEvalWorkdir::CodexRs),
            "task {} must declare workdir=codex_rs",
            t.id
        );
        assert!(
            t.category.is_some(),
            "task {} must declare a category",
            t.id
        );
        assert!(
            !t.relevant_files.is_empty(),
            "task {} must declare gold",
            t.id
        );

        // Anti-leak rule: the prompt MUST NOT contain any gold file path
        // as a substring. The whole point of the hard fixture is that
        // discovery is non-trivial; if the prompt names the file, vanilla
        // gets a free pass and RI's discovery savings are unmeasurable.
        for gold in &t.relevant_files {
            assert!(
                !t.task.contains(gold.as_str()),
                "task {} prompt MUST NOT name its gold file `{}` \
                 (defeats the discovery-cost measurement)",
                t.id,
                gold
            );
        }
    }

    // Categories represented (subset OK; we don't require all 5).
    let mut cats: std::collections::BTreeSet<TaskCategory> = std::collections::BTreeSet::new();
    for t in &tasks {
        if let Some(c) = t.category {
            cats.insert(c);
        }
    }
    assert!(
        cats.len() >= 3,
        "ri_hard_v1 should cover at least 3 distinct categories; got {cats:?}"
    );
}

#[test]
fn ri_v1_fixture_loads_15_tasks_across_five_categories() {
    use codex_context_harness::TaskCategory;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agent_eval_tasks_ri_v1.json");
    let tasks = load_agent_eval_tasks(&path).expect("ri_v1 fixture loads");
    assert_eq!(
        tasks.len(),
        15,
        "ri_v1 fixture must hold exactly 15 tasks (3 per category × 5 categories)"
    );

    // All tasks run against the codex-rs tree — `--isolated-worktrees` is
    // mandatory for this fixture.
    for t in &tasks {
        assert!(
            matches!(t.workdir, codex_context_harness::AgentEvalWorkdir::CodexRs),
            "task {} must declare workdir=codex_rs",
            t.id
        );
        assert!(!t.id.is_empty(), "task missing id");
        assert!(!t.task.is_empty(), "task {} missing prompt text", t.id);
        assert!(
            !t.relevant_files.is_empty(),
            "task {} must declare at least one relevant_file (gold)",
            t.id
        );
        assert!(
            t.category.is_some(),
            "task {} must declare a category",
            t.id
        );
    }

    // Counts per category match the proposal: 3 per category, all 5 covered.
    let mut counts = std::collections::BTreeMap::<TaskCategory, usize>::new();
    for t in &tasks {
        *counts.entry(t.category.unwrap()).or_default() += 1;
    }
    let expected = [
        (TaskCategory::FileRouting, 3),
        (TaskCategory::BridgeWiring, 3),
        (TaskCategory::TestTargeting, 3),
        (TaskCategory::LocalConvention, 3),
        (TaskCategory::CrossModuleOwnership, 3),
    ];
    for (cat, want) in expected {
        let got = counts.get(&cat).copied().unwrap_or(0);
        assert_eq!(
            got,
            want,
            "category {} count: expected {}, got {}",
            cat.slug(),
            want,
            got
        );
    }

    // All task IDs are unique.
    let mut ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        tasks.len(),
        "duplicate task ids in ri_v1 fixture"
    );
}
