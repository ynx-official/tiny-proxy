use serde_json::json;
use tiny_proxy::{
    auth::codex::{DeviceSession, Poll, parse_models, parse_poll},
    storage::Store,
};

#[test]
fn device_reply_and_pending_slowdown_denial_are_distinct() {
    let session = DeviceSession::parse(&json!({"device_auth_id":"device-test", "user_code":"ABCD-1234", "interval":"5", "expires_in":900})).unwrap();
    assert_eq!(session.user_code.as_str(), "ABCD-1234");
    assert_eq!(session.interval.as_secs(), 5);
    assert!(matches!(parse_poll(403, json!({})).unwrap(), Poll::Pending));
    assert!(matches!(
        parse_poll(400, json!({"error":"slow_down"})).unwrap(),
        Poll::SlowDown
    ));
    assert!(parse_poll(403, json!({"error":"access_denied"})).is_err());
    assert!(
        parse_poll(500, json!({})).is_err(),
        "unknown server errors must not poll forever"
    );
    assert!(
        DeviceSession::parse(&json!({"device_auth_id":"x","user_code":"x","expires_in":0}))
            .is_err()
    );
    assert!(matches!(
        parse_poll(
            200,
            json!({"authorization_code":"code","code_verifier":"verifier"})
        )
        .unwrap(),
        Poll::Code { .. }
    ));
}

#[test]
fn codex_device_pending_reply_does_not_end_authorization() {
    // Actual unauthenticated device poll observed on 2026-10-13.
    assert!(matches!(
        parse_poll(
            403,
            json!({"error":{"code":"deviceauth_authorization_pending","type":"invalid_request_error"}})
        ),
        Ok(Poll::Pending)
    ));
    for status in [403, 404] {
        assert!(matches!(
            parse_poll(status, json!({"error":"unrecognized_pending_format"})),
            Ok(Poll::Pending)
        ));
    }
    for body in [
        json!({"error":{"type":"authorization_pending"}}),
        json!({"status":"in_progress"}),
        json!({"state":"waiting"}),
        json!({"error":{"code":"deviceauth_authorization_pending"}}),
    ] {
        assert!(matches!(parse_poll(400, body), Ok(Poll::Pending)));
    }
    assert!(parse_poll(500, json!({"error":"authorization_pending"})).is_err());
    assert!(parse_poll(403, json!({"error":{"type":"access_denied"}})).is_err());
    assert!(parse_poll(404, json!({"error":{"code":"expired_token"}})).is_err());
}

#[test]
fn codex_catalog_keeps_real_ids_and_filters_unavailable_models() {
    let models = parse_models(json!({"models":[
        {"slug":"visible", "display_name":"Visible", "supported_reasoning_levels":[{"effort":"high"}]},
        {"slug":"hidden", "visibility":"hidden"},
        {"slug":"not-api", "supported_in_api":false},
        {"slug":"visible"}
    ]})).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].slug, "visible");
    assert_eq!(models[0].reasoning_efforts, ["high"]);
}

#[test]
fn catalog_accepts_reference_aliases_and_orders_the_actual_default() {
    let models = parse_models(json!({"data":[
        {"model":" first ","title":" First title ","reasoningLevels":[{"id":"high"},"high",{"value":"low"}]},
        {"name":"default","displayName":"Default title","supportedReasoningEfforts":[{"name":"medium"}]},
        {"id":"unavailable","supportedInApi":false},
        {"id":"hidden","visibility":" Hidden "},
        {"slug":"first"}
    ],"defaultModel":{"id":"default"}})).unwrap();
    assert_eq!(
        models.iter().map(|m| m.slug.as_str()).collect::<Vec<_>>(),
        ["default", "first"]
    );
    assert_eq!(models[0].display_name, "Default title");
    assert_eq!(models[0].reasoning_efforts, ["medium"]);
    assert_eq!(models[1].display_name, "First title");
    assert_eq!(models[1].reasoning_efforts, ["high", "low"]);
}

#[tokio::test]
async fn codex_credentials_are_separate_from_existing_siwc_and_versioned() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let registration = store.retain_registration("oaiapp_original").await.unwrap();
    store
        .complete_sign_in(&registration, "subject", true, b"siwc-encrypted", 100)
        .await
        .unwrap();
    assert!(store.codex_credentials().await.unwrap().is_none());
    store
        .save_codex_credentials(0, b"codex-encrypted", 200)
        .await
        .unwrap();
    assert!(
        store
            .save_codex_credentials(0, b"stale", 300)
            .await
            .is_err()
    );
    assert_eq!(
        store
            .load_credentials(&registration)
            .await
            .unwrap()
            .unwrap()
            .1,
        b"siwc-encrypted"
    );
    store
        .save_codex_credentials(1, b"rotated", 300)
        .await
        .unwrap();
    assert_eq!(
        store.codex_credentials().await.unwrap().unwrap().1,
        b"rotated"
    );
}
