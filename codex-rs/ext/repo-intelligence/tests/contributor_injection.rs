use std::sync::Arc;

use codex_context_harness::BuildPacketOptions;
use codex_context_harness::RunMemory;
use codex_context_harness::build_context_packet;
use codex_extension_api::ContextContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::PromptSlot;
use codex_extension_api::TurnInputContributor;
use codex_protocol::user_input::UserInput;
use codex_repo_index::RepoMap;
use codex_repo_intelligence_extension::RepoIntelligenceExtension;
use codex_repo_intelligence_extension::RepoIntelligenceExtensionConfig;
use codex_repo_intelligence_extension::narrow_verification_hint;
use codex_utils_absolute_path::AbsolutePathBuf;

fn fixture_map() -> RepoMap {
    let json = include_str!("../../../context-harness/tests/fixtures/repo_map_restaurant.json");
    serde_json::from_str(json).expect("fixture RepoMap")
}

#[tokio::test]
async fn contributor_emits_directive_repo_intelligence_fragment_when_enabled() {
    let extension = RepoIntelligenceExtension;
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("thread");
    thread_store.insert(RepoIntelligenceExtensionConfig {
        enabled: true,
        cwd: AbsolutePathBuf::try_from("/fixture").expect("cwd"),
        cached_map: Some(fixture_map()),
        task_override: Some("fix restaurant search pagination".to_string()),
    });

    let fragments = extension.contribute(&session_store, &thread_store).await;
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].slot(), PromptSlot::ContextualUser);
    let text = fragments[0].text();
    assert!(text.contains("Harness repo intelligence:"));
    assert!(text.contains("Use this as task-routing guidance before editing."));
    assert!(text.contains("Before editing, inspect these files first:"));
    assert!(
        text.contains("After editing, likely narrow verification:"),
        "extension should append the planner-driven verification hint when one exists\n{text}"
    );
    assert!(!text.contains("<codex-context-packet>"));
}

#[test]
fn narrow_verification_hint_present_for_restaurant_task() {
    let map = fixture_map();
    let task = "fix restaurant search pagination";
    let packet = build_context_packet(
        task,
        &map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    let hint = narrow_verification_hint(task, &map, &packet)
        .expect("planner should emit a narrow command for an included Python file");
    assert!(
        hint.starts_with("After editing, likely narrow verification:\n- "),
        "hint must lead with the post-edit directive, got: {hint}"
    );
    assert!(
        hint.contains("python -m pytest tests/")
            && hint.lines().any(|line| line.ends_with(".py")),
        "hint should name a narrow `python -m pytest tests/test_*.py` command, got: {hint}"
    );
}

#[test]
fn narrow_verification_hint_absent_when_no_narrow_command() {
    // Minimal RepoMap: no packages, no files, no test_map.
    // The planner has no signal to produce a narrow command, so the hint
    // must be omitted (not rendered as an empty bullet).
    let empty_map = RepoMap {
        version: 2,
        repo_id: "empty".to_string(),
        root: "/empty".to_string(),
        files: Vec::new(),
        tests: Vec::new(),
        areas: Vec::new(),
        packages: Vec::new(),
        area_maps: Vec::new(),
        commands: Vec::new(),
        test_map: Vec::new(),
        agents_md: None,
        warnings: Vec::new(),
    };
    let task = "do something with no clear area";
    let packet = build_context_packet(
        task,
        &empty_map,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    assert!(
        narrow_verification_hint(task, &empty_map, &packet).is_none(),
        "expected no hint when planner produces no narrow command"
    );
}

#[tokio::test]
async fn contributor_returns_empty_when_disabled() {
    let extension = RepoIntelligenceExtension;
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("thread");
    thread_store.insert(RepoIntelligenceExtensionConfig {
        enabled: false,
        cwd: AbsolutePathBuf::try_from("/fixture").expect("cwd"),
        cached_map: Some(fixture_map()),
        task_override: None,
    });

    let fragments = extension.contribute(&session_store, &thread_store).await;
    assert!(fragments.is_empty());
}

#[test]
fn turn_input_contributor_sets_task_from_user_text() {
    let extension = RepoIntelligenceExtension;
    let thread_store = ExtensionData::new("thread");
    thread_store.insert(RepoIntelligenceExtensionConfig {
        enabled: true,
        cwd: AbsolutePathBuf::try_from("/fixture").expect("cwd"),
        cached_map: None,
        task_override: None,
    });

    extension.prepare_turn_input(
        &thread_store,
        &[UserInput::Text {
            text: "Fix the failing calculator test.".to_string(),
            text_elements: Vec::new(),
        }],
    );

    let config = thread_store
        .get::<RepoIntelligenceExtensionConfig>()
        .expect("config");
    assert_eq!(
        config.task_override.as_deref(),
        Some("Fix the failing calculator test.")
    );
}

#[tokio::test]
async fn contributor_returns_empty_when_map_build_fails() {
    let extension = RepoIntelligenceExtension;
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("thread");
    thread_store.insert(RepoIntelligenceExtensionConfig {
        enabled: true,
        cwd: AbsolutePathBuf::try_from("/nonexistent-codex-repo-intelligence-cwd").expect("cwd"),
        cached_map: None,
        task_override: Some("fix restaurant search pagination".to_string()),
    });

    let fragments = extension.contribute(&session_store, &thread_store).await;
    assert!(fragments.is_empty());
}

#[tokio::test]
async fn contributor_returns_empty_without_thread_config() {
    let extension = Arc::new(RepoIntelligenceExtension);
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("thread");
    let fragments = extension.contribute(&session_store, &thread_store).await;
    assert!(fragments.is_empty());
}
