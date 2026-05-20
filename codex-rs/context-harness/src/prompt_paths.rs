use std::collections::BTreeSet;

/// Extract repo-like paths embedded in vanilla prompt-input JSON.
pub fn extract_paths_from_prompt_json(json: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return paths;
    };
    collect_paths_from_value(&value, &mut paths);
    paths
}

/// Rough token estimate from serialized prompt input (chars / 4 heuristic).
pub fn estimate_tokens_from_prompt_json(json: &str) -> u32 {
    (json.len() as u32).saturating_div(4)
}

fn collect_paths_from_value(value: &serde_json::Value, paths: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::String(s) => {
            for token in s.split_whitespace() {
                if looks_like_repo_path(token) {
                    paths.insert(token.trim_matches('`').to_string());
                }
            }
            for line in s.lines() {
                if looks_like_repo_path(line.trim()) {
                    paths.insert(line.trim().trim_matches('`').to_string());
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_paths_from_value(item, paths);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, v) in map {
                collect_paths_from_value(v, paths);
            }
        }
        _ => {}
    }
}

fn looks_like_repo_path(token: &str) -> bool {
    token.contains('/')
        && !token.starts_with("http")
        && token.len() < 256
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
}
