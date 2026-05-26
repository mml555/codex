use std::path::Path;

use pretty_assertions::assert_eq;

use super::ClassifiedRg;
use super::RgFlags;
use super::evidence::EvidenceOptions;
use super::evidence::FileClass;
use super::evidence_builder::ProxyOutcome;
use super::evidence_builder::ProxyPassThroughReason;
use super::evidence_builder::build_proxy_response;
use super::rg_runner::RgExitStatus;
use super::rg_runner::StaticRunner;

fn classified(query: &str) -> ClassifiedRg {
    ClassifiedRg {
        query: query.to_string(),
        target_paths: Vec::new(),
        flags: RgFlags::default(),
        normalized: format!("rg {query:?}"),
    }
}

fn begin(path: &str) -> String {
    format!(
        r#"{{"type":"begin","data":{{"path":{{"text":{path:?}}}}}}}"#
    )
}

fn match_event(path: &str, line: u32, text: &str, start: u64) -> String {
    format!(
        r#"{{"type":"match","data":{{"path":{{"text":{path:?}}},"lines":{{"text":{text:?}}},"line_number":{line},"absolute_offset":0,"submatches":[{{"match":{{"text":""}},"start":{start},"end":{end_pos}}}]}}}}"#,
        path = path,
        text = text,
        line = line,
        start = start,
        end_pos = start + 1
    )
}

fn end(path: &str) -> String {
    format!(
        r#"{{"type":"end","data":{{"path":{{"text":{path:?}}}, "binary_offset":null,"stats":{{}}}}}}"#
    )
}

fn jsonl(events: &[String]) -> Vec<u8> {
    events.join("\n").into_bytes()
}

fn opts() -> EvidenceOptions {
    EvidenceOptions {
        // Generous so the size-guard doesn't fire in unit tests
        // unless we are specifically testing it.
        raw_pass_through_bytes: 0,
        raw_pass_through_lines: 0,
        ..EvidenceOptions::default()
    }
}

#[test]
fn no_matches_passes_through() {
    let runner = StaticRunner::no_matches();
    let outcome = build_proxy_response(
        &classified("AgentEvalResult"),
        Path::new("."),
        &runner,
        &opts(),
    );
    assert_eq!(
        outcome,
        ProxyOutcome::PassThrough(ProxyPassThroughReason::NoMatches)
    );
}

#[test]
fn rg_error_passes_through() {
    let runner = StaticRunner::error();
    let outcome = build_proxy_response(
        &classified("AgentEvalResult"),
        Path::new("."),
        &runner,
        &opts(),
    );
    assert_eq!(
        outcome,
        ProxyOutcome::PassThrough(ProxyPassThroughReason::RgError)
    );
}

#[test]
fn matched_but_empty_stdout_passes_through() {
    let runner = StaticRunner {
        bytes: Vec::new(),
        status: RgExitStatus::Matched,
    };
    let outcome = build_proxy_response(
        &classified("AgentEvalResult"),
        Path::new("."),
        &runner,
        &opts(),
    );
    assert_eq!(
        outcome,
        ProxyOutcome::PassThrough(ProxyPassThroughReason::NoMatches)
    );
}

#[test]
fn substitutes_with_owner_ranked_first() {
    // Two files match: a Source-class file alphabetically before the
    // Owner. The Owner must rank first regardless of path order.
    let bytes = jsonl(&[
        begin("aaa-source.rs"),
        match_event(
            "aaa-source.rs",
            10,
            "    let r = AgentEvalResult::Comparable {",
            12,
        ),
        end("aaa-source.rs"),
        begin("context-harness/src/agent_eval.rs"),
        match_event(
            "context-harness/src/agent_eval.rs",
            42,
            "pub enum AgentEvalResult {",
            4,
        ),
        end("context-harness/src/agent_eval.rs"),
    ]);
    let runner = StaticRunner::matched(bytes);
    let outcome = build_proxy_response(
        &classified("AgentEvalResult"),
        Path::new("."),
        &runner,
        &opts(),
    );
    let ProxyOutcome::Substitute {
        evidence, rendered, ..
    } = outcome
    else {
        panic!("expected Substitute, got {outcome:?}");
    };
    assert_eq!(evidence.files.len(), 2);
    assert_eq!(evidence.files[0].class, FileClass::Owner);
    assert_eq!(evidence.files[0].path, "context-harness/src/agent_eval.rs");
    assert_eq!(evidence.files[1].class, FileClass::Source);
    assert!(rendered.contains("Search proxy intercepted:"));
    assert!(
        rendered.find("context-harness/src/agent_eval.rs").unwrap()
            < rendered.find("aaa-source.rs").unwrap(),
        "owner must appear before source in rendered output: {rendered}"
    );
}

#[test]
fn test_files_classified_separately_from_source() {
    let bytes = jsonl(&[
        begin("context-harness/src/agent_eval.rs"),
        match_event(
            "context-harness/src/agent_eval.rs",
            42,
            "pub enum AgentEvalResult {",
            4,
        ),
        end("context-harness/src/agent_eval.rs"),
        begin("context-harness/tests/agent_eval.rs"),
        match_event(
            "context-harness/tests/agent_eval.rs",
            88,
            "    assert!(matches!(x, AgentEvalResult::Excluded));",
            24,
        ),
        end("context-harness/tests/agent_eval.rs"),
    ]);
    let runner = StaticRunner::matched(bytes);
    let outcome = build_proxy_response(
        &classified("AgentEvalResult"),
        Path::new("."),
        &runner,
        &opts(),
    );
    let ProxyOutcome::Substitute {
        evidence, rendered, ..
    } = outcome
    else {
        panic!("expected Substitute");
    };
    assert_eq!(evidence.files[0].class, FileClass::Owner);
    assert_eq!(evidence.files[1].class, FileClass::RelatedTest);
    assert!(
        rendered.contains("test expectations"),
        "next-step bullet missing: {rendered}"
    );
}

#[test]
fn caps_total_files_at_max_files() {
    let mut events: Vec<String> = Vec::new();
    for i in 0..10 {
        let path = format!("file{i}.rs");
        events.push(begin(&path));
        events.push(match_event(&path, 1, "fn x() {}", 3));
        events.push(end(&path));
    }
    let bytes = jsonl(&events);
    let runner = StaticRunner::matched(bytes);
    let options = EvidenceOptions {
        max_files: 3,
        ..opts()
    };
    let outcome = build_proxy_response(&classified("x"), Path::new("."), &runner, &options);
    let ProxyOutcome::Substitute {
        evidence, rendered, ..
    } = outcome
    else {
        panic!("expected Substitute");
    };
    assert_eq!(evidence.files.len(), 3);
    assert_eq!(evidence.total_files_matched, 10);
    assert!(
        rendered.contains("showing top 3 of 10 matching files"),
        "should announce cap: {rendered}"
    );
}

#[test]
fn caps_per_file_hits_at_max_hits_per_file() {
    let mut events: Vec<String> = vec![begin("file.rs")];
    for line in 1..=10 {
        events.push(match_event("file.rs", line, "fn x() {}", 3));
    }
    events.push(end("file.rs"));
    let bytes = jsonl(&events);
    let runner = StaticRunner::matched(bytes);
    let options = EvidenceOptions {
        max_hits_per_file: 2,
        ..opts()
    };
    let outcome = build_proxy_response(&classified("x"), Path::new("."), &runner, &options);
    let ProxyOutcome::Substitute { evidence, .. } = outcome else {
        panic!("expected Substitute");
    };
    assert_eq!(evidence.files[0].hits.len(), 2);
    assert_eq!(evidence.total_hits, 10);
}

#[test]
fn truncates_long_snippets_with_ellipsis() {
    let long_line = "x".repeat(500);
    let bytes = jsonl(&[
        begin("file.rs"),
        match_event("file.rs", 1, &long_line, 0),
        end("file.rs"),
    ]);
    let runner = StaticRunner::matched(bytes);
    let options = EvidenceOptions {
        max_snippet_chars: 50,
        ..opts()
    };
    let outcome = build_proxy_response(&classified("x"), Path::new("."), &runner, &options);
    let ProxyOutcome::Substitute { evidence, .. } = outcome else {
        panic!("expected Substitute");
    };
    let snip = &evidence.files[0].hits[0].snippet;
    assert!(snip.chars().count() <= 50, "snippet too long: {snip}");
    assert!(snip.ends_with('…'), "missing ellipsis: {snip}");
}

#[test]
fn raw_smaller_than_compact_passes_through() {
    // Tiny raw output (well under the 2KB / 30-line default) where
    // the compact render would be bigger.
    let bytes = jsonl(&[
        begin("file.rs"),
        match_event("file.rs", 1, "x", 0),
        end("file.rs"),
    ]);
    let runner = StaticRunner::matched(bytes);
    let options = EvidenceOptions::default(); // defaults: 2048 bytes / 30 lines threshold
    let outcome = build_proxy_response(&classified("x"), Path::new("."), &runner, &options);
    match outcome {
        ProxyOutcome::PassThrough(ProxyPassThroughReason::RawIsSmallerThanCompact {
            raw_bytes,
            compact_bytes,
        }) => {
            assert!(
                raw_bytes < compact_bytes,
                "raw should be smaller: {raw_bytes} vs {compact_bytes}"
            );
        }
        other => panic!("expected RawIsSmallerThanCompact, got {other:?}"),
    }
}

#[test]
fn raw_above_threshold_still_substitutes_even_if_compact_is_smaller() {
    // Force the size guard to NOT fire by making raw bigger than the
    // threshold. The proxy should substitute even though raw might be
    // larger than the compact output — the threshold's whole point.
    let big_line = "x".repeat(100);
    let mut events: Vec<String> = vec![begin("file.rs")];
    for line in 1..=40 {
        events.push(match_event("file.rs", line, &big_line, 0));
    }
    events.push(end("file.rs"));
    let bytes = jsonl(&events);
    let runner = StaticRunner::matched(bytes);
    let options = EvidenceOptions {
        max_hits_per_file: 3,
        max_total_bytes: 4_096,
        max_total_lines: 120,
        raw_pass_through_bytes: 512, // well below the simulated raw
        raw_pass_through_lines: 10,  // well below the 40 match events
        ..EvidenceOptions::default()
    };
    let outcome = build_proxy_response(&classified("x"), Path::new("."), &runner, &options);
    let ProxyOutcome::Substitute { evidence, .. } = outcome else {
        panic!("expected Substitute, got {outcome:?}");
    };
    assert_eq!(evidence.files[0].hits.len(), 3);
}

#[test]
fn rendered_output_contains_required_sections() {
    let bytes = jsonl(&[
        begin("context-harness/src/agent_eval.rs"),
        match_event(
            "context-harness/src/agent_eval.rs",
            42,
            "pub enum AgentEvalResult {",
            4,
        ),
        end("context-harness/src/agent_eval.rs"),
    ]);
    let runner = StaticRunner::matched(bytes);
    let outcome = build_proxy_response(
        &classified("AgentEvalResult"),
        Path::new("."),
        &runner,
        &opts(),
    );
    let ProxyOutcome::Substitute { rendered, .. } = outcome else {
        panic!("expected Substitute");
    };
    for required in [
        "Search proxy intercepted:",
        "Original command:",
        "rg \"AgentEvalResult\"",
        "Compact evidence:",
        "context-harness/src/agent_eval.rs",
        "line 42",
        "pub enum AgentEvalResult {",
        "Reason: likely owner",
        "Suggested next step:",
        "Repeat the exact same rg command",
    ] {
        assert!(
            rendered.contains(required),
            "missing required text {required:?}; rendered:\n{rendered}"
        );
    }
}
