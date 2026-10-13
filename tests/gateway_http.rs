use axum::{
    Json, Router,
    http::HeaderMap,
    response::IntoResponse,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tiny_proxy::{
    auth::codex::{Service, now},
    gateway::{Gateway, validate_bind},
    storage::Store,
};
use tokio_util::sync::CancellationToken;

fn token() -> String {
    format!("e30.{}.signature",URL_SAFE_NO_PAD.encode(json!({"sub":"test-sub","exp":now()+3600,"https://api.openai.com/auth":{"chatgpt_account_id":"test-account"}}).to_string()))
}

#[test]
fn only_explicit_private_or_loopback_listener_is_allowed() {
    assert!(validate_bind("127.0.0.1:8787").is_ok());
    assert!(validate_bind("192.168.1.20:8787").is_ok());
    assert!(validate_bind("100.64.1.20:8787").is_ok());
    assert!(validate_bind("0.0.0.0:8787").is_err());
    assert!(validate_bind("8.8.8.8:8787").is_err());
    assert!(validate_bind("localhost:8787").is_err());
}

#[tokio::test]
async fn full_device_login_and_gateway_text_stream_require_independent_key() {
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let upstream = Router::new()
        .route("/api/accounts/deviceauth/usercode",post(|| async { Json(json!({"device_auth_id":"fixture","user_code":"ABCD-1234","interval":1})) }))
        .route("/api/accounts/deviceauth/token",post(|| async { Json(json!({"authorization_code":"fixture-code","code_verifier":"fixture-verifier"})) }))
        .route("/oauth/token",post(|| async { Json(json!({"access_token":token(),"refresh_token":"fixture-refresh","expires_in":3600})) }))
        .route("/models",get(|headers:HeaderMap| async move {
            assert_eq!(headers["ChatGPT-Account-Id"],"test-account");
            assert_eq!(headers["originator"],"codex_cli_rs");
            Json(json!({"models":[{"slug":"fixture-model","display_name":"Fixture"}]}))
        }))
        .route("/responses",post(move |headers:HeaderMap,Json(body):Json<serde_json::Value>| { let seen=seen.clone(); async move {
            assert!(headers["authorization"].to_str().unwrap().starts_with("Bearer e30."));
            assert_eq!(body["stream"],true); assert_eq!(body["store"],false);
            for field in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
                assert!(body.get(field).is_none(), "Codex must not receive {field}");
            }
            seen.fetch_add(1,Ordering::SeqCst);
            let events = [json!({"type":"response.created","response":{"id":"resp_fixture","created_at":1234,"model":"fixture-model","status":"in_progress"}}),
                json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"你好"}),
                json!({"type":"response.completed","response":{"id":"resp_fixture","created_at":1234,"model":"fixture-model","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"你好"}]}],"usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}})];
            ([("content-type","text/event-stream")],events.iter().map(|e|format!("event: {}\ndata: {e}\n\n",e["type"].as_str().unwrap())).collect::<String>()).into_response()
        }}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let service = Service::for_test(store.clone(), address).unwrap();
    service
        .finish_device(service.begin().await.unwrap(), CancellationToken::new())
        .await
        .unwrap();
    let key = store.create_key("fixture Trae").await.unwrap();
    let mut gateway = Gateway::start("127.0.0.1:0".parse().unwrap(), store.clone(), service)
        .await
        .unwrap();
    let url = format!("http://{}", gateway.address);
    let http = reqwest::Client::new();
    assert_eq!(
        http.get(format!("{url}/v1/models"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let models = http
        .get(format!("{url}/v1/models"))
        .bearer_auth(key.value.as_str())
        .send()
        .await
        .unwrap();
    assert_eq!(models.status(), 200);
    assert_eq!(
        models.json::<serde_json::Value>().await.unwrap()["data"][0]["id"],
        "fixture-model"
    );
    let body = json!({"model":"fixture-model","messages":[{"role":"user","content":"hi"}],"stream":true,"stream_options":{"include_usage":true}});
    let response = http
        .post(format!("{url}/v1/chat/completions"))
        .bearer_auth(key.value.as_str())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.headers().contains_key("x-request-id"));
    assert!(
        !response
            .headers()
            .contains_key("x-tiny-proxy-ignored-parameters")
    );
    assert!(!response.headers().contains_key("x-tiny-proxy-warning"));
    let sse = response.text().await.unwrap();
    assert!(sse.contains("你好"));
    assert!(sse.ends_with("data: [DONE]\n\n"));
    assert!(sse.contains("prompt_tokens"));
    let response = http
        .post(format!("{url}/v1/responses"))
        .bearer_auth(key.value.as_str())
        .json(&json!({"model":"fixture-model","input":"hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["status"],
        "completed"
    );
    for stream in [false, true] {
        for field in [
            "max_tokens",
            "max_completion_tokens",
            "max_output_tokens",
            "both",
        ] {
            let responses = field == "max_output_tokens";
            let route = if responses {
                "responses"
            } else {
                "chat/completions"
            };
            let mut request = if responses {
                json!({"model":"fixture-model","input":"hi","stream":stream})
            } else {
                json!({"model":"fixture-model","messages":[{"role":"user","content":"hi"}],"stream":stream})
            };
            let ignored = if field == "both" {
                request["max_tokens"] = json!(1);
                request["max_completion_tokens"] = json!(2);
                "max_tokens, max_completion_tokens"
            } else {
                request[field] = json!(1);
                field
            };
            let response = http
                .post(format!("{url}/v1/{route}"))
                .bearer_auth(key.value.as_str())
                .json(&request)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200, "{field}, stream={stream}");
            assert_eq!(
                response.headers()["x-tiny-proxy-ignored-parameters"],
                ignored
            );
            assert_eq!(
                response.headers()["x-tiny-proxy-warning"],
                "Output token limits are not enforced by the Codex upstream"
            );
            if stream {
                let sse = response.text().await.unwrap();
                assert!(sse.contains("你好"));
                assert!(sse.contains(if responses {
                    "response.completed"
                } else {
                    "[DONE]"
                }));
            } else {
                let result = response.json::<serde_json::Value>().await.unwrap();
                if responses {
                    assert_eq!(result["status"], "completed");
                    assert_eq!(result["output"][0]["content"][0]["text"], "你好");
                } else {
                    assert_eq!(result["choices"][0]["message"]["content"], "你好");
                    assert_eq!(result["choices"][0]["finish_reason"], "stop");
                }
            }
        }
    }
    for field in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
        let responses = field == "max_output_tokens";
        let route = if responses {
            "responses"
        } else {
            "chat/completions"
        };
        let mut request = if responses {
            json!({"model":"fixture-model","input":"hi"})
        } else {
            json!({"model":"fixture-model","messages":[{"role":"user","content":"hi"}]})
        };
        request[field] = json!(-1);
        let response = http
            .post(format!("{url}/v1/{route}"))
            .bearer_auth(key.value.as_str())
            .json(&request)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        assert!(
            response.json::<serde_json::Value>().await.unwrap()["error"]["message"]
                .as_str()
                .unwrap()
                .contains(field)
        );
        request[field] = serde_json::Value::Null;
        let response = http
            .post(format!("{url}/v1/{route}"))
            .bearer_auth(key.value.as_str())
            .json(&request)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(
            !response
                .headers()
                .contains_key("x-tiny-proxy-ignored-parameters")
        );
        assert!(!response.headers().contains_key("x-tiny-proxy-warning"));
        let _ = response.bytes().await.unwrap();
    }
    for (field, parameters) in [
        ("temperature", json!({"max_tokens":1,"temperature":0.2})),
        (
            "max_tokens",
            json!({"max_tokens":-1,"max_completion_tokens":1}),
        ),
        (
            "max_completion_tokens",
            json!({"max_tokens":1,"max_completion_tokens":-1}),
        ),
    ] {
        let mut request =
            json!({"model":"fixture-model","messages":[{"role":"user","content":"hi"}]});
        request
            .as_object_mut()
            .unwrap()
            .extend(parameters.as_object().unwrap().clone());
        let response = http
            .post(format!("{url}/v1/chat/completions"))
            .bearer_auth(key.value.as_str())
            .json(&request)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        assert!(
            response.json::<serde_json::Value>().await.unwrap()["error"]["message"]
                .as_str()
                .unwrap()
                .contains(field)
        );
    }
    let response = http.post(format!("{url}/v1/chat/completions"))
        .bearer_auth(key.value.as_str())
        .json(&json!({"model":"missing-model","messages":[{"role":"user","content":"hi"}],"max_tokens":1}))
        .send().await.unwrap();
    assert_eq!(response.status(), 404);
    assert_eq!(
        response.headers()["x-tiny-proxy-ignored-parameters"],
        "max_tokens"
    );
    assert!(response.headers().contains_key("x-tiny-proxy-warning"));
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
        "model_not_found"
    );
    store.revoke_key(&key.id).await.unwrap();
    gateway.cancel_key(&key.id);
    for route in ["models", "chat/completions", "responses"] {
        let request = if route == "models" {
            http.get(format!("{url}/v1/{route}"))
        } else {
            http.post(format!("{url}/v1/{route}")).json(&json!({}))
        };
        let response = request
            .bearer_auth(key.value.as_str())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["code"],
            "invalid_gateway_key"
        );
    }
    assert_eq!(count.load(Ordering::SeqCst), 13);
    gateway.stop().await;
    assert!(
        tokio::net::TcpStream::connect(gateway.address)
            .await
            .is_err()
    );
    task.abort();
}
