use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::repo_map::AreaMap;
use crate::walk::is_test_path;

/// CLI entrypoints that implement commands for a workspace crate area.
const AREA_CLI_BRIDGES: &[(&str, &[&str])] = &[
    ("context-harness", &["cli/src/context_cmd.rs"]),
    (
        "repo-index",
        &["cli/src/context_cmd.rs", "repo-index/src/builder.rs"],
    ),
    ("ext/repo-intelligence", &["app-server/src/extensions.rs"]),
];

pub fn build_area_maps(paths: &[String]) -> Vec<AreaMap> {
    let crate_roots = discover_crate_roots(paths);
    if crate_roots.is_empty() {
        return Vec::new();
    }

    let all_roots: BTreeSet<String> = crate_roots.keys().cloned().collect();
    let mut owned_by_area: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut tests_by_area: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for path in paths {
        let Some(root_path) = longest_crate_root(path, &crate_roots) else {
            continue;
        };
        if is_test_path(path) {
            tests_by_area
                .entry(root_path.to_string())
                .or_default()
                .push(path.clone());
        } else if !is_manifest_file(path) {
            owned_by_area
                .entry(root_path.to_string())
                .or_default()
                .push(path.clone());
        }
    }

    let mut area_maps = Vec::new();
    for (root_path, area_id) in &crate_roots {
        let owned_files = owned_by_area.remove(root_path).unwrap_or_default();
        let test_paths = tests_by_area.remove(root_path).unwrap_or_default();
        if owned_files.is_empty() && test_paths.is_empty() {
            continue;
        }

        let prefix = format!("{root_path}/");
        let related_cli_paths = AREA_CLI_BRIDGES
            .iter()
            .find(|(id, _)| *id == area_id)
            .map(|(_, bridges)| bridges.iter().map(|p| (*p).to_string()).collect())
            .unwrap_or_default();

        let negative_paths: Vec<String> = all_roots
            .iter()
            .filter(|root| *root != root_path)
            .map(|root| format!("{root}/"))
            .collect();

        let confidence = if owned_files.len() >= 3 { 0.85 } else { 0.65 };

        area_maps.push(AreaMap {
            area_id: area_id.clone(),
            root_paths: vec![prefix],
            owned_files,
            test_paths,
            related_cli_paths,
            negative_paths,
            confidence,
        });
    }

    area_maps.sort_by(|a, b| a.area_id.cmp(&b.area_id));
    area_maps
}

fn discover_crate_roots(paths: &[String]) -> BTreeMap<String, String> {
    let mut roots = BTreeMap::new();
    for path in paths {
        let Some(root_path) = path.strip_suffix("/Cargo.toml") else {
            continue;
        };
        if root_path.is_empty() {
            continue;
        }
        let area_id = root_path.to_string();
        roots.insert(root_path.to_string(), area_id);
    }
    roots
}

fn longest_crate_root<'a>(path: &str, roots: &'a BTreeMap<String, String>) -> Option<&'a str> {
    roots
        .keys()
        .filter(|root| path == root.as_str() || path.starts_with(&format!("{root}/")))
        .max_by_key(|root| root.len())
        .map(|root| root.as_str())
}

fn is_manifest_file(path: &str) -> bool {
    path.ends_with("Cargo.toml") || path.ends_with("BUILD.bazel") || path.ends_with("package.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_crate_roots_use_manifest_directory() {
        let paths = vec![
            "ext/repo-intelligence/Cargo.toml".to_string(),
            "ext/repo-intelligence/src/extension.rs".to_string(),
            "context-harness/Cargo.toml".to_string(),
            "context-harness/src/eval.rs".to_string(),
        ];
        let maps = build_area_maps(&paths);
        assert!(maps.iter().any(|a| a.area_id == "ext/repo-intelligence"));
        let ext = maps
            .iter()
            .find(|a| a.area_id == "ext/repo-intelligence")
            .unwrap();
        assert!(
            ext.owned_files
                .contains(&"ext/repo-intelligence/src/extension.rs".to_string())
        );
    }
}
