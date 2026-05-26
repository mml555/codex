use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ContextDecisionLog {
    pub included: Vec<DecisionEntry>,
    pub dropped: Vec<DecisionEntry>,
    pub budget_exhausted: Vec<DecisionEntry>,
    pub low_confidence: Vec<DecisionEntry>,
}

impl ContextDecisionLog {
    pub fn empty() -> Self {
        Self::default()
    }
}
