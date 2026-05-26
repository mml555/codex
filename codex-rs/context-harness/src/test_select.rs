use std::path::Path;

use codex_repo_index::RepoMap;

use crate::ownership::ResolvedOwnership;
use crate::packet::SelectedTest;
use crate::selection::SelectionCaps;
use crate::task_terms::TaskTerms;
use crate::task_terms::path_matches_term;
use crate::task_terms::task_targets_crate;

const FIXTURE_TASK_TERMS: &[&str] = &["fixture", "fixtures", "eval", "golden", "metrics"];

const HARNESS_EVAL_TEST_PATHS: &[&str] = &[
    "context-harness/tests/eval_codex_fixtures.rs",
    "context-harness/tests/eval_fixtures.rs",
];

pub fn select_tests_for_task(
    map: &RepoMap,
    terms: &TaskTerms,
    ownership: &ResolvedOwnership,
    candidate_sources: &[String],
    caps: SelectionCaps,
) -> Vec<SelectedTest> {
    let wants_fixture_tests = task_wants_fixture_tests(terms);
    let mut scored: Vec<SelectedTest> = map
        .tests
        .iter()
        .filter(|test| test.confidence >= caps.drop_confidence_below)
        .filter_map(|test| {
            let score = score_test(
                test,
                map,
                terms,
                ownership,
                candidate_sources,
                wants_fixture_tests,
            );
            if score < caps.include_relevance_min * 0.9 {
                return None;
            }
            Some(SelectedTest {
                path: test.path.clone(),
                command: default_test_command(&test.path),
                reason: test.reason.clone(),
                confidence: score.clamp(0.0, 1.0),
            })
        })
        .collect();

    if let Some(area_id) = &ownership.primary_area {
        scored.retain(|test| test.path.starts_with(area_id));
        if let Some(area) = map.area_map_for_id(area_id) {
            for test_path in &area.test_paths {
                if scored.iter().any(|test| &test.path == test_path) {
                    continue;
                }
                let score = score_area_listed_test(test_path, terms, wants_fixture_tests);
                if score >= caps.include_relevance_min * 0.85 {
                    scored.push(SelectedTest {
                        path: test_path.clone(),
                        command: default_test_command(test_path),
                        reason: "Listed in area test_paths".to_string(),
                        confidence: score,
                    });
                }
            }
        }
    }

    if task_targets_crate(&terms.phrases, "context-harness") && task_wants_fixture_tests(terms) {
        for path in HARNESS_EVAL_TEST_PATHS {
            if scored.iter().any(|test| test.path == *path) {
                continue;
            }
            scored.push(SelectedTest {
                path: path.to_string(),
                command: default_test_command(path),
                reason: "Harness eval fixture test".to_string(),
                confidence: 0.9,
            });
        }
    }

    scored.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    scored.truncate(caps.max_tests);
    scored
}

fn score_area_listed_test(path: &str, terms: &TaskTerms, wants_fixture_tests: bool) -> f64 {
    let mut score: f64 = 0.7;
    if test_path_aligns_eval_fixture(path, terms) {
        score = 0.85;
    }
    for term in &terms.phrases {
        if path_matches_term(path, term) {
            score += 0.1;
        }
    }
    if path.contains("/fixtures/") && !wants_fixture_tests {
        score -= 0.35;
    }
    score.clamp(0.0, 1.0)
}

fn score_test(
    test: &codex_repo_index::RepoTestEntry,
    map: &RepoMap,
    terms: &TaskTerms,
    ownership: &ResolvedOwnership,
    candidate_sources: &[String],
    wants_fixture_tests: bool,
) -> f64 {
    let path_lower = test.path.to_ascii_lowercase();
    let mut score = test.confidence * 0.55;
    let mut strong_signal = false;

    if is_snapshot_only_test(&path_lower) {
        return 0.1;
    }
    if path_lower.contains("/fixtures/") && !wants_fixture_tests {
        score -= 0.4;
    } else if wants_fixture_tests && path_lower.contains("fixture") {
        score += 0.25;
        strong_signal = true;
    }

    for term in &terms.phrases {
        if path_matches_term(&test.path, term) {
            score += 0.22;
            strong_signal = true;
        }
    }

    if test_path_aligns_eval_fixture(&test.path, terms) {
        score += 0.35;
        strong_signal = true;
    }

    for source in candidate_sources {
        if test_references_source(&test.path, source, map) {
            score += 0.4;
            strong_signal = true;
        }
    }

    for entry in &map.test_map {
        if !entry.test_paths.contains(&test.path) {
            continue;
        }
        if entry.confidence >= 0.65 {
            score += entry.confidence * 0.25;
            strong_signal = true;
        }
        if candidate_sources.contains(&entry.source_path) {
            score += 0.3;
            strong_signal = true;
        }
    }

    if let Some(area_id) = &ownership.primary_area {
        if test.path.starts_with(area_id) {
            let term_overlap = terms
                .phrases
                .iter()
                .any(|term| path_matches_term(&test.path, term));
            if term_overlap {
                score += 0.2;
                strong_signal = true;
            } else if !strong_signal {
                score -= 0.2;
            }
            if let Some(area) = map.area_map_for_id(area_id) {
                if area.test_paths.iter().any(|tp| tp == &test.path) {
                    score += if term_overlap { 0.35 } else { 0.25 };
                    strong_signal = true;
                }
            }
        } else if !strong_signal {
            return 0.05;
        }
    }

    if path_lower.contains("schema_fixtures")
        && !terms
            .phrases
            .iter()
            .any(|p| p == "schema" || p == "protocol")
    {
        return 0.05;
    }

    if task_targets_crate(&terms.phrases, "context-harness")
        && !test.path.starts_with("context-harness/")
    {
        score -= 0.35;
    }

    for related in &test.related_paths {
        if terms
            .expanded
            .iter()
            .any(|term| path_matches_term(related, term))
        {
            score += 0.15;
            strong_signal = true;
        }
    }

    if test_path_aligns_eval_fixture(&test.path, terms)
        || HARNESS_EVAL_TEST_PATHS.contains(&test.path.as_str())
    {
        score = score.max(0.72);
        strong_signal = true;
    }

    if !strong_signal && score < 0.5 {
        score *= 0.7;
    }
    if strong_signal {
        score.max(0.55)
    } else {
        score
    }
}

fn task_wants_fixture_tests(terms: &TaskTerms) -> bool {
    FIXTURE_TASK_TERMS
        .iter()
        .any(|term| terms.phrases.iter().any(|p| p == *term))
}

fn is_snapshot_only_test(path_lower: &str) -> bool {
    path_lower.contains("__snapshots__") || path_lower.ends_with(".snap")
}

fn test_path_aligns_eval_fixture(path: &str, terms: &TaskTerms) -> bool {
    let path_lower = path.to_ascii_lowercase();
    let wants_eval = terms.phrases.iter().any(|p| p == "eval");
    let wants_fixture = terms
        .phrases
        .iter()
        .any(|p| p == "fixture" || p == "fixtures");
    if !(wants_eval && wants_fixture) {
        return false;
    }
    path_lower.contains("eval") && path_lower.contains("fixture")
}

fn test_references_source(test_path: &str, source_path: &str, map: &RepoMap) -> bool {
    let stem = Path::new(source_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if stem.is_empty() {
        return false;
    }
    if test_path.contains(stem) {
        return true;
    }
    map.test_map.iter().any(|entry| {
        entry.source_path == source_path && entry.test_paths.contains(&test_path.to_string())
    })
}

fn default_test_command(path: &str) -> String {
    if path.ends_with(".py") {
        format!("pytest {path}")
    } else if path.ends_with(".rs") {
        format!("cargo test --test {}", path)
    } else if path.ends_with(".ts") || path.ends_with(".tsx") || path.ends_with(".js") {
        format!("npm test -- {path}")
    } else {
        format!("# run tests for {path}")
    }
}
