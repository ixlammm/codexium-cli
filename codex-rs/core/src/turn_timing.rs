use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use tokio::sync::Mutex;

/// Classified wall-clock time spent in each phase of a turn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TurnProfile {
    pub(crate) before_first_sampling_ms: u64,
    pub(crate) sampling_ms: u64,
    pub(crate) compaction_ms: u64,
    pub(crate) between_sampling_overhead_ms: u64,
    pub(crate) tool_blocking_ms: u64,
    pub(crate) after_last_sampling_ms: u64,
    pub(crate) sampling_request_count: u32,
    pub(crate) sampling_retry_count: u32,
}

#[derive(Debug, Default)]
pub(crate) struct TurnTimingState {
    state: Mutex<TurnTimingStateInner>,
    profile: StdMutex<TurnProfileState>,
}

#[derive(Debug, Default)]
struct TurnTimingStateInner {
    started_at: Option<Instant>,
    started_at_unix_secs: Option<i64>,
    item_started_at_ms: HashMap<String, i64>,
}

#[derive(Debug, Default)]
struct TurnProfileState {
    started_at: Option<Instant>,
    last_transition_at: Option<Instant>,
    active_phase: Option<TurnProfilePhase>,
    seen_sampling: bool,
    before_first_sampling: Duration,
    sampling: Duration,
    compaction: Duration,
    between_sampling_overhead: Duration,
    tool_blocking: Duration,
    pending_idle_after_sampling: Duration,
    sampling_request_count: u32,
    sampling_retry_count: u32,
    completed_profile: Option<TurnProfile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TurnProfilePhase {
    Sampling,
    Compaction,
    ToolBlocking,
}

#[must_use]
pub(crate) struct TurnProfileTimingGuard {
    timing: Arc<TurnTimingState>,
    phase: TurnProfilePhase,
    active: bool,
}

impl TurnTimingState {
    pub(crate) async fn mark_turn_started(&self, started_at: Instant) -> i64 {
        let started_at_unix_ms = now_unix_timestamp_ms();
        let mut state = self.state.lock().await;
        state.started_at = Some(started_at);
        state.started_at_unix_secs = Some(started_at_unix_ms / 1000);
        state.item_started_at_ms.clear();
        self.profile_state().start(started_at);
        started_at_unix_ms
    }

    pub(crate) async fn started_at_unix_secs(&self) -> Option<i64> {
        self.state.lock().await.started_at_unix_secs
    }

    pub(crate) async fn record_item_started(&self, item_id: String, started_at_ms: i64) -> i64 {
        *self
            .state
            .lock()
            .await
            .item_started_at_ms
            .entry(item_id)
            .or_insert(started_at_ms)
    }

    pub(crate) async fn take_item_started(&self, item_id: &str) -> Option<i64> {
        self.state.lock().await.item_started_at_ms.remove(item_id)
    }

    pub(crate) async fn complete_profile_and_duration_ms(
        &self,
    ) -> (Option<i64>, Option<i64>, TurnProfile) {
        let completed_at_instant = Instant::now();
        let state = self.state.lock().await;
        let completed_at = Some(now_unix_timestamp_secs());
        let duration_ms = state.started_at.map(|started_at| {
            i64::try_from(
                completed_at_instant
                    .saturating_duration_since(started_at)
                    .as_millis(),
            )
            .unwrap_or(i64::MAX)
        });
        let profile = self.profile_state().complete(completed_at_instant);
        (completed_at, duration_ms, profile)
    }

    pub(crate) fn begin_sampling(self: &Arc<Self>) -> TurnProfileTimingGuard {
        let active = self.profile_state().begin_sampling(Instant::now());
        TurnProfileTimingGuard {
            timing: Arc::clone(self),
            phase: TurnProfilePhase::Sampling,
            active,
        }
    }

    pub(crate) fn record_sampling_retry(&self) {
        self.profile_state().record_sampling_retry();
    }

    pub(crate) fn begin_compaction(self: &Arc<Self>) -> TurnProfileTimingGuard {
        let active = self.profile_state().begin_compaction(Instant::now());
        TurnProfileTimingGuard {
            timing: Arc::clone(self),
            phase: TurnProfilePhase::Compaction,
            active,
        }
    }

    pub(crate) fn begin_tool_blocking(self: &Arc<Self>) -> TurnProfileTimingGuard {
        let active = self.profile_state().begin_tool_blocking(Instant::now());
        TurnProfileTimingGuard {
            timing: Arc::clone(self),
            phase: TurnProfilePhase::ToolBlocking,
            active,
        }
    }

    fn profile_state(&self) -> std::sync::MutexGuard<'_, TurnProfileState> {
        self.profile
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for TurnProfileTimingGuard {
    fn drop(&mut self) {
        if self.active {
            self.timing
                .profile_state()
                .end_phase(Instant::now(), self.phase);
        }
    }
}

fn now_unix_timestamp_secs() -> i64 {
    now_unix_timestamp_ms() / 1000
}

pub(crate) fn now_unix_timestamp_ms() -> i64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn duration_to_u64_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

impl TurnProfileState {
    fn start(&mut self, started_at: Instant) {
        *self = Self {
            started_at: Some(started_at),
            last_transition_at: Some(started_at),
            ..Self::default()
        };
    }

    fn begin_sampling(&mut self, now: Instant) -> bool {
        if self.completed_profile.is_some()
            || self.started_at.is_none()
            || self.active_phase.is_some()
        {
            return false;
        }
        self.advance(now);
        if self.seen_sampling {
            self.between_sampling_overhead += std::mem::take(&mut self.pending_idle_after_sampling);
        }
        self.seen_sampling = true;
        self.active_phase = Some(TurnProfilePhase::Sampling);
        self.sampling_request_count = self.sampling_request_count.saturating_add(1);
        true
    }

    fn record_sampling_retry(&mut self) {
        if self.completed_profile.is_none() && self.started_at.is_some() {
            self.sampling_retry_count = self.sampling_retry_count.saturating_add(1);
        }
    }

    fn begin_tool_blocking(&mut self, now: Instant) -> bool {
        if self.completed_profile.is_some()
            || self.started_at.is_none()
            || self.active_phase.is_some()
        {
            return false;
        }
        self.advance(now);
        self.active_phase = Some(TurnProfilePhase::ToolBlocking);
        true
    }

    fn begin_compaction(&mut self, now: Instant) -> bool {
        if self.completed_profile.is_some()
            || self.started_at.is_none()
            || self.active_phase.is_some()
        {
            return false;
        }
        self.advance(now);
        self.active_phase = Some(TurnProfilePhase::Compaction);
        true
    }

    fn end_phase(&mut self, now: Instant, phase: TurnProfilePhase) {
        if self.completed_profile.is_some() || self.active_phase != Some(phase) {
            return;
        }
        self.advance(now);
        self.active_phase = None;
    }

    fn advance(&mut self, now: Instant) {
        let Some(previous) = self.last_transition_at.replace(now) else {
            return;
        };
        let elapsed = now.saturating_duration_since(previous);
        match self.active_phase {
            Some(TurnProfilePhase::Sampling) => self.sampling += elapsed,
            Some(TurnProfilePhase::Compaction) => self.compaction += elapsed,
            Some(TurnProfilePhase::ToolBlocking) => self.tool_blocking += elapsed,
            None if self.seen_sampling => self.pending_idle_after_sampling += elapsed,
            None => self.before_first_sampling += elapsed,
        }
    }

    fn complete(&mut self, now: Instant) -> TurnProfile {
        if let Some(profile) = self.completed_profile.as_ref() {
            return profile.clone();
        }

        let final_phase = self.active_phase;
        self.advance(now);
        let after_last_sampling = if self.seen_sampling {
            std::mem::take(&mut self.pending_idle_after_sampling)
        } else {
            Duration::ZERO
        };

        let mut profile = TurnProfile {
            before_first_sampling_ms: duration_to_u64_ms(self.before_first_sampling),
            sampling_ms: duration_to_u64_ms(self.sampling),
            compaction_ms: duration_to_u64_ms(self.compaction),
            between_sampling_overhead_ms: duration_to_u64_ms(self.between_sampling_overhead),
            tool_blocking_ms: duration_to_u64_ms(self.tool_blocking),
            after_last_sampling_ms: duration_to_u64_ms(after_last_sampling),
            sampling_request_count: self.sampling_request_count,
            sampling_retry_count: self.sampling_retry_count,
        };
        let total_ms = self
            .started_at
            .map(|started_at| duration_to_u64_ms(now.saturating_duration_since(started_at)))
            .unwrap_or_default();
        let classified_ms = profile
            .before_first_sampling_ms
            .saturating_add(profile.sampling_ms)
            .saturating_add(profile.compaction_ms)
            .saturating_add(profile.between_sampling_overhead_ms)
            .saturating_add(profile.tool_blocking_ms)
            .saturating_add(profile.after_last_sampling_ms);
        let rounding_ms = total_ms.saturating_sub(classified_ms);
        match final_phase {
            Some(TurnProfilePhase::Sampling) => profile.sampling_ms += rounding_ms,
            Some(TurnProfilePhase::Compaction) => profile.compaction_ms += rounding_ms,
            Some(TurnProfilePhase::ToolBlocking) => profile.tool_blocking_ms += rounding_ms,
            None if self.seen_sampling => profile.after_last_sampling_ms += rounding_ms,
            None => profile.before_first_sampling_ms += rounding_ms,
        }

        self.active_phase = None;
        self.completed_profile = Some(profile.clone());
        profile
    }
}

#[cfg(test)]
#[path = "turn_timing_tests.rs"]
mod tests;
