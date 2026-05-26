use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use ignore::WalkBuilder;

/// Collect repo-relative file paths using the same ignore semantics as file-search.
pub fn collect_repo_files(root: &Path) -> Result<Vec<String>> {
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to canonicalize repo root {}", root.display()))?;

    let mut paths = walk_with_require_git(&root, /*require_git*/ true)?;
    if paths.is_empty() {
        paths = walk_with_require_git(&root, false)?;
    }
    paths.sort();
    Ok(paths)
}

fn walk_with_require_git(root: &Path, require_git: bool) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .require_git(require_git)
        .build();

    for entry in walker {
        let entry = entry.with_context(|| "repo walk failed")?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("path {} is outside root", path.display()))?;
        paths.push(relative.to_string_lossy().replace('\\', "/"));
    }
    Ok(paths)
}

pub fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.starts_with("tests/")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_test.py")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".test.tsx")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".spec.tsx")
        || lower.ends_with("_test.go")
}

pub fn is_route_like_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/routes/")
        || lower.contains("/route/")
        || lower.contains("/pages/")
        || lower.contains("/api/")
        || lower.contains("/controllers/")
}

pub fn detect_area_name(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    if lower.contains("/migrations/") || lower.contains("/migration/") {
        return Some("migrations");
    }
    if lower.contains("/components/") || lower.ends_with(".tsx") || lower.ends_with(".jsx") {
        return Some("frontend");
    }
    if is_route_like_path(path) {
        return Some("routes");
    }
    if lower.contains("/services/") || lower.contains("/service/") {
        return Some("services");
    }
    if lower.contains("/models/") || lower.contains("/entities/") {
        return Some("models");
    }
    None
}

pub fn detect_package_manifest(path: &str) -> Option<&'static str> {
    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())?;
    match file_name {
        "Cargo.toml" => Some("cargo"),
        "package.json" => Some("npm"),
        "pyproject.toml" | "pytest.ini" | "setup.py" => Some("python"),
        "go.mod" => Some("go"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_nested_python_manifest() {
        assert_eq!(
            detect_package_manifest("services/foo/pyproject.toml"),
            Some("python")
        );
        assert_eq!(detect_package_manifest("Cargo.toml"), Some("cargo"));
        assert_eq!(detect_package_manifest("not-a-manifest.txt"), None);
    }
}

pub fn extract_import_targets(content: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for line in content.lines().take(200) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("from ") {
            if let Some(module) = rest.split_whitespace().next() {
                targets.push(module.trim_matches('.').to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("import ") {
            let module = rest
                .trim_matches(|c: char| c == '{' || c == ';' || c == '(')
                .split(',')
                .next()
                .unwrap_or("")
                .trim();
            if !module.is_empty() {
                targets.push(module.to_string());
            }
        } else if trimmed.starts_with("use ") {
            let module = trimmed
                .trim_start_matches("use ")
                .trim_end_matches(';')
                .split("::")
                .next()
                .unwrap_or("")
                .trim();
            if !module.is_empty() && module != "crate" && module != "super" {
                targets.push(module.to_string());
            }
        }
    }
    targets.sort();
    targets.dedup();
    targets
}

pub fn read_file_snippet(root: &Path, relative: &str, max_lines: usize) -> Option<String> {
    let path = root.join(relative);
    let content = std::fs::read_to_string(path).ok()?;
    Some(
        content
            .lines()
            .take(max_lines)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}
