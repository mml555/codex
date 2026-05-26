use crate::decision_log::DecisionEntry;
use crate::packet::ContextPacket;

/// Normalize packet output for stable golden tests.
pub fn normalize_packet(packet: &mut ContextPacket) {
    packet.items.sort_by(|a, b| {
        a.id.cmp(&b.id).then_with(|| {
            a.path
                .as_deref()
                .unwrap_or("")
                .cmp(b.path.as_deref().unwrap_or(""))
        })
    });
    for item in &mut packet.items {
        item.evidence.sort();
        item.relevance = round_score(item.relevance);
        item.confidence = round_score(item.confidence);
    }
    packet.task.confidence = round_score(packet.task.confidence);
    packet.selected_tests.sort_by(|a, b| a.path.cmp(&b.path));
    for test in &mut packet.selected_tests {
        test.confidence = round_score(test.confidence);
    }
    packet.warnings.sort();

    sort_decision_entries(&mut packet.decision_log.included);
    sort_decision_entries(&mut packet.decision_log.dropped);
    sort_decision_entries(&mut packet.decision_log.budget_exhausted);
    sort_decision_entries(&mut packet.decision_log.low_confidence);
}

fn sort_decision_entries(entries: &mut [DecisionEntry]) {
    entries.sort_by(|a, b| {
        a.id.cmp(&b.id).then_with(|| {
            a.path
                .as_deref()
                .unwrap_or("")
                .cmp(b.path.as_deref().unwrap_or(""))
        })
    });
    for entry in entries {
        entry.evidence.sort();
        if let Some(relevance) = entry.relevance.as_mut() {
            *relevance = round_score(*relevance);
        }
        if let Some(confidence) = entry.confidence.as_mut() {
            *confidence = round_score(*confidence);
        }
    }
}

fn round_score(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}
