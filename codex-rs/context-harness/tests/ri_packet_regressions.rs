//! End-to-end regression tests for tasks that previously surfaced
//! the wrong edit target in their RI directive packet. Both tasks
//! were exposed by the no-cloud packet checks run after the
//! intent-file scoring commit:
//!
//! 1. `area-package-alias` (gold: `verification/src/rules.rs`) — the
//!    task says "(like `\"cli\"`)" as a quoted example, which the
//!    ranker mistook for a routing hint to the CLI crate.
//!
//! 2. `pytest-target-check` (gold: `verification/src/python_rules.rs`)
//!    — area inference correctly picked `verification`, but the
//!    within-crate ranker picked `command_exec.rs` over
//!    `python_rules.rs` because of stronger surface term overlap.
//!
//! Both tests run against the LIVE codex-rs RepoMap (slow — indexes
//! the full tree) and are `#[ignore]`d for now because each needs an
//! additional follow-up fix beyond the area-affinity generalization:
//!
//! - Test 1 needs the area inference itself to stop being fooled by
//!   quoted example strings.
//! - Test 2 needs within-crate ownership ranking.
//!
//! Un-ignore each as the corresponding fix lands. Until then they
//! serve as `cargo test --ignored` documentation of the known gaps.

use codex_context_harness::BuildPacketOptions;
use codex_context_harness::ContextPacketRenderer;
use codex_context_harness::RunMemory;
use codex_context_harness::build_context_packet;
use codex_repo_index::RepoMapBuilder;

fn live_codex_rs_map() -> codex_repo_index::RepoMap {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("codex-rs workspace root");
    RepoMapBuilder::build(root).expect("build codex-rs RepoMap")
}

#[test]
fn ri_packet_for_area_package_alias_picks_verification_rules() {
    let map = live_codex_rs_map();
    let task = "Inside the verification crate there is a static array that \
                maps area-id prefixes (like `\"cli\"`) to Cargo package names \
                (like `\"codex-cli\"`). Add a new entry mapping \
                `\"protocol\"` → `\"codex-protocol\"`, keeping the existing \
                tuple shape and alphabetic ordering.";
    let packet = build_context_packet(task, &map, &RunMemory::default(), BuildPacketOptions::default());
    let fragment = ContextPacketRenderer::render_prompt_fragment(&packet);
    let (edit_targets, _orientation) = ContextPacketRenderer::parse_directive_file_lists(&fragment);

    assert_eq!(
        edit_targets.first().map(String::as_str),
        Some("verification/src/rules.rs"),
        "edit target should be the verification rules file, not a CLI \
         file matched only by the quoted `cli` example. Got: {edit_targets:?}\n\
         Fragment:\n{fragment}"
    );
}

#[test]
fn ri_packet_for_pytest_target_picks_python_rules() {
    let map = live_codex_rs_map();
    let task = "In the verification crate, find the helper that returns true \
                when a path matches the narrow single-file pytest target shape \
                (`tests/test_X.py` only, no globs, no `::`, no extra args). \
                Add a `// Narrow single-file pytest target gate.` doc comment \
                immediately above its definition. Do not change any other file.";
    let packet = build_context_packet(task, &map, &RunMemory::default(), BuildPacketOptions::default());
    let fragment = ContextPacketRenderer::render_prompt_fragment(&packet);
    let (edit_targets, _orientation) = ContextPacketRenderer::parse_directive_file_lists(&fragment);

    assert_eq!(
        edit_targets.first().map(String::as_str),
        Some("verification/src/python_rules.rs"),
        "edit target should be python_rules.rs (the actual pytest-target \
         helper owner), not command_exec.rs. Got: {edit_targets:?}\n\
         Fragment:\n{fragment}"
    );
}
