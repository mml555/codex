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
fn report_renders_human_summary_with_8_column_table() {
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
    // Header columns are present in the rendered table.
    for column in [
        "Task",
        "Valid?",
        "RI visible?",
        "Target files V/RI",
        "Extra files V/RI",
        "Turns V/RI",
        "Tokens V/RI",
        "Result",
    ] {
        assert!(text.contains(column), "missing column `{column}`:\n{text}");
    }
    // The task row contains the canonical V vs RI values.
    assert!(text.contains("calculator_fix"), "{text}");
    assert!(text.contains("0/1 vs 1/1"), "target column missing:\n{text}");
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
