//! Classify the exact rg shapes the model emitted during the
//! sp-mvp-run1 cloud A/B, so we can confirm the search-proxy hook
//! should have fired.

use codex_search_proxy::ClassifyOutcome;
use codex_search_proxy::classify_command;

fn main() {
    let cmds = [
        r#"/bin/zsh -lc "rg -n \"mod agent_eval|classify_result|AgentEvalResult|\\#\\[cfg\\(test\\)\\]\" -S .""#,
        r#"/bin/zsh -lc "rg -n \"classify_result_excluded_when_pair_invalid|fn classify\\(|fn zero_score\\(|mod tests\" context-harness/src/agent_eval.rs -S""#,
        r#"/bin/zsh -lc 'rg -n "fn classify_result_excluded_when_pair_invalid" context-harness/src/agent_eval.rs'"#,
    ];

    for (i, cmd) in cmds.iter().enumerate() {
        println!("=== cmd {i} ===");
        println!("raw: {cmd}");
        match classify_command(cmd) {
            ClassifyOutcome::Eligible(c) => println!(
                "ELIGIBLE query={:?} target_paths={:?} normalized={:?}",
                c.query, c.target_paths, c.normalized
            ),
            ClassifyOutcome::PassThrough(r) => println!("PASS_THROUGH reason={r}"),
        }
        println!();
    }
}
