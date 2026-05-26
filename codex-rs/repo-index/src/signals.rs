use serde::Deserialize;
use serde::Serialize;

/// Heuristic signals attached to a repo file entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoSignals {
    pub confidence: f64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_churn_30d: Option<u32>,
}

impl RepoSignals {
    pub fn new(confidence: f64) -> Self {
        Self {
            confidence,
            tags: Vec::new(),
            summary: None,
            evidence: Vec::new(),
            git_churn_30d: None,
        }
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence.push(evidence.into());
        self
    }
}
