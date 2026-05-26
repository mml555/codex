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
    {
        let registry = session.services.search_proxy_intercepts.lock().await;
        if registry.contains(&classified.normalized) {
            tracing::info!(
                target: "search_proxy",
                event = "escape_hatch_repeat",
                normalized = %classified.normalized,
                "repeat rg command — allowing raw output"
            );
            return Ok(None);
        }
    }

    let runner = RipgrepRunner::default();
    let options = EvidenceOptions::default();
    let outcome = build_proxy_response(&classified, cwd, &runner, &options);

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
            Ok(None)
        }
    }
}
