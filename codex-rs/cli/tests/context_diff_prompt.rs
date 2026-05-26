use anyhow::Result;
use tempfile::TempDir;

#[test]
fn context_diff_prompt_ignores_host_env_vars() -> Result<()> {
    let cwd = TempDir::new()?;
    std::fs::create_dir_all(cwd.path().join("core/src"))?;
    std::fs::write(
        cwd.path().join("core/src/prompt_debug.rs"),
        "fn main() {}\n",
    )?;
    std::fs::write(
        cwd.path().join("AGENTS.md"),
        "Relevant code lives in core/src/prompt_debug.rs.\n",
    )?;

    let mut cmd = codex_command()?;
    let output = cmd
        .env("CODEX_HOME", cwd.path().join("missing-codex-home"))
        .env("CODEX_EXEC_SERVER_URL", "not-a-websocket-url")
        .args([
            "context",
            "diff-prompt",
            "make context diff-prompt self-contained without env vars",
            "--cwd",
        ])
        .arg(cwd.path())
        .output()?;

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)?;
    let report: serde_json::Value = serde_json::from_str(&stdout)?;
    let vanilla_paths = report["vanilla_paths"]
        .as_array()
        .expect("vanilla_paths should be an array");
    assert!(
        vanilla_paths
            .iter()
            .any(|path| path.as_str() == Some("core/src/prompt_debug.rs")),
        "stdout:\n{stdout}"
    );

    Ok(())
}

fn codex_command() -> Result<assert_cmd::Command> {
    Ok(assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin(
        "codex",
    )?))
}
