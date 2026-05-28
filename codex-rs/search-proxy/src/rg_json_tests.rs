use pretty_assertions::assert_eq;

use super::rg_json::ParsedHit;
use super::rg_json::parse_rg_json;

/// Build an `rg --json` line for a `begin` event.
fn begin(path: &str) -> String {
    format!(r#"{{"type":"begin","data":{{"path":{{"text":{path:?}}}}}}}"#)
}

/// Build an `rg --json` line for a `match` event.
fn match_event(path: &str, line_number: u32, line_text: &str, sub_start: u64) -> String {
    format!(
        r#"{{"type":"match","data":{{"path":{{"text":{path:?}}},"lines":{{"text":{lines:?}}},"line_number":{line_number},"absolute_offset":0,"submatches":[{{"match":{{"text":""}},"start":{start},"end":{end}}}]}}}}"#,
        path = path,
        lines = line_text,
        line_number = line_number,
        start = sub_start,
        end = sub_start + 1
    )
}

fn end(path: &str) -> String {
    format!(
        r#"{{"type":"end","data":{{"path":{{"text":{path:?}}}, "binary_offset":null,"stats":{{}}}}}}"#
    )
}

fn summary() -> String {
    r#"{"type":"summary","data":{}}"#.to_string()
}

#[test]
fn parses_single_file_single_match() {
    let bytes = [
        begin("context-harness/src/agent_eval.rs"),
        match_event(
            "context-harness/src/agent_eval.rs",
            42,
            "pub enum AgentEvalResult {",
            4,
        ),
        end("context-harness/src/agent_eval.rs"),
        summary(),
    ]
    .join("\n");
    let parsed = parse_rg_json(bytes.as_bytes());
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].path, "context-harness/src/agent_eval.rs");
    assert_eq!(
        parsed[0].hits,
        vec![ParsedHit {
            line: 42,
            column: Some(5), // start=4 → column = 4+1
            line_text: "pub enum AgentEvalResult {".to_string(),
        }]
    );
}

#[test]
fn parses_multiple_files_in_order() {
    let bytes = [
        begin("a.rs"),
        match_event("a.rs", 10, "fn foo() {}", 3),
        end("a.rs"),
        begin("b.rs"),
        match_event("b.rs", 20, "fn bar() {}", 3),
        match_event("b.rs", 30, "fn baz() {}", 3),
        end("b.rs"),
        summary(),
    ]
    .join("\n");
    let parsed = parse_rg_json(bytes.as_bytes());
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].path, "a.rs");
    assert_eq!(parsed[1].path, "b.rs");
    assert_eq!(parsed[1].hits.len(), 2);
    assert_eq!(parsed[1].hits[0].line, 20);
    assert_eq!(parsed[1].hits[1].line, 30);
}

#[test]
fn match_without_begin_uses_match_path() {
    // Some `rg` modes may emit match events without a preceding begin.
    // Make sure the parser still attributes them correctly.
    let bytes = [match_event("standalone.rs", 1, "line text", 0), summary()].join("\n");
    let parsed = parse_rg_json(bytes.as_bytes());
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].path, "standalone.rs");
}

#[test]
fn ignores_garbage_lines_between_valid_events() {
    let bytes = [
        "not valid json".to_string(),
        begin("a.rs"),
        "{ truncated".to_string(),
        match_event("a.rs", 1, "hit", 0),
        "".to_string(),
        end("a.rs"),
    ]
    .join("\n");
    let parsed = parse_rg_json(bytes.as_bytes());
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].hits.len(), 1);
}

#[test]
fn empty_input_yields_empty_result() {
    assert!(parse_rg_json(b"").is_empty());
    assert!(parse_rg_json(b"\n\n").is_empty());
}

#[test]
fn match_without_submatch_has_no_column() {
    let bytes = r#"{"type":"match","data":{"path":{"text":"a.rs"},"lines":{"text":"line"},"line_number":1,"absolute_offset":0,"submatches":[]}}"#.to_string();
    let parsed = parse_rg_json(bytes.as_bytes());
    assert_eq!(parsed[0].hits[0].column, None);
}
