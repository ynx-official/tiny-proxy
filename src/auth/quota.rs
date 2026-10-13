//! Read-only quota metadata. Unknown values stay unknown, never zero-filled.
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Window {
    pub name: String,
    pub remaining_percent: Option<f64>,
    pub reset_at: Option<i64>,
    pub window_seconds: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quota {
    pub plan: Option<String>,
    pub windows: Vec<Window>,
    pub reset_credits: Option<u64>,
    #[serde(default)]
    pub reset_checked_at: Option<i64>,
    pub reset_error: Option<String>,
    pub limit_reached: Option<bool>,
}
fn field<'a>(v: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|n| v.get(n).filter(|v| !v.is_null()))
}
fn numeric(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|v| v.is_finite())
}
fn count(v: &Value) -> Option<u64> {
    numeric(v)
        .filter(|n| *n >= 0.0 && *n < (u64::MAX as f64))
        .map(|n| n.floor() as u64)
}
fn timestamp(v: &Value) -> Option<i64> {
    if let Some(n) = numeric(v).filter(|n| *n >= 0.0 && *n < 1e15) {
        return Some((if n > 10_000_000_000.0 { n / 1000.0 } else { n }) as i64);
    }
    chrono::DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|t| t.timestamp())
}
pub fn reset_count(v: &Value) -> Option<u64> {
    field(v, &["available_count", "availableCount"])
        .and_then(count)
        .or_else(|| {
            let credits = v.get("credits")?.as_array()?;
            if credits.iter().any(|c| !c["status"].is_string()) {
                return None;
            }
            Some(
                credits
                    .iter()
                    .filter(|c| c["status"] == "available")
                    .count() as u64,
            )
        })
}
pub fn parse_usage(value: &Value, at: i64) -> Result<Quota, &'static str> {
    if !value.is_object() || value.get("error").is_some_and(|v| !v.is_null()) {
        return Err("额度响应格式无效");
    }
    let rate = field(value, &["rate_limit", "rateLimit"]).unwrap_or(value);
    let mut windows = vec![];
    for (keys, name) in [
        (["primary_window", "primaryWindow"], "主额度窗口"),
        (["secondary_window", "secondaryWindow"], "次额度窗口"),
    ] {
        if let Some(window) = field(rate, &keys).filter(|v| v.is_object()) {
            let remaining = field(window, &["used_percent", "usedPercent"])
                .and_then(numeric)
                .map(|n| 100.0 - n)
                .or_else(|| {
                    field(window, &["remaining_percent", "remainingPercent"]).and_then(numeric)
                })
                .map(|n| n.clamp(0.0, 100.0));
            let reset = field(window, &["reset_at", "resetAt"])
                .and_then(timestamp)
                .or_else(|| {
                    let after = field(window, &["reset_after_seconds", "resetAfterSeconds"])
                        .and_then(count)?;
                    at.checked_add(i64::try_from(after).ok()?)
                });
            windows.push(Window {
                name: name.into(),
                remaining_percent: remaining,
                reset_at: reset,
                window_seconds: field(window, &["limit_window_seconds", "limitWindowSeconds"])
                    .and_then(count),
            });
        }
    }
    let plan = field(value, &["plan_type", "planType"])
        .and_then(Value::as_str)
        .filter(|s| s.len() <= 80 && !s.chars().any(char::is_control))
        .map(str::to_owned);
    if windows.is_empty() && plan.is_none() && field(value, &["rate_limit", "rateLimit"]).is_none()
    {
        return Err("额度响应缺少已知字段");
    }
    let reset_credits = field(
        value,
        &["rate_limit_reset_credits", "rateLimitResetCredits"],
    )
    .and_then(reset_count);
    Ok(Quota {
        plan,
        windows,
        reset_credits,
        reset_checked_at: reset_credits.map(|_| at),
        reset_error: None,
        limit_reached: field(rate, &["limit_reached", "limitReached"]).and_then(Value::as_bool),
    })
}
