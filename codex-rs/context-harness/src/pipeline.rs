use codex_repo_index::RepoMap;

use crate::assembler::ContextAssembler;
use crate::budgeter::ContextBudgeter;
use crate::budgeter::TokenBudget;
use crate::classifier::TaskClassifier;
use crate::decision_log::ContextDecisionLog;
use crate::normalize::normalize_packet;
use crate::ownership::resolve_ownership;
use crate::packet::CONTEXT_PACKET_VERSION;
use crate::packet::ContextPacket;
use crate::packet::ContextStage;
use crate::packet::TaskInfo;
use crate::packet::TokenBudgetInfo;
use crate::render_level::assign_render_levels;
use crate::render_level::estimate_prompt_tokens;
use crate::renderer::ContextPacketRenderer;
use crate::run_memory::RunMemory;
use crate::selection::SelectionCaps;
use crate::task_terms::build_task_terms;

#[derive(Debug, Clone)]
pub struct BuildPacketOptions {
    pub stage: ContextStage,
    pub token_budget: TokenBudget,
    pub selection: SelectionCaps,
}

impl Default for BuildPacketOptions {
    fn default() -> Self {
        Self {
            stage: ContextStage::Preflight,
            token_budget: TokenBudget::default(),
            selection: SelectionCaps::default(),
        }
    }
}

pub fn build_context_packet(
    task: &str,
    map: &RepoMap,
    run_memory: &RunMemory,
    options: BuildPacketOptions,
) -> ContextPacket {
    let classified = TaskClassifier::classify(task);
    let terms = build_task_terms(task, map);
    let ownership = resolve_ownership(task, map, &terms);
    let assembled = match options.stage {
        ContextStage::Preflight | _ => ContextAssembler::assemble_preflight(
            task,
            &classified,
            map,
            run_memory,
            options.selection,
        ),
    };

    let selected_tests = assembled.selected_tests;
    let mut warnings = assembled.warnings;
    if let Some(area) = assembled.likely_area
        && !warnings.iter().any(|w| w.starts_with("Likely area:"))
    {
        warnings.insert(0, format!("Likely area: {area}"));
    }
    let dropped = assembled.dropped;
    let low_confidence = assembled.low_confidence;
    let mut budgeted = ContextBudgeter::apply(
        crate::assembler::AssembledContext {
            candidates: assembled.candidates,
            selected_tests: Vec::new(),
            warnings: Vec::new(),
            dropped: Vec::new(),
            low_confidence: Vec::new(),
            likely_area: None,
        },
        options.token_budget,
    );

    assign_render_levels(&mut budgeted.items, &ownership, map, options.selection);

    let decision_log = ContextDecisionLog {
        included: budgeted.included_log,
        dropped,
        budget_exhausted: budgeted.budget_exhausted,
        low_confidence,
    };

    let mut packet = ContextPacket {
        version: CONTEXT_PACKET_VERSION,
        stage: options.stage,
        task: TaskInfo {
            raw: task.to_string(),
            task_type: classified.task_type.as_str().to_string(),
            confidence: classified.confidence,
        },
        items: budgeted.items,
        decision_log,
        selected_tests,
        warnings: warnings.clone(),
        token_budget: TokenBudgetInfo {
            limit: options.token_budget.limit,
            used_estimate: 0,
        },
    };

    normalize_packet(&mut packet);
    let fragment =
        ContextPacketRenderer::render_prompt_fragment_with_caps(&packet, options.selection);
    let prompt_warnings = packet
        .warnings
        .iter()
        .filter(|w| !w.starts_with("Likely area:") && !w.starts_with("Matched CLI command:"))
        .count();
    packet.token_budget.used_estimate =
        estimate_prompt_tokens(&fragment, packet.selected_tests.len(), prompt_warnings);
    packet
}
