use axum::{
    Json, Router,
    http::HeaderMap,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tiny_proxy::{
    auth::codex::{Service, now},
    platform::protection,
    storage::Store,
};
use tokio::sync::Notify;

fn token(exp: i64, account: &str) -> String {
    format!("e30.{}.sig",URL_SAFE_NO_PAD.encode(json!({"sub":"fixture-sub","exp":exp,"https://api.openai.com/auth":{"chatgpt_account_id":account}}).to_string()))
}

#[tokio::test]
async fn cancelled_caller_does_not_lose_rotation_and_sixteen_callers_share_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let bundle = json!({"access_token":token(now()+10,"fixture-account"),"refresh_token":"old-refresh","account_id":"fixture-account","subject":"fixture-sub","expires_at":now()+10});
    store
        .save_codex_credentials(
            0,
            &protection::protect(bundle.to_string().as_bytes()).unwrap(),
            now() + 10,
        )
        .await
        .unwrap();
    let started = Arc::new(Notify::new());
    let finish = Arc::new(Notify::new());
    let count = Arc::new(AtomicUsize::new(0));
    let (s, f, c) = (started.clone(), finish.clone(), count.clone());
    let app=Router::new().route("/oauth/token",post(move ||{let(s,f,c)=(s.clone(),f.clone(),c.clone());async move {
        c.fetch_add(1,Ordering::SeqCst);s.notify_one();f.notified().await;
        Json(json!({"access_token":token(now()+3600,"fixture-account"),"refresh_token":"rotated-refresh","expires_in":3600}))
    }})).route("/models",get(|headers:HeaderMap|async move{assert_eq!(headers["ChatGPT-Account-Id"],"fixture-account");Json(json!({"models":[{"slug":"fixture"}]}))}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let upstream = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let service = Service::for_test(store.clone(), address).unwrap();
    let first = service.clone();
    let caller = tokio::spawn(async move { first.models().await });
    tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    caller.abort();
    let _ = caller.await;
    let callers = (0..16)
        .map(|_| {
            let service = service.clone();
            tokio::spawn(async move { service.models().await })
        })
        .collect::<Vec<_>>();
    finish.notify_one();
    for caller in callers {
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), caller)
                .await
                .unwrap()
                .unwrap()
                .is_ok()
        );
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let (generation, blob) = store.codex_credentials().await.unwrap().unwrap();
    assert_eq!(generation, 2);
    let value: serde_json::Value =
        serde_json::from_slice(&protection::unprotect(&blob).unwrap()).unwrap();
    assert_eq!(value["refresh_token"], "rotated-refresh");
    upstream.abort();
}

#[tokio::test]
async fn refresh_cannot_replace_the_account_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let bundle = json!({"access_token":token(now()+10,"original"),"refresh_token":"old","account_id":"original","subject":"fixture-sub","expires_at":now()+10});
    store
        .save_codex_credentials(
            0,
            &protection::protect(bundle.to_string().as_bytes()).unwrap(),
            now() + 10,
        )
        .await
        .unwrap();
    let app = Router::new().route(
        "/oauth/token",
        post(|| async {
            Json(json!({"access_token":token(now()+3600,"other"),"refresh_token":"new"}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let upstream = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    assert!(
        Service::for_test(store.clone(), address)
            .unwrap()
            .models()
            .await
            .is_err()
    );
    assert_eq!(store.codex_credentials().await.unwrap().unwrap().0, 1);
    upstream.abort();
}
