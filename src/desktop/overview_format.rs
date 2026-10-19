use tiny_proxy::overview::{OverviewPeriod, OverviewTotals};

pub(super) fn number(value: Option<i64>) -> String {
    match value {
        None => "—".into(),
        Some(value) => {
            for (threshold, suffix) in [(1_000_000_000, "B"), (1_000_000, "M"), (1_000, "K")] {
                if value >= threshold {
                    let digits = format!("{:.2}", value as f64 / threshold as f64);
                    return format!(
                        "{}{suffix}",
                        digits.trim_end_matches('0').trim_end_matches('.')
                    );
                }
            }
            value.to_string()
        }
    }
}

pub(super) fn percentage(value: Option<f64>) -> String {
    value
        .filter(|value| value.is_finite() && (0. ..=1.).contains(value))
        .map(|value| format!("{:.2}%", value * 100.))
        .unwrap_or_else(|| "—".into())
}

pub(super) fn cache_read(totals: &OverviewTotals) -> String {
    let value = number(totals.cached_input_tokens);
    if totals.cached_input_tokens.is_some() && !totals.cache_usage_complete() {
        format!("≥{value}")
    } else {
        value
    }
}

pub(super) fn token_total(totals: &OverviewTotals) -> String {
    let value = number(totals.total_tokens());
    if totals.total_tokens().is_some() && !totals.usage_complete() {
        format!("≥{value}")
    } else {
        value
    }
}

pub(super) fn duration(value: Option<i64>) -> String {
    value
        .map(|ms| format!("{:.2} 秒", ms as f64 / 1_000.))
        .unwrap_or_else(|| "—".into())
}

pub(super) fn tick(value: i64, period: OverviewPeriod) -> String {
    chrono::DateTime::from_timestamp_millis(value)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format(if period == OverviewPeriod::Last7Days {
                    "%m/%d"
                } else {
                    "%H:%M"
                })
                .to_string()
        })
        .unwrap_or_else(|| "—".into())
}

pub(super) fn axis_ceiling(maximum: i64) -> i64 {
    let raw = maximum.max(1).saturating_add(2) / 3;
    let mut base = 1_i64;
    while raw > base.saturating_mul(10) && base <= i64::MAX / 10 {
        base *= 10;
    }
    let step = [1_i64, 2, 5, 10]
        .into_iter()
        .map(|n| n.saturating_mul(base))
        .find(|n| *n >= raw)
        .unwrap_or(i64::MAX / 3);
    step.saturating_mul(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_does_not_turn_missing_values_into_zero_and_marks_partial_totals() {
        assert_eq!(number(None), "—");
        assert_eq!(number(Some(0)), "0");
        assert_eq!(number(Some(49_600_000)), "49.6M");
        let mut totals = OverviewTotals {
            requests: 2,
            input_tokens: Some(100),
            output_tokens: Some(20),
            usage_samples: 1,
            ..Default::default()
        };
        assert_eq!(token_total(&totals), "≥120");
        totals.usage_samples = 2;
        assert_eq!(token_total(&totals), "120");
        assert_eq!(duration(Some(0)), "0.00 秒");
        assert_eq!(duration(None), "—");
    }

    #[test]
    fn chart_scale_contains_peaks_without_dividing_by_zero() {
        assert_eq!(axis_ceiling(0), 3);
        assert_eq!(axis_ceiling(51), 60);
        for maximum in [1, 3, 6, 7, 20, 51, 151, 10_000] {
            assert!(axis_ceiling(maximum) >= maximum);
            assert_eq!(axis_ceiling(maximum) % 3, 0);
        }
    }

    #[test]
    fn cache_display_preserves_unknown_and_zero_and_marks_incomplete_coverage() {
        assert_eq!(percentage(None), "—");
        assert_eq!(percentage(Some(0.)), "0.00%");
        assert_eq!(percentage(Some(1.)), "100.00%");
        for invalid in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            assert_eq!(percentage(Some(invalid)), "—");
        }
        assert_eq!(number(Some(45_640_000)), "45.64M");
        let mut totals = OverviewTotals {
            requests: 2,
            cached_input_tokens: Some(80),
            cache_input_tokens: Some(100),
            cache_samples: 1,
            ..Default::default()
        };
        assert_eq!(percentage(totals.cache_hit_rate()), "80.00%");
        assert_eq!(cache_read(&totals), "≥80");
        totals.cache_samples = 2;
        assert_eq!(cache_read(&totals), "80");
        totals.cached_input_tokens = None;
        assert_eq!(cache_read(&totals), "—");
    }
}
