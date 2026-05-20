use codex_context_harness::BuildPacketOptions;
use codex_context_harness::EvalLabels;
use codex_context_harness::Metrics;
use codex_context_harness::RunMemory;
use codex_context_harness::build_context_packet;
use codex_repo_index::RepoMap;

#[test]
fn metrics_eval_fixture_recall() {
    let map: RepoMap =
        serde_json::from_str(include_str!("fixtures/repo_map_restaurant.json")).unwrap();
    let packet = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    let metrics = Metrics::evaluate(
        &packet,
        &EvalLabels {
            relevant_files: vec!["backend/routes/restaurants.py".to_string()],
            relevant_tests: vec!["tests/api/test_restaurants.py".to_string()],
            bridge_files: Vec::new(),
        },
    );
    assert!(metrics.relevant_file_recall > 0.0);
    assert!(metrics.token_estimate > 0);
}
