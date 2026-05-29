//! Render a [`CompactEvidence`] as a compacted, **rg-native** result —
//! the payload the proxy returns in place of `rg`'s raw output.
//!
//! Format v2 (see `BYPASS_ANALYSIS.md`). The v1 prose format
//! ("Search proxy intercepted: … Reason: likely owner … Repeat the
//! exact same rg command to bypass") read like a provisional *hint*,
//! and in two cloud A/Bs the model re-ran (`escape_hatch_repeat`)
//! every substitution — even when the top file was correct. v2 makes
//! the output look like a trustworthy, compacted `rg` result:
//!   * match lines in rg-native `path:line:col:text` form,
//!   * a single `# likely owner` comment instead of per-file hedging,
//!   * an explicit omission count so the model knows coverage, and
//!   * a *conditional* escape-hatch line (re-run only if you need the
//!     omitted matches) rather than an open invitation.
//!
//! The escape hatch itself is unchanged — only the framing is.

use crate::ClassifiedRg;
use crate::evidence::CompactEvidence;
use crate::evidence::FileEvidence;
use crate::evidence::OwnerConfidence;

pub fn render_compact_evidence(_classified: &ClassifiedRg, evidence: &CompactEvidence) -> String {
    let shown_files = evidence.files.len();
    let shown_hits: usize = evidence.files.iter().map(|f| f.hits.len()).sum();
    let total_files = evidence.total_files_matched;
    let total_hits = evidence.total_hits;

    let mut out = String::new();
    out.push_str(&format!(
        "[search-proxy] compact rg result: {shown_files} of {total_files} file(s), \
         {shown_hits} of {total_hits} match(es) shown\n"
    ));

    // One owner signal, gated by confidence so a misranked incidental
    // file is never asserted as the owner. files[0] is the top-ranked
    // file (an Owner under Strong/Weak confidence).
    match (evidence.owner_confidence, evidence.files.first()) {
        (OwnerConfidence::Strong, Some(top)) => out.push_str(&format!(
            "# likely owner (defines the searched symbol): {}\n",
            top.path
        )),
        (OwnerConfidence::Weak, Some(top)) => out.push_str(&format!(
            "# best-guess owner (LOW confidence — do NOT trust this path as the definitive owner; rely on the matched lines below): {}\n",
            top.path
        )),
        _ => out.push_str("# no high-confidence owner found — use the matches below; do NOT pick a single file from this list as the owner.\n"),
    }

    for file in &evidence.files {
        render_file(&mut out, file);
    }

    let omitted_files = total_files.saturating_sub(shown_files);
    let omitted_hits = total_hits.saturating_sub(shown_hits);
    if omitted_files > 0 || omitted_hits > 0 {
        out.push_str(&format!(
            "# {omitted_files} more file(s) and {omitted_hits} more match(es) not shown.\n"
        ));
    }
    out.push_str("# Re-run the identical command only if you need the matches not shown above.\n");

    out
}

/// Emit each hit as an rg-native line: `path:line:col:text` (or
/// `path:line:text` when the column is unknown).
fn render_file(out: &mut String, file: &FileEvidence) {
    for hit in &file.hits {
        match hit.column {
            Some(col) => out.push_str(&format!(
                "{path}:{line}:{col}:{snippet}\n",
                path = file.path,
                line = hit.line,
                col = col,
                snippet = hit.snippet,
            )),
            None => out.push_str(&format!(
                "{path}:{line}:{snippet}\n",
                path = file.path,
                line = hit.line,
                snippet = hit.snippet,
            )),
        }
    }
}
