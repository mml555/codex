use anyhow::Result;
use pretty_assertions::assert_eq;

#[test]
fn context_harness_alias_runs_eval_with_fixture_metrics() -> Result<()> {
    let fixture = fixture_path("tasks_synthetic_restaurant.json");
    let map_fixture = fixture_path("repo_map_restaurant.json");

    let mut cmd = codex_command()?;
    let output = cmd
        .args(["context-harness", "eval", "--fixture"])
        .arg(fixture)
        .args(["--map-fixture"])
        .arg(map_fixture)
        .args(["--json"])
        .output()?;

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout)?;
    let report: serde_json::Value = serde_json::from_str(&stdout)?;
    assert_eq!(report["task_count"], 1);

    let metrics = &report["tasks"][0]["metrics"];
    assert!(
        metrics["relevant_file_recall"].as_f64().unwrap_or_default() >= 0.5,
        "stdout:\n{stdout}"
    );
    assert!(
        metrics["token_estimate"].as_u64().unwrap_or_default() > 0,
        "stdout:\n{stdout}"
    );

    Ok(())
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../context-harness/tests/fixtures")
        .join(name)
}

fn codex_command() -> Result<assert_cmd::Command> {
    Ok(assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin(
        "codex",
    )?))
}
