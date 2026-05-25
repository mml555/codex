//! Local bench for `RepoIntelligenceExtension::contribute()` — measures
//! the per-stage cost of producing the directive fragment using a
//! pre-built `RepoMap` JSON, so we don't need to burn an Azure pair to
//! find the bottleneck.
//!
//! Usage:
//!   CODEX_REPO_INTELLIGENCE_CACHED_MAP=/tmp/codex-rs-repo-map.json \
//!     cargo run -p codex-repo-intelligence-extension --example bench_contribute
//!
//! Drops out the bits that need a real codex session (ExtensionData,
//! Tokio runtime wiring, etc.) and runs the exact pipeline the
//! production `contribute()` runs: map clone, build_context_packet,
//! render_prompt_fragment, narrow_verification_hint.

use std::time::Instant;

use codex_context_harness::BuildPacketOptions;
use codex_context_harness::ContextPacketRenderer;
use codex_context_harness::RunMemory;
use codex_context_harness::build_context_packet;
use codex_repo_index::RepoMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var("CODEX_REPO_INTELLIGENCE_CACHED_MAP")
        .map_err(|_| "set CODEX_REPO_INTELLIGENCE_CACHED_MAP to a RepoMap JSON path")?;
    let task = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Inside the verification crate there is a static array that maps area-id prefixes (like `\"cli\"`) to Cargo package names (like `\"codex-cli\"`). Add a new entry mapping `\"protocol\"` → `\"codex-protocol\"`.".to_string());

    let load_start = Instant::now();
    let bytes = std::fs::read(&path)?;
    let map: RepoMap = serde_json::from_slice(&bytes)?;
    let load_ms = load_start.elapsed().as_millis();
    println!("load (read + parse): {} ms, {} files", load_ms, map.files.len());

    let clone_start = Instant::now();
    let cloned = map.clone();
    println!("RepoMap.clone(): {} ms", clone_start.elapsed().as_millis());

    let packet_start = Instant::now();
    let packet = build_context_packet(
        &task,
        &cloned,
        &RunMemory::default(),
        BuildPacketOptions::default(),
    );
    println!(
        "build_context_packet: {} ms ({} items, {} included)",
        packet_start.elapsed().as_millis(),
        packet.items.len(),
        packet.decision_log.included.len()
    );

    let render_start = Instant::now();
    let _frag = ContextPacketRenderer::render_prompt_fragment(&packet);
    println!("render_prompt_fragment: {} ms", render_start.elapsed().as_millis());

    Ok(())
}
