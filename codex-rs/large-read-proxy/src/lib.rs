//! Reactive large-read proxy for codex.
//!
//! MVP scope: classify model-initiated *large* file reads
//! ([`classify_command`]) — `cat <file>` and `sed -n '<start>,<end>p'
//! <file>` with a large requested range — and, for the eligible ones,
//! return a compact set of slices ([`build_large_read_response`]) instead
//! of dumping the whole file into context. The model bypasses by repeating
//! the exact same command (escape hatch enforced by the interceptor via a
//! per-session registry keyed on [`ClassifiedRead::normalized`]).
//!
//! Third sibling of the search-proxy / verification-policy MVPs: same
//! reactive-mediation shape (intercept an expensive model-initiated action,
//! return a compact safer alternative, escape-hatch on repeat). Search
//! proxy mediates discovery (`rg`); this mediates large file reads. The
//! crate is a pure function of (command string, file content) — the caller
//! does all I/O. See branch `large-read-proxy-mvp`.

mod command_classifier;
mod renderer;
mod slice;

pub use command_classifier::ClassifiedRead;
pub use command_classifier::ClassifyOutcome;
pub use command_classifier::MIN_SED_RANGE_LINES;
pub use command_classifier::PassThroughReason;
pub use command_classifier::ReadTool;
pub use command_classifier::classify_command;

pub use renderer::render_large_read_response;

pub use slice::Slice;
pub use slice::SliceOptions;
pub use slice::build_slices;

use std::collections::HashSet;

/// Minimum file size (lines) before a `cat`/`sed` read is worth
/// intercepting. Below this, a raw read is cheap, so the proxy declines.
pub const MIN_FILE_LINES: u32 = 120;

/// What the interceptor should do with one model-issued command, given the
/// set of large reads already substituted this session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadDecision {
    /// Not an interceptable large read — run the command unchanged.
    PassThrough(PassThroughReason),
    /// Eligible large read whose normalized form was already substituted —
    /// let the raw command run (the repeat-to-bypass escape hatch).
    Bypass { normalized: String },
    /// Eligible large read, first occurrence: the caller should read the
    /// file and call [`build_large_read_response`].
    Substitutable(ClassifiedRead),
}

/// Decide what to do with one command, read-only against the per-session
/// `already_substituted` registry. Pure. The caller registers the
/// normalized key only after a successful substitution, so a small-file
/// pass-through does not block a later (larger) read of the same path.
pub fn decide_read(raw_cmd: &str, already_substituted: &HashSet<String>) -> ReadDecision {
    match classify_command(raw_cmd) {
        ClassifyOutcome::PassThrough(reason) => ReadDecision::PassThrough(reason),
        ClassifyOutcome::Eligible(read) => {
            if already_substituted.contains(&read.normalized) {
                ReadDecision::Bypass {
                    normalized: read.normalized,
                }
            } else {
                ReadDecision::Substitutable(read)
            }
        }
    }
}

/// Outcome of building a compact response for an eligible large read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LargeReadOutcome {
    Substitute {
        rendered: String,
        slices: Vec<Slice>,
        total_lines: u32,
        raw_bytes: usize,
    },
    /// Eligible by command shape, but the file is small enough that a raw
    /// read is cheap — run it unchanged.
    PassThrough(BuildPassThroughReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildPassThroughReason {
    FileSmallEnough { lines: u32 },
    NoSlices,
}

/// Build the compact-slice response for an eligible large read, given the
/// file's content. Pure: the caller does the file I/O and is responsible
/// for passing the content of [`ClassifiedRead::path`].
pub fn build_large_read_response(
    classified: &ClassifiedRead,
    content: &str,
    hints: &[String],
    opts: &SliceOptions,
) -> LargeReadOutcome {
    let total_lines = content.lines().count() as u32;
    if total_lines < MIN_FILE_LINES {
        return LargeReadOutcome::PassThrough(BuildPassThroughReason::FileSmallEnough {
            lines: total_lines,
        });
    }
    let slices = build_slices(content, hints, opts);
    if slices.is_empty() {
        return LargeReadOutcome::PassThrough(BuildPassThroughReason::NoSlices);
    }
    let rendered = render_large_read_response(classified, &slices, total_lines);
    LargeReadOutcome::Substitute {
        rendered,
        slices,
        total_lines,
        raw_bytes: content.len(),
    }
}

#[cfg(test)]
mod command_classifier_tests;
#[cfg(test)]
mod decide_tests;
#[cfg(test)]
mod slice_tests;
