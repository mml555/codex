use codex_context_harness::BuildPacketOptions;
use codex_context_harness::ContextPacketRenderer;
use codex_context_harness::RenderLevel;
use codex_context_harness::build_context_packet;
use codex_context_harness::load_eval_fixtures;
use codex_context_harness::run_eval;
use codex_repo_index::RepoMap;

fn codex_harness_map() -> RepoMap {
    serde_json::from_str(include_str!("fixtures/repo_map_codex_harness.json")).unwrap()
}

#[test]
fn prompt_uses_primary_and_also_considered_sections() {
    let map = codex_harness_map();
    let packet = build_context_packet(
        "add codex context-harness eval command with fixture metrics",
        &map,
        &Default::default(),
        BuildPacketOptions::default(),
    );
    let fragment = ContextPacketRenderer::render_prompt_fragment(&packet);
    assert!(fragment.contains("Primary files:"));
    assert!(fragment.contains("context-harness/src/eval.rs"));

    let packet_rendering = build_context_packet(
        "improve context packet prompt fragment rendering for models",
        &map,
        &Default::default(),
        BuildPacketOptions::default(),
    );
    let fragment_rendering = ContextPacketRenderer::render_prompt_fragment(&packet_rendering);
    assert!(fragment_rendering.contains("Also considered:"));
}

#[test]
fn render_levels_compress_bridge_and_tail_files() {
    let map = codex_harness_map();
    let packet = build_context_packet(
        "improve context packet prompt fragment rendering for models",
        &map,
        &Default::default(),
        BuildPacketOptions::default(),
    );
    let levels: Vec<_> = packet
        .items
        .iter()
        .filter(|i| i.path.is_some())
        .map(|i| i.render_level)
        .collect();
    assert!(levels.iter().any(|l| *l == RenderLevel::Full));
    assert!(
        levels
            .iter()
            .any(|l| matches!(*l, RenderLevel::Compact | RenderLevel::PathOnly))
    );
}

#[test]
fn codex_live_fixture_map_token_estimate_under_budget() {
    let map = codex_harness_map();
    let fixtures = load_eval_fixtures(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tasks_codex_live.json"
    )))
    .unwrap();
    let report = run_eval(&fixtures, &map, BuildPacketOptions::default());
    assert!(
        report.avg_token_estimate < 3500,
        "avg tokens {}",
        report.avg_token_estimate
    );
}
