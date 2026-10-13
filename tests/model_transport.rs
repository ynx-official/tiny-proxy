use axum::{
    Json, Router,
    routing::{get, post},
};
use serde_json::json;
use tiny_proxy::{
    auth::codex::{Service, now},
    platform::protection,
    storage::Store,
};

#[tokio::test]
async fn missing_content_type_accepts_only_a_complete_valid_sse_stream() {
    // An actual successful Codex response lacked Content-Type on 2026-10-13.
    // Keep normal SSE integrity checks rather than inferring success from HTTP 200.
    for valid_stream in [true, false] {
        let app=Router::new()
            .route("/models",get(||async{Json(json!({"models":[{"slug":"fixture"}]}))}))
            .route("/responses",post(move ||async move {
                let text = if valid_stream {
                    let events=[
                        json!({"type":"response.created","response":{"id":"r","model":"fixture","created_at":100,"status":"in_progress"}}),
                        json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"Tiny Proxy connection OK"}),
                        json!({"type":"response.output_item.done","output_index":0,"item":{"id":"message_1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Tiny Proxy connection OK"}]}}),
                        json!({"type":"response.completed","response":{"id":"r","model":"fixture","created_at":100,"status":"completed","output":[]}}),
                    ];
                    events.iter().map(|e|format!("data: {e}\n\n")).collect::<String>()
                } else {r#"{"error":{"message":"sensitive-body"}}"#.into()};
                axum::response::Response::new(axum::body::Body::from(text))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("app.db")).await.unwrap();
        let bundle = json!({"access_token":"fixture","refresh_token":"fixture","account_id":"fixture-account","subject":"fixture","expires_at":now()+3600});
        store
            .save_codex_credentials(
                0,
                &protection::protect(bundle.to_string().as_bytes()).unwrap(),
                now() + 3600,
            )
            .await
            .unwrap();
        let service = Service::for_test(store.clone(), addr).unwrap();
        let result = tiny_proxy::app::test_and_save(
            &service,
            &store,
            "fixture",
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
        let account = service.active_id().await.unwrap();
        let history = store.model_tests(&account).await.unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].finished_at.is_some());
        assert_eq!(
            history[0].status,
            if valid_stream { "completed" } else { "failed" }
        );
        if valid_stream {
            assert!(
                result.is_ok(),
                "complete SSE without header was rejected: {}",
                result.err().unwrap_or_default()
            );
            assert_eq!(history[0].text.as_deref(), Some("Tiny Proxy connection OK"));
        } else {
            let message = result
                .err()
                .expect("JSON without header must not pass SSE checks");
            assert!(!message.contains("sensitive-body"));
        }
        store.close().await;
        task.abort();
    }
}

#[tokio::test]
async fn catalog_errors_identify_json_shape_and_empty_models_without_body_leaks() {
    for (body, expected) in [
        ("<html>sensitive-body</html>", "模型目录不是有效 JSON"),
        (
            r#"{"message":"sensitive-body"}"#,
            "模型目录缺少 models/data 数组",
        ),
        (
            r#"{"models":[{"slug":"hidden","visibility":"hidden"}]}"#,
            "模型目录中没有可用模型",
        ),
    ] {
        let app = Router::new().route("/models", get(move || async move { body }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("app.db")).await.unwrap();
        let bundle = json!({"access_token":"fixture","refresh_token":"fixture","account_id":"fixture-account","subject":"fixture","expires_at":now()+3600});
        store
            .save_codex_credentials(
                0,
                &protection::protect(bundle.to_string().as_bytes()).unwrap(),
                now() + 3600,
            )
            .await
            .unwrap();
        let service = Service::for_test(store.clone(), addr).unwrap();
        let message = service.models().await.err().unwrap().to_string();
        assert!(message.contains(expected), "{message}");
        assert!(!message.contains("sensitive-body"));
        store.close().await;
        task.abort();
    }
}

#[tokio::test]
async fn non_stream_generation_is_reported_separately_from_model_catalog() {
    let app = Router::new()
        .route(
            "/models",
            get(|| async { Json(json!({"models":[{"slug":"fixture"}]})) }),
        )
        .route(
            "/responses",
            post(|| async { Json(json!({"error":{"message":"sensitive-body"}})) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let bundle = json!({"access_token":"fixture","refresh_token":"fixture","account_id":"fixture-account","subject":"fixture","expires_at":now()+3600});
    store
        .save_codex_credentials(
            0,
            &protection::protect(bundle.to_string().as_bytes()).unwrap(),
            now() + 3600,
        )
        .await
        .unwrap();
    let service = Service::for_test(store.clone(), addr).unwrap();
    let message = tiny_proxy::gateway::test_text(&service, "fixture")
        .await
        .err()
        .unwrap();
    assert!(message.contains("模型调用未返回 SSE 流"), "{message}");
    assert!(!message.contains("sensitive-body"));
    store.close().await;
    task.abort();
}
