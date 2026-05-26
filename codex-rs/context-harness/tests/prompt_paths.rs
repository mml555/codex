use codex_context_harness::extract_paths_from_prompt_json;

#[test]
fn extracts_paths_from_prompt_input_json() {
    let json = r#"[
      {"type":"message","role":"user","content":[{"type":"input_text","text":"See backend/routes/restaurants.py for details"}]}
    ]"#;
    let paths = extract_paths_from_prompt_json(json);
    assert!(paths.contains("backend/routes/restaurants.py"));
}

#[test]
fn extracts_paths_from_prompt_context_punctuation() {
    let json = r#"[
      {"type":"message","role":"user","content":[{"type":"input_text","text":"<cwd>/tmp/work/repo</cwd> Review `core/src/prompt_debug.rs`, then cli/src/context_cmd.rs."}]}
    ]"#;
    let paths = extract_paths_from_prompt_json(json);
    assert!(paths.contains("/tmp/work/repo"));
    assert!(paths.contains("core/src/prompt_debug.rs"));
    assert!(paths.contains("cli/src/context_cmd.rs"));
}
