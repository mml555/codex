//! Example: print the rendered proxy output for the Run 8 query.
//!
//! Run with:
//!   cargo run -p codex-search-proxy --example render_run8

use std::path::Path;

use codex_search_proxy::EvidenceOptions;
use codex_search_proxy::ProxyOutcome;
use codex_search_proxy::StaticRunner;
use codex_search_proxy::build_proxy_response;
use codex_search_proxy::classify_command;
use codex_search_proxy::ClassifyOutcome;

fn main() {
    // The exact rg command the Run 8 vanilla arm fired first.
    let raw = r#"/bin/zsh -lc "rg -n \"AgentEvalResult\" -S .""#;
    let classified = match classify_command(raw) {
        ClassifyOutcome::Eligible(c) => c,
        other => {
            println!("classifier punted: {other:?}");
            return;
        }
    };
    println!("classifier normalized: {}", classified.normalized);
    println!("classifier query:      {}", classified.query);
    println!("classifier flags:      {:?}", classified.flags);
    println!();

    // Pretend rg returned three matches: the Owner file, a related
    // test file, and a tangential source file (renderer).
    let stdout = [
        r#"{"type":"begin","data":{"path":{"text":"context-harness/src/agent_eval.rs"}}}"#,
        r#"{"type":"match","data":{"path":{"text":"context-harness/src/agent_eval.rs"},"lines":{"text":"pub enum AgentEvalResult {"},"line_number":42,"absolute_offset":0,"submatches":[{"match":{"text":""},"start":9,"end":24}]}}"#,
        r#"{"type":"match","data":{"path":{"text":"context-harness/src/agent_eval.rs"},"lines":{"text":"fn classify_result(score: ScorePair) -> AgentEvalResult {"},"line_number":155,"absolute_offset":0,"submatches":[{"match":{"text":""},"start":40,"end":55}]}}"#,
        r#"{"type":"end","data":{"path":{"text":"context-harness/src/agent_eval.rs"},"binary_offset":null,"stats":{}}}"#,
        r#"{"type":"begin","data":{"path":{"text":"context-harness/tests/agent_eval.rs"}}}"#,
        r#"{"type":"match","data":{"path":{"text":"context-harness/tests/agent_eval.rs"},"lines":{"text":"    assert!(matches!(out, AgentEvalResult::Excluded { .. }));"},"line_number":88,"absolute_offset":0,"submatches":[{"match":{"text":""},"start":26,"end":41}]}}"#,
        r#"{"type":"end","data":{"path":{"text":"context-harness/tests/agent_eval.rs"},"binary_offset":null,"stats":{}}}"#,
        r#"{"type":"begin","data":{"path":{"text":"context-harness/src/renderer.rs"}}}"#,
        r#"{"type":"match","data":{"path":{"text":"context-harness/src/renderer.rs"},"lines":{"text":"            AgentEvalResult::Comparable { .. } => render_compare(arm),"},"line_number":210,"absolute_offset":0,"submatches":[{"match":{"text":""},"start":12,"end":27}]}}"#,
        r#"{"type":"end","data":{"path":{"text":"context-harness/src/renderer.rs"},"binary_offset":null,"stats":{}}}"#,
    ]
    .join("\n");

    let runner = StaticRunner::matched(stdout);
    let options = EvidenceOptions::default();
    let outcome = build_proxy_response(&classified, Path::new("."), &runner, &options);

    match outcome {
        ProxyOutcome::Substitute {
            rendered,
            raw_bytes,
            evidence,
        } => {
            println!(
                "decision: SUBSTITUTE  (raw_bytes={raw_bytes}, total_files={tf}, total_hits={th})",
                tf = evidence.total_files_matched,
                th = evidence.total_hits,
            );
            println!("=== rendered output ===");
            println!("{rendered}");
            println!("=== end rendered output ===");
            println!("compact rendered byte count: {}", rendered.len());
        }
        ProxyOutcome::PassThrough(reason) => {
            println!("decision: PASS THROUGH ({reason:?})");
        }
    }
}
