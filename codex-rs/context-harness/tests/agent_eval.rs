use std::path::PathBuf;

use codex_context_harness::AgentArm;
use codex_context_harness::AgentRunRecord;
use codex_context_harness::FailureRecoveryQuality;
use codex_context_harness::build_report;
use codex_context_harness::compare_task;
use codex_context_harness::load_agent_eval_tasks;
use codex_context_harness::render_agent_eval_human;
use pretty_assertions::assert_eq;

fn fixture_tasks() -> Vec<codex_context_harness::AgentEvalTask> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agent_eval_tasks.json");
    load_agent_eval_tasks(&path).expect("fixture tasks")
}

#[test]
fn fixture_tasks_load_with_ids() {
    let tasks = fixture_tasks();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id, "calculator_fix");
    assert!(!tasks[0].requires_post_failure);
    assert!(tasks[1].requires_post_failure);
}

#[test]
fn compare_vanilla_vs_harness_on_synthetic_records() {
    let tasks = fixture_tasks();
    let task = &tasks[0];
    let vanilla = AgentRunRecord {
        arm: AgentArm::Vanilla,
        task_id: task.id.clone(),
        changed_files: vec![],
        tests_passed: false,
        turn_count: Some(4),
        used_post_failure: false,
        exec_exit_code: Some(1),
    };
    let harness = AgentRunRecord {
        arm: AgentArm::Harness,
        task_id: task.id.clone(),
        changed_files: vec!["src/calculator.py".to_string()],
        tests_passed: true,
        turn_count: Some(2),
        used_post_failure: false,
        exec_exit_code: Some(0),
    };
    let row = compare_task(task, &vanilla, &harness);
    assert_eq!(row.vanilla.tests_passed, false);
    assert_eq!(row.harness.tests_passed, true);
    assert_eq!(row.harness.unnecessary_files_changed, Vec::<String>::new());
}

#[test]
fn report_renders_human_summary() {
    let tasks = fixture_tasks();
    let task = &tasks[1];
    let vanilla = AgentRunRecord {
        arm: AgentArm::Vanilla,
        task_id: task.id.clone(),
        changed_files: vec![],
        tests_passed: false,
        turn_count: Some(3),
        used_post_failure: false,
        exec_exit_code: Some(1),
    };
    let harness = AgentRunRecord {
        arm: AgentArm::Harness,
        task_id: task.id.clone(),
        changed_files: vec!["src/calculator.py".to_string()],
        tests_passed: true,
        turn_count: Some(2),
        used_post_failure: true,
        exec_exit_code: Some(0),
    };
    let report = build_report(vec![compare_task(task, &vanilla, &harness)]);
    let text = render_agent_eval_human(&report);
    assert!(text.contains("calculator_recovery"));
    assert_eq!(
        report.comparisons[0].harness.failure_recovery_quality,
        FailureRecoveryQuality::Good
    );
}
