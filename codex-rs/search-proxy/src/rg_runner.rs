//! Subprocess wrapper around the `rg` binary, plus a fake runner for
//! unit tests.
//!
//! The interceptor (Commit 3) hands the proxy a [`ClassifiedRg`] and
//! a `cwd` and asks for raw search bytes. The default implementation
//! shells out to the system `rg` with `--json --line-number --column
//! --max-count=<options.internal_max_count>` so the
//! [`crate::rg_json`] parser has a stable schema to read. Tests use
//! [`StaticRunner`] to pre-bake the rg-JSON output.

use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use crate::ClassifiedRg;
use crate::evidence::EvidenceOptions;

/// Wall-clock ceiling for one internal `rg` invocation. `--max-count`
/// bounds output SIZE but not elapsed TIME, so a pathological tree (huge,
/// many binaries, slow disk) could otherwise stall the model's turn on a
/// search the proxy is supposed to make cheaper. On timeout the runner
/// kills `rg` and reports an error, which the builder turns into a
/// pass-through so the model's own command runs normally.
const DEFAULT_RG_TIMEOUT: Duration = Duration::from_secs(5);

/// Poll granularity for the wait-with-deadline loop.
const RG_WAIT_POLL: Duration = Duration::from_millis(10);

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
    /// `rg` ran past the wall-clock ceiling and was killed. The builder
    /// treats this like any other no-output case: pass through so the
    /// model's original command runs unmediated.
    Timeout(Duration),
}

impl std::fmt::Display for SearchRunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchRunnerError::Spawn(s) => write!(f, "search runner spawn failed: {s}"),
            SearchRunnerError::Timeout(d) => {
                write!(f, "search runner timed out after {d:?}")
            }
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
#[derive(Debug, Clone)]
pub struct RipgrepRunner {
    /// Optional override for the `rg` executable path. `None` means
    /// "look up `rg` on PATH".
    pub binary: Option<PathBuf>,
    /// Wall-clock ceiling for the `rg` subprocess. Defaults to
    /// [`DEFAULT_RG_TIMEOUT`]; do NOT derive `Default` (that would zero
    /// this and time out every search instantly).
    pub timeout: Duration,
}

impl Default for RipgrepRunner {
    fn default() -> Self {
        Self {
            binary: None,
            timeout: DEFAULT_RG_TIMEOUT,
        }
    }
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

        run_with_timeout(cmd, self.timeout)
    }
}

/// Spawn `cmd`, draining stdout on a side thread (so a large stream never
/// dead-locks against a full pipe buffer) while polling the child against a
/// wall-clock deadline. On timeout the child is killed and
/// [`SearchRunnerError::Timeout`] is returned. The side thread joins on
/// every exit path, so no thread or zombie is leaked.
fn run_with_timeout(
    mut cmd: Command,
    timeout: Duration,
) -> Result<RawSearchOutput, SearchRunnerError> {
    let mut child = cmd
        .spawn()
        .map_err(|e| SearchRunnerError::Spawn(e.to_string()))?;

    // Drain stdout concurrently. `rg --json` on a large tree can emit
    // megabytes; reading it only after the child exits would dead-lock once
    // the OS pipe buffer fills (child blocks on write, we block on wait).
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| SearchRunnerError::Spawn("rg stdout not captured".to_string()))?;
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let res = stdout.read_to_end(&mut buf).map(|_| buf);
        // Receiver may already be gone on the error paths; ignore send error.
        let _ = tx.send(res);
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(SearchRunnerError::Timeout(timeout));
                }
                std::thread::sleep(RG_WAIT_POLL);
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(SearchRunnerError::Spawn(e.to_string()));
            }
        }
    };

    // Child exited within the deadline; collect the fully-drained stdout
    // from the side thread's channel, then join it.
    let stdout_bytes = match rx.recv() {
        Ok(Ok(buf)) => buf,
        // Read error, panicked reader, or dropped sender: treat as no
        // output. The builder passes through on empty/unparsable output,
        // which is the correct fail-safe.
        _ => Vec::new(),
    };
    let _ = reader.join();

    let exit_status = match status.code() {
        Some(0) => RgExitStatus::Matched,
        Some(1) => RgExitStatus::NoMatches,
        _ => RgExitStatus::Error,
    };

    Ok(RawSearchOutput {
        stdout_bytes,
        exit_status,
    })
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

#[cfg(all(test, unix))]
mod timeout_tests {
    //! Exercises `run_with_timeout` directly with stand-in binaries
    //! (`/bin/sleep`, `/bin/echo`) so the kill-on-deadline path and the
    //! concurrent stdout drain are covered without depending on `rg`.
    use super::*;

    #[test]
    fn default_runner_timeout_is_nonzero() {
        // Guards the `Default` footgun: a zero timeout would kill every
        // search instantly. Must stay well above any realistic rg run.
        assert!(RipgrepRunner::default().timeout >= Duration::from_secs(1));
    }

    #[test]
    fn run_with_timeout_kills_a_slow_child() {
        let mut cmd = Command::new("/bin/sleep");
        cmd.arg("10")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let start = Instant::now();
        let res = run_with_timeout(cmd, Duration::from_millis(100));
        assert!(
            matches!(res, Err(SearchRunnerError::Timeout(_))),
            "expected Timeout, got {res:?}"
        );
        // The child must be killed promptly, nowhere near its 10s sleep.
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout did not kill the child promptly: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn run_with_timeout_returns_drained_output_for_fast_child() {
        let mut cmd = Command::new("/bin/echo");
        cmd.arg("hello")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let res = run_with_timeout(cmd, Duration::from_secs(5)).expect("echo should succeed");
        assert_eq!(res.exit_status, RgExitStatus::Matched);
        assert_eq!(res.stdout_bytes, b"hello\n");
    }
}
