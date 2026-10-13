use serde_json::json;
use tiny_proxy::auth::quota::{parse_usage, reset_count};

#[test]
fn quota_preserves_unknowns_and_converts_percent_and_reset_times() {
    let q=parse_usage(&json!({"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":25,"limit_window_seconds":18000,"reset_after_seconds":600},"secondary_window":{"used_percent":60,"reset_at":1800000000000_u64}},"rate_limit_reset_credits":{"available_count":2}}),1000).unwrap();
    assert_eq!(q.windows[0].remaining_percent, Some(75.0));
    assert_eq!(q.windows[0].reset_at, Some(1600));
    assert_eq!(q.windows[1].reset_at, Some(1800000000));
    assert_eq!(q.reset_credits, Some(2));
    let q = parse_usage(&json!({"rateLimit":{"primaryWindow":{}}}), 1000).unwrap();
    assert_eq!(q.windows[0].remaining_percent, None);
    assert_eq!(q.reset_credits, None);
    assert!(parse_usage(&json!({"error":"upstream failure"}), 1000).is_err());
    assert_eq!(reset_count(&json!({"available_count":0})), Some(0));
    assert_eq!(reset_count(&json!({})), None);
    assert_eq!(
        reset_count(&json!({"credits":[{"status":"available"},{"status":"consumed"}]})),
        Some(1)
    );
}
