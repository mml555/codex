use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use sha2::Digest;
use sha2::Sha256;

use crate::area_map::build_area_maps;
use crate::cache::RepoIndexCache;
use crate::churn::git_churn_by_path;
use crate::command_map::build_command_maps;
use crate::repo_map::REPO_MAP_VERSION;
use crate::repo_map::RepoArea;
use crate::repo_map::RepoFileEntry;
use crate::repo_map::RepoMap;
use crate::repo_map::RepoPackage;
use crate::repo_map::RepoTestEntry;
use crate::signals::RepoSignals;
use crate::test_map::build_test_map;
use crate::test_map::enhance_test_entries;
use crate::test_map::link_tests_to_related_files;
use crate::walk::collect_repo_files;
use crate::walk::detect_area_name;
use crate::walk::detect_package_manifest;
use crate::walk::extract_import_targets;
use crate::walk::is_route_like_path;
use crate::walk::is_test_path;
use crate::walk::read_file_snippet;

const AGENTS_MD: &str = "AGENTS.md";
const AGENTS_OVERRIDE_MD: &str = "AGENTS.override.md";

#[derive(Debug, Clone, Default)]
pub struct RepoMapBuilderOptions {
    /// When false and `cache` is set, return a cached map when available.
    pub refresh: bool,
    /// Optional on-disk cache (typically under `~/.codex/repo-index`).
    pub cache: Option<RepoIndexCache>,
}

impl RepoMapBuilderOptions {
    pub fn with_cache(cache: RepoIndexCache) -> Self {
        Self {
            refresh: false,
            cache: Some(cache),
        }
    }
}

pub struct RepoMapBuilder;

impl RepoMapBuilder {
    pub fn build(root: &Path) -> Result<RepoMap> {
        Self::build_with_options(root, RepoMapBuilderOptions::default())
    }

    pub fn build_with_options(root: &Path, options: RepoMapBuilderOptions) -> Result<RepoMap> {
        let root = root
            .canonicalize()
            .with_context(|| format!("canonicalize {}", root.display()))?;
        let repo_id = compute_repo_id(&root)?;
        let root_str = root.to_string_lossy().into_owned();

        if !options.refresh {
            if let Some(cache) = &options.cache {
                if let Some(map) = cache.load(&repo_id)? {
                    if map.root == root_str {
                        return Ok(map);
                    }
                }
            }
        }

        let map = Self::build_fresh(&root, &root_str, repo_id)?;
        if let Some(cache) = options.cache {
            let _ = cache.store(&map);
        }
        Ok(map)
    }

    fn build_fresh(root: &Path, root_str: &str, repo_id: String) -> Result<RepoMap> {
        let paths = collect_repo_files(root)?;
        let churn = git_churn_by_path(root, 30);
        let agents_md = load_agents_md_ladder(root);

        let mut files = Vec::new();
        let mut tests = Vec::new();
        let mut packages = Vec::new();
        let mut area_paths: HashMap<String, Vec<String>> = HashMap::new();

        for path in &paths {
            if let Some(kind) = detect_package_manifest(path) {
                packages.push(RepoPackage {
                    path: path.clone(),
                    kind: kind.to_string(),
                    confidence: 0.95,
                });
            }

            let mut signals = RepoSignals::new(base_path_confidence(path));
            if is_test_path(path) {
                signals = signals.with_tag("test");
                signals.evidence.push("path:test_pattern".to_string());
                tests.push(RepoTestEntry {
                    path: path.clone(),
                    confidence: 0.85,
                    related_paths: Vec::new(),
                    reason: "path matches test file conventions".to_string(),
                });
            }
            if is_route_like_path(path) {
                signals = signals.with_tag("route").with_evidence("path:routes");
            }
            if let Some(area) = detect_area_name(path) {
                signals = signals.with_tag(area).with_evidence(format!("path:{area}"));
                area_paths
                    .entry(area.to_string())
                    .or_default()
                    .push(path.clone());
            }
            if let Some(count) = churn.get(path) {
                signals.git_churn_30d = Some(*count);
                signals.evidence.push(format!("git_churn:{count}"));
            }

            if let Some(snippet) = read_file_snippet(root, path, 80) {
                for import in extract_import_targets(&snippet) {
                    signals.evidence.push(format!("import:{import}"));
                }
            }

            signals.summary = Some(summarize_path(path));
            files.push(RepoFileEntry {
                path: path.clone(),
                signals,
            });
        }

        link_tests_to_related_files(&mut tests, &files);

        let areas = area_paths
            .into_iter()
            .map(|(name, paths)| RepoArea {
                name,
                confidence: 0.7,
                paths,
            })
            .collect();

        let area_maps = build_area_maps(&paths);
        let commands = build_command_maps(root, &area_maps);
        let test_map = build_test_map(root, &files, &tests);
        enhance_test_entries(&mut tests, &test_map);

        let mut map = RepoMap {
            version: REPO_MAP_VERSION,
            repo_id,
            root: root_str.to_string(),
            files,
            tests,
            areas,
            packages,
            area_maps,
            commands,
            test_map,
            agents_md,
            warnings: Vec::new(),
        };
        map.sort_for_determinism();
        Ok(map)
    }
}

fn compute_repo_id(root: &Path) -> Result<String> {
    let head = std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "rev-parse", "HEAD"])
        .output();
    let head_suffix = match head {
        Ok(output) if output.status.success() => {
            let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if hash.len() >= 12 {
                hash[..12].to_string()
            } else {
                hash
            }
        }
        _ => "no-git".to_string(),
    };
    let mut hasher = Sha256::new();
    hasher.update(root.to_string_lossy().as_bytes());
    hasher.update(head_suffix.as_bytes());
    Ok(format!("{:x}", hasher.finalize())[..16].to_string())
}

fn base_path_confidence(path: &str) -> f64 {
    if path.ends_with(".md") {
        0.55
    } else if path.contains("/target/") || path.contains("/node_modules/") {
        0.1
    } else {
        0.5
    }
}

fn summarize_path(path: &str) -> String {
    if is_test_path(path) {
        format!("Test file at {path}")
    } else if is_route_like_path(path) {
        format!("Route-like file at {path}")
    } else {
        format!("Source file at {path}")
    }
}

fn load_agents_md_ladder(root: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for name in [AGENTS_OVERRIDE_MD, AGENTS_MD] {
        let path = root.join(name);
        if path.is_file() {
            if let Ok(content) = std::fs::read_to_string(path) {
                parts.push(content);
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n---\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires git executable and writable temp directory"]
    fn builds_map_for_fixture_tree() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join("backend/routes")).unwrap();
        std::fs::write(
            root.join("backend/routes/restaurants.py"),
            "def search_restaurants():\n    pass\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tests/test_restaurants.py"),
            "def test_search():\n    pass\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init", "--allow-empty"])
            .current_dir(root)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();

        let map = RepoMapBuilder::build(root).expect("build map");
        assert!(!map.files.is_empty());
    }
}
