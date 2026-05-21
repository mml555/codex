use codex_verification::VerificationPlanner;
use codex_verification::load_plan_fixtures;
use codex_verification::python_rules::build_python_verification;
use codex_verification::python_rules::is_python_repo;
use codex_verification::run_plan_eval;
use pretty_assertions::assert_eq;

fn python_calculator_map() -> codex_repo_index::RepoMap {
    codex_repo_index::RepoMap {
        version: 2,
        repo_id: "python-calculator".to_string(),
        root: "/fixture".to_string(),
        files: vec![
            codex_repo_index::RepoFileEntry {
                path: "src/calculator.py".to_string(),
                signals: codex_repo_index::RepoSignals::new(0.9),
            },
            codex_repo_index::RepoFileEntry {
                path: "tests/test_calculator.py".to_string(),
                signals: codex_repo_index::RepoSignals::new(0.9),
            },
        ],
        tests: vec![codex_repo_index::RepoTestEntry {
            path: "tests/test_calculator.py".to_string(),
            confidence: 0.9,
            related_paths: vec!["src/calculator.py".to_string()],
            reason: "pytest test file".to_string(),
        }],
        areas: vec![],
        packages: vec![codex_repo_index::RepoPackage {
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
        agents_md: Some("Run pytest for verification.".to_string()),
        warnings: vec![],
    }
}

#[test]
fn nested_pyproject_classifies_python_repo_for_manifest_only_change() {
    let map = codex_repo_index::RepoMap {
        version: 2,
        repo_id: "mono".to_string(),
        root: "/mono".to_string(),
        files: vec![],
        tests: vec![],
        areas: vec![],
        packages: vec![codex_repo_index::RepoPackage {
            path: "services/foo/pyproject.toml".to_string(),
            kind: "python".to_string(),
            confidence: 0.95,
        }],
        area_maps: vec![],
        commands: vec![],
        test_map: vec![],
        agents_md: None,
        warnings: vec![],
    };
    let changed = vec!["services/foo/pyproject.toml".to_string()];
    assert!(is_python_repo(&map, &changed));
    let partial = build_python_verification(&map, &changed);
    assert!(
        partial.commands.is_empty(),
        "manifest-only change must not invent a pytest target"
    );
}

#[test]
fn python_calculator_plan_targets_explicit_pytest_file() {
    let plan =
        VerificationPlanner::plan(&["src/calculator.py".to_string()], &python_calculator_map());
    assert_eq!(plan.commands.len(), 1);
    assert_eq!(
        plan.commands[0].command,
        "python -m pytest tests/test_calculator.py"
    );
    assert!(
        !plan
            .commands
            .iter()
            .any(|cmd| cmd.command.contains("cargo test"))
    );
    assert!(!codex_verification::is_safe_to_run("python -m pytest"));
    assert!(!codex_verification::is_safe_to_run(
        "python -m pytest tests/"
    ));
}

#[test]
fn nested_changed_pytest_file_is_a_narrow_target() {
    let plan = VerificationPlanner::plan(
        &["services/foo/tests/calculator_test.py".to_string()],
        &python_calculator_map(),
    );
    assert_eq!(plan.commands.len(), 1);
    assert_eq!(
        plan.commands[0].command,
        "python -m pytest services/foo/tests/calculator_test.py"
    );
}

#[test]
fn python_plan_rejects_option_like_pytest_targets() {
    for command in [
        "python -m pytest --rootdir=/tmp/test_calculator.py",
        "python -m pytest -c/tests/test_calculator.py",
        "python -m pytest tests/-opts/test_calculator.py",
        "python -m pytest tests/test_calculator.py -q",
        "python -m pytest tests/test_calculator.py::test_add",
        "python -m pytest tests/test_*.py",
        "python -m pytest tests/test_$USER.py",
    ] {
        assert!(
            !codex_verification::is_safe_to_run(command),
            "{command} must not be runnable"
        );
    }

    for target in [
        "--rootdir=/tmp/test_calculator.py",
        "-c/tests/test_calculator.py",
        "tests/-opts/test_calculator.py",
    ] {
        let plan = VerificationPlanner::plan(&[target.to_string()], &python_calculator_map());
        assert!(
            !plan
                .commands
                .iter()
                .any(|cmd| cmd.command.starts_with("python -m pytest")),
            "{target} must not produce a pytest command"
        );
    }

    let mut map = python_calculator_map();
    map.test_map[0].test_paths = vec![
        "--rootdir=/tmp/test_calculator.py".to_string(),
        "-c/tests/test_calculator.py".to_string(),
        "tests/-opts/test_calculator.py".to_string(),
        "tests/test_calculator.py -q".to_string(),
        "tests/test_calculator.py::test_add".to_string(),
        "tests/test_*.py".to_string(),
        "tests/test_$USER.py".to_string(),
    ];
    let plan = VerificationPlanner::plan(&["src/calculator.py".to_string()], &map);
    assert!(
        plan.commands.is_empty(),
        "unsafe paired pytest targets must not produce commands"
    );
}

#[test]
fn python_unknown_src_has_no_runnable_command() {
    let plan = VerificationPlanner::plan(&["src/unknown.py".to_string()], &python_calculator_map());
    assert!(plan.commands.is_empty());
    assert!(
        plan.skipped
            .iter()
            .any(|cmd| cmd.reason.contains("No narrow pytest target"))
    );
}

#[test]
fn python_plan_fixtures_meet_accuracy_target() {
    let fixtures = load_plan_fixtures(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/plan_python_calculator.json"
    )))
    .unwrap();
    let report = run_plan_eval(&fixtures, &python_calculator_map());
    assert!(
        report.avg_accuracy >= 0.70,
        "avg accuracy {:.2}",
        report.avg_accuracy
    );
    for fixture in &report.fixtures {
        assert!(
            fixture.forbidden_hits.is_empty(),
            "forbidden hits {:?} for {:?}",
            fixture.forbidden_hits,
            fixture.changed_files
        );
    }
}
