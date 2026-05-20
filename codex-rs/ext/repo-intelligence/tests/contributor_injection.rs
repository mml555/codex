use std::sync::Arc;

use codex_extension_api::ContextContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::PromptSlot;
use codex_repo_index::RepoMap;
use codex_repo_intelligence_extension::RepoIntelligenceExtension;
use codex_repo_intelligence_extension::RepoIntelligenceExtensionConfig;
use codex_utils_absolute_path::AbsolutePathBuf;

fn fixture_map() -> RepoMap {
    let json = include_str!("../../../context-harness/tests/fixtures/repo_map_restaurant.json");
    serde_json::from_str(json).expect("fixture RepoMap")
}

#[tokio::test]
async fn contributor_emits_contextual_user_harness_fragment_when_enabled() {
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
    assert!(text.contains("Harness repo context:"));
    assert!(!text.contains("<codex-context-packet>"));
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

#[tokio::test]
async fn contributor_returns_empty_without_thread_config() {
    let extension = Arc::new(RepoIntelligenceExtension);
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("thread");
    let fragments = extension.contribute(&session_store, &thread_store).await;
    assert!(fragments.is_empty());
}
