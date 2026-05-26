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

use crate::file_class::definition_symbol;
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
    for branch in query.split('|') {
        let cleaned = branch.trim().trim_matches(|c| c == '^' || c == '$');
        let phrase = cleaned.to_ascii_lowercase();
        if !phrase.is_empty() {
            phrases.push(phrase.clone());
        }
        for word in tokenize_words(&phrase) {
            words.insert(word);
        }
    }
    QueryTerms { phrases, words }
}

/// Split an arbitrary string into lowercase `[a-z0-9]+` word tokens.
fn tokenize_words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
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

/// Weight for a query-word that appears in a *defined symbol's* name.
/// Defining `package_name_for_area` when the query is about "package
/// name" / "area" is the strongest ownership signal we have.
const DEFINITION_OVERLAP_WEIGHT: u32 = 3;

/// Score a file's matches against the query. Higher is more relevant.
///
/// Two additive signals:
/// 1. Definition-symbol overlap (weighted): for each matched line that
///    is a definition, how many distinct query words appear in the
///    defined symbol's tokenized name. Summed across definition lines.
/// 2. Distinct phrase coverage: how many distinct query phrases appear
///    (case-insensitive substring) anywhere in the file's matched
///    lines. Rewards files that hit more branches of an alternation.
pub fn relevance_score(hits: &ParsedFileHits, terms: &QueryTerms) -> u32 {
    let mut definition_overlap: u32 = 0;
    for hit in &hits.hits {
        if let Some(symbol) = definition_symbol(&hit.line_text) {
            let symbol_words: BTreeSet<String> = tokenize_identifier(&symbol).into_iter().collect();
            let overlap = symbol_words
                .iter()
                .filter(|w| terms.words.contains(*w))
                .count();
            definition_overlap += overlap as u32;
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

    DEFINITION_OVERLAP_WEIGHT * definition_overlap + phrases_present
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
}
