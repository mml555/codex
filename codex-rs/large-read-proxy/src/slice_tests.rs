use crate::BuildPassThroughReason;
use crate::ClassifyOutcome;
use crate::LargeReadOutcome;
use crate::SliceOptions;
use crate::build_large_read_response;
use crate::build_slices;
use crate::classify_command;

fn big_rust_file() -> String {
    let mut s = String::from("//! module header\nuse std::collections::HashSet;\n\n");
    for i in 0..200 {
        s.push_str(&format!("// filler line {i}\n"));
    }
    s.push_str("pub fn alpha(x: u32) -> u32 {\n    x + 1\n}\n");
    for i in 0..50 {
        s.push_str(&format!("// more filler {i}\n"));
    }
    s.push_str("pub struct Beta {\n    field: u32,\n}\n");
    s
}

fn classified(cmd: &str) -> crate::ClassifiedRead {
    match classify_command(cmd) {
        ClassifyOutcome::Eligible(c) => c,
        other => panic!("expected Eligible, got {other:?}"),
    }
}

#[test]
fn build_slices_includes_header_and_public_defs() {
    let content = big_rust_file();
    let slices = build_slices(&content, &[], &SliceOptions::default());
    assert!(!slices.is_empty());
    assert!(slices.len() <= 3, "capped at max_slices: {slices:?}");
    assert_eq!(slices[0].start, 1, "first slice is the header");
    assert!(slices.iter().all(|s| s.line_count() <= 30));
    for w in slices.windows(2) {
        assert!(w[0].end < w[1].start, "slices must be disjoint: {slices:?}");
    }
    assert!(
        slices.iter().any(|s| s.reason.contains("definition:")),
        "expected a definition slice: {slices:?}"
    );
}

#[test]
fn build_slices_uses_hints_when_provided() {
    let content = big_rust_file();
    let slices = build_slices(
        &content,
        &["struct Beta".to_string()],
        &SliceOptions::default(),
    );
    assert!(
        slices
            .iter()
            .any(|s| s.reason.contains("match: struct Beta")),
        "expected a hint-match slice: {slices:?}"
    );
}

#[test]
fn build_response_substitutes_for_large_file() {
    let content = big_rust_file();
    let c = classified("cat foo.rs");
    match build_large_read_response(&c, &content, &[], &SliceOptions::default()) {
        LargeReadOutcome::Substitute {
            rendered,
            total_lines,
            ..
        } => {
            // v2: emits a compact-view header, actual line-numbered content
            // (not just a range pointer), an omitted-lines note, and a
            // CONDITIONAL re-run line (no open "repeat to bypass" invitation).
            assert!(rendered.contains("[large-read proxy] compact view"));
            assert!(rendered.contains("# lines "));
            assert!(rendered.contains("lines not shown"));
            assert!(rendered.contains("Re-run the identical command only if"));
            assert!(!rendered.contains("Repeat the exact same command"));
            assert!(total_lines > 120);
        }
        other => panic!("expected Substitute, got {other:?}"),
    }
}

#[test]
fn rendered_slices_contain_actual_line_numbered_content() {
    // The whole point of v2: the model must see real code lines, not just a
    // "lines X-Y" pointer. Assert a known source line appears, prefixed by
    // its line number.
    let content = big_rust_file();
    let c = classified("cat foo.rs");
    if let LargeReadOutcome::Substitute { rendered, .. } =
        build_large_read_response(&c, &content, &[], &SliceOptions::default())
    {
        // The header slice starts at line 1; its content must be emitted
        // with a leading "1: " line number.
        assert!(
            rendered.lines().any(|l| l.starts_with("1: ")),
            "expected line-numbered content starting at 1; got:\n{rendered}"
        );
    } else {
        panic!("expected Substitute");
    }
}

#[test]
fn build_response_passes_through_small_file() {
    let c = classified("cat foo.rs");
    let content = "//! tiny\npub fn x() {}\n";
    assert!(matches!(
        build_large_read_response(&c, content, &[], &SliceOptions::default()),
        LargeReadOutcome::PassThrough(BuildPassThroughReason::FileSmallEnough { .. })
    ));
}

/// C2 failure-mode coverage: an empty file (zero bytes / zero lines) must
/// pass through cleanly, never panic. Mirrors the corner the LRP handler
/// hits when an opted-in operator runs `cat <empty_file>`.
#[test]
fn build_response_passes_through_empty_content() {
    let c = classified("cat foo.rs");
    match build_large_read_response(&c, "", &[], &SliceOptions::default()) {
        LargeReadOutcome::PassThrough(_) => {}
        other => panic!("expected PassThrough on empty content, got {other:?}"),
    }
}

/// C2 failure-mode coverage: a file with the minimum eligible line count
/// but no slice-worthy content (no public defs, no test mod, no header
/// space remaining after header) must pass through as `NoSlices` rather
/// than substitute an empty slice list.
#[test]
fn build_response_passes_through_no_slices_when_options_yield_none() {
    let c = classified("cat foo.rs");
    // 200 blank lines past MIN_FILE_LINES; no public defs, no test module.
    let content: String = std::iter::repeat("\n").take(200).collect();
    let opts = SliceOptions {
        // Cap at 0 slices so try_push always rejects; exercises the
        // NoSlices branch without depending on content quirks.
        max_slices: 0,
        ..SliceOptions::default()
    };
    match build_large_read_response(&c, &content, &[], &opts) {
        LargeReadOutcome::PassThrough(BuildPassThroughReason::NoSlices) => {}
        other => panic!("expected PassThrough(NoSlices), got {other:?}"),
    }
}
