use codex_protocol::ThreadId;
use codex_protocol::items::EnteredReviewModeItem;
use codex_protocol::items::ExitedReviewModeItem;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EnteredReviewModeEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ExitedReviewModeEvent;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ReviewTarget;
use codex_protocol::protocol::ThreadHistoryMode;
use pretty_assertions::assert_eq;

use super::measure_and_filter_rollout_items;
use crate::ResponseItemEnvelope;
use crate::RolloutItem;

fn retained_message(text: &str) -> RolloutItem {
    RolloutItem::ResponseItem(ResponseItemEnvelope::new(ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }))
}

#[test]
fn mixed_batch_applies_policy_and_drops_other_items() {
    let kept = retained_message("hello");
    let dropped = RolloutItem::ResponseItem(ResponseItemEnvelope::new(ResponseItem::Other));
    let items = vec![kept.clone(), dropped];

    let persisted = measure_and_filter_rollout_items(&items, ThreadHistoryMode::Legacy);

    assert_eq!(
        serde_json::to_value(persisted).expect("serialize persisted items"),
        serde_json::to_value([kept]).expect("serialize expected items")
    );
}

#[test]
fn retained_items_are_byte_identical() {
    let item = retained_message("a moderately sized payload");
    let persisted =
        measure_and_filter_rollout_items(std::slice::from_ref(&item), ThreadHistoryMode::Legacy);

    assert_eq!(
        serde_json::to_vec(&persisted[0]).expect("serialize persisted item"),
        serde_json::to_vec(&item).expect("serialize candidate item")
    );
}

#[test]
fn item_completion_persistence_depends_on_history_mode() {
    let item = RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
        thread_id: ThreadId::default(),
        turn_id: "turn".to_string(),
        item: TurnItem::UserMessage(UserMessageItem {
            id: "item".to_string(),
            client_id: None,
            content: Vec::new(),
        }),
        started_at_ms: Some(0),
        completed_at_ms: 0,
    }));

    let legacy_persisted =
        measure_and_filter_rollout_items(std::slice::from_ref(&item), ThreadHistoryMode::Legacy);
    assert!(legacy_persisted.is_empty());

    let paginated_persisted =
        measure_and_filter_rollout_items(std::slice::from_ref(&item), ThreadHistoryMode::Paginated);
    assert_eq!(
        serde_json::to_value(paginated_persisted).expect("serialize persisted items"),
        serde_json::to_value([item]).expect("serialize expected items")
    );
}

#[test]
fn review_mode_persistence_depends_on_history_mode() {
    let completed_items = vec![
        RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id: ThreadId::default(),
            turn_id: "turn".to_string(),
            item: TurnItem::EnteredReviewMode(EnteredReviewModeItem {
                id: "entered-review".to_string(),
                target: ReviewTarget::Custom {
                    instructions: "review this".to_string(),
                },
                user_facing_hint: "Review requested.".to_string(),
            }),
            started_at_ms: Some(0),
            completed_at_ms: 0,
        })),
        RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id: ThreadId::default(),
            turn_id: "turn".to_string(),
            item: TurnItem::ExitedReviewMode(ExitedReviewModeItem {
                id: "exited-review".to_string(),
                review_output: None,
            }),
            started_at_ms: Some(0),
            completed_at_ms: 0,
        })),
    ];
    let legacy_events = vec![
        RolloutItem::EventMsg(EventMsg::EnteredReviewMode(EnteredReviewModeEvent {
            target: ReviewTarget::Custom {
                instructions: "review this".to_string(),
            },
            user_facing_hint: Some("Review requested.".to_string()),
            turn_id: Some("turn".to_string()),
            item_id: Some("entered-review".to_string()),
        })),
        RolloutItem::EventMsg(EventMsg::ExitedReviewMode(ExitedReviewModeEvent {
            turn_id: Some("turn".to_string()),
            item_id: Some("exited-review".to_string()),
            review_output: None,
        })),
    ];
    let items = completed_items
        .iter()
        .chain(&legacy_events)
        .cloned()
        .collect::<Vec<_>>();

    let persisted_legacy = measure_and_filter_rollout_items(&items, ThreadHistoryMode::Legacy);
    assert_eq!(
        serde_json::to_value(persisted_legacy).expect("serialize persisted items"),
        serde_json::to_value(legacy_events).expect("serialize expected items")
    );

    let persisted_paginated =
        measure_and_filter_rollout_items(&items, ThreadHistoryMode::Paginated);
    assert_eq!(
        serde_json::to_value(persisted_paginated).expect("serialize persisted items"),
        serde_json::to_value(completed_items).expect("serialize expected items")
    );
}
