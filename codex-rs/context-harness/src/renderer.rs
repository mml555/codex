use anyhow::Result;

use crate::packet::ContextItemKind;
use crate::packet::ContextItemState;
use crate::packet::ContextPacket;
use crate::packet::RenderLevel;
use crate::selection::SelectionCaps;

/// Sentinel line that opens every directive repo-intelligence fragment.
/// Used by `prompt_visibility::model_prompt_contains_harness_context` and by
/// downstream tooling that needs to detect whether harness context reached
/// the model.
pub const HARNESS_MARKER: &str = "Harness repo intelligence:";

pub struct ContextPacketRenderer;

impl ContextPacketRenderer {
    pub fn render_json(packet: &ContextPacket) -> Result<String> {
        Ok(serde_json::to_string_pretty(packet)?)
    }

    pub fn render_prompt_fragment(packet: &ContextPacket) -> String {
        Self::render_prompt_fragment_with_caps(packet, SelectionCaps::default())
    }

    /// Max files in the "Before editing, inspect these files first" list.
    /// Kept small (≤5) so the directive stays operational, not descriptive.
    pub const MAX_INSPECT_FILES: usize = 5;

    pub fn render_prompt_fragment_with_caps(packet: &ContextPacket, caps: SelectionCaps) -> String {
        let mut lines = vec![
            HARNESS_MARKER.to_string(),
            "Use this as task-routing guidance before editing.".to_string(),
            String::new(),
            format!("Task: {}", shorten_for_prompt(&packet.task.raw, 240)),
        ];

        let mut file_items: Vec<_> = packet
            .items
            .iter()
            .filter(|item| prompt_visible_item(item, ContextItemKind::FileSummary))
            .filter(|item| item.path.is_some())
            .collect();
        sort_prompt_items(&mut file_items);
        let inspect_cap = caps.max_prompt_included_files.min(Self::MAX_INSPECT_FILES);
        file_items.truncate(inspect_cap);

        if !file_items.is_empty() {
            lines.push(String::new());
            lines.push("Before editing, inspect these files first:".to_string());
            for (idx, item) in file_items.iter().enumerate() {
                let path = item.path.as_deref().unwrap_or("");
                let reason = shorten_for_prompt(&item.reason, 96);
                lines.push(format!("{n}. {path} — {reason}", n = idx + 1));
            }
        }

        let likely_area = packet
            .warnings
            .iter()
            .find_map(|w| w.strip_prefix("Likely area: ").map(str::to_string))
            .or_else(|| infer_area_from_included(packet));
        if let Some(area) = likely_area {
            lines.push(String::new());
            lines.push(format!("Likely area: {area}"));
        }

        lines.join("\n")
    }

    pub fn render_human_debug(packet: &ContextPacket) -> String {
        let mut lines = vec![
            format!("Context packet v{}", packet.version),
            format!(
                "Task: {} [{}] (confidence {:.2})",
                packet.task.raw, packet.task.task_type, packet.task.confidence
            ),
            format!("Stage: {:?}", packet.stage),
            format!(
                "Token budget: {}/{}",
                packet.token_budget.used_estimate, packet.token_budget.limit
            ),
            format!(
                "Included items: {} | dropped: {} | budget exhausted: {} | low confidence: {}",
                packet.decision_log.included.len(),
                packet.decision_log.dropped.len(),
                packet.decision_log.budget_exhausted.len(),
                packet.decision_log.low_confidence.len()
            ),
            String::new(),
            "Included:".to_string(),
        ];

        for entry in &packet.decision_log.included {
            let path = entry.path.as_deref().unwrap_or("-");
            lines.push(format!("  + {path}: {}", entry.reason));
        }

        lines.push(String::new());
        lines.push("Dropped:".to_string());
        for entry in &packet.decision_log.dropped {
            let path = entry.path.as_deref().unwrap_or("-");
            lines.push(format!("  - {path}: {}", entry.reason));
        }

        if !packet.decision_log.budget_exhausted.is_empty() {
            lines.push(String::new());
            lines.push("Budget exhausted:".to_string());
            for entry in &packet.decision_log.budget_exhausted {
                let path = entry.path.as_deref().unwrap_or("-");
                lines.push(format!("  ! {path}: {}", entry.reason));
            }
        }

        if !packet.decision_log.low_confidence.is_empty() {
            lines.push(String::new());
            lines.push("Low confidence:".to_string());
            for entry in &packet.decision_log.low_confidence {
                let path = entry.path.as_deref().unwrap_or("-");
                lines.push(format!("  ? {path}: {}", entry.reason));
            }
        }

        lines.join("\n")
    }
}

fn prompt_visible_item(item: &crate::packet::ContextItem, kind: ContextItemKind) -> bool {
    matches!(
        item.state,
        ContextItemState::Included | ContextItemState::Pinned
    ) && item.kind == kind
        && item.render_level != RenderLevel::HiddenDebugOnly
}

fn sort_prompt_items(items: &mut [&crate::packet::ContextItem]) {
    items.sort_by(|a, b| {
        render_level_rank(a.render_level)
            .cmp(&render_level_rank(b.render_level))
            .then_with(|| {
                b.relevance
                    .partial_cmp(&a.relevance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                a.path
                    .as_deref()
                    .unwrap_or("")
                    .cmp(b.path.as_deref().unwrap_or(""))
            })
    });
}

fn shorten_for_prompt(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    if max_chars <= 3 {
        return text.chars().take(max_chars).collect();
    }
    let keep = max_chars.saturating_sub(3);
    let truncated = text.chars().take(keep).collect::<String>();
    format!("{truncated}...")
}

fn render_level_rank(level: RenderLevel) -> u8 {
    match level {
        RenderLevel::Full => 0,
        RenderLevel::Compact => 1,
        RenderLevel::PathOnly => 2,
        RenderLevel::HiddenDebugOnly => 3,
    }
}

fn infer_area_from_included(packet: &ContextPacket) -> Option<String> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for item in &packet.items {
        if item.state != ContextItemState::Included {
            continue;
        }
        let Some(path) = &item.path else {
            continue;
        };
        if let Some(prefix) = path.split('/').next() {
            *counts.entry(prefix.to_string()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(prefix, _)| prefix)
}
