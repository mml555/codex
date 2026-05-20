use std::collections::BTreeMap;
use std::path::Path;

use crate::repo_map::RepoFileEntry;
use crate::repo_map::RepoTestEntry;
use crate::repo_map::TestMapEntry;
use crate::walk::is_test_path;
use crate::walk::read_file_snippet;

pub fn build_test_map(
    root: &Path,
    files: &[RepoFileEntry],
    tests: &[RepoTestEntry],
) -> Vec<TestMapEntry> {
    let source_paths: Vec<&str> = files
        .iter()
        .filter(|file| !is_test_path(&file.path))
        .map(|file| file.path.as_str())
        .collect();

    let mut by_source: BTreeMap<String, TestMapEntry> = BTreeMap::new();

    for test in tests {
        for source in &test.related_paths {
            let entry = by_source
                .entry(source.clone())
                .or_insert_with(|| TestMapEntry {
                    source_path: source.clone(),
                    test_paths: Vec::new(),
                    confidence: 0.5,
                    evidence: Vec::new(),
                });
            if !entry.test_paths.contains(&test.path) {
                entry.test_paths.push(test.path.clone());
            }
            entry.confidence = entry.confidence.max(test.confidence.min(0.95));
            if !entry.evidence.contains(&test.reason) {
                entry.evidence.push(test.reason.clone());
            }
        }
    }

    for source in source_paths {
        let entry = by_source
            .entry(source.to_string())
            .or_insert_with(|| TestMapEntry {
                source_path: source.to_string(),
                test_paths: Vec::new(),
                confidence: 0.45,
                evidence: Vec::new(),
            });

        for test in tests {
            if test.path == source {
                continue;
            }
            let score = test_pair_score(root, source, &test.path);
            if score >= 0.55 {
                if !entry.test_paths.contains(&test.path) {
                    entry.test_paths.push(test.path.clone());
                }
                entry.confidence = entry.confidence.max(score);
                let evidence = format!("paired:{score:.2}");
                if !entry.evidence.contains(&evidence) {
                    entry.evidence.push(evidence);
                }
            }
        }
    }

    let mut map: Vec<TestMapEntry> = by_source.into_values().collect();
    map.retain(|entry| !entry.test_paths.is_empty());
    map.sort_by(|a, b| a.source_path.cmp(&b.source_path));
    map
}

fn test_pair_score(root: &Path, source: &str, test_path: &str) -> f64 {
    let mut score: f64 = 0.45;
    let source_crate = source.split('/').next().unwrap_or("");
    let test_crate = test_path.split('/').next().unwrap_or("");
    if !source_crate.is_empty() && source_crate == test_crate {
        score += 0.2;
    }

    let source_stem = Path::new(source)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let test_name = Path::new(test_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    if !source_stem.is_empty() {
        if test_name.contains(source_stem) || test_path.contains(source_stem) {
            score += 0.2;
        }
        if let Some(snippet) = read_file_snippet(root, test_path, 120) {
            let module = source_stem.replace('_', "::");
            if snippet.contains(source_stem) || snippet.contains(&module) {
                score += 0.15;
                score = score.min(0.95);
            }
        }
    }

    if test_path.contains("/tests/") && source.contains("/src/") {
        score += 0.05;
    }

    score.min(0.95)
}

pub fn enhance_test_entries(tests: &mut [RepoTestEntry], test_map: &[TestMapEntry]) {
    for test in tests {
        for entry in test_map {
            if entry.test_paths.contains(&test.path) {
                if !test.related_paths.contains(&entry.source_path) {
                    test.related_paths.push(entry.source_path.clone());
                }
                test.confidence = test.confidence.max(entry.confidence * 0.9);
            }
        }
        test.related_paths.sort();
        test.related_paths.dedup();
    }
}

pub fn link_tests_to_related_files(tests: &mut [RepoTestEntry], files: &[RepoFileEntry]) {
    use std::collections::HashSet;

    let file_paths: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    for test in tests {
        let stem = Path::new(&test.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let normalized = stem.trim_start_matches("test_");
        for path in &file_paths {
            if path.contains(normalized) && !is_test_path(path) {
                test.related_paths.push((*path).to_string());
            }
        }
        test.related_paths.sort();
        test.related_paths.dedup();
    }
}
