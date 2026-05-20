use anyhow::Context;
use anyhow::bail;
use codex_context_harness::PostFailureContext;
use codex_context_harness::infer_repair_hint;

use crate::runner::VerificationRunReport;
use crate::runner::VerificationRunStatus;

pub fn load_verification_run_report(
    path: &std::path::Path,
) -> anyhow::Result<VerificationRunReport> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn post_failure_context_from_report(
    report: &VerificationRunReport,
    task: &str,
    changed_files: &[String],
) -> anyhow::Result<PostFailureContext> {
    if report.status != VerificationRunStatus::Failed {
        bail!(
            "verification report status is {:?}; expected failed",
            report.status
        );
    }

    let failed_command = report
        .commands
        .iter()
        .find(|cmd| cmd.exit_code != Some(0) || cmd.error.is_some())
        .or_else(|| report.commands.last());

    let Some(cmd) = failed_command else {
        bail!("verification report has no command results");
    };

    let failure_packet = report.failure_packet.as_ref();
    let failure_summary = failure_packet
        .map(|fp| fp.summary.clone())
        .unwrap_or_else(|| format!("{} failed", cmd.command));
    let relevant_output = failure_packet
        .map(|fp| fp.relevant_output.clone())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| {
            let mut combined = String::new();
            if !cmd.stderr.is_empty() {
                combined.push_str(&cmd.stderr);
            }
            if !cmd.stdout.is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&cmd.stdout);
            }
            if let Some(err) = &cmd.error {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(err);
            }
            combined
        });

    let changed = if !report.changed_files.is_empty() {
        report.changed_files.clone()
    } else {
        changed_files.to_vec()
    };

    let failed_command = cmd.command.clone();
    let run_reason = cmd.reason.clone();
    let repair_hint = infer_repair_hint(&relevant_output, &changed, &failed_command);

    Ok(PostFailureContext {
        task: task.to_string(),
        changed_files: changed,
        failed_command,
        run_reason,
        failure_summary,
        relevant_output,
        repair_hint,
    }
    .with_capped_output(codex_context_harness::MAX_POST_FAILURE_PROMPT_OUTPUT_CHARS))
}
