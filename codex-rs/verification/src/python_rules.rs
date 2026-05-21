use std::collections::BTreeSet;
use std::path::Path;

use codex_repo_index::RepoMap;

use crate::planner::PlanScope;
use crate::planner::PlannedCommand;
use crate::planner::SkippedCommand;

pub struct PythonPlanPartial {
    pub commands: Vec<PlannedCommand>,
    pub skipped: Vec<SkippedCommand>,
}

pub fn is_python_manifest_path(path: &str) -> bool {
    path.ends_with("pyproject.toml") || path.ends_with("pytest.ini") || path.ends_with("setup.py")
}

pub fn is_python_repo(map: &RepoMap, changed: &[String]) -> bool {
    map.packages.iter().any(|pkg| pkg.kind == "python")
        || changed
            .iter()
            .any(|path| path.ends_with(".py") || is_python_manifest_path(path))
}

pub fn changed_paths_are_python_only(changed: &[String]) -> bool {
    changed
        .iter()
        .all(|path| path.ends_with(".py") || is_python_manifest_path(path))
}

pub fn build_python_verification(map: &RepoMap, changed: &[String]) -> PythonPlanPartial {
    let mut command_keys: BTreeSet<String> = BTreeSet::new();
    let mut commands: Vec<PlannedCommand> = Vec::new();
    let mut skipped: Vec<SkippedCommand> = Vec::new();

    for path in changed {
        if !path.ends_with(".py") {
            continue;
        }

        let test_file = if is_narrow_pytest_target(path) {
            Some(path.clone())
        } else {
            paired_test_for_source(map, path)
        };

        let Some(test_file) = test_file else {
            if path.starts_with("src/") {
                skipped.push(SkippedCommand {
                    command: format!("python -m pytest (no target for `{path}`)"),
                    reason: format!("No narrow pytest target found for `{path}`"),
                });
            }
            continue;
        };

        let command = format!("python -m pytest {test_file}");
        if command_keys.insert(command.clone()) {
            let reason = if path == &test_file {
                format!("changed test file `{path}`")
            } else {
                format!("changed source `{path}`; paired test `{test_file}`")
            };
            commands.push(PlannedCommand {
                command,
                reason,
                scope: PlanScope::Narrow,
                confidence: 0.88,
            });
        }
    }

    skipped.push(SkippedCommand {
        command: "python -m pytest".to_string(),
        reason: "bare pytest invocation is broader than a single test file".to_string(),
    });
    skipped.push(SkippedCommand {
        command: "pytest".to_string(),
        reason: "bare pytest executable is broader than a single test file".to_string(),
    });

    commands.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.command.cmp(&b.command))
    });

    PythonPlanPartial { commands, skipped }
}

fn paired_test_for_source(map: &RepoMap, source_path: &str) -> Option<String> {
    if let Some(entry) = map
        .test_map
        .iter()
        .find(|entry| entry.source_path == source_path)
    {
        return entry
            .test_paths
            .iter()
            .find(|path| is_narrow_pytest_target(path))
            .cloned();
    }

    let stem = Path::new(source_path).file_stem()?.to_str()?;
    let candidate = format!("tests/test_{stem}.py");
    if repo_has_path(map, &candidate) {
        return Some(candidate);
    }
    None
}

fn repo_has_path(map: &RepoMap, path: &str) -> bool {
    is_narrow_pytest_target(path)
        && (map.tests.iter().any(|test| test.path == path)
            || map.files.iter().any(|file| file.path == path))
}

fn is_narrow_pytest_target(path: &str) -> bool {
    if path.is_empty()
        || path == "."
        || path.starts_with('-')
        || path.starts_with('/')
        || path.ends_with('/')
        || path.ends_with('\\')
        || !path.ends_with(".py")
        || path.contains("__pycache__")
    {
        return false;
    }

    if path.chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '$' | '(' | ')' | ';' | '|' | '&' | '>' | '<' | '`' | '"' | '\'' | ':' | '\\'
            )
    }) {
        return false;
    }

    let mut components = path.split('/').peekable();
    while let Some(component) = components.next() {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.starts_with('-')
        {
            return false;
        }
        if components.peek().is_none() {
            return component.starts_with("test_") || component.ends_with("_test.py");
        }
    }

    false
}

/// Returns true when the command is a narrow `python -m pytest <file>` invocation.
pub fn is_narrow_pytest_command(command: &str) -> bool {
    narrow_pytest_file_target(command).is_some()
}

pub(crate) fn narrow_pytest_file_target(command: &str) -> Option<&str> {
    let trimmed = command.trim();
    let Some(rest) = trimmed.strip_prefix("python -m pytest ") else {
        return None;
    };
    let path = rest.trim();
    is_explicit_test_file(path).then_some(path)
}

fn is_explicit_test_file(path: &str) -> bool {
    if path.is_empty() || path == "." {
        return false;
    }
    if path.ends_with('/') || path.ends_with('\\') {
        return false;
    }
    is_narrow_pytest_target(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_repo_index::RepoMap;
    use codex_repo_index::RepoPackage;
    use codex_repo_index::RepoTestEntry;
    use pretty_assertions::assert_eq;

    fn python_map() -> RepoMap {
        RepoMap {
            version: 2,
            repo_id: "py".to_string(),
            root: "/py".to_string(),
            files: vec![],
            tests: vec![RepoTestEntry {
                path: "tests/test_calculator.py".to_string(),
                confidence: 0.9,
                related_paths: vec!["src/calculator.py".to_string()],
                reason: "test".to_string(),
            }],
            areas: vec![],
            packages: vec![RepoPackage {
                path: "pyproject.toml".to_string(),
                kind: "python".to_string(),
                confidence: 0.95,
            }],
            area_maps: vec![],
            commands: vec![],
            test_map: vec![codex_repo_index::TestMapEntry {
                source_path: "src/calculator.py".to_string(),
                test_paths: vec!["tests/test_calculator.py".to_string()],
                confidence: 0.9,
                evidence: vec![],
            }],
            agents_md: None,
            warnings: vec![],
        }
    }

    #[test]
    fn pairs_src_to_test_file() {
        let plan = build_python_verification(&python_map(), &["src/calculator.py".to_string()]);
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(
            plan.commands[0].command,
            "python -m pytest tests/test_calculator.py"
        );
    }

    #[test]
    fn skips_src_without_paired_test() {
        let plan = build_python_verification(&python_map(), &["src/unknown.py".to_string()]);
        assert!(plan.commands.is_empty());
        assert!(
            plan.skipped
                .iter()
                .any(|s| s.reason.contains("No narrow pytest target"))
        );
    }

    #[test]
    fn ignores_pycache_test_map_entries() {
        let mut map = python_map();
        map.test_map[0].test_paths = vec![
            "tests/__pycache__/test_calculator.cpython-313-pytest-9.0.0.pyc".to_string(),
            "tests/test_calculator.py".to_string(),
        ];
        let plan = build_python_verification(&map, &["src/calculator.py".to_string()]);
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(
            plan.commands[0].command,
            "python -m pytest tests/test_calculator.py"
        );
    }

    #[test]
    fn narrow_pytest_command_check() {
        assert!(is_narrow_pytest_command(
            "python -m pytest tests/test_calculator.py"
        ));
        assert!(!is_narrow_pytest_command("python -m pytest"));
        assert!(!is_narrow_pytest_command("python -m pytest tests/"));
        assert!(!is_narrow_pytest_command(
            "python -m pytest tests/test_calculator.py -q"
        ));
        assert!(!is_narrow_pytest_command(
            "python -m pytest tests/test_calculator.py::test_add"
        ));
        assert!(!is_narrow_pytest_command(
            "python -m pytest --rootdir=/tmp/test_calculator.py"
        ));
        assert!(!is_narrow_pytest_command(
            "python -m pytest -c/tests/test_calculator.py"
        ));
        assert!(!is_narrow_pytest_command(
            "python -m pytest tests/-opts/test_calculator.py"
        ));
        assert!(!is_narrow_pytest_command("pytest tests/test_calculator.py"));
        assert!(!is_narrow_pytest_command(
            "python -m pytest src/calculator.py"
        ));
    }

    #[test]
    fn narrow_pytest_target_requires_relative_test_file() {
        assert!(is_narrow_pytest_target("tests/test_calculator.py"));
        assert!(is_narrow_pytest_target(
            "services/foo/tests/calculator_test.py"
        ));
        assert!(!is_narrow_pytest_target("/tmp/test_calculator.py"));
        assert!(!is_narrow_pytest_target("tests/../test_calculator.py"));
        assert!(!is_narrow_pytest_target("tests/test_calculator.py -q"));
        assert!(!is_narrow_pytest_target(
            "--rootdir=/tmp/test_calculator.py"
        ));
        assert!(!is_narrow_pytest_target("-c/tests/test_calculator.py"));
        assert!(!is_narrow_pytest_target("tests/-opts/test_calculator.py"));
        assert!(!is_narrow_pytest_target("src/calculator.py"));
    }
}
