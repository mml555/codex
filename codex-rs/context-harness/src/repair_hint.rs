use std::collections::BTreeSet;

/// Deterministic failure classification for post-failure context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureType {
    CompileError,
    TestAssertionFailure,
    MissingImport,
    SnapshotFailure,
    LintFailure,
    Timeout,
    Unknown,
}

/// Non-model repair guidance derived from verification output.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairHint {
    pub likely_failure_type: FailureType,
    pub likely_files: Vec<String>,
    pub suggested_focus: String,
}

pub fn infer_repair_hint(
    relevant_output: &str,
    changed_files: &[String],
    failed_command: &str,
) -> RepairHint {
    let failure_type = classify_failure(relevant_output, failed_command);
    let likely_files = collect_likely_files(relevant_output, changed_files);
    let suggested_focus = suggested_focus_for(failure_type, &likely_files);

    RepairHint {
        likely_failure_type: failure_type,
        likely_files,
        suggested_focus,
    }
}

fn classify_failure(output: &str, failed_command: &str) -> FailureType {
    let lower = output.to_ascii_lowercase();
    let cmd_lower = failed_command.to_ascii_lowercase();

    if lower.contains("timed out") || cmd_lower.contains("timeout") {
        return FailureType::Timeout;
    }
    if lower.contains("insta")
        || lower.contains("snapshot")
        || lower.contains(".snap")
        || lower.contains("cargo insta")
    {
        return FailureType::SnapshotFailure;
    }
    if lower.contains("clippy")
        || lower.contains("deny(warnings)")
        || (lower.contains("warning:") && lower.contains("denied"))
    {
        return FailureType::LintFailure;
    }
    if lower.contains("unresolved import")
        || lower.contains("no module named")
        || lower.contains("use of unresolved")
        || lower.contains("error[e0432]")
        || lower.contains("error[e0433]")
        || lower.contains("failed to resolve")
        || lower.contains("cannot find crate")
        || lower.contains("cannot find module")
    {
        return FailureType::MissingImport;
    }
    if lower.contains("assertion failed")
        || lower.contains("assertionerror")
        || lower.contains("assert_eq!")
        || lower.contains("panicked at")
        || lower.contains("test result: failed")
        || lower.contains("error: test failed")
        || lower.contains("test failed")
        || lower.contains("failed tests/")
        || (lower.contains("left:") && lower.contains("right:"))
        || (lower.contains("assert ") && lower.contains("=="))
    {
        return FailureType::TestAssertionFailure;
    }
    if lower.contains("could not compile")
        || lower.contains("compilation failed")
        || lower.contains("error[e0")
        || (lower.contains("error: ") && !lower.contains("test result:"))
    {
        return FailureType::CompileError;
    }

    FailureType::Unknown
}

fn collect_likely_files(output: &str, changed_files: &[String]) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for path in changed_files {
        paths.insert(normalize_path(path));
    }
    for path in extract_paths_from_output(output) {
        paths.insert(path);
    }
    paths.into_iter().collect()
}

fn extract_paths_from_output(output: &str) -> Vec<String> {
    let mut found = BTreeSet::new();
    for line in output.lines() {
        if let Some(path) = path_after_arrow(line) {
            found.insert(path);
            continue;
        }
        if let Some(path) = path_after_panic(line) {
            found.insert(path);
            continue;
        }
        for token in line.split_whitespace() {
            if looks_like_source_path(token) {
                found.insert(normalize_path(
                    token.trim_end_matches(|c: char| matches!(c, ',' | ')' | ';')),
                ));
            }
        }
    }
    found.into_iter().collect()
}

fn path_after_panic(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let marker = "panicked at ";
    let idx = lower.find(marker)?;
    let rest = &line[idx + marker.len()..];
    let path = rest.split(':').next()?.trim();
    if looks_like_source_path(path) {
        Some(normalize_path(path))
    } else {
        None
    }
}

fn path_after_arrow(line: &str) -> Option<String> {
    let marker = "-->";
    let idx = line.find(marker)?;
    let rest = line[idx + marker.len()..].trim();
    let path = rest.split(':').next()?.trim();
    if looks_like_source_path(path) {
        Some(normalize_path(path))
    } else {
        None
    }
}

fn looks_like_source_path(token: &str) -> bool {
    (token.ends_with(".rs")
        || token.ends_with(".ts")
        || token.ends_with(".tsx")
        || token.ends_with(".py"))
        && (token.contains('/') || token.contains('\\'))
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn suggested_focus_for(failure_type: FailureType, likely_files: &[String]) -> String {
    let file_hint = likely_files
        .first()
        .map(|path| format!(" Start with `{path}`."))
        .unwrap_or_default();

    match failure_type {
        FailureType::CompileError => {
            format!("Fix the compile error before changing tests.{file_hint}")
        }
        FailureType::TestAssertionFailure => {
            format!("Fix the failing assertion or test expectation.{file_hint}")
        }
        FailureType::MissingImport => {
            format!("Resolve the missing import or module path.{file_hint}")
        }
        FailureType::SnapshotFailure => {
            format!(
                "Review the snapshot diff; update snapshots only if the UI change is intentional.{file_hint}"
            )
        }
        FailureType::LintFailure => {
            format!("Address the lint/clippy finding in the reported location.{file_hint}")
        }
        FailureType::Timeout => {
            "Investigate the slow or hanging test; avoid broad workspace test runs.".to_string()
        }
        FailureType::Unknown => {
            format!(
                "Read the failure output and fix the underlying issue before expanding scope.{file_hint}"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_compile_error() {
        let hint = infer_repair_hint(
            "error[E0425]: cannot find value `x` in this scope",
            &["context-harness/src/metrics.rs".to_string()],
            "cargo test -p codex-context-harness",
        );
        assert_eq!(hint.likely_failure_type, FailureType::CompileError);
        assert!(hint.suggested_focus.contains("compile error"));
    }

    #[test]
    fn classifies_assertion_failure() {
        let hint = infer_repair_hint(
            "thread 'test' panicked at context-harness/src/metrics.rs:12\nassertion failed",
            &[],
            "cargo test -p codex-context-harness",
        );
        assert_eq!(hint.likely_failure_type, FailureType::TestAssertionFailure);
        assert!(hint.likely_files.iter().any(|p| p.contains("metrics.rs")));
    }

    #[test]
    fn classifies_snapshot_failure() {
        let hint = infer_repair_hint(
            "insta review: snapshot mismatch in tui/src/app.rs",
            &[],
            "cargo test -p codex-tui",
        );
        assert_eq!(hint.likely_failure_type, FailureType::SnapshotFailure);
    }
}
