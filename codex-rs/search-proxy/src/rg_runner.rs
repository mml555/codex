//! Subprocess wrapper around the `rg` binary, plus a fake runner for
//! unit tests.
//!
//! The interceptor (Commit 3) hands the proxy a [`ClassifiedRg`] and
//! a `cwd` and asks for raw search bytes. The default implementation
//! shells out to the system `rg` with `--json --line-number --column
//! --max-count=<options.internal_max_count>` so the
//! [`crate::rg_json`] parser has a stable schema to read. Tests use
//! [`StaticRunner`] to pre-bake the rg-JSON output.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use crate::ClassifiedRg;
use crate::evidence::EvidenceOptions;

/// Result of one internal search run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSearchOutput {
    /// Raw bytes of `rg --json` stdout. Parsed by
    /// [`crate::rg_json::parse_rg_json`].
    pub stdout_bytes: Vec<u8>,
    /// Process exit status. `rg` returns 1 when no matches were
    /// found — that's a normal outcome the builder handles, not an
    /// error.
    pub exit_status: RgExitStatus,
}

/// Coarse `rg` exit code interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RgExitStatus {
    /// Matched at least one line.
    Matched,
    /// Ran successfully but matched nothing.
    NoMatches,
    /// `rg` reported an error (bad regex, missing path, etc.) or the
    /// subprocess failed to launch.
    Error,
}

/// Errors that can prevent the runner from producing output. Distinct
/// from `RgExitStatus::Error` because these prevent us from getting
/// any output bytes at all (e.g. binary missing entirely).
#[derive(Debug)]
pub enum SearchRunnerError {
    /// `rg` is not on PATH, the subprocess failed to spawn, or stdout
    /// could not be captured.
    Spawn(String),
}

impl std::fmt::Display for SearchRunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchRunnerError::Spawn(s) => write!(f, "search runner spawn failed: {s}"),
        }
    }
}

impl std::error::Error for SearchRunnerError {}

/// Trait so the interceptor can be unit-tested without shelling out
/// to a real `rg` binary.
pub trait SearchRunner {
    fn run(
        &self,
        classified: &ClassifiedRg,
        cwd: &Path,
        options: &EvidenceOptions,
    ) -> Result<RawSearchOutput, SearchRunnerError>;
}

/// Production runner: invokes the `rg` binary on PATH.
#[derive(Debug, Clone, Default)]
pub struct RipgrepRunner {
    /// Optional override for the `rg` executable path. `None` means
    /// "look up `rg` on PATH".
    pub binary: Option<PathBuf>,
}

impl SearchRunner for RipgrepRunner {
    fn run(
        &self,
        classified: &ClassifiedRg,
        cwd: &Path,
        options: &EvidenceOptions,
    ) -> Result<RawSearchOutput, SearchRunnerError> {
        let binary = self.binary.clone().unwrap_or_else(|| PathBuf::from("rg"));
        let mut cmd = Command::new(&binary);
        cmd.arg("--json")
            .arg("--line-number")
            .arg("--column")
            .arg("--color")
            .arg("never")
            .arg(format!("--max-count={}", options.internal_max_count));

        if classified.flags.ignore_case {
            cmd.arg("-i");
        }
        if classified.flags.smart_case {
            cmd.arg("-S");
        }
        if classified.flags.word_regexp {
            cmd.arg("-w");
        }
        if classified.flags.fixed_strings {
            cmd.arg("-F");
        }
        if classified.flags.multiline {
            cmd.arg("--multiline");
        }
        for t in &classified.flags.type_filters {
            cmd.arg("-t").arg(t);
        }
        for g in &classified.flags.glob_filters {
            cmd.arg("-g").arg(g);
        }

        cmd.arg("--").arg(&classified.query);
        for path in &classified.target_paths {
            cmd.arg(path);
        }

        cmd.current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let output = cmd
            .output()
            .map_err(|e| SearchRunnerError::Spawn(e.to_string()))?;

        let status = match output.status.code() {
            Some(0) => RgExitStatus::Matched,
            Some(1) => RgExitStatus::NoMatches,
            _ => RgExitStatus::Error,
        };

        Ok(RawSearchOutput {
            stdout_bytes: output.stdout,
            exit_status: status,
        })
    }
}

/// Test runner: returns pre-baked bytes regardless of the query.
/// Used by unit tests so the rest of the pipeline doesn't depend on
/// a working `rg` binary.
#[derive(Debug, Clone)]
pub struct StaticRunner {
    pub bytes: Vec<u8>,
    pub status: RgExitStatus,
}

impl StaticRunner {
    pub fn matched(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            status: RgExitStatus::Matched,
        }
    }

    pub fn no_matches() -> Self {
        Self {
            bytes: Vec::new(),
            status: RgExitStatus::NoMatches,
        }
    }

    pub fn error() -> Self {
        Self {
            bytes: Vec::new(),
            status: RgExitStatus::Error,
        }
    }
}

impl SearchRunner for StaticRunner {
    fn run(
        &self,
        _classified: &ClassifiedRg,
        _cwd: &Path,
        _options: &EvidenceOptions,
    ) -> Result<RawSearchOutput, SearchRunnerError> {
        Ok(RawSearchOutput {
            stdout_bytes: self.bytes.clone(),
            exit_status: self.status,
        })
    }
}
