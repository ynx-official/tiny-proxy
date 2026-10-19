//! Read-only overview statistics over retained generation-request metadata.

use crate::storage::RequestRecord;
use chrono::{DateTime, Duration, NaiveDate, TimeZone};
use std::collections::BTreeMap;

const HOUR_MS: i64 = 3_600_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverviewPeriod {
    Today,
    #[default]
    Last24Hours,
    Last7Days,
}

impl OverviewPeriod {
    pub const ALL: [Self; 3] = [Self::Today, Self::Last24Hours, Self::Last7Days];

    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "今日",
            Self::Last24Hours => "近24小时",
            Self::Last7Days => "近7天",
        }
    }
}

#[derive(Clone, Debug)]
pub struct OverviewWindow {
    pub period: OverviewPeriod,
    pub start_ms: i64,
    /// Exclusive query boundary: future hours of today are never counted.
    pub end_ms: i64,
    pub chart_end_ms: i64,
    pub bucket_ms: i64,
}

impl OverviewWindow {
    pub fn at<T: TimeZone>(period: OverviewPeriod, now: DateTime<T>) -> Option<Self> {
        let end_ms = now.timestamp_millis();
        let (start_ms, chart_end_ms, bucket_ms) = match period {
            OverviewPeriod::Today => {
                let date = now.date_naive();
                let timezone = now.timezone();
                (
                    day_start(&timezone, date)?,
                    day_start(&timezone, date.succ_opt()?)?,
                    HOUR_MS,
                )
            }
            OverviewPeriod::Last24Hours => (end_ms.checked_sub(24 * HOUR_MS)?, end_ms, HOUR_MS),
            OverviewPeriod::Last7Days => {
                (end_ms.checked_sub(7 * 24 * HOUR_MS)?, end_ms, 6 * HOUR_MS)
            }
        };
        Some(Self {
            period,
            start_ms,
            end_ms,
            chart_end_ms,
            bucket_ms,
        })
    }

    pub fn contains(&self, record: &RequestRecord) -> bool {
        record.method == "POST"
            && matches!(
                record.route.as_str(),
                "/v1/responses" | "/v1/chat/completions"
            )
            && record.started_at >= self.start_ms
            && record.started_at < self.end_ms
    }
}

fn day_start<T: TimeZone>(timezone: &T, date: NaiveDate) -> Option<i64> {
    let midnight = date.and_hms_opt(0, 0, 0)?;
    // Some zones skip midnight or even a whole calendar date. Find the first
    // valid local instant; a fixed UTC offset would miscount DST transitions.
    (0..=48 * 60).find_map(|minute| {
        let local = midnight.checked_add_signed(Duration::minutes(minute))?;
        timezone
            .from_local_datetime(&local)
            .earliest()
            .map(|t| t.timestamp_millis())
    })
}

#[derive(Clone, Debug, Default)]
pub struct OverviewTotals {
    pub requests: i64,
    pub completed: i64,
    pub failed: i64,
    pub cancelled: i64,
    pub running: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    /// Cache numerator and denominator always cover the same valid samples.
    pub cached_input_tokens: Option<i64>,
    pub cache_input_tokens: Option<i64>,
    pub cache_samples: i64,
    pub usage_samples: i64,
    pub latency_samples: usize,
    pub p50_ms: Option<i64>,
    pub p95_ms: Option<i64>,
}

impl OverviewTotals {
    pub fn cache_hit_rate(&self) -> Option<f64> {
        let cached = self.cached_input_tokens?;
        let input = self.cache_input_tokens?;
        (input > 0 && cached >= 0 && cached <= input).then(|| cached as f64 / input as f64)
    }

    pub fn cache_usage_complete(&self) -> bool {
        self.cache_samples == self.requests
            && self.cached_input_tokens.is_some()
            && self.cache_input_tokens.is_some()
    }

    pub fn success_rate(&self) -> Option<f64> {
        let finished = self.completed + self.failed;
        (finished > 0).then(|| self.completed as f64 / finished as f64)
    }

    pub fn usage_complete(&self) -> bool {
        self.usage_samples == self.requests
            && self.input_tokens.is_some()
            && self.output_tokens.is_some()
    }

    pub fn total_tokens(&self) -> Option<i64> {
        if self.input_tokens.is_none() && self.output_tokens.is_none() {
            return None;
        }
        self.input_tokens
            .unwrap_or(0)
            .checked_add(self.output_tokens.unwrap_or(0))
    }
}

#[derive(Clone, Debug)]
pub struct OverviewBucket {
    pub start_ms: i64,
    pub end_ms: i64,
    pub completed: i64,
    pub failed: i64,
    pub cancelled: i64,
    pub running: i64,
}

impl OverviewBucket {
    pub fn total(&self) -> i64 {
        self.completed + self.failed + self.cancelled + self.running
    }
}

#[derive(Clone, Debug)]
pub struct OverviewSnapshot {
    pub window: OverviewWindow,
    pub totals: OverviewTotals,
    pub buckets: Vec<OverviewBucket>,
    pub recent: Vec<RequestRecord>,
}

/// Merge only actual live observations, keeping committed terminal rows final.
pub fn aggregate(
    window: OverviewWindow,
    persisted: Vec<RequestRecord>,
    live: &[RequestRecord],
) -> OverviewSnapshot {
    let mut records: BTreeMap<_, _> = persisted
        .into_iter()
        .filter(|r| window.contains(r))
        .map(|r| (r.id.clone(), r))
        .collect();
    for record in live.iter().filter(|r| window.contains(r)) {
        match records.entry(record.id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(record.clone());
            }
            std::collections::btree_map::Entry::Occupied(mut entry)
                if entry.get().ended_at.is_none() =>
            {
                entry.insert(record.clone());
            }
            _ => {}
        }
    }
    let mut buckets = Vec::new();
    let mut start = window.start_ms;
    while start < window.chart_end_ms {
        let end = start
            .saturating_add(window.bucket_ms)
            .min(window.chart_end_ms);
        buckets.push(OverviewBucket {
            start_ms: start,
            end_ms: end,
            completed: 0,
            failed: 0,
            cancelled: 0,
            running: 0,
        });
        start = end;
    }
    let mut totals = OverviewTotals::default();
    let mut input = Vec::new();
    let mut output = Vec::new();
    let mut cached = Vec::new();
    let mut cache_input = Vec::new();
    let mut latencies = Vec::new();
    for record in records.values() {
        totals.requests += 1;
        let index = ((record.started_at - window.start_ms) / window.bucket_ms) as usize;
        let bucket = &mut buckets[index];
        if record.ended_at.is_none() {
            totals.running += 1;
            bucket.running += 1;
        } else {
            match record.outcome.as_str() {
                "completed" => {
                    totals.completed += 1;
                    bucket.completed += 1;
                }
                "cancelled" => {
                    totals.cancelled += 1;
                    bucket.cancelled += 1;
                }
                _ => {
                    totals.failed += 1;
                    bucket.failed += 1;
                }
            }
        }
        let in_tokens = record.input_tokens.filter(|n| *n >= 0);
        let out_tokens = record.output_tokens.filter(|n| *n >= 0);
        if let Some(value) = in_tokens {
            input.push(value);
        }
        if let Some(value) = out_tokens {
            output.push(value);
        }
        if in_tokens.is_some() && out_tokens.is_some() {
            totals.usage_samples += 1;
        }
        if let Some(input) = in_tokens
            && let Some(cache) = record
                .cached_input_tokens
                .filter(|n| *n >= 0 && *n <= input)
        {
            // Do not mix a known numerator with inputs from unknown legacy rows,
            // or average per-request percentages with different token counts.
            totals.cache_samples += 1;
            cached.push(cache);
            cache_input.push(input);
        }
        if record.stream == Some(true)
            && let Some(at) = record.first_output_at
            && at >= record.started_at
            && at < window.end_ms
        {
            latencies.push(at - record.started_at);
        }
    }
    let sum = |values: &[i64]| {
        if values.is_empty() && totals.requests > 0 {
            None
        } else {
            values
                .iter()
                .try_fold(0_i64, |sum, value| sum.checked_add(*value))
        }
    };
    totals.input_tokens = sum(&input);
    totals.output_tokens = sum(&output);
    totals.cached_input_tokens = sum(&cached);
    totals.cache_input_tokens = sum(&cache_input);
    latencies.sort_unstable();
    totals.latency_samples = latencies.len();
    let percentile = |percent: usize| {
        latencies
            .get((latencies.len() * percent).div_ceil(100).saturating_sub(1))
            .copied()
    };
    totals.p50_ms = percentile(50);
    totals.p95_ms = percentile(95);
    let mut recent: Vec<_> = records.into_values().collect();
    recent.sort_unstable_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    recent.truncate(4);
    OverviewSnapshot {
        window,
        totals,
        buckets,
        recent,
    }
}
