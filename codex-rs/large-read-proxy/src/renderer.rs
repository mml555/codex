//! Render the compact-slice payload the proxy returns in place of a large
//! raw file dump.
//!
//! Format v2 (ported from the search-proxy v2 lessons, see
//! `search-proxy/BYPASS_ANALYSIS.md`). v1 emitted only line RANGES + reasons
//! ("lines 60-89 — definition: ...") — a pointer, not content — wrapped in
//! "No raw file output was returned. Repeat the exact same command to
//! bypass". That reads as a provisional hint with the rerun handed to the
//! model, and the search-proxy experience showed the model then re-runs to
//! get the real bytes. v2 instead emits the ACTUAL slice content,
//! line-numbered (the `cat -n` / `sed` shape the model expects), and frames
//! the escape hatch conditionally.

use crate::command_classifier::ClassifiedRead;
use crate::slice::Slice;

pub fn render_large_read_response(
    classified: &ClassifiedRead,
    slices: &[Slice],
    total_lines: u32,
) -> String {
    let shown_lines: u32 = slices.iter().map(Slice::line_count).sum();
    let omitted = total_lines.saturating_sub(shown_lines);

    let mut out = String::new();
    out.push_str(&format!(
        "[large-read proxy] compact view of {} ({shown_lines} of {total_lines} lines shown)\n",
        classified.path
    ));

    for s in slices {
        out.push_str(&format!("# lines {}-{} — {}\n", s.start, s.end, s.reason));
        // Emit the real content, line-numbered so positions are unambiguous.
        let mut lineno = s.start;
        for line in s.text.split('\n') {
            out.push_str(&format!("{lineno}: {line}\n"));
            lineno += 1;
        }
    }

    if omitted > 0 {
        out.push_str(&format!(
            "# {omitted} lines not shown (gaps between the slices above).\n"
        ));
    }
    out.push_str("# Re-run the identical command only if you need a region not shown above.\n");

    out
}
