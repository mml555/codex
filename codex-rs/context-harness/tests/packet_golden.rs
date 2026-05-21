use codex_context_harness::BuildPacketOptions;
use codex_context_harness::ContextPacketRenderer;
use codex_context_harness::EvalLabels;
use codex_context_harness::Metrics;
use codex_context_harness::RunMemory;
use codex_context_harness::SelectionCaps;
use codex_context_harness::TaskType;
use codex_context_harness::build_context_packet;
use codex_context_harness::normalize_packet;
use codex_repo_index::RepoMap;
use pretty_assertions::assert_eq;

fn fixture_map() -> RepoMap {
    let json = include_str!("fixtures/repo_map_restaurant.json");
    serde_json::from_str(json).expect("fixture RepoMap")
}

#[test]
fn golden_packet_shape_for_restaurant_search_task() {
    let map = fixture_map();
    let packet = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );

    assert_eq!(packet.version, 1);
    assert_eq!(packet.task.task_type, TaskType::BugFix.as_str());
    assert!(!packet.decision_log.included.is_empty());
    assert!(!packet.included_paths().is_empty());
    assert!(
        packet
            .included_paths()
            .iter()
            .any(|p| p.contains("restaurants.py"))
    );
    assert!(!packet.selected_tests.is_empty());
}

#[test]
fn renderer_outputs_non_empty_fragments() {
    let map = fixture_map();
    let packet = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    let fragment = ContextPacketRenderer::render_prompt_fragment(&packet);
    assert!(fragment.contains("Harness repo context:"));
    assert!(fragment.contains("Task: fix restaurant search pagination"));
    assert!(fragment.contains("Guidance: Treat this as a repo map."));
    assert!(fragment.contains("Repo rules:"));
    assert!(fragment.contains("- Project AGENTS.md instructions"));
    assert!(fragment.contains("Primary files:"));
    assert!(fragment.contains("restaurants.py"));
    assert!(fragment.contains("why: Path matches"));
    assert!(fragment.contains("relevance="));
    assert!(fragment.contains("evidence="));
    assert!(fragment.contains("path: tests/api/test_restaurants.py; why:"));
    assert!(!fragment.contains("<codex-context-packet>"));
    assert!(!fragment.contains("Primary files:\n- Project AGENTS.md instructions"));
}

#[test]
fn repo_rules_do_not_spend_file_prompt_cap() {
    let map = fixture_map();
    let packet = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    let fragment = ContextPacketRenderer::render_prompt_fragment_with_caps(
        &packet,
        SelectionCaps {
            max_prompt_included_files: 1,
            max_prompt_tests: 0,
            max_prompt_warnings: 0,
            ..SelectionCaps::default()
        },
    );

    assert!(fragment.contains("Repo rules:"));
    assert!(fragment.contains("- Project AGENTS.md instructions"));
    assert!(fragment.contains("Primary files:"));
    assert!(fragment.contains("restaurants.py"));
    assert_eq!(
        fragment
            .lines()
            .filter(|line| line.starts_with("- ") && line.contains('/'))
            .count(),
        1
    );
}

#[test]
fn repo_rules_do_not_spend_primary_file_slots() {
    let map = fixture_map();
    let caps = SelectionCaps {
        max_prompt_full_files: 1,
        max_prompt_compact_files: 0,
        max_prompt_tests: 0,
        max_prompt_warnings: 0,
        ..SelectionCaps::default()
    };
    let packet = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &RunMemory::default(),
        BuildPacketOptions {
            selection: caps,
            ..BuildPacketOptions::default()
        },
    );
    let fragment = ContextPacketRenderer::render_prompt_fragment_with_caps(&packet, caps);

    assert!(fragment.contains("Repo rules:"));
    assert!(fragment.contains("- Project AGENTS.md instructions"));
    assert!(fragment.contains("Primary files:"));
    assert!(fragment.contains("restaurants.py"));
}

#[test]
fn zero_test_prompt_cap_omits_likely_tests_section() {
    let map = fixture_map();
    let packet = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    let fragment = ContextPacketRenderer::render_prompt_fragment_with_caps(
        &packet,
        SelectionCaps {
            max_prompt_tests: 0,
            ..SelectionCaps::default()
        },
    );

    assert!(!fragment.contains("Likely tests:"));
}

#[test]
fn metrics_recall_on_fixture_labels() {
    let map = fixture_map();
    let packet = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    let metrics = Metrics::evaluate(
        &packet,
        &EvalLabels {
            relevant_files: vec![
                "backend/routes/restaurants.py".to_string(),
                "backend/services/restaurant_search.py".to_string(),
            ],
            relevant_tests: vec!["tests/api/test_restaurants.py".to_string()],
            bridge_files: Vec::new(),
        },
    );
    assert!(metrics.relevant_file_recall >= 0.5);
    assert!(metrics.context_waste < 0.75);
    assert!(metrics.test_selection_accuracy >= 0.6);
}

#[test]
fn normalization_is_stable_across_runs() {
    let map = fixture_map();
    let mut first = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    let mut second = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    normalize_packet(&mut first);
    normalize_packet(&mut second);
    let first_json = serde_json::to_string(&first).unwrap();
    let second_json = serde_json::to_string(&second).unwrap();
    assert_eq!(first_json, second_json);
}
