use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;

use crate::repo_map::RepoMap;

#[derive(Debug, Clone)]
pub struct RepoIndexCache {
    codex_home: PathBuf,
}

impl RepoIndexCache {
    pub fn new(codex_home: impl Into<PathBuf>) -> Self {
        Self {
            codex_home: codex_home.into(),
        }
    }

    pub fn cache_path(&self, repo_id: &str) -> PathBuf {
        self.codex_home
            .join("repo-index")
            .join(repo_id)
            .join("repo_map.json")
    }

    pub fn load(&self, repo_id: &str) -> Result<Option<RepoMap>> {
        let path = self.cache_path(repo_id);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let map: RepoMap = serde_json::from_slice(&bytes).context("parse cached RepoMap")?;
        Ok(Some(map))
    }

    pub fn store(&self, map: &RepoMap) -> Result<()> {
        let path = self.cache_path(&map.repo_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create cache dir {}", parent.display()))?;
        }
        let mut normalized = map.clone();
        normalized.sort_for_determinism();
        let json = serde_json::to_vec_pretty(&normalized).context("serialize RepoMap")?;
        fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }
}
