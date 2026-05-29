//! Interception hook for the large-read-proxy MVP.
//!
//! Called from the shell handler (and the unified_exec exec path) before
//! normal command execution. Mirrors
//! [`crate::tools::handlers::search_proxy::intercept_search_proxy`]:
//! returns `Ok(Some(output))` to short-circuit with compact slices,
//! `Ok(None)` to let the handler run the model's command verbatim.
//!
//! Lock discipline (important): the per-session registry lock is held ONLY
//! to read the escape-hatch decision, and separately to record a
//! substitution. It is NEVER held across the file read — I/O happens with
//! no lock held.
//!
//! The proxy reads only the target file and executes nothing else. All
//! decisions go to `tracing::*!(target = "large_read_proxy", …)`.
//!
//! Failure-mode contract (C2): every path that does NOT substitute returns
//! `Ok(None)` so the shell handler runs the user's command unchanged. The
//! proxy NEVER panics, NEVER mutates anything outside its own per-session
//! intercept registry + telemetry counters, and NEVER swallows errors:
//!
//!   * `Feature::LargeReadProxy` disabled              → Ok(None)
//!   * Non-`cat`/`sed` shape, chained command, flags   → Ok(None) (classifier)
//!   * `sed` range below `MIN_SED_RANGE_LINES`         → Ok(None) (classifier)
//!   * Multi-file `cat` / `cat -<flag>` / pipes        → Ok(None) (classifier)
//!   * Already-substituted command repeated            → Ok(None) (escape hatch)
//!   * Target file missing / read error                → Ok(None) (build_pass_through)
//!   * Target file is not valid UTF-8 (binary)         → Ok(None) (build_pass_through)
//!   * File <`MIN_FILE_LINES` (120)                    → Ok(None) (build_pass_through)
//!   * Slicer returns no slices                        → Ok(None) (build_pass_through)
//!
//! See `slice_tests::build_response_passes_through_*` for the leaf
//! behaviors; the handler's read_error / binary branches are exercised by
//! integration runs (the binary case fires on any non-UTF-8 file).

use std::path::Path;
use std::path::PathBuf;

use codex_features::Feature;
use codex_large_read_proxy::BuildPassThroughReason;
use codex_large_read_proxy::LargeReadOutcome;
use codex_large_read_proxy::ReadDecision;
use codex_large_read_proxy::SliceOptions;
use codex_large_read_proxy::build_large_read_response;
use codex_large_read_proxy::decide_read;

use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::tools::context::FunctionToolOutput;

pub(crate) async fn intercept_large_read_proxy(
    hook_command: &str,
    cwd: &Path,
    session: &Session,
) -> Result<Option<FunctionToolOutput>, FunctionCallError> {
    if !session.enabled(Feature::LargeReadProxy) {
        return Ok(None);
    }
    // Mark the proxy as ENABLED for this session so the session-end
    // telemetry distinguishes "enabled but inert" from "not enabled."
    {
        let mut tel = session.services.proxy_telemetry.lock().await;
        tel.large_read_proxy_enabled = true;
    }

    // Decide under the registry lock, then DROP it before any file I/O.
    let classified = {
        let registry = session.services.large_read_proxy_intercepts.lock().await;
        match decide_read(hook_command, &registry) {
            ReadDecision::PassThrough(reason) => {
                tracing::debug!(
                    target: "large_read_proxy",
                    event = "classify_pass_through",
                    reason = %reason,
                    "large-read proxy declined: not an interceptable large read"
                );
                return Ok(None);
            }
            ReadDecision::Bypass { normalized } => {
                tracing::info!(
                    target: "large_read_proxy",
                    event = "escape_hatch_repeat",
                    normalized = %normalized,
                    "repeat large read — allowing raw output"
                );
                drop(registry); // release the intercepts lock before taking telemetry
                session
                    .services
                    .proxy_telemetry
                    .lock()
                    .await
                    .large_read_proxy_escape_hatch_repeats += 1;
                return Ok(None);
            }
            ReadDecision::Substitutable(classified) => classified,
        }
    };

    // Read the target file with NO lock held. Resolve relative to cwd.
    let path = if Path::new(&classified.path).is_absolute() {
        PathBuf::from(&classified.path)
    } else {
        cwd.join(&classified.path)
    };
    let content = match tokio::fs::read(&path).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                tracing::info!(
                    target: "large_read_proxy",
                    event = "build_pass_through",
                    reason = "binary",
                    normalized = %classified.normalized,
                    "large-read proxy declined: file is not valid UTF-8"
                );
                session
                    .services
                    .proxy_telemetry
                    .lock()
                    .await
                    .large_read_proxy_build_pass_throughs += 1;
                return Ok(None);
            }
        },
        Err(_) => {
            tracing::info!(
                target: "large_read_proxy",
                event = "build_pass_through",
                reason = "read_error",
                normalized = %classified.normalized,
                "large-read proxy declined: could not read target file"
            );
            session
                .services
                .proxy_telemetry
                .lock()
                .await
                .large_read_proxy_build_pass_throughs += 1;
            return Ok(None);
        }
    };

    // Search-proxy evidence integration is deferred (LRP-4); no hints yet.
    let hints: &[String] = &[];
    let options = SliceOptions::default();
    match build_large_read_response(&classified, &content, hints, &options) {
        LargeReadOutcome::Substitute {
            rendered,
            slices,
            total_lines,
            raw_bytes,
        } => {
            {
                let mut registry = session.services.large_read_proxy_intercepts.lock().await;
                registry.insert(classified.normalized.clone());
            }
            let compact_bytes_len = rendered.len();
            tracing::info!(
                target: "large_read_proxy",
                event = "substitute",
                normalized = %classified.normalized,
                file = %classified.path,
                compact_bytes = compact_bytes_len,
                raw_bytes,
                total_lines,
                slices = slices.len(),
                "large-read proxy substituted compact slices"
            );
            {
                let mut tel = session.services.proxy_telemetry.lock().await;
                tel.large_read_proxy_substitutions += 1;
                tel.large_read_proxy_compact_bytes += compact_bytes_len as u64;
                tel.large_read_proxy_raw_bytes += raw_bytes as u64;
            }
            Ok(Some(FunctionToolOutput::from_text(rendered, Some(true))))
        }
        LargeReadOutcome::PassThrough(reason) => {
            let reason_label = match reason {
                BuildPassThroughReason::FileSmallEnough { .. } => "file_small_enough",
                BuildPassThroughReason::NoSlices => "no_slices",
            };
            tracing::info!(
                target: "large_read_proxy",
                event = "build_pass_through",
                reason = reason_label,
                normalized = %classified.normalized,
                "large-read proxy ran but declined to substitute"
            );
            session
                .services
                .proxy_telemetry
                .lock()
                .await
                .large_read_proxy_build_pass_throughs += 1;
            Ok(None)
        }
    }
}
