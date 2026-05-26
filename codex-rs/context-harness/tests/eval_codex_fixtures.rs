use codex_context_harness::BuildPacketOptions;
use codex_context_harness::load_eval_fixtures;
use codex_context_harness::run_eval;
use codex_repo_index::RepoMap;

#[test]
fn codex_labeled_tasks_parse_with_gold_aliases() {
    let fixtures = load_eval_fixtures(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tasks_codex_live.json"
    )))
    .unwrap();
    assert_eq!(fixtures.len(), 4);
    assert!(!fixtures[0].danger_zones.is_empty());
}

#[test]
fn codex_fixture_eval_against_restaurant_map_for_synthetic_task() {
    let map: RepoMap =
        serde_json::from_str(include_str!("fixtures/repo_map_restaurant.json")).unwrap();
    let fixtures = load_eval_fixtures(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tasks_synthetic_restaurant.json"
    )))
    .unwrap();
    let restaurant_only = fixtures;
    let report = run_eval(&restaurant_only, &map, BuildPacketOptions::default());
    assert_eq!(report.task_count, 1);
    assert!(report.avg_recall >= 0.5);
}
