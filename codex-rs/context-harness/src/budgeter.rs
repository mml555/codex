use crate::assembler::AssembledContext;
use crate::decision_log::DecisionEntry;
use crate::packet::ContextItem;
use crate::packet::ContextItemState;
use crate::render_level::estimate_item_render_tokens;

#[derive(Debug, Clone, Copy)]
pub struct TokenBudget {
    pub limit: u32,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self { limit: 12_000 }
    }
}

pub struct BudgetResult {
    pub items: Vec<ContextItem>,
    pub used_estimate: u32,
    pub budget_exhausted: Vec<DecisionEntry>,
    pub included_log: Vec<DecisionEntry>,
}

pub struct ContextBudgeter;

impl ContextBudgeter {
    pub fn apply(assembled: AssembledContext, budget: TokenBudget) -> BudgetResult {
        let mut used_estimate = 0u32;
        let mut items = Vec::new();
        let mut budget_exhausted = Vec::new();
        let mut included_log = Vec::new();

        for mut item in assembled.candidates {
            let cost = estimate_item_tokens(&item);
            if used_estimate.saturating_add(cost) > budget.limit {
                budget_exhausted.push(DecisionEntry {
                    id: item.id.clone(),
                    path: item.path.clone(),
                    reason: "Excluded because token budget was exhausted".to_string(),
                    evidence: item.evidence.clone(),
                    relevance: Some(item.relevance),
                    confidence: Some(item.confidence),
                });
                item.state = ContextItemState::Dropped;
                continue;
            }
            used_estimate = used_estimate.saturating_add(cost);
            item.state = ContextItemState::Included;
            included_log.push(DecisionEntry {
                id: item.id.clone(),
                path: item.path.clone(),
                reason: item.reason.clone(),
                evidence: item.evidence.clone(),
                relevance: Some(item.relevance),
                confidence: Some(item.confidence),
            });
            items.push(item);
        }

        BudgetResult {
            items,
            used_estimate,
            budget_exhausted,
            included_log,
        }
    }
}

fn estimate_item_tokens(item: &ContextItem) -> u32 {
    let selection_cost: u32 = match item.kind {
        crate::packet::ContextItemKind::RepoRule => 120,
        crate::packet::ContextItemKind::FileSnippet => 180,
        crate::packet::ContextItemKind::FileSummary => 90,
        _ => 80,
    };
    selection_cost
        .saturating_add(estimate_item_render_tokens(item))
        .saturating_add(item.path.as_ref().map(|p| p.len() as u32 / 8).unwrap_or(0))
}
