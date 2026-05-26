/// Maximum characters kept in a single stream before summarization.
pub const DEFAULT_MAX_STREAM_CHARS: usize = 24_000;

/// Maximum characters in `failure_packet.relevant_output`.
pub const DEFAULT_MAX_RELEVANT_OUTPUT_CHARS: usize = 8_000;

pub fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!(
        "{truncated}\n… [truncated, {} chars omitted]",
        text.chars().count() - max_chars
    )
}

pub fn combine_output(stdout: &str, stderr: &str, max_chars: usize) -> String {
    let mut combined = String::new();
    if !stdout.is_empty() {
        combined.push_str("--- stdout ---\n");
        combined.push_str(stdout);
        if !stdout.ends_with('\n') {
            combined.push('\n');
        }
    }
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str("--- stderr ---\n");
        combined.push_str(stderr);
    }
    truncate_text(&combined, max_chars)
}

/// Extract failure-focused lines from command output (cargo test, etc.).
pub fn summarize_failure_output(stdout: &str, stderr: &str, max_chars: usize) -> String {
    let combined = combine_output(stdout, stderr, DEFAULT_MAX_STREAM_CHARS);
    let lines: Vec<&str> = combined.lines().collect();
    let mut picked: Vec<&str> = Vec::new();

    for line in &lines {
        let lower = line.to_ascii_lowercase();
        if lower.contains("error:")
            || lower.contains("failed")
            || lower.contains("panicked")
            || lower.contains("assertion")
            || lower.contains("test result:")
            || lower.contains("could not compile")
            || lower.contains("compilation failed")
        {
            picked.push(line);
        }
    }

    let body = if picked.is_empty() {
        let tail_start = lines.len().saturating_sub(80);
        lines[tail_start..].join("\n")
    } else {
        picked.join("\n")
    };

    truncate_text(&body, max_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_text_adds_omission_marker() {
        let out = truncate_text("abcdef", 3);
        assert!(out.contains("truncated"));
        assert!(out.starts_with("abc"));
    }

    #[test]
    fn summarize_failure_output_prefers_error_lines() {
        let stdout = "running 1 test\nok\n";
        let stderr = "error: test failed\n  left: 1\n  right: 2\n";
        let summary = summarize_failure_output(stdout, stderr, 4_000);
        assert!(summary.contains("error:"));
        assert!(!summary.contains("running 1 test"));
    }
}
