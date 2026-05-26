use serde::Deserialize;
use serde::Serialize;

use crate::signals::RepoSignals;

pub const REPO_MAP_VERSION: u32 = 2;

/// Structural repo map consumed by the context harness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoMap {
    pub version: u32,
    pub repo_id: String,
    pub root: String,
    pub files: Vec<RepoFileEntry>,
    pub tests: Vec<RepoTestEntry>,
    /// Legacy coarse areas (path tags); prefer [`RepoMap::area_maps`] when present.
    pub areas: Vec<RepoArea>,
    pub packages: Vec<RepoPackage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub area_maps: Vec<AreaMap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandMapEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_map: Vec<TestMapEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents_md: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl RepoMap {
    pub fn sort_for_determinism(&mut self) {
        self.files.sort_by(|a, b| a.path.cmp(&b.path));
        self.tests.sort_by(|a, b| a.path.cmp(&b.path));
        self.areas.sort_by(|a, b| a.name.cmp(&b.name));
        self.packages.sort_by(|a, b| a.path.cmp(&b.path));
        self.area_maps.sort_by(|a, b| a.area_id.cmp(&b.area_id));
        self.commands.sort_by(|a, b| a.command.cmp(&b.command));
        self.test_map
            .sort_by(|a, b| a.source_path.cmp(&b.source_path));
        self.warnings.sort();
        for file in &mut self.files {
            file.signals.tags.sort();
            file.signals.evidence.sort();
        }
        for area in &mut self.area_maps {
            area.root_paths.sort();
            area.owned_files.sort();
            area.test_paths.sort();
            area.related_cli_paths.sort();
            area.negative_paths.sort();
        }
        for command in &mut self.commands {
            command.related_files.sort();
        }
        for entry in &mut self.test_map {
            entry.test_paths.sort();
            entry.evidence.sort();
        }
    }

    pub fn area_map_for_id(&self, area_id: &str) -> Option<&AreaMap> {
        self.area_maps.iter().find(|area| area.area_id == area_id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoFileEntry {
    pub path: String,
    pub signals: RepoSignals,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoTestEntry {
    pub path: String,
    pub confidence: f64,
    pub related_paths: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoArea {
    pub name: String,
    pub paths: Vec<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoPackage {
    pub path: String,
    pub kind: String,
    pub confidence: f64,
}

/// First-class feature area with ownership and exclusion hints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreaMap {
    pub area_id: String,
    pub root_paths: Vec<String>,
    pub owned_files: Vec<String>,
    pub test_paths: Vec<String>,
    pub related_cli_paths: Vec<String>,
    pub negative_paths: Vec<String>,
    pub confidence: f64,
}

/// CLI command ownership (Codex-style monorepos).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandMapEntry {
    pub command: String,
    pub entrypoint: String,
    pub implementation_area: String,
    pub related_files: Vec<String>,
}

/// Source file to covering tests mapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestMapEntry {
    pub source_path: String,
    pub test_paths: Vec<String>,
    pub confidence: f64,
    pub evidence: Vec<String>,
}
