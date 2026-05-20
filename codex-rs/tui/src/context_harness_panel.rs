//! Experimental panel rendering harness context packet debug state.

use codex_context_harness::ContextPacket;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

/// Compact summary of a context packet for the TUI.
pub struct ContextHarnessPanel<'a> {
    pub packet: Option<&'a ContextPacket>,
}

impl Widget for ContextHarnessPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let Some(packet) = self.packet else {
            return;
        };
        let lines = [
            format!(
                "context: {} [{}] tokens {}/{}",
                packet.task.task_type,
                format!("{:?}", packet.stage),
                packet.token_budget.used_estimate,
                packet.token_budget.limit
            ),
            format!(
                "included={} dropped={} exhausted={}",
                packet.decision_log.included.len(),
                packet.decision_log.dropped.len(),
                packet.decision_log.budget_exhausted.len()
            ),
        ];
        for (idx, line) in lines.iter().enumerate() {
            if idx as u16 >= area.height {
                break;
            }
            let y = area.y + idx as u16;
            if y < area.y.saturating_add(area.height) {
                buf[(area.x, y)].set_symbol("");
                for (col, ch) in line.chars().enumerate() {
                    let x = area.x.saturating_add(col as u16);
                    if x < area.x.saturating_add(area.width) {
                        buf[(x, y)].set_symbol(&ch.to_string());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use codex_context_harness::BuildPacketOptions;
    use codex_context_harness::RunMemory;
    use codex_context_harness::build_context_packet;
    use codex_repo_index::RepoMap;

    #[test]
    fn panel_renders_without_panic() {
        let map: RepoMap = serde_json::from_str(include_str!(
            "../../context-harness/tests/fixtures/repo_map_restaurant.json"
        ))
        .unwrap();
        let packet = build_context_packet(
            "fix restaurant search pagination",
            &map,
            &RunMemory::default(),
            BuildPacketOptions::default(),
        );
        let mut buf = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 80, 4));
        ContextHarnessPanel {
            packet: Some(&packet),
        }
        .render(Rect::new(0, 0, 80, 4), &mut buf);
    }
}
