//! Dry-run the proxy against the LIVE filesystem for one `rg` command,
//! so a fixture author can confirm — before spending a cloud A/B —
//! that a symbol-heavy task will (a) produce enough raw output to
//! trigger substitution and (b) rank the intended gold file as the top
//! owner.
//!
//! Run from the codex-rs root. Pass the rg PATTERN as the first argument
//! (wrapped as `rg -n "<query>" -S <root>`). Optional second argument is
//! the search root — default `.` (whole tree). Use a scoped root when
//! mirroring what the model actually types in practice, e.g.
//!   cargo run -p codex-search-proxy --example render_query -- '<query>' rollout
//! to predict ranking on a `rg ... rollout` query.

use std::path::Path;

use codex_search_proxy::ClassifyOutcome;
use codex_search_proxy::EvidenceOptions;
use codex_search_proxy::ProxyOutcome;
use codex_search_proxy::RipgrepRunner;
use codex_search_proxy::build_proxy_response;
use codex_search_proxy::classify_command;

fn main() {
    let query = std::env::args().nth(1).unwrap_or_default();
    let root = std::env::args().nth(2).unwrap_or_else(|| ".".to_string());
    if query.trim().is_empty() {
        eprintln!("usage: render_query -- '<rg pattern>' [search_root]");
        std::process::exit(2);
    }
    let raw = format!(
        r#"rg -n "{}" -S {}"#,
        query.replace('"', r#"\""#),
        root
    );
    println!("raw: {raw}");
    let classified = match classify_command(&raw) {
        ClassifyOutcome::Eligible(c) => c,
        ClassifyOutcome::PassThrough(r) => {
            println!("classifier PASS_THROUGH: {r}");
            return;
        }
    };
    println!("query: {:?}", classified.query);

    let runner = RipgrepRunner::default();
    let options = EvidenceOptions::default();
    let outcome = build_proxy_response(&classified, Path::new("."), &runner, &options);
    match outcome {
        ProxyOutcome::Substitute {
            rendered,
            raw_bytes,
            evidence,
        } => {
            println!(
                "decision: SUBSTITUTE  raw_bytes={raw_bytes}  files_matched={}  hits={}  compact_bytes={}  confidence={:?}",
                evidence.total_files_matched,
                evidence.total_hits,
                rendered.len(),
                evidence.owner_confidence,
            );
            println!("--- ranked (top {}) ---", evidence.files.len());
            for (i, f) in evidence.files.iter().enumerate() {
                let marker = if i == 0 { " <== TOP" } else { "" };
                println!("  [{i}] {:?}  {}{marker}", f.class, f.path);
            }
            if std::env::var("RENDERED").is_ok() {
                println!("--- rendered (what the model sees) ---");
                println!("{rendered}");
            }
        }
        ProxyOutcome::PassThrough(reason) => {
            println!("decision: PASS_THROUGH  reason={reason:?}");
        }
    }
}
