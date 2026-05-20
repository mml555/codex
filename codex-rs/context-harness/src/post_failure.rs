use codex_repo_index::RepoMap;

use crate::packet::ContextStage;
use crate::pipeline::BuildPacketOptions;
use crate::pipeline::build_context_packet;
use crate::renderer::ContextPacketRenderer;
use crate::repair_hint::RepairHint;
use crate::repair_hint::infer_repair_hint;
use crate::run_memory::RunMemory;
use crate::selection::SelectionCaps;

/// Hard cap on failure output included in the model-visible prompt.
pub const MAX_POST_FAILURE_PROMPT_OUTPUT_CHARS: usize = 6_000;

/// Inputs for a verification-failure follow-up context packet.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostFailureContext {
    pub task: String,
    pub changed_files: Vec<String>,
    pub failed_command: String,
    pub run_reason: String,
    pub failure_summary: String,
    pub relevant_output: String,
    pub repair_hint: RepairHint,
}

impl PostFailureContext {
    pub fn with_capped_output(mut self, max_chars: usize) -> Self {
        self.relevant_output = cap_prompt_text(&self.relevant_output, max_chars);
        self.repair_hint = infer_repair_hint(
            &self.relevant_output,
            &self.changed_files,
            &self.failed_command,
        );
        self
    }
}

pub fn build_post_failure_context_packet(
    map: &RepoMap,
    failure: &PostFailureContext,
    options: BuildPacketOptions,
) -> crate::packet::ContextPacket {
    let mut options = options;
    options.stage = ContextStage::PostFailure;
    let selection = options.selection;
    let mut packet = build_context_packet(&failure.task, map, &RunMemory::default(), options);
    packet.stage = ContextStage::PostFailure;
    let fragment = render_post_failure_prompt_fragment_with_caps(&packet, failure, selection);
    let prompt_warnings = packet
        .warnings
        .iter()
        .filter(|w| !w.starts_with("Likely area:") && !w.starts_with("Matched CLI command:"))
        .count();
    packet.token_budget.used_estimate = crate::render_level::estimate_prompt_tokens(
        &fragment,
        packet.selected_tests.len(),
        prompt_warnings,
    );
    packet
}

pub fn render_post_failure_prompt_fragment(
    packet: &crate::packet::ContextPacket,
    failure: &PostFailureContext,
) -> String {
    render_post_failure_prompt_fragment_with_caps(packet, failure, SelectionCaps::default())
}

pub fn render_post_failure_prompt_fragment_with_caps(
    packet: &crate::packet::ContextPacket,
    failure: &PostFailureContext,
    caps: SelectionCaps,
) -> String {
    let capped_output = cap_prompt_text(
        &failure.relevant_output,
        MAX_POST_FAILURE_PROMPT_OUTPUT_CHARS,
    );

    let mut lines = vec![
        "Verification failed:".to_string(),
        format!("- Command: {}", failure.failed_command),
        format!("- Why it was run: {}", failure.run_reason),
        format!("- Failure summary: {}", failure.failure_summary),
        format!("- Relevant output:\n{capped_output}"),
    ];

    if !failure.changed_files.is_empty() {
        lines.push(String::new());
        lines.push("Changed files:".to_string());
        for path in &failure.changed_files {
            lines.push(format!("- {path}"));
        }
    }

    lines.push(String::new());
    lines.push("Repair hint:".to_string());
    lines.push(format!(
        "- Failure type: {}",
        failure_type_label(failure.repair_hint.likely_failure_type)
    ));
    if !failure.repair_hint.likely_files.is_empty() {
        lines.push("- Likely files:".to_string());
        for path in failure.repair_hint.likely_files.iter().take(6) {
            lines.push(format!("  - {path}"));
        }
    }
    lines.push(format!("- Focus: {}", failure.repair_hint.suggested_focus));

    let repo_context = ContextPacketRenderer::render_prompt_fragment_with_caps(packet, caps);
    if !repo_context.is_empty() {
        lines.push(String::new());
        lines.push(repo_context);
    }

    lines.join("\n")
}

fn failure_type_label(failure_type: crate::repair_hint::FailureType) -> &'static str {
    match failure_type {
        crate::repair_hint::FailureType::CompileError => "compile_error",
        crate::repair_hint::FailureType::TestAssertionFailure => "test_assertion_failure",
        crate::repair_hint::FailureType::MissingImport => "missing_import",
        crate::repair_hint::FailureType::SnapshotFailure => "snapshot_failure",
        crate::repair_hint::FailureType::LintFailure => "lint_failure",
        crate::repair_hint::FailureType::Timeout => "timeout",
        crate::repair_hint::FailureType::Unknown => "unknown",
    }
}

fn cap_prompt_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!(
        "{truncated}\n… [truncated, {} chars omitted]",
        text.chars().count() - max_chars
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budgeter::TokenBudget;
    use codex_repo_index::AreaMap;
    use codex_repo_index::RepoMap;

    fn fixture_map() -> RepoMap {
        RepoMap {
            version: 2,
            repo_id: "t".to_string(),
            root: "/t".to_string(),
            files: Vec::new(),
            tests: Vec::new(),
            areas: Vec::new(),
            packages: Vec::new(),
            area_maps: vec![AreaMap {
                area_id: "context-harness".to_string(),
                root_paths: vec!["context-harness/".to_string()],
                owned_files: vec!["context-harness/src/metrics.rs".to_string()],
                test_paths: vec!["context-harness/tests/eval_fixtures.rs".to_string()],
                related_cli_paths: Vec::new(),
                negative_paths: Vec::new(),
                confidence: 0.9,
            }],
            commands: Vec::new(),
            test_map: Vec::new(),
            agents_md: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn post_failure_prompt_includes_failure_block_not_decision_log() {
        let failure = PostFailureContext {
            task: "fix metrics".to_string(),
            changed_files: vec!["context-harness/src/metrics.rs".to_string()],
            failed_command: "cargo test -p codex-context-harness".to_string(),
            run_reason: "changed file belongs to context-harness crate".to_string(),
            failure_summary: "cargo test -p codex-context-harness failed".to_string(),
            relevant_output: "error: assertion failed".to_string(),
            repair_hint: infer_repair_hint(
                "error: assertion failed",
                &["context-harness/src/metrics.rs".to_string()],
                "cargo test -p codex-context-harness",
            ),
        };
        let packet = build_post_failure_context_packet(
            &fixture_map(),
            &failure,
            BuildPacketOptions {
                token_budget: TokenBudget { limit: 12_000 },
                ..BuildPacketOptions::default()
            },
        );
        assert_eq!(packet.stage, ContextStage::PostFailure);

        let fragment = render_post_failure_prompt_fragment(&packet, &failure);
        assert!(fragment.contains("Verification failed:"));
        assert!(fragment.contains("cargo test -p codex-context-harness"));
        assert!(fragment.contains("Changed files:"));
        assert!(fragment.contains("error: assertion failed"));
        assert!(fragment.contains("Repair hint:"));
        assert!(fragment.contains("test_assertion_failure"));
        assert!(!fragment.contains("Dropped:"));
        assert!(!fragment.contains("Budget exhausted:"));
    }
}
