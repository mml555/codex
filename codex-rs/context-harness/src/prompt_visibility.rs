use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;

use crate::renderer::HARNESS_MARKER;

/// Returns true when formatted model prompt input includes the harness fragment.
pub fn model_prompt_contains_harness_context(items: &[ResponseItem]) -> bool {
    items.iter().any(|item| match item {
        ResponseItem::Message { content, .. } => content.iter().any(|part| match part {
            ContentItem::InputText { text } => text.contains(HARNESS_MARKER),
            _ => false,
        }),
        _ => false,
    })
}

/// Collect user-role message texts from formatted prompt input.
pub fn user_message_texts(items: &[ResponseItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            ResponseItem::Message { role, content, .. } if role == "user" => {
                let texts = content
                    .iter()
                    .filter_map(|part| match part {
                        ContentItem::InputText { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                (!texts.is_empty()).then_some(texts.join("\n"))
            }
            _ => None,
        })
        .collect()
}
