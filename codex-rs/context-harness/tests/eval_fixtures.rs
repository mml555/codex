use codex_context_harness::BuildPacketOptions;
use codex_context_harness::load_eval_fixtures;
use codex_context_harness::run_eval;
use codex_repo_index::RepoMap;
use pretty_assertions::assert_eq;

fn fixture_map() -> RepoMap {
    serde_json::from_str(include_str!("fixtures/repo_map_restaurant.json")).unwrap()
}

#[test]
fn eval_fixture_aggregates_metrics() {
    let fixtures = load_eval_fixtures(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tasks.json"
    )))
    .unwrap();
    let map = fixture_map();
    let report = run_eval(&fixtures, &map, BuildPacketOptions::default());
    assert_eq!(report.task_count, 2);
    assert!(report.avg_recall > 0.0);
    assert!(report.avg_token_estimate > 0);
    assert_eq!(report.tasks.len(), 2);
}
