use codex_context_harness::BuildPacketOptions;
use codex_context_harness::TokenBudget;
use codex_context_harness::build_post_failure_context_packet;
use codex_context_harness::render_post_failure_prompt_fragment;
use codex_verification::load_verification_run_report;
use codex_verification::post_failure_context_from_report;

#[test]
fn report_fixture_produces_post_failure_prompt() {
    let report = load_verification_run_report(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/report_failed.json"
    )))
    .unwrap();
    let failure =
        post_failure_context_from_report(&report, "fix context harness metrics", &[]).unwrap();

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

    let packet = build_post_failure_context_packet(
        &map,
        &failure,
        BuildPacketOptions {
            token_budget: TokenBudget { limit: 12_000 },
            ..BuildPacketOptions::default()
        },
    );
    let fragment = render_post_failure_prompt_fragment(&packet, &failure);
    assert!(fragment.contains("Verification failed:"));
    assert!(fragment.contains("Repair hint:"));
    assert_eq!(
        failure.repair_hint.likely_failure_type,
        codex_context_harness::FailureType::TestAssertionFailure
    );
    assert!(fragment.contains("Why it was run:"));
    assert!(fragment.contains("context-harness/src/metrics.rs"));
    assert!(!fragment.contains("cargo test --workspace"));
}
