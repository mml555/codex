use std::path::Path;

use pretty_assertions::assert_eq;

use super::ClassifiedRg;
use super::RgFlags;
use super::evidence::EvidenceOptions;
use super::evidence::FileClass;
use super::evidence::OwnerConfidence;
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
    format!(r#"{{"type":"begin","data":{{"path":{{"text":{path:?}}}}}}}"#)
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
fn run3_ranks_gold_owner_first_by_relevance() {
    // End-to-end gate for the run3 ranking miss. The model's query was
    // a broad alternation; three files all classify as Owner. Before
    // the relevance fix, the alphabetical tiebreak put
    // context-harness/src/task_terms.rs first and the gold
    // verification/src/rules.rs third. After the fix, rules.rs must
    // rank first because its DEFINITIONS (package_name_for_area,
    // area_id_for_path) align with the query words.
    //
    // Each file carries its full matched set (not just the 3 lines the
    // renderer would display) because the relevance scorer reads all
    // matched lines.
    let bytes = jsonl(&[
        begin("context-harness/src/task_terms.rs"),
        match_event(
            "context-harness/src/task_terms.rs",
            27,
            "/// `area_id` appearing only inside a quoted example does NOT",
            8,
        ),
        match_event(
            "context-harness/src/task_terms.rs",
            302,
            "        .any(|term| area.area_id.contains(term) || term.contains(&area.area_id))",
            30,
        ),
        match_event(
            "context-harness/src/task_terms.rs",
            418,
            "pub fn build_task_terms(task: &str, map: &RepoMap) -> TaskTerms {",
            7,
        ),
        end("context-harness/src/task_terms.rs"),
        begin("repo-index/src/repo_map.rs"),
        match_event(
            "repo-index/src/repo_map.rs",
            63,
            "    pub fn area_map_for_id(&self, area_id: &str) -> Option<&AreaMap> {",
            15,
        ),
        end("repo-index/src/repo_map.rs"),
        begin("verification/src/rules.rs"),
        match_event(
            "verification/src/rules.rs",
            16,
            "/// Cargo package names for codex-rs area roots (path -> `cargo test -p` name).",
            4,
        ),
        match_event(
            "verification/src/rules.rs",
            365,
            "fn area_id_for_path(path: &str, map: &RepoMap) -> Option<String> {",
            3,
        ),
        match_event(
            "verification/src/rules.rs",
            386,
            "fn package_name_for_area(area_id: &str) -> Option<String> {",
            3,
        ),
        end("verification/src/rules.rs"),
    ]);
    let runner = StaticRunner::matched(bytes);
    let outcome = build_proxy_response(
        &classified("area id|area_id|cargo test -p|targeted cargo|package name|lookup table"),
        Path::new("."),
        &runner,
        &opts(),
    );
    let ProxyOutcome::Substitute { evidence, .. } = outcome else {
        panic!("expected Substitute, got {outcome:?}");
    };
    assert_eq!(
        evidence.files[0].path,
        "verification/src/rules.rs",
        "gold file must rank first; got order: {:?}",
        evidence.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
    // task_terms.rs must no longer outrank the gold file.
    let rules_idx = evidence
        .files
        .iter()
        .position(|f| f.path == "verification/src/rules.rs");
    let task_terms_idx = evidence
        .files
        .iter()
        .position(|f| f.path == "context-harness/src/task_terms.rs");
    assert!(
        rules_idx < task_terms_idx,
        "rules.rs ({rules_idx:?}) must rank above task_terms.rs ({task_terms_idx:?})"
    );
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

/// C2 failure-mode coverage: when the runner can't spawn at all (rg
/// missing from PATH is the realistic operator failure), the builder
/// must return `RunnerError` carrying the spawn message so the handler
/// can log a useful reason — NOT panic, NOT silently substitute empty.
#[test]
fn runner_spawn_failure_carries_message_and_passes_through() {
    /// A runner that always reports a spawn failure with a distinctive
    /// message — mimics rg-missing-from-PATH at the SearchRunner trait.
    struct SpawnFailRunner;
    impl super::rg_runner::SearchRunner for SpawnFailRunner {
        fn run(
            &self,
            _classified: &super::ClassifiedRg,
            _cwd: &Path,
            _options: &EvidenceOptions,
        ) -> Result<super::rg_runner::RawSearchOutput, super::rg_runner::SearchRunnerError>
        {
            Err(super::rg_runner::SearchRunnerError::Spawn(
                "rg-binary-missing".to_string(),
            ))
        }
    }
    let outcome = build_proxy_response(
        &classified("AgentEvalResult"),
        Path::new("."),
        &SpawnFailRunner,
        &opts(),
    );
    match outcome {
        ProxyOutcome::PassThrough(ProxyPassThroughReason::RunnerError(msg)) => {
            assert!(
                msg.contains("rg-binary-missing"),
                "spawn error message must be preserved for debug logs; got {msg:?}"
            );
        }
        other => panic!("expected PassThrough(RunnerError), got {other:?}"),
    }
}

/// A wall-clock timeout on the internal `rg` must pass through (run the
/// model's own command) — never panic, never substitute partial/empty output.
#[test]
fn runner_timeout_passes_through() {
    struct TimeoutRunner;
    impl super::rg_runner::SearchRunner for TimeoutRunner {
        fn run(
            &self,
            _classified: &super::ClassifiedRg,
            _cwd: &Path,
            _options: &EvidenceOptions,
        ) -> Result<super::rg_runner::RawSearchOutput, super::rg_runner::SearchRunnerError>
        {
            Err(super::rg_runner::SearchRunnerError::Timeout(
                std::time::Duration::from_secs(5),
            ))
        }
    }
    let outcome = build_proxy_response(
        &classified("AgentEvalResult"),
        Path::new("."),
        &TimeoutRunner,
        &opts(),
    );
    assert!(
        matches!(outcome, ProxyOutcome::PassThrough(_)),
        "rg timeout must pass through, got {outcome:?}"
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
    assert!(rendered.contains("[search-proxy] compact rg result"));
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
    // v2 surfaces the test file as an rg-native match line (no prose
    // "next step" bullet), ranked after the owner.
    assert!(
        rendered.contains("context-harness/tests/agent_eval.rs:"),
        "test file missing from rendered output: {rendered}"
    );
    assert!(
        rendered.find("context-harness/src/agent_eval.rs:").unwrap()
            < rendered
                .find("context-harness/tests/agent_eval.rs:")
                .unwrap(),
        "owner must rank before test file: {rendered}"
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
    // v2 announces coverage in the header and the omission footer.
    assert!(
        rendered.contains("3 of 10 file(s)"),
        "should announce shown/total in header: {rendered}"
    );
    assert!(
        rendered.contains("7 more file(s)"),
        "should announce omitted count: {rendered}"
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
fn lean_v2_compact_substitutes_even_for_tiny_raw() {
    // v2's rg-native compact has no prose, so it is essentially always
    // smaller than rg's verbose `--json` stream — even for a single
    // tiny match. The size guard (RawIsSmallerThanCompact) remains as a
    // defensive net but rarely fires under v2, so a tiny match now
    // substitutes rather than passing through.
    let bytes = jsonl(&[
        begin("file.rs"),
        match_event("file.rs", 1, "x", 0),
        end("file.rs"),
    ]);
    let runner = StaticRunner::matched(bytes);
    let options = EvidenceOptions::default();
    let outcome = build_proxy_response(&classified("x"), Path::new("."), &runner, &options);
    assert!(
        matches!(outcome, ProxyOutcome::Substitute { .. }),
        "v2 lean compact should substitute a tiny match, got {outcome:?}"
    );
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
        "[search-proxy] compact rg result",
        "# likely owner (defines the searched symbol): context-harness/src/agent_eval.rs",
        // rg-native match line: path:line:col:text
        "context-harness/src/agent_eval.rs:42:",
        "pub enum AgentEvalResult {",
        "# Re-run the identical command only if you need the matches not shown above.",
    ] {
        assert!(
            rendered.contains(required),
            "missing required text {required:?}; rendered:\n{rendered}"
        );
    }
}

// ---- A4: confidence-gate regression tests ----
//
// Lock the conservative Strong/Weak/None rules: a Strong owner must be a
// multi-word exact symbol that is roughly unique, OR a multi-word dominant
// concept query. Single generic words ("config", "redact", "truncate")
// must NEVER render as Strong — those are the queries that previously
// produced confident-but-wrong owners. The renderer's Weak/None labels
// must be non-directive ("do NOT trust this path …", "use the matches
// below … do NOT pick").

fn run(query: &str, events: Vec<String>) -> ProxyOutcome {
    let bytes = jsonl(&events);
    let runner = StaticRunner::matched(bytes);
    build_proxy_response(&classified(query), Path::new("."), &runner, &opts())
}

fn confidence(query: &str, events: Vec<String>) -> OwnerConfidence {
    match run(query, events) {
        ProxyOutcome::Substitute { evidence, .. } => evidence.owner_confidence,
        other => panic!("expected Substitute, got {other:?}"),
    }
}

#[test]
fn confidence_strong_for_multi_word_unique_exact_symbol() {
    // `git_churn_by_path` (multi-word) defined in exactly one file → Strong.
    let evs = vec![
        begin("repo-index/src/churn.rs"),
        match_event(
            "repo-index/src/churn.rs",
            6,
            "pub fn git_churn_by_path(root: &Path, days: u32) -> HashMap<String, u32> {",
            8,
        ),
        end("repo-index/src/churn.rs"),
    ];
    assert_eq!(
        confidence("git_churn_by_path", evs),
        OwnerConfidence::Strong
    );
}

#[test]
fn confidence_never_strong_for_single_generic_word_even_if_exact() {
    // `fn config(...)` exists in some files. A single-word query "config"
    // matches widely; even a lone exact-symbol match must NOT render Strong
    // because the word is generic enough that the file is rarely the owner.
    let evs = vec![
        begin("util/src/cfg.rs"),
        match_event("util/src/cfg.rs", 12, "pub fn config() -> Config {", 8),
        end("util/src/cfg.rs"),
    ];
    let c = confidence("config", evs);
    assert_ne!(
        c,
        OwnerConfidence::Strong,
        "single generic word must never be Strong; got {c:?}"
    );
}

#[test]
fn confidence_strong_for_multi_word_dominant_concept_query() {
    // Run4 shape: multi-word concept query whose top owner clears the
    // absolute-score floor and dominates the second owner by margin.
    let evs = vec![
        begin("verification/src/rules.rs"),
        match_event(
            "verification/src/rules.rs",
            10,
            "fn package_name_for_area(area_id: &str) -> Option<String> {",
            4,
        ),
        match_event(
            "verification/src/rules.rs",
            20,
            "fn area_id_for_path(path: &str, map: &RepoMap) -> Option<String> {",
            4,
        ),
        match_event(
            "verification/src/rules.rs",
            30,
            "// Cargo package names for area ids (path -> `cargo test -p` name).",
            0,
        ),
        end("verification/src/rules.rs"),
    ];
    assert_eq!(
        confidence(
            "area id|area_id|cargo test -p|package name|lookup table",
            evs
        ),
        OwnerConfidence::Strong
    );
}

#[test]
fn confidence_never_strong_for_broad_alternation_of_generic_words() {
    // Track-D Slot-1 shape: an unscoped OR-search of single generic words
    // (`rollout|jsonl|resume|reopen|record`). Even when one file dominates the
    // hit count it must NOT be asserted as a Strong owner — the top file is
    // just whoever has the most incidental hits across the generic terms.
    // (Here `log_db.rs` wins on `Record`/`record` noise while the real target
    // lives elsewhere.) Regression guard for the confidently-wrong owner that
    // would otherwise derail the task.
    let evs = vec![
        begin("state/src/log_db.rs"),
        match_event("state/src/log_db.rs", 35, "use tracing::span::Record;", 18),
        match_event(
            "state/src/log_db.rs",
            37,
            "use tracing_subscriber::field::RecordFields;",
            30,
        ),
        match_event("state/src/log_db.rs", 153, "attrs.record(&mut visitor);", 6),
        end("state/src/log_db.rs"),
        begin("other/src/misc.rs"),
        match_event("other/src/misc.rs", 4, "// rollout note", 3),
        end("other/src/misc.rs"),
    ];
    let c = confidence("rollout|jsonl|resume|reopen|record", evs);
    assert_ne!(
        c,
        OwnerConfidence::Strong,
        "a broad alternation of single generic words must never be Strong; got {c:?}"
    );
}

#[test]
fn confidence_none_when_top_match_is_plain_source_not_owner() {
    // No definition keywords on the matched lines → best file classifies
    // as Source, not Owner → no confident owner can be named.
    let evs = vec![
        begin("some/src/uses.rs"),
        match_event(
            "some/src/uses.rs",
            10,
            "    let _ = thing_we_dont_define();",
            8,
        ),
        end("some/src/uses.rs"),
    ];
    assert_eq!(
        confidence("thing_we_dont_define", evs),
        OwnerConfidence::None
    );
}

#[test]
fn renderer_strong_says_likely_owner() {
    let evs = vec![
        begin("repo-index/src/churn.rs"),
        match_event(
            "repo-index/src/churn.rs",
            6,
            "pub fn git_churn_by_path(root: &Path) -> u32 {",
            8,
        ),
        end("repo-index/src/churn.rs"),
    ];
    let ProxyOutcome::Substitute { rendered, .. } = run("git_churn_by_path", evs) else {
        panic!("expected Substitute");
    };
    assert!(rendered.contains("# likely owner (defines the searched symbol):"));
}

#[test]
fn renderer_weak_is_explicitly_non_directive() {
    // A single-word generic-but-exact match yields Weak; the label must
    // tell the model NOT to trust the path as the owner.
    let evs = vec![
        begin("util/src/cfg.rs"),
        match_event("util/src/cfg.rs", 12, "pub fn config() -> Config {", 8),
        end("util/src/cfg.rs"),
    ];
    let ProxyOutcome::Substitute { rendered, .. } = run("config", evs) else {
        panic!("expected Substitute");
    };
    assert!(rendered.contains("# best-guess owner (LOW confidence"));
    assert!(rendered.contains("do NOT trust this path"));
}

#[test]
fn renderer_none_label_is_explicitly_non_directive() {
    // Best match is plain Source → no owner asserted; the label must
    // tell the model NOT to pick a single file from the list.
    let evs = vec![
        begin("some/src/uses.rs"),
        match_event("some/src/uses.rs", 10, "    let _ = thing();", 8),
        end("some/src/uses.rs"),
    ];
    let ProxyOutcome::Substitute { rendered, .. } = run("thing", evs) else {
        panic!("expected Substitute");
    };
    assert!(rendered.contains("# no high-confidence owner found"));
    assert!(rendered.contains("do NOT pick a single file"));
}
