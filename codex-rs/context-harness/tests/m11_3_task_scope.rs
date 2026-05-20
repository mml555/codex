use codex_context_harness::BuildPacketOptions;
use codex_context_harness::ContextPacketRenderer;
use codex_context_harness::TaskScope;
use codex_context_harness::build_context_packet;
use codex_context_harness::build_task_terms;
use codex_context_harness::infer_task_scope;
use codex_context_harness::load_eval_fixtures;
use codex_context_harness::ownership::resolve_ownership;
use codex_context_harness::run_eval;
use codex_repo_index::RepoMap;

fn codex_harness_map() -> RepoMap {
    serde_json::from_str(include_str!("fixtures/repo_map_codex_harness.json")).unwrap()
}

#[test]
fn diff_prompt_task_uses_core_integration_scope() {
    let map = codex_harness_map();
    let task = "make context diff-prompt self-contained without env vars";
    let terms = build_task_terms(task, &map);
    let ownership = resolve_ownership(task, &map, &terms);
    assert_eq!(ownership.task_scope, TaskScope::CoreIntegration);
    assert!(ownership.bridge_paths.contains("cli/src/context_cmd.rs"));
    assert!(ownership.scoped_paths.contains("core/src/prompt_debug.rs"));
}

#[test]
fn extension_task_uses_cross_area_scope() {
    let map = codex_harness_map();
    let task = "wire repo intelligence extension into session prompt assembly";
    let terms = build_task_terms(task, &map);
    let ownership = resolve_ownership(task, &map, &terms);
    assert_eq!(ownership.task_scope, TaskScope::CrossArea);
}

#[test]
fn diff_prompt_fixture_includes_bridge_paths() {
    let map = codex_harness_map();
    let packet = build_context_packet(
        "make context diff-prompt self-contained without env vars",
        &map,
        &Default::default(),
        BuildPacketOptions::default(),
    );
    let paths = packet.included_paths();
    assert!(paths.iter().any(|p| p.contains("prompt_paths.rs")));
    assert!(paths.iter().any(|p| p.contains("context_cmd.rs")));
}

#[test]
fn bridge_file_recall_on_live_fixtures_map() {
    let map = codex_harness_map();
    let fixtures = load_eval_fixtures(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tasks_codex_live.json"
    )))
    .unwrap();
    let report = run_eval(&fixtures, &map, BuildPacketOptions::default());
    assert!(
        report
            .avg_bridge_file_recall
            .is_some_and(|bridge| bridge >= 0.5),
        "bridge recall {:?}",
        report.avg_bridge_file_recall
    );
}

#[test]
fn renderer_task_scope_is_single_area() {
    let map = codex_harness_map();
    let task = "improve context packet prompt fragment rendering for models";
    let terms = build_task_terms(task, &map);
    let scope = infer_task_scope(task, &terms, &resolve_ownership(task, &map, &terms));
    assert_eq!(scope, TaskScope::SingleArea);
    let fragment = ContextPacketRenderer::render_prompt_fragment(&build_context_packet(
        task,
        &map,
        &Default::default(),
        BuildPacketOptions::default(),
    ));
    assert!(!fragment.contains("Task scope:"));
}
