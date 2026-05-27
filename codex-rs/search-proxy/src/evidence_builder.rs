//! Orchestrates the proxy pipeline: run rg → parse JSON → classify
//! files → rank → cap → render. Pure once given a [`SearchRunner`]
//! and a [`ClassifiedRg`]; tests inject a [`StaticRunner`] so this
//! module exercises the whole flow without a real `rg`.

use std::path::Path;

use crate::ClassifiedRg;
use crate::evidence::CompactEvidence;
use crate::evidence::EvidenceOptions;
use crate::evidence::FileClass;
use crate::evidence::FileEvidence;
use crate::evidence::HitLine;
use crate::evidence_renderer::render_compact_evidence;
use crate::file_class::classify_file;
use crate::relevance::parse_query_terms;
use crate::relevance::relevance_score;
use crate::rg_json::ParsedFileHits;
use crate::rg_json::parse_rg_json;
use crate::rg_runner::RgExitStatus;
use crate::rg_runner::SearchRunner;
use crate::rg_runner::SearchRunnerError;

/// What the proxy decided to do for one search attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyOutcome {
    /// Replace the model's raw `rg` output with the rendered compact
    /// evidence string. The interceptor passes `rendered` back as
    /// the synthetic tool result.
    Substitute {
        evidence: CompactEvidence,
        rendered: String,
        raw_bytes: usize,
    },
    /// Don't intercept; let the model's raw command run as normal.
    /// Carries a reason for metrics + logs.
    PassThrough(ProxyPassThroughReason),
}

/// Reasons the builder declines to substitute even when the
/// classifier said the command was Eligible. Distinct from
/// [`crate::PassThroughReason`] (which fires at classification time
/// before the runner is even consulted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyPassThroughReason {
    /// `rg` ran successfully but found zero matches.
    NoMatches,
    /// `rg` returned a non-success exit code other than "no matches".
    /// The raw command is allowed through so the model sees the same
    /// error it would have seen without the proxy.
    RgError,
    /// Subprocess could not spawn (`rg` missing, permissions, etc.).
    RunnerError(String),
    /// `rg` matched, but the raw output is small enough that
    /// substituting compact evidence would add bytes rather than
    /// remove them.
    RawIsSmallerThanCompact {
        raw_bytes: usize,
        compact_bytes: usize,
    },
}

/// Top-level entry: classify-then-run-then-render.
///
/// The caller is responsible for ensuring `classified` came from
/// [`crate::classify_command`] and is the [`ClassifyOutcome::Eligible`]
/// arm. Passing a hand-built [`ClassifiedRg`] is fine for tests.
pub fn build_proxy_response(
    classified: &ClassifiedRg,
    cwd: &Path,
    runner: &dyn SearchRunner,
    options: &EvidenceOptions,
) -> ProxyOutcome {
    let raw = match runner.run(classified, cwd, options) {
        Ok(r) => r,
        Err(SearchRunnerError::Spawn(s)) => {
            return ProxyOutcome::PassThrough(ProxyPassThroughReason::RunnerError(s));
        }
    };

    match raw.exit_status {
        RgExitStatus::NoMatches => {
            return ProxyOutcome::PassThrough(ProxyPassThroughReason::NoMatches);
        }
        RgExitStatus::Error => {
            return ProxyOutcome::PassThrough(ProxyPassThroughReason::RgError);
        }
        RgExitStatus::Matched => {}
    }

    let raw_bytes = raw.stdout_bytes.len();
    let parsed = parse_rg_json(&raw.stdout_bytes);
    if parsed.is_empty() {
        // rg said matched but the parser found nothing — most likely
        // a non-UTF-8 stream we couldn't decode. Treat as
        // "intercept failed safely" and pass through.
        return ProxyOutcome::PassThrough(ProxyPassThroughReason::NoMatches);
    }

    let total_hits: usize = parsed.iter().map(|p| p.hits.len()).sum();
    let total_files_matched = parsed.len();

    // Rank by (class, relevance desc, path). Class still dominates so
    // definition sites (Owner) come before plain Source matches, but
    // within a class the relevance score breaks ties on query
    // alignment instead of alphabetical path order — the run3 fix.
    let query_terms = parse_query_terms(&classified.query);
    let mut scored: Vec<(FileEvidence, u32)> = parsed
        .iter()
        .map(|p| {
            (
                build_file_evidence(p, options),
                relevance_score(p, &query_terms),
            )
        })
        .collect();
    scored.sort_by(|(a, a_score), (b, b_score)| {
        a.class
            .rank()
            .cmp(&b.class.rank())
            .then(b_score.cmp(a_score))
            .then_with(|| a.path.cmp(&b.path))
    });
    let ranked: Vec<FileEvidence> = scored.into_iter().map(|(evidence, _)| evidence).collect();

    let mut files: Vec<FileEvidence> = Vec::new();
    let mut byte_budget = options.max_total_bytes;
    let mut line_budget = options.max_total_lines;
    for file in ranked.into_iter().take(options.max_files) {
        // Per-file budget estimate: 1 header line + 1 reason line +
        // N hit lines + 1 blank trailing line. Rough; the renderer
        // does the final accounting.
        let est_lines = 3 + file.hits.len();
        let est_bytes = estimate_file_bytes(&file);
        if est_lines > line_budget || est_bytes > byte_budget {
            break;
        }
        line_budget -= est_lines;
        byte_budget = byte_budget.saturating_sub(est_bytes);
        files.push(file);
    }

    if files.is_empty() {
        return ProxyOutcome::PassThrough(ProxyPassThroughReason::RawIsSmallerThanCompact {
            raw_bytes,
            compact_bytes: 0,
        });
    }

    let evidence = CompactEvidence {
        files,
        total_files_matched,
        total_hits,
    };
    let rendered = render_compact_evidence(classified, &evidence);
    let compact_bytes = rendered.len();

    // Final size guard: if raw output is already smaller than the
    // compact render AND under both the raw-pass-through thresholds,
    // pass through. The thresholds make sure the guard only fires on
    // genuinely-cheap raw output, not on a 50 KB raw stream that
    // happens to render to 55 KB.
    let raw_is_short_enough = raw_bytes <= options.raw_pass_through_bytes
        && lines_in(&raw.stdout_bytes) <= options.raw_pass_through_lines;
    if raw_is_short_enough && compact_bytes >= raw_bytes {
        return ProxyOutcome::PassThrough(ProxyPassThroughReason::RawIsSmallerThanCompact {
            raw_bytes,
            compact_bytes,
        });
    }

    ProxyOutcome::Substitute {
        evidence,
        rendered,
        raw_bytes,
    }
}

fn build_file_evidence(parsed: &ParsedFileHits, options: &EvidenceOptions) -> FileEvidence {
    let class = classify_file(&parsed.path, parsed);
    let reason = reason_for(class);
    let hits = parsed
        .hits
        .iter()
        .take(options.max_hits_per_file)
        .map(|h| HitLine {
            line: h.line,
            column: h.column,
            snippet: truncate_snippet(&h.line_text, options.max_snippet_chars),
        })
        .collect();
    FileEvidence {
        path: parsed.path.clone(),
        hits,
        class,
        reason,
    }
}

fn reason_for(class: FileClass) -> &'static str {
    class.default_reason()
}

fn truncate_snippet(line: &str, max_chars: usize) -> String {
    let trimmed = line.trim_end_matches(['\n', '\r']).trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn estimate_file_bytes(file: &FileEvidence) -> usize {
    // Header + reason + per-hit ~50 bytes overhead beyond snippet.
    let header = file.path.len() + 16;
    let reason = file.reason.len() + 16;
    let hits: usize = file.hits.iter().map(|h| h.snippet.len() + 32).sum();
    header + reason + hits + 8
}

fn lines_in(bytes: &[u8]) -> usize {
    bytes.iter().filter(|b| **b == b'\n').count()
}
