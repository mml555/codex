use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Returns per-path change counts in the last `days` from git log.
pub fn git_churn_by_path(root: &Path, days: u32) -> HashMap<String, u32> {
    let output = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "log",
            &format!("--since={days}.days.ago"),
            "--name-only",
            "--pretty=format:",
        ])
        .output();

    let Ok(output) = output else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }

    let mut counts = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let path = line.trim();
        if path.is_empty() {
            continue;
        }
        *counts.entry(path.replace('\\', "/")).or_insert(0) += 1;
    }
    counts
}
