use pretty_assertions::assert_eq;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use super::TurnProfile;
use super::TurnProfilePhase;
use super::TurnProfileState;
use super::TurnTimingState;

#[tokio::test]
async fn turn_timing_state_records_turn_started_epoch_millis() {
    let state = TurnTimingState::default();
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_millis();

    let started_at_unix_ms = state.mark_turn_started(Instant::now()).await;

    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_millis();
    assert!(u128::try_from(started_at_unix_ms).is_ok_and(|ms| before <= ms && ms <= after));
    assert_eq!(
        state.started_at_unix_secs().await,
        Some(started_at_unix_ms / 1000)
    );
}

#[tokio::test]
async fn turn_timing_state_tracks_concurrent_items_and_preserves_first_start() {
    let state = TurnTimingState::default();

    state
        .record_item_started("first".to_string(), /*started_at_ms*/ 100)
        .await;
    state
        .record_item_started("second".to_string(), /*started_at_ms*/ 200)
        .await;
    assert_eq!(
        state
            .record_item_started("first".to_string(), /*started_at_ms*/ 300)
            .await,
        100
    );

    assert_eq!(state.take_item_started("second").await, Some(200));
    assert_eq!(state.take_item_started("first").await, Some(100));
}

#[tokio::test]
async fn turn_timing_state_preserves_in_flight_items_after_turn_completion() {
    let state = TurnTimingState::default();
    state
        .record_item_started("first".to_string(), /*started_at_ms*/ 100)
        .await;

    state.mark_turn_started(Instant::now()).await;
    assert_eq!(state.take_item_started("first").await, None);

    state
        .record_item_started("second".to_string(), /*started_at_ms*/ 200)
        .await;
    let _ = state.complete_profile_and_duration_ms().await;
    assert_eq!(state.take_item_started("second").await, Some(200));
}

#[test]
fn turn_profile_breaks_down_sampling_blocking_and_retry_overhead() {
    let started_at = Instant::now();
    let mut state = TurnProfileState::default();
    state.start(started_at);

    let _ = state.begin_sampling(started_at + Duration::from_millis(100));
    state.end_phase(
        started_at + Duration::from_millis(600),
        TurnProfilePhase::Sampling,
    );
    let _ = state.begin_tool_blocking(started_at + Duration::from_millis(600));
    state.end_phase(
        started_at + Duration::from_millis(900),
        TurnProfilePhase::ToolBlocking,
    );
    state.record_sampling_retry();
    let _ = state.begin_sampling(started_at + Duration::from_millis(1_000));
    state.end_phase(
        started_at + Duration::from_millis(1_200),
        TurnProfilePhase::Sampling,
    );

    assert_eq!(
        state.complete(started_at + Duration::from_millis(1_300)),
        TurnProfile {
            before_first_sampling_ms: 100,
            sampling_ms: 700,
            compaction_ms: 0,
            between_sampling_overhead_ms: 100,
            tool_blocking_ms: 300,
            after_last_sampling_ms: 100,
            sampling_request_count: 2,
            sampling_retry_count: 1,
        }
    );
}

#[test]
fn turn_profile_counts_compaction_as_an_exclusive_phase() {
    let started_at = Instant::now();
    let mut state = TurnProfileState::default();
    state.start(started_at);

    let _ = state.begin_compaction(started_at + Duration::from_millis(100));
    state.end_phase(
        started_at + Duration::from_millis(300),
        TurnProfilePhase::Compaction,
    );
    let _ = state.begin_sampling(started_at + Duration::from_millis(400));
    state.end_phase(
        started_at + Duration::from_millis(700),
        TurnProfilePhase::Sampling,
    );
    let _ = state.begin_compaction(started_at + Duration::from_millis(800));

    assert_eq!(
        state.complete(started_at + Duration::from_millis(1_100)),
        TurnProfile {
            before_first_sampling_ms: 200,
            sampling_ms: 300,
            compaction_ms: 500,
            between_sampling_overhead_ms: 0,
            tool_blocking_ms: 0,
            after_last_sampling_ms: 100,
            sampling_request_count: 1,
            sampling_retry_count: 0,
        }
    );
}

#[tokio::test]
async fn turn_profile_and_duration_share_a_completion_instant() {
    let state = TurnTimingState::default();
    state
        .mark_turn_started(Instant::now() - Duration::from_millis(100))
        .await;

    let (_, duration_ms, profile) = state.complete_profile_and_duration_ms().await;
    let classified_ms = profile.before_first_sampling_ms
        + profile.sampling_ms
        + profile.compaction_ms
        + profile.between_sampling_overhead_ms
        + profile.tool_blocking_ms
        + profile.after_last_sampling_ms;

    assert_eq!(
        duration_ms,
        Some(i64::try_from(classified_ms).expect("profile duration should fit i64"))
    );
}
