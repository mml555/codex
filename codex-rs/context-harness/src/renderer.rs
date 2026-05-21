use anyhow::Result;

use crate::packet::ContextItemKind;
use crate::packet::ContextItemState;
use crate::packet::ContextPacket;
use crate::packet::RenderLevel;
use crate::selection::SelectionCaps;

pub struct ContextPacketRenderer;

impl ContextPacketRenderer {
    pub fn render_json(packet: &ContextPacket) -> Result<String> {
        Ok(serde_json::to_string_pretty(packet)?)
    }

    pub fn render_prompt_fragment(packet: &ContextPacket) -> String {
        Self::render_prompt_fragment_with_caps(packet, SelectionCaps::default())
    }

    pub fn render_prompt_fragment_with_caps(packet: &ContextPacket, caps: SelectionCaps) -> String {
        let mut lines = vec![
            "Harness repo context:".to_string(),
            format!("Task: {}", shorten_for_prompt(&packet.task.raw, 240)),
            format!("Task type: {}", packet.task.task_type),
            "Guidance: Treat this as a repo map. Inspect primary files first; use also-considered paths only if the primary set is insufficient.".to_string(),
        ];

        if let Some(area) = packet
            .warnings
            .iter()
            .find_map(|w| w.strip_prefix("Likely area: "))
        {
            lines.push(format!("Likely area: {area}"));
        } else if let Some(area) = infer_area_from_included(packet) {
            lines.push(format!("Likely area: {area}"));
        }

        let mut repo_rules: Vec<_> = packet
            .items
            .iter()
            .filter(|item| prompt_visible_item(item, ContextItemKind::RepoRule))
            .collect();
        sort_prompt_items(&mut repo_rules);

        let mut file_items: Vec<_> = packet
            .items
            .iter()
            .filter(|item| prompt_visible_item(item, ContextItemKind::FileSummary))
            .collect();
        sort_prompt_items(&mut file_items);
        file_items.truncate(caps.max_prompt_included_files);

        let primary: Vec<_> = file_items
            .iter()
            .copied()
            .filter(|item| item.render_level == RenderLevel::Full)
            .collect();
        let also: Vec<_> = file_items
            .iter()
            .copied()
            .filter(|item| {
                matches!(
                    item.render_level,
                    RenderLevel::Compact | RenderLevel::PathOnly
                )
            })
            .collect();

        if !repo_rules.is_empty() {
            lines.push(String::new());
            lines.push("Repo rules:".to_string());
            for item in repo_rules {
                lines.extend(format_prompt_lines(item));
            }
        }
        if !primary.is_empty() {
            lines.push(String::new());
            lines.push("Primary files:".to_string());
            for item in primary {
                lines.extend(format_prompt_lines(item));
            }
        }
        if !also.is_empty() {
            lines.push(String::new());
            lines.push("Also considered:".to_string());
            for item in also {
                lines.extend(format_prompt_lines(item));
            }
        }

        let selected_tests: Vec<_> = packet
            .selected_tests
            .iter()
            .take(caps.max_prompt_tests)
            .collect();
        if !selected_tests.is_empty() {
            lines.push(String::new());
            lines.push("Likely tests:".to_string());
            for test in selected_tests {
                lines.push(format!("- {}", test.command));
                lines.push(format!(
                    "  path: {}; why: {}; confidence={:.2}",
                    test.path,
                    shorten_for_prompt(&test.reason, 96),
                    test.confidence
                ));
            }
        }

        if !packet.warnings.is_empty() {
            let prompt_warnings: Vec<_> = packet
                .warnings
                .iter()
                .filter(|w| {
                    !w.starts_with("Likely area:")
                        && !w.starts_with("Matched CLI command:")
                        && !w.starts_with("Task scope:")
                })
                .take(caps.max_prompt_warnings)
                .collect();
            if !prompt_warnings.is_empty() {
                lines.push(String::new());
                lines.push("Warnings:".to_string());
                for warning in prompt_warnings {
                    lines.push(format!("- {warning}"));
                }
            }
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

fn format_prompt_lines(item: &crate::packet::ContextItem) -> Vec<String> {
    let label = prompt_item_label(item);
    match item.render_level {
        RenderLevel::Full => vec![
            format!("- {label}"),
            format!("  {}", format_signal_line(item, SignalDetail::WithEvidence)),
        ],
        RenderLevel::Compact => vec![format!(
            "- {label} | {}",
            format_signal_line(item, SignalDetail::WithoutEvidence)
        )],
        RenderLevel::PathOnly => vec![format!(
            "- {label} | relevance={:.2} confidence={:.2}",
            item.relevance, item.confidence
        )],
        RenderLevel::HiddenDebugOnly => vec![format!("- {label}")],
    }
}

fn prompt_item_label(item: &crate::packet::ContextItem) -> String {
    item.path.clone().unwrap_or_else(|| item.reason.clone())
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

#[derive(Clone, Copy)]
enum SignalDetail {
    WithEvidence,
    WithoutEvidence,
}

fn format_signal_line(item: &crate::packet::ContextItem, detail: SignalDetail) -> String {
    let mut parts = vec![format!("why: {}", shorten_for_prompt(&item.reason, 96))];
    parts.push(format!(
        "relevance={:.2} confidence={:.2}",
        item.relevance, item.confidence
    ));
    if matches!(detail, SignalDetail::WithEvidence) && !item.evidence.is_empty() {
        let evidence = item
            .evidence
            .iter()
            .take(3)
            .map(|entry| shorten_for_prompt(entry, 48))
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("evidence={evidence}"));
    }
    parts.join("; ")
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
