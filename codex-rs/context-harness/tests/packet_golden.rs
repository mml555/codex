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
fn renderer_emits_directive_routing_fragment() {
    let map = fixture_map();
    let packet = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    let fragment = ContextPacketRenderer::render_prompt_fragment(&packet);

    // New directive header + routing language.
    assert!(fragment.contains("Harness repo intelligence:"));
    assert!(fragment.contains("Use this as task-routing guidance before editing."));
    assert!(fragment.contains("Task: fix restaurant search pagination"));
    assert!(fragment.contains("Before editing, inspect these files first:"));
    // Inspect entries are numbered (`1. `, `2. `, ...) with a reason after `—`.
    let numbered_lines: Vec<_> = fragment
        .lines()
        .filter(|line| line.starts_with("1. ") || line.starts_with("2. "))
        .collect();
    assert!(
        !numbered_lines.is_empty(),
        "expected numbered inspect entries:\n{fragment}"
    );
    assert!(
        numbered_lines.iter().all(|line| line.contains(" — ")),
        "every numbered entry must carry a `<path> — <reason>` shape:\n{fragment}"
    );
    // The most-relevant fixture path is in the inspect list.
    assert!(fragment.contains("restaurants.py"), "{fragment}");

    // Background-only sections that used to dominate the fragment must be gone.
    assert!(
        !fragment.contains("Guidance: Treat this as a repo map."),
        "old descriptive guidance line leaked:\n{fragment}"
    );
    assert!(!fragment.contains("Primary files:"), "{fragment}");
    assert!(!fragment.contains("Also considered:"), "{fragment}");
    assert!(!fragment.contains("Likely tests:"), "{fragment}");
    assert!(!fragment.contains("Repo rules:"), "{fragment}");
    assert!(!fragment.contains("Warnings:"), "{fragment}");
    assert!(!fragment.contains("evidence="), "{fragment}");
    assert!(!fragment.contains("<codex-context-packet>"), "{fragment}");
}

#[test]
fn renderer_inspect_list_is_capped_at_five() {
    let map = fixture_map();
    let packet = build_context_packet(
        "fix restaurant search pagination",
        &map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    // Caller asks for more than the hard cap; renderer must clamp to ≤5.
    let fragment = ContextPacketRenderer::render_prompt_fragment_with_caps(
        &packet,
        SelectionCaps {
            max_prompt_included_files: 50,
            ..SelectionCaps::default()
        },
    );

    let inspect_entries = fragment
        .lines()
        .filter(|line| {
            line.chars().next().is_some_and(|c| c.is_ascii_digit())
                && line.contains(". ")
                && line.contains(" — ")
        })
        .count();
    assert!(
        inspect_entries <= ContextPacketRenderer::MAX_INSPECT_FILES,
        "inspect list exceeded {} entries: {inspect_entries}\n{fragment}",
        ContextPacketRenderer::MAX_INSPECT_FILES
    );
}

#[test]
fn renderer_caller_cap_can_shrink_inspect_list_below_max() {
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
            ..SelectionCaps::default()
        },
    );

    let inspect_entries = fragment
        .lines()
        .filter(|line| line.starts_with("1. ") || line.starts_with("2. "))
        .count();
    assert_eq!(
        inspect_entries, 1,
        "caller cap should clamp inspect list to 1 entry:\n{fragment}"
    );
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
