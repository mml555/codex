//! Render a [`CompactEvidence`] as the plain-text payload the proxy
//! returns to the model in place of `rg`'s raw output.
//!
//! Format mirrors the MVP spec: a header naming the proxy, the
//! original command echoed back so the model knows what was
//! intercepted, a numbered list of files with at most a handful of
//! hit lines each, a brief reason per file, a "suggested next step"
//! block keyed off classification, and a closing line that documents
//! the escape hatch ("Repeat the same command to get raw output").

use crate::ClassifiedRg;
use crate::evidence::CompactEvidence;
use crate::evidence::FileClass;
use crate::evidence::FileEvidence;

pub fn render_compact_evidence(classified: &ClassifiedRg, evidence: &CompactEvidence) -> String {
    let mut out = String::new();
    out.push_str("Search proxy intercepted:\n\n");

    out.push_str("Original command:\n");
    out.push_str("  ");
    out.push_str(&classified.normalized);
    out.push_str("\n\n");

    out.push_str("Compact evidence:\n");
    if evidence.total_files_matched > evidence.files.len() {
        out.push_str(&format!(
            "  (showing top {shown} of {total} matching files; {hits} total hits)\n",
            shown = evidence.files.len(),
            total = evidence.total_files_matched,
            hits = evidence.total_hits,
        ));
    }
    for (i, file) in evidence.files.iter().enumerate() {
        render_file(&mut out, i + 1, file);
    }

    let next_steps = next_step_bullets(evidence);
    if !next_steps.is_empty() {
        out.push_str("\nSuggested next step:\n");
        for bullet in next_steps {
            out.push_str("  - ");
            out.push_str(&bullet);
            out.push('\n');
        }
    }

    out.push_str("\nRaw rg output was not run. Repeat the exact same rg command to bypass the proxy and get raw output.\n");

    out
}

fn render_file(out: &mut String, index: usize, file: &FileEvidence) {
    out.push_str(&format!("{index}. {}\n", file.path));
    for hit in &file.hits {
        match hit.column {
            Some(col) => out.push_str(&format!(
                "   - line {line} col {col}: {snippet}\n",
                line = hit.line,
                col = col,
                snippet = hit.snippet,
            )),
            None => out.push_str(&format!(
                "   - line {line}: {snippet}\n",
                line = hit.line,
                snippet = hit.snippet,
            )),
        }
    }
    out.push_str("   Reason: ");
    out.push_str(file.reason);
    out.push('\n');
}

fn next_step_bullets(evidence: &CompactEvidence) -> Vec<String> {
    let mut bullets: Vec<String> = Vec::new();

    if let Some(owner) = evidence.files.iter().find(|f| f.class == FileClass::Owner) {
        bullets.push(format!("Inspect {} around the matching lines.", owner.path));
    } else if let Some(first) = evidence.files.first() {
        bullets.push(format!("Inspect {} around the matching lines.", first.path));
    }

    if let Some(test) = evidence
        .files
        .iter()
        .find(|f| f.class == FileClass::RelatedTest)
    {
        bullets.push(format!(
            "Open {} for test expectations covering the searched symbol.",
            test.path
        ));
    }

    bullets
}
