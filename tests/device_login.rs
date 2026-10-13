use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tiny_proxy::{
    auth::codex::{Service, now},
    storage::Store,
};
use tokio_util::sync::CancellationToken;

fn token() -> String {
    format!("e30.{}.signature", URL_SAFE_NO_PAD.encode(json!({"sub":"fixture","exp":now()+3600,"https://api.openai.com/auth":{"chatgpt_account_id":"fixture-account"}}).to_string()))
}

fn pending_then_code(poll: usize) -> (StatusCode, &'static str) {
    match poll {
        0 => (
            StatusCode::FORBIDDEN,
            r#"{"error":{"code":"deviceauth_authorization_pending","type":"invalid_request_error"}}"#,
        ),
        1 => (StatusCode::NOT_FOUND, ""),
        2 => (StatusCode::FORBIDDEN, "waiting"),
        _ => (
            StatusCode::OK,
            r#"{"authorization_code":"fixture-code","code_verifier":"fixture-verifier"}"#,
        ),
    }
}

#[tokio::test]
async fn waiting_responses_keep_the_device_session_alive_until_exchange() {
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let upstream = Router::new()
        .route("/api/accounts/deviceauth/usercode", post(|| async {Json(json!({"device_auth_id":"fixture","user_code":"TEST-0000","interval":1,"expires_in":30}))}))
        .route("/api/accounts/deviceauth/token", post(move || {let seen=seen.clone(); async move {pending_then_code(seen.fetch_add(1,Ordering::SeqCst)).into_response()}}))
        .route("/oauth/token", post(|| async {Json(json!({"access_token":token(),"refresh_token":"fixture-refresh"}))}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let service = Service::for_test(store.clone(), address).unwrap();
    let session = service.begin().await.unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        service.finish_device(session, CancellationToken::new()),
    )
    .await
    .unwrap();
    assert!(result.is_ok(), "pending poll ended the login: {result:?}");
    assert_eq!(count.load(Ordering::SeqCst), 4);
    assert!(service.connected().await.unwrap());
    store.close().await;
    task.abort();
}

#[tokio::test]
async fn auth_requests_advertise_json_and_real_403_identifies_its_stage() {
    // Real failures at the two other endpoints must never be treated as pending.
    for exchange in [false, true] {
        let upstream = Router::new()
            .route("/api/accounts/deviceauth/usercode", post(move |headers:HeaderMap| async move {
                assert_eq!(headers.get("accept").and_then(|v|v.to_str().ok()),Some("application/json"));
                if exchange { Json(json!({"device_auth_id":"fixture","user_code":"TEST-0000","interval":1})).into_response() }
                else { (StatusCode::FORBIDDEN,"sensitive-upstream-body").into_response() }
            }))
            .route("/api/accounts/deviceauth/token", post(|headers:HeaderMap| async move {
                assert_eq!(headers.get("accept").and_then(|v|v.to_str().ok()),Some("application/json"));
                Json(json!({"authorization_code":"fixture-code","code_verifier":"fixture-verifier"}))
            }))
            .route("/oauth/token", post(|headers:HeaderMap| async move {
                assert_eq!(headers.get("accept").and_then(|v|v.to_str().ok()),Some("application/json"));
                (StatusCode::FORBIDDEN,"sensitive-upstream-body")
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("app.db")).await.unwrap();
        let service = Service::for_test(store.clone(), address).unwrap();
        let error = if exchange {
            service
                .finish_device(service.begin().await.unwrap(), CancellationToken::new())
                .await
                .err()
                .unwrap()
        } else {
            service.begin().await.err().unwrap()
        };
        let message = error.to_string();
        assert!(message.contains("403"), "{message}");
        assert!(
            message.contains(if exchange {
                "换取登录凭据"
            } else {
                "申请设备码"
            }),
            "{message}"
        );
        assert!(!message.contains("sensitive-upstream-body"));
        assert!(!service.connected().await.unwrap());
        store.close().await;
        task.abort();
    }
}

#[tokio::test]
async fn pending_login_can_be_cancelled_or_expire_without_replacing_credentials() {
    let polled = Arc::new(tokio::sync::Notify::new());
    let signal = polled.clone();
    let upstream = Router::new()
        .route("/api/accounts/deviceauth/usercode", post(|| async {
            Json(json!({"device_auth_id":"fixture","user_code":"TEST-0000","interval":1,"expires_in":2}))
        }))
        .route("/api/accounts/deviceauth/token", post(move || {let signal=signal.clone(); async move {
            signal.notify_one();
            pending_then_code(0).into_response()
        }}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    store
        .save_codex_credentials(0, b"existing-ciphertext", now() + 3600)
        .await
        .unwrap();
    let service = Service::for_test(store.clone(), address).unwrap();
    for cancel_early in [true, false] {
        let session = service.begin().await.unwrap();
        let worker = service.clone();
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let login = tokio::spawn(async move { worker.finish_device(session, stop).await });
        tokio::time::timeout(Duration::from_secs(3), polled.notified())
            .await
            .unwrap();
        if cancel_early {
            cancel.cancel();
        }
        let result = tokio::time::timeout(Duration::from_secs(3), login)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            result,
            Err(tiny_proxy::auth::codex::Error::Cancelled)
        ));
        assert_eq!(
            store.codex_credentials().await.unwrap().unwrap(),
            (1, b"existing-ciphertext".to_vec())
        );
    }
    store.close().await;
    server.abort();
}
