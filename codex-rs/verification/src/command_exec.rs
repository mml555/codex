use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use crate::python_rules;

/// Parsed narrow verification command (no shell interpretation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NarrowCommand {
    CargoTestPackage { package: String },
    PythonPytestFile { test_file: String },
}

/// Returns true when the command matches a supported narrow argv form.
pub fn is_safe_to_run(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    if lower.contains("--workspace") {
        return false;
    }
    if lower.contains("cargo test") && lower.contains(" --all") {
        return false;
    }
    parse_narrow_command(command).is_some()
}

pub fn parse_narrow_command(command: &str) -> Option<NarrowCommand> {
    let trimmed = command.trim();
    if let Some(package) = trimmed.strip_prefix("cargo test -p ") {
        let package = package.trim();
        if is_safe_cargo_package_name(package) {
            return Some(NarrowCommand::CargoTestPackage {
                package: package.to_string(),
            });
        }
        return None;
    }

    if let Some(path) = python_rules::narrow_pytest_file_target(trimmed) {
        return Some(NarrowCommand::PythonPytestFile {
            test_file: path.to_string(),
        });
    }

    None
}

fn is_safe_cargo_package_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

pub fn spawn_narrow_command(command: &str, cwd: &Path) -> std::io::Result<std::process::Child> {
    let parsed = parse_narrow_command(command).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "unparseable command")
    })?;

    let mut cmd = match parsed {
        NarrowCommand::CargoTestPackage { package } => {
            let mut c = Command::new("cargo");
            c.args(["test", "-p", &package]);
            c
        }
        NarrowCommand::PythonPytestFile { test_file } => {
            let mut c = Command::new("python");
            c.args(["-m", "pytest", &test_file]);
            c
        }
    };

    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_cargo_and_pytest() {
        assert_eq!(
            parse_narrow_command("cargo test -p codex-core"),
            Some(NarrowCommand::CargoTestPackage {
                package: "codex-core".to_string()
            })
        );
        assert_eq!(
            parse_narrow_command("python -m pytest tests/test_foo.py"),
            Some(NarrowCommand::PythonPytestFile {
                test_file: "tests/test_foo.py".to_string()
            })
        );
    }

    #[test]
    fn rejects_shell_metacharacters() {
        assert!(parse_narrow_command("cargo test -p foo; rm -rf /").is_none());
        assert!(parse_narrow_command("cargo test -p foo --all-features").is_none());
        assert!(parse_narrow_command("python -m pytest tests/$(whoami).py").is_none());
        assert!(!is_safe_to_run("echo ok"));
    }

    #[test]
    fn rejects_pytest_extra_args_and_non_test_files() {
        assert!(parse_narrow_command("python -m pytest tests/test_foo.py -q").is_none());
        assert!(parse_narrow_command("python -m pytest --rootdir=/tmp/test_foo.py").is_none());
        assert!(parse_narrow_command("python -m pytest -c/tests/test_foo.py").is_none());
        assert!(parse_narrow_command("python -m pytest tests/-opts/test_foo.py").is_none());
        assert!(parse_narrow_command("python -m pytest src/foo.py").is_none());
        assert!(parse_narrow_command("python -m pytest /tmp/test_foo.py").is_none());
        assert!(parse_narrow_command("python -m pytest tests/../test_foo.py").is_none());
    }
}
