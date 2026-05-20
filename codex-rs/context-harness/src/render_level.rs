use codex_repo_index::RepoMap;

use crate::ownership::ResolvedOwnership;
use crate::packet::ContextItem;
use crate::packet::ContextItemKind;
use crate::packet::ContextItemState;
use crate::packet::RenderLevel;
use crate::selection::SelectionCaps;
use crate::task_scope::TaskScope;
use crate::task_scope::is_bridge_path;

pub fn assign_render_levels(
    items: &mut [ContextItem],
    ownership: &ResolvedOwnership,
    _map: &RepoMap,
    caps: SelectionCaps,
) {
    let bridge_primary = ownership.matched_command.is_some();

    let mut ranked: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            matches!(
                item.state,
                ContextItemState::Included | ContextItemState::Pinned
            ) && matches!(
                item.kind,
                ContextItemKind::FileSummary | ContextItemKind::RepoRule
            )
        })
        .map(|(idx, _)| idx)
        .collect();
    ranked.sort_by(|&a, &b| {
        items[b]
            .relevance
            .partial_cmp(&items[a].relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                items[a]
                    .path
                    .as_deref()
                    .unwrap_or("")
                    .cmp(items[b].path.as_deref().unwrap_or(""))
            })
    });

    let mut full_slots = 0usize;
    let mut compact_slots = 0usize;
    for idx in ranked {
        let path = items[idx].path.as_deref().unwrap_or("");
        let bridge = !path.is_empty()
            && (ownership.bridge_paths.contains(path)
                || is_bridge_path(path, ownership.primary_area.as_deref()));
        let level = if bridge {
            bridge_render_level(path, ownership.task_scope, bridge_primary)
        } else if full_slots < caps.max_prompt_full_files {
            full_slots += 1;
            RenderLevel::Full
        } else if compact_slots < caps.max_prompt_compact_files {
            compact_slots += 1;
            RenderLevel::Compact
        } else if path.is_empty() {
            RenderLevel::Compact
        } else {
            RenderLevel::PathOnly
        };
        items[idx].render_level = level;
    }

    for item in items.iter_mut() {
        if !matches!(
            item.state,
            ContextItemState::Included | ContextItemState::Pinned
        ) {
            item.render_level = RenderLevel::HiddenDebugOnly;
        }
    }
}

pub fn estimate_prompt_tokens(fragment: &str, test_count: usize, warning_count: usize) -> u32 {
    let base = fragment.len() as u32 / 4;
    base.saturating_add(test_count as u32 * 24)
        .saturating_add(warning_count as u32 * 16)
        .saturating_add(80)
}

fn bridge_render_level(path: &str, scope: TaskScope, command_primary: bool) -> RenderLevel {
    if path == "cli/src/context_cmd.rs" && command_primary {
        return RenderLevel::Compact;
    }
    if path.starts_with("core/") {
        return RenderLevel::PathOnly;
    }
    match scope {
        TaskScope::AreaPlusBridge if command_primary => RenderLevel::Compact,
        TaskScope::CoreIntegration | TaskScope::CrossArea => RenderLevel::PathOnly,
        _ => RenderLevel::PathOnly,
    }
}

pub fn estimate_item_render_tokens(item: &ContextItem) -> u32 {
    let path_len = item.path.as_ref().map(|p| p.len()).unwrap_or(0) as u32;
    match item.render_level {
        RenderLevel::Full => 90 + path_len / 4 + item.reason.len() as u32 / 4,
        RenderLevel::Compact => 35 + path_len / 4,
        RenderLevel::PathOnly => 10 + path_len / 4,
        RenderLevel::HiddenDebugOnly => 0,
    }
}
