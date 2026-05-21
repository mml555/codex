use std::collections::BTreeMap;
use std::collections::BTreeSet;

use codex_repo_index::RepoMap;

/// Normalized task terms used for path relevance scoring.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskTerms {
    pub phrases: Vec<String>,
    pub expanded: Vec<String>,
    pub likely_areas: Vec<String>,
}

const PENALTY_PATH_MARKERS: &[&str] = &[
    "__snapshots__",
    "examples",
    "fixtures",
    "generated",
    "legacy",
    "mock",
    "mocks",
    "node_modules",
    "snapshot",
    "snapshots",
    "testdata",
    "vendor",
];

const SYNONYMS: &[(&str, &[&str])] = &[
    ("eval", &["evaluation", "metrics", "benchmark"]),
    ("agent", &["assistant", "agent_eval"]),
    ("fixture", &["fixtures"]),
    ("harness", &["context", "packet", "assembler"]),
    ("score", &["scoring", "metric", "metrics"]),
    ("prompt", &["fragment", "contextual", "input"]),
    ("post", &["failure", "repair", "report"]),
    ("failure", &["post", "repair", "report"]),
    ("test", &["spec", "fixture", "golden"]),
    ("restaurant", &["restaurants"]),
    (
        "pagination",
        &["page", "limit", "offset", "cursor", "paginate"],
    ),
    ("search", &["query", "filter", "lookup"]),
    ("auth", &["authentication", "login", "middleware"]),
    ("middleware", &["handler", "layer"]),
    ("context", &["harness", "packet"]),
];

pub fn build_task_terms(task: &str, map: &RepoMap) -> TaskTerms {
    let mut terms = BTreeSet::new();
    for token in tokenize_raw(task) {
        for part in split_identifier(&token) {
            if part.len() >= 3 {
                terms.insert(part);
            }
        }
        if token.ends_with('s') && token.len() > 4 {
            terms.insert(token[..token.len() - 1].to_string());
        }
        terms.insert(token);
    }

    let phrases: Vec<String> = terms.iter().cloned().collect();
    let mut expanded = terms;
    for term in &phrases {
        expand_synonyms(term, &mut expanded);
    }

    let expanded: Vec<String> = expanded.into_iter().collect();
    let likely_areas = finalize_likely_areas(&phrases, infer_likely_areas(map, &expanded));
    TaskTerms {
        phrases,
        expanded,
        likely_areas,
    }
}

pub fn task_targets_crate(phrases: &[String], crate_name: &str) -> bool {
    crate_name
        .split('-')
        .all(|token| phrases.iter().any(|phrase| phrase == token))
}

fn finalize_likely_areas(phrases: &[String], mut areas: Vec<String>) -> Vec<String> {
    let has = |term: &str| phrases.iter().any(|p| p == term);
    if task_targets_crate(phrases, "context-harness") {
        areas.retain(|area| !area.starts_with("apply-patch"));
        prefer_area_front(&mut areas, "context-harness");
    }
    if has("extension") && (has("repo") || has("intelligence")) {
        prefer_area_front(&mut areas, "ext/repo-intelligence");
        // Extension wiring typically crosses app-server + core session.
        prefer_area(&mut areas, "app-server");
        prefer_area(&mut areas, "core");
    }
    if has("agent") && has("eval") && (has("score") || has("scoring") || has("metric")) {
        prefer_area_front(&mut areas, "context-harness");
        prefer_area(&mut areas, "cli");
    }
    if has("post") && has("failure") {
        prefer_area_front(&mut areas, "context-harness");
        if has("report") || has("verification") {
            prefer_area(&mut areas, "verification");
        }
    }
    areas.truncate(3);
    areas
}

fn prefer_area_front(areas: &mut Vec<String>, area: &str) {
    areas.retain(|current| current != area);
    areas.insert(0, area.to_string());
}

fn prefer_area(areas: &mut Vec<String>, area: &str) {
    if !areas.iter().any(|current| current == area) {
        areas.push(area.to_string());
    }
}

/// Repo-wide tokens that should not drive file selection on their own.
const LOW_SIGNAL_REPO_TERMS: &[&str] = &["codex", "openai", "rs"];

/// Task verbs that are too common to justify inclusion on a single match.
const WEAK_TASK_TERMS: &[&str] = &["add", "fix", "make", "new", "use", "with"];

pub fn path_matches_term(path: &str, term: &str) -> bool {
    let term_lower = term.to_ascii_lowercase();
    if LOW_SIGNAL_REPO_TERMS.contains(&term_lower.as_str()) {
        return false;
    }
    path_segment_tokens(path)
        .iter()
        .any(|segment| segments_equivalent(segment, &term_lower))
}

pub fn is_scoring_term(term: &str) -> bool {
    let term_lower = term.to_ascii_lowercase();
    !LOW_SIGNAL_REPO_TERMS.contains(&term_lower.as_str())
        && !WEAK_TASK_TERMS.contains(&term_lower.as_str())
}

fn segments_equivalent(segment: &str, term: &str) -> bool {
    if segment == term {
        return true;
    }
    if segment.len() + 1 == term.len()
        && term.ends_with('s')
        && term.strip_suffix('s') == Some(segment)
    {
        return true;
    }
    if term.len() + 1 == segment.len()
        && segment.ends_with('s')
        && segment.strip_suffix('s') == Some(term)
    {
        return true;
    }
    false
}

/// Count how many expanded terms match a path (used for multi-term boosts).
pub fn count_term_matches(path: &str, terms: &[String]) -> usize {
    terms
        .iter()
        .filter(|term| is_scoring_term(term) && path_matches_term(path, term))
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

pub fn is_penalty_path(path: &str, terms: &TaskTerms) -> bool {
    let path_lower = path.to_ascii_lowercase();
    if path_lower.ends_with(".md") && !terms.expanded.iter().any(|t| t == "doc" || t == "docs") {
        if !path_lower.contains("agents.md") {
            return true;
        }
    }
    PENALTY_PATH_MARKERS
        .iter()
        .any(|marker| path_lower.contains(marker))
}

pub fn task_mentions_path_marker(path: &str, terms: &TaskTerms) -> bool {
    let path_lower = path.to_ascii_lowercase();
    terms
        .expanded
        .iter()
        .any(|term| path_lower.contains(term) || path_lower.contains(&term.replace('_', "-")))
}

fn expand_synonyms(term: &str, out: &mut BTreeSet<String>) {
    for (key, values) in SYNONYMS {
        if term == *key {
            for value in *values {
                out.insert((*value).to_string());
            }
        }
        if values.iter().any(|value| term == *value) {
            out.insert((*key).to_string());
            for value in *values {
                out.insert((*value).to_string());
            }
        }
    }
}

fn infer_likely_areas(map: &RepoMap, terms: &[String]) -> Vec<String> {
    let mut scores: BTreeMap<String, f64> = BTreeMap::new();

    for area in &map.area_maps {
        let mut score = area.confidence;
        if terms
            .iter()
            .any(|term| area.area_id.contains(term) || term.contains(&area.area_id))
        {
            score += 0.45;
        }
        let area_tokens: Vec<String> = area
            .area_id
            .split(&['/', '-'][..])
            .filter(|t| t.len() >= 3)
            .map(str::to_string)
            .collect();
        if area_tokens
            .iter()
            .all(|token| terms.iter().any(|term| term == token))
        {
            score += 0.5;
        }
        let short_id = area
            .area_id
            .rsplit('/')
            .next()
            .unwrap_or(area.area_id.as_str());
        if terms.iter().any(|term| term == short_id) {
            score += 0.35;
        }
        for path in area.owned_files.iter().chain(area.related_cli_paths.iter()) {
            if count_term_matches(path, terms) >= 1 {
                score += 0.2;
            }
        }
        if score > 0.5 {
            scores.insert(area.area_id.clone(), score);
        }
    }

    for area in &map.areas {
        let mut score = area.confidence;
        for term in terms {
            if area.name.to_ascii_lowercase().contains(term) {
                score += 0.35;
            }
            for path in &area.paths {
                if path_matches_term(path, term) {
                    score += 0.25;
                }
            }
        }
        if score > 0.0 {
            scores.insert(area.name.clone(), score);
        }
    }

    for file in &map.files {
        let match_count = count_term_matches(&file.path, terms);
        for prefix in path_prefixes(&file.path, 3) {
            if !prefix_matches_terms(&prefix, terms) {
                continue;
            }
            let entry = scores.entry(prefix).or_default();
            *entry += match_count as f64 * 0.2;
            if match_count >= 2 {
                *entry += 0.45;
            }
        }
    }

    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    ranked.truncate(3);
    ranked.into_iter().map(|(prefix, _)| prefix).collect()
}

fn prefix_matches_terms(prefix: &str, terms: &[String]) -> bool {
    terms.iter().any(|term| path_matches_term(prefix, term))
}

fn path_prefixes(path: &str, max_depth: usize) -> Vec<String> {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Vec::new();
    }
    let depth = parts.len().min(max_depth);
    (1..=depth).map(|n| parts[..n].join("/")).collect()
}

fn path_segment_tokens(path: &str) -> Vec<String> {
    path.split(|c: char| !c.is_alphanumeric())
        .flat_map(|segment| {
            let lower = segment.to_ascii_lowercase();
            let mut tokens = vec![lower.clone()];
            tokens.extend(split_identifier(&lower));
            tokens
        })
        .filter(|t| t.len() >= 3)
        .collect()
}

fn tokenize_raw(task: &str) -> Vec<String> {
    task.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

fn split_identifier(value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    let mut parts = Vec::new();
    let mut start = 0usize;
    let chars: Vec<char> = value.chars().collect();
    for i in 1..chars.len() {
        let prev = chars[i - 1];
        let curr = chars[i];
        let boundary = (prev.is_lowercase() || prev.is_ascii_digit()) && curr.is_uppercase()
            || prev.is_uppercase()
                && curr.is_uppercase()
                && chars.get(i + 1).is_some_and(|n| n.is_lowercase());
        if boundary {
            parts.push(
                chars[start..i]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase(),
            );
            start = i;
        }
    }
    parts.push(
        chars[start..]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase(),
    );
    parts
        .into_iter()
        .flat_map(|part| {
            part.split(|c: char| !c.is_alphanumeric())
                .filter(|t| t.len() >= 3)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_harness_eval_synonyms() {
        let map = RepoMap {
            version: 2,
            repo_id: "t".to_string(),
            root: "/t".to_string(),
            files: Vec::new(),
            tests: Vec::new(),
            areas: Vec::new(),
            packages: Vec::new(),
            area_maps: Vec::new(),
            commands: Vec::new(),
            test_map: Vec::new(),
            agents_md: None,
            warnings: Vec::new(),
        };
        let terms = build_task_terms("improve context harness eval fixture metrics", &map);
        assert!(terms.expanded.iter().any(|t| t == "metrics"));
        assert!(terms.expanded.iter().any(|t| t == "fixtures"));
    }

    #[test]
    fn matches_kebab_path_segments() {
        assert!(path_matches_term("context-harness/src/eval.rs", "eval"));
        assert!(path_matches_term(
            "context-harness/tests/fixtures/tasks_codex_live.json",
            "harness"
        ));
    }

    #[test]
    fn short_terms_require_segment_boundaries() {
        assert!(!path_matches_term(
            "app-server-protocol/schema/json/DynamicToolCallParams.json",
            "context"
        ));
        assert!(path_matches_term("context-harness/src/eval.rs", "context"));
    }

    fn empty_repo_map() -> RepoMap {
        RepoMap {
            version: 2,
            repo_id: "t".to_string(),
            root: "/t".to_string(),
            files: Vec::new(),
            tests: Vec::new(),
            areas: Vec::new(),
            packages: Vec::new(),
            area_maps: Vec::new(),
            commands: Vec::new(),
            test_map: Vec::new(),
            agents_md: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn repo_intelligence_extension_prefers_cross_area_ids() {
        let terms = build_task_terms(
            "wire repo intelligence extension into session prompt assembly",
            &empty_repo_map(),
        );
        assert_eq!(
            terms.likely_areas.first().map(String::as_str),
            Some("ext/repo-intelligence")
        );
        assert!(terms.likely_areas.iter().any(|area| area == "app-server"));
    }

    #[test]
    fn agent_eval_scoring_prefers_harness_and_cli() {
        let terms = build_task_terms(
            "extend agent-eval scoring to support repo_intelligence session-injection arms",
            &empty_repo_map(),
        );
        assert_eq!(
            terms.likely_areas.first().map(String::as_str),
            Some("context-harness")
        );
        assert!(terms.likely_areas.iter().any(|area| area == "cli"));
    }

    #[test]
    fn post_failure_prompt_prefers_context_harness() {
        let terms = build_task_terms(
            "adjust post-failure prompt fragment formatting for verification reports",
            &empty_repo_map(),
        );
        assert_eq!(
            terms.likely_areas.first().map(String::as_str),
            Some("context-harness")
        );
    }
}
