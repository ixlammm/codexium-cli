use codex_protocol::protocol::ThreadHistoryMode;

use crate::RolloutItem;
use crate::policy::is_persisted_rollout_item;

/// Applies the shared rollout persistence policy once and returns the items that should persist.
pub fn measure_and_filter_rollout_items(
    items: &[RolloutItem],
    history_mode: ThreadHistoryMode,
) -> Vec<RolloutItem> {
    let mut persisted = Vec::new();

    for item in items {
        let kept = is_persisted_rollout_item(item, history_mode);
        if kept {
            persisted.push(item.clone());
        }
    }

    persisted
}

#[cfg(test)]
#[path = "persistence_metrics_tests.rs"]
mod tests;
