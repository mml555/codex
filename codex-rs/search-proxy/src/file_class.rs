//! Lightweight per-file classification: does this match look like a
//! definition site (Owner), a test file (RelatedTest), or just a
//! regular source-file hit?
//!
//! The heuristics here are deliberately simple. No regex compilation,
//! no symbol-table parsing — just textual prefix checks against the
//! `rg`-matched line. The MVP's goal is to bias ranking so the model
//! is told about the likely owner first; precision matters less than
//! never lying about the same line being a definition when it isn't.

use crate::evidence::FileClass;
use crate::rg_json::ParsedFileHits;

/// Keyword prefixes that announce a Rust definition site when they
/// appear at the start of a (post-trim) matched line, possibly prefixed
/// with `pub `, `pub(crate) `, `pub(super) `, `unsafe `, `async `, or
/// `default `. The classifier looks at the line text only — it does
/// not try to verify the searched query equals the defined symbol,
/// because the query itself may be a regex alternation across symbols
/// (the Run 8 shape) where any individual match is enough evidence.
const RUST_DEFINITION_KEYWORDS: &[&str] = &[
    "fn ",
    "enum ",
    "struct ",
    "trait ",
    "type ",
    "union ",
    "mod ",
    "const ",
    "static ",
    "impl ",
    "macro_rules!",
];

/// Substrings that mark a test PATH (anywhere in the path).
const TEST_PATH_SUBSTRINGS: &[&str] = &["/tests/", "tests/", "/test/"];

/// Suffixes the BASENAME must end with to be a test file.
const TEST_FILENAME_SUFFIXES: &[&str] = &[
    "_test.rs",
    "_tests.rs",
    "_test.py",
    "_tests.py",
    ".test.ts",
    ".test.tsx",
    ".test.js",
    ".test.jsx",
    ".spec.ts",
    ".spec.tsx",
    ".spec.js",
];

/// Pytest prefix convention: `test_x.py`. Deliberately scoped to
/// `.py` only — Rust source files are routinely named `test_*.rs`
/// (e.g. `test_map.rs`, which builds the repo test map and is NOT a
/// test file). Rust test files are caught by the `_test.rs`/`_tests.rs`
/// suffixes and `tests/` path rules instead.
const PYTEST_FILENAME_PREFIX: &str = "test_";

/// Classify a single file given its `rg`-reported path and matched
/// lines.
///
/// Test-path detection runs FIRST: a `*_tests.rs` / `tests/` file is
/// never the "owner" of a searched symbol, even though it is full of
/// `fn test_*` definitions. Letting test files reach Owner surfaced
/// `*_tests.rs` as the top result for plain concept queries (e.g.
/// "truncate" → `utils/output-truncation/src/truncate_tests.rs`).
pub fn classify_file(path: &str, hits: &ParsedFileHits) -> FileClass {
    if path_looks_like_test(path) {
        return FileClass::RelatedTest;
    }
    if hits
        .hits
        .iter()
        .any(|h| line_looks_like_definition(&h.line_text))
    {
        return FileClass::Owner;
    }
    FileClass::Source
}

/// Returns `true` if the line, after leading whitespace, starts with
/// a recognized Rust definition keyword (possibly preceded by `pub`,
/// `unsafe`, `async`, `default`, or `pub(crate)`).
fn line_looks_like_definition(line: &str) -> bool {
    definition_symbol(line).is_some()
}

/// If the line is a Rust definition, return the defined identifier
/// (the token right after the keyword, e.g. `package_name_for_area`
/// from `fn package_name_for_area(...)`). Returns `None` for
/// non-definition lines. `macro_rules!` returns the macro name.
pub(crate) fn definition_symbol(line: &str) -> Option<String> {
    definition_site(line).map(|(_, ident)| ident)
}

/// Like [`definition_symbol`] but also returns the matched definition
/// keyword (e.g. `"fn "`, `"const "`). Lets the ranker weight a
/// `fn`/`struct`/`enum` definition above an incidental `const`/`static`
/// match that merely contains a query word in its name.
pub(crate) fn definition_site(line: &str) -> Option<(&'static str, String)> {
    let trimmed = line.trim_start();
    // Strip up to three visibility / qualifier modifiers in order,
    // e.g. "pub unsafe fn foo()" or "pub(crate) async fn bar()".
    let mut rest = trimmed;
    for _ in 0..3 {
        let next = strip_modifier(rest);
        if next == rest {
            break;
        }
        rest = next.trim_start();
    }

    let kw = RUST_DEFINITION_KEYWORDS
        .iter()
        .find(|kw| rest.starts_with(**kw))?;
    let after_kw = rest[kw.len()..].trim_start();
    // The identifier runs until the first non-identifier char
    // (`(`, `<`, `{`, `:`, whitespace, `=`, `;`).
    let ident: String = after_kw
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if ident.is_empty() {
        return None;
    }
    Some((kw, ident))
}

/// Strip a single qualifier token: `pub`, `pub(...)`, `unsafe`,
/// `async`, or `default`. Returns the input unchanged if no
/// recognized qualifier is found at the start.
fn strip_modifier(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("pub(")
        && let Some(close) = rest.find(')')
    {
        return &rest[close + 1..];
    }
    for kw in ["pub ", "unsafe ", "async ", "default "] {
        if let Some(rest) = s.strip_prefix(kw) {
            return rest;
        }
    }
    s
}

fn path_looks_like_test(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if TEST_PATH_SUBSTRINGS.iter().any(|n| lower.contains(n)) {
        return true;
    }
    let basename = lower
        .rsplit_once('/')
        .map(|(_, b)| b)
        .unwrap_or(lower.as_str());
    if TEST_FILENAME_SUFFIXES.iter().any(|s| basename.ends_with(s)) {
        return true;
    }
    // Pytest `test_*.py` only — never Rust `test_*.rs` source files.
    if basename.ends_with(".py") && basename.starts_with(PYTEST_FILENAME_PREFIX) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rg_json::ParsedHit;
    use pretty_assertions::assert_eq;

    fn hits(lines: &[(u32, &str)]) -> ParsedFileHits {
        ParsedFileHits {
            path: "x.rs".to_string(),
            hits: lines
                .iter()
                .map(|(n, t)| ParsedHit {
                    line: *n,
                    column: None,
                    line_text: t.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn owner_pub_enum() {
        let p = hits(&[(42, "pub enum AgentEvalResult {")]);
        assert_eq!(
            classify_file("context-harness/src/agent_eval.rs", &p),
            FileClass::Owner
        );
    }

    #[test]
    fn owner_plain_fn() {
        let p = hits(&[(
            155,
            "fn classify_result(score: ScorePair) -> AgentEvalResult {",
        )]);
        assert_eq!(
            classify_file("context-harness/src/agent_eval.rs", &p),
            FileClass::Owner
        );
    }

    #[test]
    fn owner_pub_crate_with_inner_paren() {
        let p = hits(&[(10, "pub(crate) fn classify_result()")]);
        assert_eq!(classify_file("a.rs", &p), FileClass::Owner);
    }

    #[test]
    fn owner_with_leading_whitespace() {
        let p = hits(&[(10, "    pub fn classify_result()")]);
        assert_eq!(classify_file("a.rs", &p), FileClass::Owner);
    }

    #[test]
    fn owner_with_unsafe_async_qualifiers() {
        let p = hits(&[(10, "pub unsafe async fn foo()")]);
        assert_eq!(classify_file("a.rs", &p), FileClass::Owner);
    }

    #[test]
    fn related_test_path_under_tests_dir() {
        let p = hits(&[(
            88,
            "    assert!(matches!(x, AgentEvalResult::Excluded { .. }));",
        )]);
        assert_eq!(
            classify_file("context-harness/tests/agent_eval.rs", &p),
            FileClass::RelatedTest
        );
    }

    #[test]
    fn related_test_path_with_test_suffix() {
        let p = hits(&[(10, "    let _ = AgentEvalResult::Excluded;")]);
        assert_eq!(classify_file("foo/bar_test.rs", &p), FileClass::RelatedTest);
    }

    #[test]
    fn rust_test_prefixed_source_is_owner_not_test() {
        // `test_map.rs` is a Rust SOURCE file (builds the repo test map),
        // not a test file. The pytest `test_` prefix must not catch it.
        let p = hits(&[(129, "pub fn link_tests_to_related_files(tests: &mut [T]) {")]);
        assert_eq!(
            classify_file("repo-index/src/test_map.rs", &p),
            FileClass::Owner
        );
    }

    #[test]
    fn related_test_python_test_file() {
        let p = hits(&[(10, "    self.assertEqual(x, ExpectedResult)")]);
        assert_eq!(
            classify_file("foo/test_something.py", &p),
            FileClass::RelatedTest
        );
        assert_eq!(
            classify_file("foo/something_test.py", &p),
            FileClass::RelatedTest
        );
    }

    #[test]
    fn plain_source_match_is_source() {
        let p = hits(&[(99, "    let res = AgentEvalResult::Comparable { .. };")]);
        assert_eq!(
            classify_file("context-harness/src/renderer.rs", &p),
            FileClass::Source
        );
    }

    #[test]
    fn test_path_wins_over_definition() {
        // Policy: a test file is never the Owner of a searched symbol,
        // even when it contains `fn` definitions (test fns / helpers).
        // Test-path detection runs before definition detection so
        // `*_tests.rs` files cannot top the ranking for a concept query.
        let p = hits(&[(5, "pub fn helper_for_test() {}")]);
        assert_eq!(
            classify_file("foo/tests/helpers.rs", &p),
            FileClass::RelatedTest
        );
        let p2 = hits(&[(10, "fn truncate_middle_keeps_head() {}")]);
        assert_eq!(
            classify_file("utils/output-truncation/src/truncate_tests.rs", &p2),
            FileClass::RelatedTest
        );
    }

    #[test]
    fn comment_starting_with_fn_is_not_owner() {
        let p = hits(&[(10, "// fn foo is defined elsewhere")]);
        assert_eq!(classify_file("a.rs", &p), FileClass::Source);
    }

    #[test]
    fn use_statement_is_not_owner() {
        // `use` is not in the definition keyword set even though it
        // looks structurally similar.
        let p = hits(&[(10, "use crate::AgentEvalResult;")]);
        assert_eq!(classify_file("a.rs", &p), FileClass::Source);
    }
}
