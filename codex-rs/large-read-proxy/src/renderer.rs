//! Render the compact-slice payload the proxy returns in place of a large
//! raw file dump.
//!
//! Format v2: v1 emitted only line RANGES + reasons
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
    // For a `sed` range read, "shown of total" should be measured against the
    // requested window, not the whole file — and the omitted lines are the
    // rest of that window, not "gaps between slices" (the slices are
    // contiguous from the range start). For `cat`, it's the whole file.
    let (denominator, omission_note) = match classified.requested_range {
        Some((start, end)) => {
            let span_start = start.max(1);
            let span_end = end.min(total_lines);
            let span = span_end.saturating_sub(span_start).saturating_add(1);
            (span, "more lines in the requested range not shown")
        }
        None => (
            total_lines,
            "lines not shown (gaps between the slices above)",
        ),
    };
    let omitted = denominator.saturating_sub(shown_lines);

    let mut out = String::new();
    out.push_str(&format!(
        "[large-read proxy] compact view of {} ({shown_lines} of {denominator} lines shown)\n",
        classified.path
    ));

    for s in slices {
        out.push_str(&format!("# lines {}-{} — {}\n", s.start, s.end, s.reason));
        // Emit the real content, line-numbered so positions are unambiguous.
        for (lineno, line) in (s.start..).zip(s.text.split('\n')) {
            out.push_str(&format!("{lineno}: {line}\n"));
        }
    }

    if omitted > 0 {
        out.push_str(&format!("# {omitted} {omission_note}.\n"));
    }
    out.push_str("# Re-run the identical command only if you need a region not shown above.\n");

    out
}
