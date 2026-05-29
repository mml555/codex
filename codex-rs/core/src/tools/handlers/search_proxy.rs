//! Interception hook for the search-proxy MVP.
//!
//! Called from the shell handler before normal command execution.
//! Mirrors the [`crate::tools::handlers::apply_patch::intercept_apply_patch`]
//! shape: returns `Ok(Some(output))` to short-circuit with a synthetic
//! result, `Ok(None)` to let the shell handler run the model's command
//! verbatim, or `Err` for hard failures (currently never emitted).
//!
//! All decisions go to `tracing::info!(target = "search_proxy", ...)`
//! so the agent-eval harness (Commit 4) can scrape per-decision
//! metrics from codex stderr without a new record-schema field.
//!
//! Failure-mode contract (C2): every path that does NOT substitute
//! returns `Ok(None)` so the shell handler runs the user's command
//! unchanged. The proxy NEVER panics, NEVER mutates anything outside
//! its own per-session intercept registry + telemetry counters, and
//! NEVER swallows errors that would otherwise reach the model:
//!
//!   * `Feature::SearchProxy` disabled                → Ok(None)
//!   * Shell metacharacters / pipes / redirects       → Ok(None) (classifier)
//!   * Non-`rg` executable / unsupported `rg` flag    → Ok(None) (classifier)
//!   * Bare `rg` (no query) / unsupported shape       → Ok(None) (classifier)
//!   * Already-substituted command repeated           → Ok(None) (escape hatch)
//!   * `rg` spawn failure (binary missing)            → Ok(None) via
//!     `ProxyPassThroughReason::RunnerError(msg)`; the spawn message is
//!     preserved on the tracing event for operator debug.
//!   * `rg` matched but no parseable JSON             → Ok(None)
//!   * `rg` matched but compact would be larger       → Ok(None)
//!
//! See `evidence_builder_tests::runner_spawn_failure_carries_message_and_passes_through`
//! and the `command_classifier_tests::*_passes_through_*` cases.

use std::path::Path;

use codex_features::Feature;
use codex_search_proxy::ClassifyOutcome;
use codex_search_proxy::EvidenceOptions;
use codex_search_proxy::ProxyOutcome;
use codex_search_proxy::RipgrepRunner;
use codex_search_proxy::build_proxy_response;
use codex_search_proxy::classify_command;

use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::tools::context::FunctionToolOutput;

pub(crate) async fn intercept_search_proxy(
    hook_command: &str,
    cwd: &Path,
    session: &Session,
) -> Result<Option<FunctionToolOutput>, FunctionCallError> {
    if !session.enabled(Feature::SearchProxy) {
        return Ok(None);
    }
    // Mark the proxy as ENABLED for this session even if every call ends in
    // pass-through — the session-end telemetry summary distinguishes "enabled
    // but inert" from "not enabled at all" so an opted-in operator gets
    // confirmation the feature was wired correctly.
    {
        let mut tel = session.services.proxy_telemetry.lock().await;
        tel.search_proxy_enabled = true;
    }

    let classified = match classify_command(hook_command) {
        ClassifyOutcome::Eligible(c) => c,
        ClassifyOutcome::PassThrough(reason) => {
            tracing::debug!(
                target: "search_proxy",
                event = "classify_pass_through",
                reason = %reason,
                "search proxy declined: command not eligible"
            );
            return Ok(None);
        }
    };

    // Escape hatch: a normalized command that's already been
    // substituted in this session must NOT be substituted again, so
    // the model can retrieve raw rg output by repeating the call.
    // We only need a quick presence check — the registry is per-Session
    // and the value is the normalized command string.
    //
    // The check and the later insert are not atomic, so two *concurrent*
    // identical intercepts can both substitute (and both insert). That is
    // benign — both arms return compact evidence, and a model that wants raw
    // output simply repeats on a later turn, by which point the entry exists.
    // We deliberately don't hold the lock across the rg run to avoid
    // serializing unrelated searches.
    {
        let registry = session.services.search_proxy_intercepts.lock().await;
        if registry.contains(&classified.normalized) {
            tracing::info!(
                target: "search_proxy",
                event = "escape_hatch_repeat",
                normalized = %classified.normalized,
                "repeat rg command — allowing raw output"
            );
            drop(registry); // release the intercepts lock before taking the telemetry lock
            session
                .services
                .proxy_telemetry
                .lock()
                .await
                .search_proxy_escape_hatch_repeats += 1;
            return Ok(None);
        }
    }

    // `build_proxy_response` shells out to `rg` (a synchronous subprocess with
    // a wall-clock timeout, up to a few seconds). Run it on the blocking pool
    // so it never stalls a tokio worker thread. If the blocking task panics,
    // fail safe and pass through.
    let cwd_owned = cwd.to_path_buf();
    let classified_for_run = classified.clone();
    let outcome = match tokio::task::spawn_blocking(move || {
        let runner = RipgrepRunner::default();
        let options = EvidenceOptions::default();
        build_proxy_response(&classified_for_run, &cwd_owned, &runner, &options)
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(join_err) => {
            tracing::warn!(
                target: "search_proxy",
                event = "runner_join_error",
                error = %join_err,
                "search proxy rg task failed to join; passing through"
            );
            return Ok(None);
        }
    };

    match outcome {
        ProxyOutcome::Substitute {
            rendered,
            evidence,
            raw_bytes,
        } => {
            // Record substitution AFTER the builder confirms it's
            // safe. We deliberately don't register on PassThrough
            // outcomes so a NoMatches first call doesn't block a
            // future legitimate substitute on the same query.
            {
                let mut registry = session.services.search_proxy_intercepts.lock().await;
                registry.insert(classified.normalized.clone());
            }

            let top_file = evidence
                .files
                .first()
                .map(|f| f.path.as_str())
                .unwrap_or("");
            let compact_bytes = rendered.len();
            tracing::info!(
                target: "search_proxy",
                event = "substitute",
                normalized = %classified.normalized,
                compact_bytes,
                raw_bytes,
                total_files_matched = evidence.total_files_matched,
                total_hits = evidence.total_hits,
                files_in_evidence = evidence.files.len(),
                top_file,
                "search proxy substituted compact evidence"
            );
            {
                let mut tel = session.services.proxy_telemetry.lock().await;
                tel.search_proxy_substitutions += 1;
                tel.search_proxy_compact_bytes += compact_bytes as u64;
                tel.search_proxy_raw_bytes += raw_bytes as u64;
            }

            Ok(Some(FunctionToolOutput::from_text(rendered, Some(true))))
        }
        ProxyOutcome::PassThrough(reason) => {
            tracing::info!(
                target: "search_proxy",
                event = "build_pass_through",
                normalized = %classified.normalized,
                reason = ?reason,
                "search proxy ran but declined to substitute"
            );
            session
                .services
                .proxy_telemetry
                .lock()
                .await
                .search_proxy_build_pass_throughs += 1;
            Ok(None)
        }
    }
}
