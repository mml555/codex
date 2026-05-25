use codex_context_harness::BuildPacketOptions;
use codex_context_harness::ContextPacketRenderer;
use codex_context_harness::SelectionCaps;
use codex_context_harness::build_context_packet;
use codex_context_harness::load_eval_fixtures;
use codex_context_harness::run_eval;
use codex_repo_index::RepoMap;

fn restaurant_map() -> RepoMap {
    serde_json::from_str(include_str!("fixtures/repo_map_restaurant.json")).unwrap()
}

fn codex_harness_map() -> RepoMap {
    serde_json::from_str(include_str!("fixtures/repo_map_codex_harness.json")).unwrap()
}

fn noisy_codex_map() -> RepoMap {
    use codex_repo_index::RepoFileEntry;
    use codex_repo_index::RepoSignals;
    let mut map = codex_harness_map();
    map.files.extend([
        RepoFileEntry {
            path: "core/src/tools/handlers/apply_patch_spec_tests.rs".to_string(),
            signals: RepoSignals::new(0.65),
        },
        RepoFileEntry {
            path: "otel/src/metrics/mod.rs".to_string(),
            signals: RepoSignals::new(0.6),
        },
        RepoFileEntry {
            path: "apply-patch/tests/fixtures/scenarios/001_add_file/patch.txt".to_string(),
            signals: RepoSignals::new(0.5),
        },
    ]);
    map
}

#[test]
fn restaurant_task_meets_m11_targets() {
    let map = restaurant_map();
    let packet = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &Default::default(),
        BuildPacketOptions::default(),
    );
    let metrics = codex_context_harness::Metrics::evaluate(
        &packet,
        &codex_context_harness::EvalLabels {
            relevant_files: vec![
                "backend/routes/restaurants.py".to_string(),
                "backend/services/restaurant_search.py".to_string(),
            ],
            relevant_tests: vec!["tests/api/test_restaurants.py".to_string()],
            bridge_files: Vec::new(),
        },
    );
    assert!(
        metrics.relevant_file_recall >= 0.5,
        "recall {:.2}",
        metrics.relevant_file_recall
    );
    assert!(
        metrics.context_waste < 0.75,
        "waste {:.2}",
        metrics.context_waste
    );
    assert!(
        metrics.test_selection_accuracy >= 0.6,
        "test accuracy {:.2}",
        metrics.test_selection_accuracy
    );
    assert!(packet.included_paths().len() <= 8);
}

#[test]
fn codex_harness_eval_task_improves_relevance_on_fixture_map() {
    let map = codex_harness_map();
    let fixtures = load_eval_fixtures(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tasks_codex_live.json"
    )))
    .unwrap();
    let task = fixtures
        .iter()
        .find(|f| f.task.contains("context-harness eval"))
        .expect("eval task");
    let report = run_eval(
        std::slice::from_ref(task),
        &map,
        BuildPacketOptions::default(),
    );
    let metrics = &report.tasks[0].metrics;
    assert!(
        metrics.relevant_file_recall >= 0.5,
        "recall {:.2}",
        metrics.relevant_file_recall
    );
    assert!(
        metrics.context_waste < 0.75,
        "waste {:.2}",
        metrics.context_waste
    );
}

#[test]
fn prompt_fragment_omits_dropped_and_caps_visible_items() {
    let map = restaurant_map();
    let packet = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &Default::default(),
        BuildPacketOptions::default(),
    );
    let fragment = ContextPacketRenderer::render_prompt_fragment(&packet);
    assert!(!fragment.contains("Dropped:"));
    // Directive shape emits a numbered "Likely edit targets:" list,
    // with the remainder split out under "Orientation only:".
    assert!(fragment.contains("Likely edit targets:"));
    assert!(!fragment.contains("legacy_restaurants"));
    let inspect_entries = fragment
        .lines()
        .filter(|line| {
            line.chars().next().is_some_and(|c| c.is_ascii_digit())
                && line.contains(". ")
                && line.contains(" — ")
        })
        .count();
    assert!(
        inspect_entries
            <= SelectionCaps::default()
                .max_prompt_included_files
                .min(ContextPacketRenderer::MAX_INSPECT_FILES)
    );
}

#[test]
fn fixture_metrics_task_prefers_harness_over_noise_on_busy_map() {
    let map = noisy_codex_map();
    let packet = build_context_packet(
        "add codex context-harness eval command with fixture metrics",
        &map,
        &Default::default(),
        BuildPacketOptions::default(),
    );
    let paths = packet.included_paths();
    assert!(
        paths
            .iter()
            .any(|p| p.contains("context-harness/src/eval.rs")),
        "expected eval.rs, got {paths:?}"
    );
    assert!(
        paths
            .iter()
            .filter(|p| p.contains("context-harness/"))
            .count()
            >= 2
    );
}

#[test]
fn fixture_metrics_task_keeps_context_harness_on_codex_map() {
    let map = codex_harness_map();
    let packet = build_context_packet(
        "add codex context-harness eval command with fixture metrics",
        &map,
        &Default::default(),
        BuildPacketOptions::default(),
    );
    let paths = packet.included_paths();
    assert!(
        paths
            .iter()
            .any(|p| p.contains("context-harness/src/eval.rs")),
        "expected eval.rs, got {paths:?}"
    );
}

#[test]
fn task_term_normalization_matches_kebab_paths() {
    let map = codex_harness_map();
    let packet = build_context_packet(
        "add codex context-harness eval command with fixture metrics",
        &map,
        &Default::default(),
        BuildPacketOptions::default(),
    );
    let paths = packet.included_paths();
    assert!(
        paths
            .iter()
            .any(|p| p.contains("context-harness/src/eval.rs"))
    );
    assert!(
        paths
            .iter()
            .any(|p| p.contains("context-harness/src/metrics.rs"))
    );
    assert!(!paths.iter().any(|p| p.contains("vendor/")));
    assert!(!paths.iter().any(|p| p.contains("examples/")));
}
