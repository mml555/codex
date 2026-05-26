//! Types describing the compact-evidence result the search proxy
//! returns to the model in place of raw `rg` output.
//!
//! No I/O happens here. The orchestration in
//! [`crate::evidence_builder`] consumes these types after the runner
//! and parser run.

/// One file's worth of evidence the proxy plans to surface to the
/// model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEvidence {
    /// Path as `rg` reported it, relative to the runner's cwd.
    pub path: String,
    /// Up to [`EvidenceOptions::max_hits_per_file`] sample lines.
    pub hits: Vec<HitLine>,
    /// How the file classifier labelled this file.
    pub class: FileClass,
    /// Short, model-facing explanation of `class`. Stored as a
    /// `&'static str` so we never accidentally render a free-form
    /// human sentence the model could misinterpret.
    pub reason: &'static str,
}

/// One sample line returned for a file. The line text is the literal
/// matched line from `rg`, trimmed and length-capped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitLine {
    pub line: u32,
    pub column: Option<u32>,
    pub snippet: String,
}

/// How the file classifier ranked a file relative to the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClass {
    /// At least one matching line in this file looks like a
    /// definition site (`pub enum X`, `fn snake_case`, etc.).
    Owner,
    /// Path lives under a test root (`tests/`, `_test.rs`,
    /// `_tests.rs`) and matched. Surfaced separately from regular
    /// source so the model can pick test expectations easily.
    RelatedTest,
    /// Non-test source file that simply contains the matched query.
    Source,
}

impl FileClass {
    pub fn default_reason(self) -> &'static str {
        match self {
            FileClass::Owner => "likely owner file; defines or declares the searched symbol.",
            FileClass::RelatedTest => "likely related test file.",
            FileClass::Source => "additional source-file match.",
        }
    }

    /// Sort-key used to rank files within a [`CompactEvidence`].
    /// Lower is better.
    pub fn rank(self) -> u8 {
        match self {
            FileClass::Owner => 0,
            FileClass::Source => 1,
            FileClass::RelatedTest => 2,
        }
    }
}

/// Bundle of evidence the proxy surfaces. Always carries at least one
/// file — empty results are signalled via
/// [`crate::ProxyOutcome::PassThrough`] instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactEvidence {
    pub files: Vec<FileEvidence>,
    /// Number of files `rg` matched in total, BEFORE capping to
    /// `files`. Lets the renderer say "showing top 3 of 12".
    pub total_files_matched: usize,
    /// Number of distinct match lines across all matching files,
    /// before per-file capping. Surfaces in metrics.
    pub total_hits: usize,
}

/// Caller-tunable knobs for the evidence builder. All fields have
/// defaults aimed at the MVP target: keep the model-facing payload
/// well under 120 lines / 4 KB and avoid substituting when raw `rg`
/// output is already smaller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceOptions {
    pub max_files: usize,
    pub max_hits_per_file: usize,
    pub max_total_lines: usize,
    pub max_total_bytes: usize,
    /// Per-line snippet character cap. Lines longer than this are
    /// truncated with a trailing single-character ellipsis ("…").
    pub max_snippet_chars: usize,
    /// Upper bound on lines `rg` returns. Threaded into the internal
    /// `--max-count` flag by the runner.
    pub internal_max_count: u32,
    /// Pass-through threshold (raw size). If the runner reports
    /// `raw_bytes <= raw_pass_through_bytes` AND the compact render
    /// would be larger than the raw output, skip substitution.
    pub raw_pass_through_bytes: usize,
    /// Pass-through threshold (raw line count). Same effect, easier
    /// to reason about for human-scale eval reports.
    pub raw_pass_through_lines: usize,
}

impl Default for EvidenceOptions {
    fn default() -> Self {
        Self {
            max_files: 5,
            max_hits_per_file: 3,
            max_total_lines: 120,
            max_total_bytes: 4_096,
            max_snippet_chars: 200,
            internal_max_count: 50,
            raw_pass_through_bytes: 2_048,
            raw_pass_through_lines: 30,
        }
    }
}
