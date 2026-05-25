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

/// Header that opens the "likely edit targets" block in the directive
/// fragment. Eval tooling parses the file list back out of the rollout
/// by anchoring on this exact string.
pub const EDIT_TARGETS_HEADER: &str = "Likely edit targets:";

/// Header that opens the "orientation only" block in the directive
/// fragment. Same parsing contract as `EDIT_TARGETS_HEADER`.
pub const ORIENTATION_HEADER: &str = "Orientation only:";

/// Hard scope-constraint footer rendered after the file lists. Added
/// in response to the first cloud batch: the `rate_limit` failure
/// showed the directive needed an explicit "don't broaden" sentence
/// rather than just an enumerated file list.
pub const SCOPE_CONSTRAINT: &str = "Do not broaden the edit scope just because a file appears in orientation context. Prefer the smallest patch that satisfies the task.";

pub struct ContextPacketRenderer;

impl ContextPacketRenderer {
    pub fn render_json(packet: &ContextPacket) -> Result<String> {
        Ok(serde_json::to_string_pretty(packet)?)
    }

    pub fn render_prompt_fragment(packet: &ContextPacket) -> String {
        Self::render_prompt_fragment_with_caps(packet, SelectionCaps::default())
    }

    /// Max combined files across the "Likely edit targets" and
    /// "Orientation only" sections. Kept small (≤5) so the directive
    /// stays operational, not descriptive.
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
            // Split the inspect list into two sections based on
            // `caps.max_edit_targets`. Top-K (by relevance score, since
            // `sort_prompt_items` already ordered the slice) are the
            // "likely edit targets" — the model is told to PREFER these
            // for the actual patch. Remaining files render under
            // "Orientation only" so the model treats them as routing
            // context rather than additional edit candidates. Hard
            // scope-constraint footer closes the block.
            let edit_target_k = caps.max_edit_targets.min(file_items.len());
            let (edit_targets, orientation) = file_items.split_at(edit_target_k);

            lines.push(String::new());
            lines.push(EDIT_TARGETS_HEADER.to_string());
            for (idx, item) in edit_targets.iter().enumerate() {
                let path = item.path.as_deref().unwrap_or("");
                let reason = shorten_for_prompt(&item.reason, 96);
                lines.push(format!("{n}. {path} — {reason}", n = idx + 1));
            }

            if !orientation.is_empty() {
                lines.push(String::new());
                lines.push(ORIENTATION_HEADER.to_string());
                for (idx, item) in orientation.iter().enumerate() {
                    let path = item.path.as_deref().unwrap_or("");
                    let reason = shorten_for_prompt(&item.reason, 96);
                    lines.push(format!("{n}. {path} — {reason}", n = idx + 1));
                }
            }

            lines.push(String::new());
            lines.push(SCOPE_CONSTRAINT.to_string());
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

    /// Parse the `Likely edit targets` and `Orientation only` file lists
    /// back out of a rendered directive fragment. Used by the eval
    /// runner to populate `ri_surfaced_edit_targets` /
    /// `ri_surfaced_orientation` on a record from the rollout's
    /// user/developer message. Returns `(edit_targets, orientation)`,
    /// both empty when the fragment does not carry the harness marker
    /// at all (the typical "this is a vanilla arm" case) or when only
    /// the marker is present.
    ///
    /// Robust to extra blank lines between sections, but assumes the
    /// section headers and the `N. <path> — <reason>` line shape that
    /// `render_prompt_fragment_with_caps` produces. Stops scanning a
    /// section the first time a line doesn't match the numbered shape
    /// (which is also how a section ends).
    pub fn parse_directive_file_lists(fragment: &str) -> (Vec<String>, Vec<String>) {
        if !fragment.contains(HARNESS_MARKER) {
            return (Vec::new(), Vec::new());
        }
        let mut edit_targets = Vec::new();
        let mut orientation = Vec::new();
        let mut section: Option<&mut Vec<String>> = None;
        for raw in fragment.lines() {
            let line = raw.trim_end();
            if line == EDIT_TARGETS_HEADER {
                section = Some(&mut edit_targets);
                continue;
            }
            if line == ORIENTATION_HEADER {
                section = Some(&mut orientation);
                continue;
            }
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                // Blank line: tentatively end the current section but
                // wait until the next non-blank line to decide; if it's
                // the orientation header we just keep going.
                continue;
            }
            // A numbered entry: "N. <path> — <reason>".
            if let Some(path) = parse_numbered_entry(trimmed)
                && let Some(target) = section.as_deref_mut()
            {
                target.push(path);
                continue;
            }
            // Anything else closes the current section.
            section = None;
        }
        (edit_targets, orientation)
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

/// Pull the `<path>` field out of a `N. <path> — <reason>` line.
/// Returns None if the line doesn't open with `<digits>. ` or doesn't
/// contain the ` — ` separator the renderer always emits. Tolerates
/// arbitrary numbering since the eval parser doesn't care about order.
fn parse_numbered_entry(line: &str) -> Option<String> {
    let mut iter = line.chars();
    let first = iter.next()?;
    if !first.is_ascii_digit() {
        return None;
    }
    // Consume remaining digits + the literal ". " separator.
    let mut chars = first.to_string();
    for c in iter {
        if c.is_ascii_digit() {
            chars.push(c);
            continue;
        }
        if c == '.' {
            chars.push(c);
            break;
        }
        return None;
    }
    if !chars.ends_with('.') {
        return None;
    }
    let rest = line.strip_prefix(&chars)?.strip_prefix(' ')?;
    // The path is everything up to the ` — ` em-dash separator.
    let path = rest.split(" — ").next()?.trim();
    if path.is_empty() {
        return None;
    }
    Some(path.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::ContextItem;
    use crate::packet::ContextItemKind;
    use crate::packet::ContextItemState;
    use crate::packet::ContextPacket;
    use crate::packet::ContextStage;
    use crate::packet::RenderLevel;
    use crate::packet::TaskInfo;
    use crate::packet::TokenBudgetInfo;
    use crate::decision_log::ContextDecisionLog;

    fn make_item(id: &str, path: &str, reason: &str, relevance: f64) -> ContextItem {
        ContextItem {
            id: id.to_string(),
            kind: ContextItemKind::FileSummary,
            state: ContextItemState::Included,
            path: Some(path.to_string()),
            relevance,
            confidence: 0.9,
            reason: reason.to_string(),
            evidence: Vec::new(),
            presentation: None,
            render_level: RenderLevel::Full,
        }
    }

    fn make_packet(items: Vec<ContextItem>) -> ContextPacket {
        ContextPacket {
            version: 1,
            stage: ContextStage::Preflight,
            task: TaskInfo {
                raw: "fix it".to_string(),
                task_type: "bug_fix".to_string(),
                confidence: 0.8,
            },
            items,
            decision_log: ContextDecisionLog::default(),
            selected_tests: Vec::new(),
            warnings: Vec::new(),
            token_budget: TokenBudgetInfo {
                limit: 4_000,
                used_estimate: 800,
            },
        }
    }

    #[test]
    fn fragment_splits_top_scored_into_edit_targets_and_rest_into_orientation() {
        // Three included files, distinct relevance scores. K=1 by
        // default → top scorer is the edit target; rest is orientation.
        let packet = make_packet(vec![
            make_item("a", "src/a.rs", "primary owner of change", 0.9),
            make_item("b", "src/b.rs", "registration path", 0.6),
            make_item("c", "src/c.rs", "similar pattern", 0.4),
        ]);
        let fragment = ContextPacketRenderer::render_prompt_fragment(&packet);

        // Two named sections in order.
        let edit_pos = fragment.find(EDIT_TARGETS_HEADER).expect("edit header");
        let orient_pos = fragment.find(ORIENTATION_HEADER).expect("orientation header");
        assert!(edit_pos < orient_pos, "edit targets must precede orientation");

        // Top file goes to edit_targets, rest goes to orientation.
        let edit_block = &fragment[edit_pos..orient_pos];
        assert!(edit_block.contains("src/a.rs"), "top scorer in edit block:\n{edit_block}");
        assert!(!edit_block.contains("src/b.rs"));
        assert!(!edit_block.contains("src/c.rs"));

        let orient_block = &fragment[orient_pos..];
        assert!(orient_block.contains("src/b.rs"));
        assert!(orient_block.contains("src/c.rs"));

        // Orientation numbering restarts at 1, not "2." — the user's
        // proposed rendering had each section number from 1.
        assert!(
            orient_block.contains("\n1. src/b.rs"),
            "orientation numbering should restart at 1:\n{orient_block}"
        );

        // Hard scope-constraint footer follows the lists.
        assert!(
            fragment.contains(SCOPE_CONSTRAINT),
            "scope-constraint footer missing:\n{fragment}"
        );
    }

    #[test]
    fn fragment_with_single_file_emits_no_orientation_section() {
        let packet = make_packet(vec![make_item("a", "src/a.rs", "owner", 0.9)]);
        let fragment = ContextPacketRenderer::render_prompt_fragment(&packet);
        assert!(fragment.contains(EDIT_TARGETS_HEADER));
        assert!(
            !fragment.contains(ORIENTATION_HEADER),
            "single-file packet must skip orientation header:\n{fragment}"
        );
        // Constraint still appears (it applies even when there's nothing
        // labeled orientation — frames the single edit target as final).
        assert!(fragment.contains(SCOPE_CONSTRAINT));
    }

    #[test]
    fn caps_max_edit_targets_controls_section_split() {
        let packet = make_packet(vec![
            make_item("a", "src/a.rs", "first", 0.9),
            make_item("b", "src/b.rs", "second", 0.7),
            make_item("c", "src/c.rs", "third", 0.5),
        ]);
        // K=2 → first two are edit targets, third is orientation.
        let fragment = ContextPacketRenderer::render_prompt_fragment_with_caps(
            &packet,
            SelectionCaps {
                max_edit_targets: 2,
                ..SelectionCaps::default()
            },
        );
        let edit_pos = fragment.find(EDIT_TARGETS_HEADER).unwrap();
        let orient_pos = fragment.find(ORIENTATION_HEADER).unwrap();
        let edit_block = &fragment[edit_pos..orient_pos];
        let orient_block = &fragment[orient_pos..];
        assert!(edit_block.contains("src/a.rs"));
        assert!(edit_block.contains("src/b.rs"));
        assert!(!edit_block.contains("src/c.rs"));
        assert!(orient_block.contains("src/c.rs"));
    }

    #[test]
    fn parse_directive_file_lists_recovers_paths_from_rendered_fragment() {
        let packet = make_packet(vec![
            make_item("a", "src/a.rs", "primary", 0.9),
            make_item("b", "src/b.rs", "registration", 0.6),
            make_item("c", "src/c.rs", "similar pattern", 0.4),
        ]);
        let fragment = ContextPacketRenderer::render_prompt_fragment(&packet);
        let (edit, orient) = ContextPacketRenderer::parse_directive_file_lists(&fragment);
        assert_eq!(edit, vec!["src/a.rs".to_string()]);
        assert_eq!(orient, vec!["src/b.rs".to_string(), "src/c.rs".to_string()]);
    }

    #[test]
    fn parse_directive_file_lists_returns_empty_for_non_ri_prompts() {
        let prompt = "Just a plain task description with no harness marker.";
        let (edit, orient) = ContextPacketRenderer::parse_directive_file_lists(prompt);
        assert!(edit.is_empty());
        assert!(orient.is_empty());
    }

    #[test]
    fn parse_directive_file_lists_handles_legacy_single_section_fragment() {
        // Older rollouts used "Before editing, inspect these files first:"
        // as a single header. The new parser anchors on the new header
        // names; legacy fragments return empty without panicking. This
        // guarantees back-compat artifacts from before the split don't
        // crash the rescore path — they simply contribute no
        // ri_surfaced_* data.
        let legacy = format!(
            "{HARNESS_MARKER}\n\
             Before editing, inspect these files first:\n\
             1. src/a.rs — reason\n\
             2. src/b.rs — reason\n"
        );
        let (edit, orient) = ContextPacketRenderer::parse_directive_file_lists(&legacy);
        assert!(edit.is_empty());
        assert!(orient.is_empty());
    }
}
