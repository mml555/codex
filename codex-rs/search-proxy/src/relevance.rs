//! Within-tier relevance scoring for ranked files.
//!
//! Background: the file classifier ([`crate::file_class`]) is binary —
//! Owner / RelatedTest / Source. When several files all classify as
//! Owner (each has at least one definition-shaped matched line), the
//! original ranker broke ties alphabetically by path, which is
//! meaningless for relevance. On the run3 generalization A/B that put
//! `context-harness/src/task_terms.rs` above the gold
//! `verification/src/rules.rs` purely because "c" < "v".
//!
//! This module computes a relevance score used as the secondary sort
//! key WITHIN a class, so the file whose definitions and matches align
//! most strongly with the query wins. Class rank still dominates
//! (Owner before Source before Test).

use std::collections::BTreeSet;

use crate::file_class::definition_site;
use crate::rg_json::ParsedFileHits;

/// Query decomposed into matchable pieces. `rg` patterns are often
/// alternations (`a|b|c`); each branch is a phrase, and each phrase is
/// further split into lowercase word tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryTerms {
    /// Alternation branches, trimmed + lowercased (e.g. "package name").
    pub phrases: Vec<String>,
    /// Distinct lowercased word tokens across all phrases
    /// (e.g. {package, name, area, id, cargo, test, ...}).
    pub words: BTreeSet<String>,
    /// The largest per-branch word count. Distinguishes a precise concept
    /// (`package name for area` → 4; `cargo test -p` → 3) from a broad
    /// OR-search of single generic tokens (`rollout|jsonl|resume` → 1).
    /// Used by the owner-confidence gate: a Strong owner on the score/margin
    /// path requires at least one multi-word branch, so an alternation of
    /// generic single words is never confidently owned.
    pub max_phrase_words: usize,
}

/// Decompose an `rg` query string into phrases + word tokens.
///
/// Splits on the regex alternation `|`, strips a few common regex
/// adornments (anchors, escapes, char-class brackets), then tokenizes
/// each branch into `[a-z0-9]+` words. Regex metacharacters that
/// survive are harmless — they just won't match any identifier word.
pub fn parse_query_terms(query: &str) -> QueryTerms {
    let mut phrases = Vec::new();
    let mut words = BTreeSet::new();
    let mut max_phrase_words = 0usize;
    for branch in query.split('|') {
        let cleaned = branch.trim().trim_matches(|c| c == '^' || c == '$');
        let phrase = cleaned.to_ascii_lowercase();
        if !phrase.is_empty() {
            phrases.push(phrase);
        }
        // Tokenize the CASE-PRESERVED branch so CamelCase boundaries
        // survive (tokenize_words lowercases per-token internally).
        let branch_words = tokenize_words(cleaned);
        max_phrase_words = max_phrase_words.max(branch_words.len());
        for word in branch_words {
            words.insert(word);
        }
    }
    QueryTerms {
        phrases,
        words,
        max_phrase_words,
    }
}

/// Split an arbitrary string into lowercase word tokens, splitting on
/// non-alphanumerics AND CamelCase boundaries. Models often search for
/// a type name directly (`AgentEvalResult`, `RepoSignals`); splitting
/// CamelCase lets those query words line up with the tokenized defined
/// symbol (`{agent,eval,result}`), which is the strongest owner signal.
fn tokenize_words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .flat_map(tokenize_identifier)
        .collect()
}

/// Split a Rust identifier into lowercase word tokens, handling both
/// `snake_case` and `CamelCase` (e.g. `package_name_for_area` ->
/// [package, name, for, area]; `AgentEvalResult` -> [agent, eval, result]).
fn tokenize_identifier(ident: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for ch in ident.chars() {
        if ch == '_' || ch == '-' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower && !current.is_empty() {
            // camelCase boundary: flush before the uppercase letter.
            words.push(std::mem::take(&mut current));
        }
        current.extend(ch.to_lowercase());
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Weight per query-word that appears in a *strong* defined symbol's
/// name (`fn`/`struct`/`enum`/`trait`/`type`/`union`/`macro_rules!`).
/// Defining `package_name_for_area` when the query is about "package
/// name" / "area" is the strongest ownership signal we have.
const STRONG_DEF_WEIGHT: u32 = 3;

/// Weight per query-word that appears in a *weak* defined symbol's name
/// (`const`/`static`/`mod`/`impl`). These are usually incidental: a
/// `const REMOTE_CONTROL_RECONNECT_BACKOFF_CAP` mentions "backoff" but
/// the *owner* of the `backoff` concept is `fn backoff`, not the file
/// that happens to define a constant naming it.
const WEAK_DEF_WEIGHT: u32 = 1;

/// Bonus when a strong definition's symbol matches the query *as a
/// whole* (tokenized symbol set == query word set), e.g. `fn backoff`
/// for query "backoff", or `fn truncate_middle` for "truncate_middle".
/// A whole-symbol match is much stronger ownership than an incidental
/// token overlap buried in a longer name, so it decisively breaks ties
/// that would otherwise fall back to alphabetical path order.
const EXACT_SYMBOL_BONUS: u32 = 5;

/// Definition keywords treated as strong ownership signals. Everything
/// else recognized by the classifier (`const`/`static`/`mod`/`impl`) is
/// weak.
fn is_strong_definition(keyword: &str) -> bool {
    matches!(
        keyword,
        "fn " | "struct " | "enum " | "trait " | "type " | "union " | "macro_rules!"
    )
}

/// Score a file's matches against the query. Higher is more relevant.
///
/// Additive signals:
/// 1. Definition-symbol overlap, weighted by definition strength: for
///    each matched definition line, how many distinct query words
///    appear in the defined symbol's tokenized name, times the
///    strong/weak weight. Summed across definition lines.
/// 2. Exact whole-symbol bonus when a strong definition's symbol is
///    exactly the query (set-equal on word tokens).
/// 3. Distinct phrase coverage: how many distinct query phrases appear
///    (case-insensitive substring) anywhere in the file's matched
///    lines. Rewards files that hit more branches of an alternation.
pub fn relevance_score(hits: &ParsedFileHits, terms: &QueryTerms) -> u32 {
    let mut score: u32 = 0;
    for hit in &hits.hits {
        if let Some((keyword, symbol)) = definition_site(&hit.line_text) {
            let symbol_words: BTreeSet<String> = tokenize_identifier(&symbol).into_iter().collect();
            let overlap = symbol_words
                .iter()
                .filter(|w| terms.words.contains(*w))
                .count() as u32;
            if overlap == 0 {
                continue;
            }
            let strong = is_strong_definition(keyword);
            let weight = if strong {
                STRONG_DEF_WEIGHT
            } else {
                WEAK_DEF_WEIGHT
            };
            score += weight * overlap;
            // Exact-whole-symbol bonus only for MULTI-word queries: a
            // single short word ("compaction", "value", "cache") that
            // happens to be a whole symbol somewhere is not distinctive
            // enough to promote that file over the conceptual owner.
            if strong && terms.words.len() >= 2 && symbol_words == terms.words {
                score += EXACT_SYMBOL_BONUS;
            }
        }
    }

    let mut phrases_present: u32 = 0;
    for phrase in &terms.phrases {
        if hits
            .hits
            .iter()
            .any(|h| h.line_text.to_ascii_lowercase().contains(phrase))
        {
            phrases_present += 1;
        }
    }

    score + phrases_present
}

/// True iff some strong definition in this file defines a symbol whose
/// word tokens are EXACTLY the query's word set (e.g. `fn backoff` for
/// "backoff", `fn git_churn_by_path` for "git_churn_by_path"). This is
/// the strongest single ownership signal — used by the confidence
/// scorer to mark a result "high confidence" regardless of margin.
pub(crate) fn has_exact_symbol_owner(hits: &ParsedFileHits, terms: &QueryTerms) -> bool {
    // Multi-word only: a single-word "exact" match (e.g. a `fn value`)
    // is not a confident ownership signal.
    if terms.words.len() < 2 {
        return false;
    }
    for hit in &hits.hits {
        if let Some((keyword, symbol)) = definition_site(&hit.line_text)
            && is_strong_definition(keyword)
        {
            let symbol_words: BTreeSet<String> = tokenize_identifier(&symbol).into_iter().collect();
            if symbol_words == terms.words {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rg_json::ParsedHit;
    use pretty_assertions::assert_eq;

    fn file(path: &str, lines: &[&str]) -> ParsedFileHits {
        ParsedFileHits {
            path: path.to_string(),
            hits: lines
                .iter()
                .enumerate()
                .map(|(i, t)| ParsedHit {
                    line: i as u32 + 1,
                    column: None,
                    line_text: t.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn tokenize_identifier_handles_snake_and_camel() {
        assert_eq!(
            tokenize_identifier("package_name_for_area"),
            vec!["package", "name", "for", "area"]
        );
        assert_eq!(
            tokenize_identifier("AgentEvalResult"),
            vec!["agent", "eval", "result"]
        );
        assert_eq!(tokenize_identifier("area_id"), vec!["area", "id"]);
    }

    #[test]
    fn parse_query_terms_splits_alternation_and_words() {
        let q = parse_query_terms("area id|area_id|cargo test -p|package name");
        assert!(q.phrases.contains(&"area id".to_string()));
        assert!(q.phrases.contains(&"cargo test -p".to_string()));
        assert!(q.words.contains("area"));
        assert!(q.words.contains("id"));
        assert!(q.words.contains("package"));
        assert!(q.words.contains("cargo"));
    }

    #[test]
    fn run3_gold_file_outscores_incidental_owner() {
        // Reproduces the run3 ranking miss with the real query.
        let terms = parse_query_terms(
            "area id|area_id|cargo test -p|targeted cargo|package name|lookup table",
        );

        // Gold: verification/src/rules.rs — defines the on-concept
        // helpers. Two strong definition-overlap lines.
        let rules = file(
            "verification/src/rules.rs",
            &[
                "/// Cargo package names for codex-rs area roots (path -> `cargo test -p` name).",
                "fn package_name_for_area(area_id: &str) -> Option<String> {",
                "fn area_id_for_path(path: &str, map: &RepoMap) -> Option<String> {",
            ],
        );
        // Incidental Owner: task_terms.rs — matches area_id as field
        // accesses + one unrelated definition.
        let task_terms = file(
            "context-harness/src/task_terms.rs",
            &[
                "/// `area_id` appearing only inside a quoted example does NOT",
                ".any(|term| area.area_id.contains(term) || term.contains(&area.area_id))",
                "pub fn build_task_terms(task: &str, map: &RepoMap) -> TaskTerms {",
            ],
        );

        let rules_score = relevance_score(&rules, &terms);
        let task_terms_score = relevance_score(&task_terms, &terms);
        assert!(
            rules_score > task_terms_score,
            "gold rules.rs ({rules_score}) must outscore task_terms.rs ({task_terms_score})"
        );
    }

    #[test]
    fn definition_overlap_dominates_incidental_phrase_hits() {
        let terms = parse_query_terms("package name|area");
        // File A defines an on-concept symbol.
        let a = file("a.rs", &["fn package_name_for_area() {}"]);
        // File B merely mentions the phrases in non-definition lines.
        let b = file(
            "b.rs",
            &[
                "// the package name is resolved elsewhere",
                "let area = compute_area();",
            ],
        );
        assert!(relevance_score(&a, &terms) > relevance_score(&b, &terms));
    }

    #[test]
    fn no_matches_scores_zero() {
        let terms = parse_query_terms("totally unrelated query");
        let f = file("x.rs", &["fn something_else() {}"]);
        assert_eq!(relevance_score(&f, &terms), 0);
    }

    #[test]
    fn strong_fn_def_outranks_incidental_const() {
        // The `backoff` regression: `fn backoff` is the owner; a file
        // that only defines `const ...BACKOFF_CAP` and calls backoff()
        // must not outrank it.
        let terms = parse_query_terms("backoff");
        let owner = file(
            "core/src/util.rs",
            &["pub fn backoff(attempt: u64) -> Duration {"],
        );
        let incidental = file(
            "app-server-transport/src/websocket.rs",
            &[
                "const REMOTE_CONTROL_RECONNECT_BACKOFF_CAP: Duration = Duration::from_secs(30);",
                "let reconnect_delay = backoff(*reconnect_attempt);",
            ],
        );
        assert!(
            relevance_score(&owner, &terms) > relevance_score(&incidental, &terms),
            "fn backoff ({}) must outrank const ...BACKOFF_CAP ({})",
            relevance_score(&owner, &terms),
            relevance_score(&incidental, &terms),
        );
    }

    #[test]
    fn exact_whole_symbol_match_beats_more_partial_defs() {
        // A multi-word whole-symbol definition (`fn git_churn_by_path`)
        // should beat a file with MORE partial-overlap defs but no exact
        // match. (Multi-word only — single short words get no bonus.)
        let terms = parse_query_terms("git_churn_by_path");
        let exact = file(
            "repo-index/src/churn.rs",
            &["pub fn git_churn_by_path(root: &Path) -> u32 {"],
        );
        let partial = file(
            "other.rs",
            &[
                "pub fn compute_churn_rate() {}",
                "pub fn git_churn_helper() {}",
            ],
        );
        assert!(
            relevance_score(&exact, &terms) > relevance_score(&partial, &terms),
            "exact git_churn_by_path ({}) must beat partial churn defs ({})",
            relevance_score(&exact, &terms),
            relevance_score(&partial, &terms),
        );
    }
}
