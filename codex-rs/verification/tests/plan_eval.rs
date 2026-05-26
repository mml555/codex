use codex_verification::VerificationPlanner;
use codex_verification::load_plan_fixtures;
use codex_verification::run_plan_eval;

#[test]
fn plan_metrics_change_targets_harness_crate_only() {
    let map = codex_repo_index::RepoMap {
        version: 2,
        repo_id: "t".to_string(),
        root: "/t".to_string(),
        files: Vec::new(),
        tests: Vec::new(),
        areas: Vec::new(),
        packages: Vec::new(),
        area_maps: vec![codex_repo_index::AreaMap {
            area_id: "context-harness".to_string(),
            root_paths: vec!["context-harness/".to_string()],
            owned_files: vec!["context-harness/src/metrics.rs".to_string()],
            test_paths: Vec::new(),
            related_cli_paths: vec!["cli/src/context_cmd.rs".to_string()],
            negative_paths: vec!["core/".to_string()],
            confidence: 0.9,
        }],
        commands: Vec::new(),
        test_map: Vec::new(),
        agents_md: None,
        warnings: Vec::new(),
    };
    let plan = VerificationPlanner::plan(&["context-harness/src/metrics.rs".to_string()], &map);
    assert!(
        plan.commands
            .iter()
            .any(|cmd| cmd.command == "cargo test -p codex-context-harness")
    );
    assert!(
        !plan
            .commands
            .iter()
            .any(|cmd| cmd.command.contains("codex-core"))
    );
    assert!(
        plan.skipped
            .iter()
            .any(|cmd| cmd.command.contains("workspace"))
    );
}

#[test]
fn plan_eval_fixtures_meet_accuracy_target() {
    let fixtures = load_plan_fixtures(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/plan_codex.json"
    )))
    .unwrap();
    let map = codex_repo_index::RepoMap {
        version: 2,
        repo_id: "fixture".to_string(),
        root: "/fixture".to_string(),
        files: Vec::new(),
        tests: Vec::new(),
        areas: Vec::new(),
        packages: Vec::new(),
        area_maps: vec![
            codex_repo_index::AreaMap {
                area_id: "context-harness".to_string(),
                root_paths: vec!["context-harness/".to_string()],
                owned_files: vec![
                    "context-harness/src/metrics.rs".to_string(),
                    "context-harness/tests/eval_codex_fixtures.rs".to_string(),
                ],
                test_paths: vec!["context-harness/tests/eval_codex_fixtures.rs".to_string()],
                related_cli_paths: vec!["cli/src/context_cmd.rs".to_string()],
                negative_paths: vec!["core/".to_string(), "tui/".to_string()],
                confidence: 0.9,
            },
            codex_repo_index::AreaMap {
                area_id: "cli".to_string(),
                root_paths: vec!["cli/".to_string()],
                owned_files: vec!["cli/src/context_cmd.rs".to_string()],
                test_paths: Vec::new(),
                related_cli_paths: Vec::new(),
                negative_paths: vec!["context-harness/".to_string()],
                confidence: 0.85,
            },
            codex_repo_index::AreaMap {
                area_id: "core".to_string(),
                root_paths: vec!["core/".to_string()],
                owned_files: vec!["core/src/prompt_debug.rs".to_string()],
                test_paths: Vec::new(),
                related_cli_paths: Vec::new(),
                negative_paths: vec!["context-harness/".to_string()],
                confidence: 0.85,
            },
            codex_repo_index::AreaMap {
                area_id: "ext/repo-intelligence".to_string(),
                root_paths: vec!["ext/repo-intelligence/".to_string()],
                owned_files: vec!["ext/repo-intelligence/src/extension.rs".to_string()],
                test_paths: Vec::new(),
                related_cli_paths: vec!["app-server/src/extensions.rs".to_string()],
                negative_paths: vec!["tui/".to_string()],
                confidence: 0.88,
            },
        ],
        commands: Vec::new(),
        test_map: Vec::new(),
        agents_md: None,
        warnings: Vec::new(),
    };
    let report = run_plan_eval(&fixtures, &map);
    assert!(
        report.avg_accuracy >= 0.70,
        "avg accuracy {:.2}",
        report.avg_accuracy
    );
    for fixture in &report.fixtures {
        assert!(
            fixture.forbidden_hits.is_empty(),
            "forbidden hits {:?} for {:?}",
            fixture.forbidden_hits,
            fixture.changed_files
        );
    }
}

#[test]
fn planned_commands_include_reason_and_confidence() {
    let map = codex_repo_index::RepoMap {
        version: 2,
        repo_id: "t".to_string(),
        root: "/t".to_string(),
        files: Vec::new(),
        tests: Vec::new(),
        areas: Vec::new(),
        packages: Vec::new(),
        area_maps: vec![codex_repo_index::AreaMap {
            area_id: "context-harness".to_string(),
            root_paths: vec!["context-harness/".to_string()],
            owned_files: vec!["context-harness/src/metrics.rs".to_string()],
            test_paths: Vec::new(),
            related_cli_paths: Vec::new(),
            negative_paths: Vec::new(),
            confidence: 0.9,
        }],
        commands: Vec::new(),
        test_map: Vec::new(),
        agents_md: None,
        warnings: Vec::new(),
    };
    let plan = codex_verification::VerificationPlanner::plan(
        &["context-harness/src/metrics.rs".to_string()],
        &map,
    );
    assert!(!plan.commands.is_empty());
    for cmd in &plan.commands {
        assert!(!cmd.reason.is_empty());
        assert!(cmd.confidence > 0.0 && cmd.confidence <= 1.0);
        assert_eq!(cmd.scope, codex_verification::PlanScope::Narrow);
    }
}
