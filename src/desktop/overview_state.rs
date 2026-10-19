use std::time::{Duration, Instant};
use tiny_proxy::overview::{OverviewPeriod, OverviewSnapshot};

#[derive(Default)]
pub(super) struct OverviewState {
    pub period: OverviewPeriod,
    pub data: Option<Box<OverviewSnapshot>>,
    pub error: Option<String>,
    pub loading: bool,
    next_id: u64,
    active_id: Option<u64>,
    last_attempt: Option<Instant>,
}

impl OverviewState {
    pub fn begin(&mut self, period: OverviewPeriod, now: Instant) -> u64 {
        if period != self.period {
            self.data = None;
        }
        self.period = period;
        self.next_id = self.next_id.wrapping_add(1);
        self.active_id = Some(self.next_id);
        self.error = None;
        self.loading = true;
        self.last_attempt = Some(now);
        self.next_id
    }

    pub fn receive(
        &mut self,
        id: u64,
        period: OverviewPeriod,
        result: Result<Box<OverviewSnapshot>, String>,
        now: Instant,
    ) {
        if self.active_id != Some(id) || self.period != period {
            return;
        }
        self.active_id = None;
        self.loading = false;
        self.last_attempt = Some(now);
        match result {
            Ok(snapshot) if snapshot.window.period == period => {
                self.data = Some(snapshot);
                self.error = None;
            }
            Ok(_) => self.error = Some("统计范围不一致，请重试".into()),
            Err(error) => self.error = Some(error),
        }
    }

    pub fn should_refresh(&self, now: Instant) -> bool {
        !self.loading
            && self
                .last_attempt
                .is_none_or(|last| now.saturating_duration_since(last) >= Duration::from_secs(5))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_proxy::overview::{OverviewWindow, aggregate};

    fn snapshot(period: OverviewPeriod) -> Box<OverviewSnapshot> {
        Box::new(aggregate(
            OverviewWindow::at(period, chrono::Utc::now()).unwrap(),
            vec![],
            &[],
        ))
    }

    #[test]
    fn changing_period_discards_old_values_and_ignores_out_of_order_results() {
        let now = Instant::now();
        let mut state = OverviewState::default();
        let first = state.begin(OverviewPeriod::Today, now);
        let next = state.begin(OverviewPeriod::Last7Days, now);
        state.receive(
            first,
            OverviewPeriod::Today,
            Ok(snapshot(OverviewPeriod::Today)),
            now,
        );
        assert!(state.loading);
        assert!(state.data.is_none());
        state.receive(
            next,
            OverviewPeriod::Last7Days,
            Ok(snapshot(OverviewPeriod::Last7Days)),
            now,
        );
        assert!(!state.loading);
        assert_eq!(
            state.data.as_ref().unwrap().window.period,
            OverviewPeriod::Last7Days
        );
        state.begin(OverviewPeriod::Last24Hours, now);
        assert!(state.data.is_none());
    }

    #[test]
    fn refresh_failure_keeps_the_last_snapshot_and_allows_retry_without_a_hot_loop() {
        let now = Instant::now();
        let mut state = OverviewState::default();
        assert!(state.should_refresh(now));
        let id = state.begin(OverviewPeriod::Today, now);
        assert!(!state.should_refresh(now + Duration::from_secs(30)));
        state.receive(
            id,
            OverviewPeriod::Today,
            Ok(snapshot(OverviewPeriod::Today)),
            now,
        );
        let id = state.begin(OverviewPeriod::Today, now);
        state.receive(
            id,
            OverviewPeriod::Today,
            Err("local read failed".into()),
            now,
        );
        assert!(state.data.is_some());
        assert!(state.error.is_some());
        assert!(!state.should_refresh(now));
        assert!(state.should_refresh(now + Duration::from_secs(5)));
        let id = state.begin(OverviewPeriod::Today, now);
        state.receive(
            id,
            OverviewPeriod::Today,
            Ok(snapshot(OverviewPeriod::Today)),
            now,
        );
        assert!(state.error.is_none());
    }

    #[test]
    fn late_errors_do_not_cancel_current_queries_and_wrong_period_payloads_are_retryable() {
        let now = Instant::now();
        let mut state = OverviewState::default();
        let old = state.begin(OverviewPeriod::Today, now);
        let current = state.begin(OverviewPeriod::Last7Days, now);
        state.receive(old, OverviewPeriod::Today, Err("late failure".into()), now);
        assert!(state.loading);
        assert!(state.error.is_none());
        state.receive(
            current,
            OverviewPeriod::Last7Days,
            Ok(snapshot(OverviewPeriod::Today)),
            now,
        );
        assert!(!state.loading);
        assert!(state.data.is_none());
        assert!(state.error.is_some());
        let retry = state.begin(OverviewPeriod::Last7Days, now);
        state.receive(
            retry,
            OverviewPeriod::Last7Days,
            Ok(snapshot(OverviewPeriod::Last7Days)),
            now,
        );
        assert!(state.data.is_some());
        assert!(state.error.is_none());
    }
}
