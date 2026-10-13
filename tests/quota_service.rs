use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use tiny_proxy::{
    auth::{
        codex::{Service, now},
        quota::Quota,
    },
    platform::protection,
    storage::Store,
};

#[tokio::test]
async fn queries_use_each_accounts_credentials_and_preserve_failed_snapshots() {
    let mode = Arc::new(AtomicU8::new(0));
    let router = Router::new()
        .route("/wham/usage", get(|State(mode): State<Arc<AtomicU8>>, headers: HeaderMap| async move {
            if mode.load(Ordering::SeqCst) == 2 { return StatusCode::TOO_MANY_REQUESTS.into_response(); }
            let one = headers["chatgpt-account-id"] == "one";
            assert_eq!(headers["authorization"], if one { "Bearer token-one" } else { "Bearer token-two" });
            Json(json!({"rate_limit":{"primary_window":{"used_percent":if one {20} else {40},"reset_after_seconds":600}}})).into_response()
        }))
        .route("/wham/rate-limit-reset-credits", get(|State(mode): State<Arc<AtomicU8>>, headers: HeaderMap| async move {
            if mode.load(Ordering::SeqCst) != 0 { return StatusCode::BAD_GATEWAY.into_response(); }
            Json(json!({"available_count":if headers["chatgpt-account-id"] == "one" {2} else {3}})).into_response()
        })).with_state(mode.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = Store::open(&path).await.unwrap();
    for id in ["one", "two"] {
        let blob = protection::protect(json!({"access_token":format!("token-{id}"),"refresh_token":"fixture","subject":id,"account_id":id,"expires_at":now()+3600}).to_string().as_bytes()).unwrap();
        store
            .upsert_account(id, id, &blob, now() + 3600, true)
            .await
            .unwrap();
    }
    let service = Service::for_test(store.clone(), address).unwrap();
    service.refresh_usage("one").await.unwrap();
    service.refresh_usage("two").await.unwrap();
    assert_eq!(
        store.active_account().await.unwrap().as_deref(),
        Some("two")
    );
    for (id, remaining, count) in [("one", 80.0, 2), ("two", 60.0, 3)] {
        let account = store.account(id).await.unwrap().unwrap();
        let q: Quota = serde_json::from_str(account.usage_json.as_deref().unwrap()).unwrap();
        assert_eq!(q.windows[0].remaining_percent, Some(remaining));
        assert_eq!(q.reset_credits, Some(count));
    }
    mode.store(1, Ordering::SeqCst);
    service.refresh_usage("one").await.unwrap();
    let saved = store.account("one").await.unwrap().unwrap();
    let q: Quota = serde_json::from_str(saved.usage_json.as_deref().unwrap()).unwrap();
    assert_eq!(
        q.reset_credits,
        Some(2),
        "failed count query must keep its previous value"
    );
    assert!(q.reset_error.is_some());
    mode.store(2, Ordering::SeqCst);
    assert!(service.refresh_usage("one").await.is_err());
    store.close().await;
    let reopened = Store::open(&path).await.unwrap();
    let failed = reopened.account("one").await.unwrap().unwrap();
    assert_eq!(failed.usage_json, saved.usage_json);
    assert_eq!(failed.usage_checked_at, saved.usage_checked_at);
    assert!(failed.usage_error.unwrap().contains("429"));
    reopened.close().await;
    server.abort();
}
