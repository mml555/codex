//! Dry-run the large-read proxy against a real file: classify the read,
//! build the compact slices, and print exactly what the model would see
//! in place of the raw file dump. Confirms (before any cloud spend) that
//! a candidate large-read task actually intercepts and that the slices
//! carry enough to do the edit.
//!
//!   cargo run -p codex-large-read-proxy --example render_read -- "cat <file>"

use codex_large_read_proxy::BuildPassThroughReason;
use codex_large_read_proxy::ClassifyOutcome;
use codex_large_read_proxy::LargeReadOutcome;
use codex_large_read_proxy::SliceOptions;
use codex_large_read_proxy::build_large_read_response;
use codex_large_read_proxy::classify_command;

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    if cmd.trim().is_empty() {
        eprintln!("usage: render_read -- \"cat <file>\"  |  \"sed -n '1,400p' <file>\"");
        std::process::exit(2);
    }
    println!("raw: {cmd}");

    let classified = match classify_command(&cmd) {
        ClassifyOutcome::Eligible(c) => c,
        ClassifyOutcome::PassThrough(r) => {
            println!("classify: PASS_THROUGH ({r:?})");
            return;
        }
    };

    let content = match std::fs::read_to_string(&classified.path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("read {}: {e}", classified.path);
            std::process::exit(1);
        }
    };

    let opts = SliceOptions::default();
    match build_large_read_response(&classified, &content, &[], &opts) {
        LargeReadOutcome::Substitute {
            rendered,
            slices,
            total_lines,
            raw_bytes,
        } => {
            println!(
                "decision: SUBSTITUTE  total_lines={total_lines}  raw_bytes={raw_bytes}  \
                 slices={}  compact_bytes={}  reduction={:.1}%",
                slices.len(),
                rendered.len(),
                100.0 * (1.0 - rendered.len() as f64 / raw_bytes.max(1) as f64),
            );
            println!("--- rendered (what the model sees) ---");
            println!("{rendered}");
        }
        LargeReadOutcome::PassThrough(BuildPassThroughReason::FileSmallEnough { lines }) => {
            println!("decision: PASS_THROUGH (file small enough, {lines} lines)");
        }
        LargeReadOutcome::PassThrough(BuildPassThroughReason::NoSlices) => {
            println!("decision: PASS_THROUGH (no slices)");
        }
    }
}
