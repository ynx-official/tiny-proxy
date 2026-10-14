use axum::{
    Json, Router,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tiny_proxy::{
    auth::codex::{Service, now},
    gateway::Gateway,
    platform::protection,
    protocol::gateway::{normalize_chat, normalize_responses},
    storage::Store,
};

#[test]
fn extension_fields_and_unused_parameter_types_do_not_block_chat_conversion() {
    let args = "partial-json{";
    let request = normalize_chat(json!({
        "model":"fixture", "stream":"true", "stream_options":{"include_usage":"true","extra":1},
        "temperature":0.8, "top_p":0.9, "max_tokens":"unlimited", "n":3, "store":true,
        "private-extension":{"value":"synthetic-private"},
        "messages":[
            {"role":"system","content":"first","name":"client-label"},
            {"role":"developer","content":"second"},
            {"role":"assistant","content":"answer","reasoning_content":{"text":"synthetic-private"},
             "annotations":[{"anything":true}],"audio":{"id":"metadata"},
             "tool_calls":[{"id":"c1","type":"function","index":-1,"extra":true,
                            "function":{"name":"vendor.lookup/path","arguments":args,"extra":null}}]},
            {"role":"tool","tool_call_id":"orphan","name":"does-not-match","content":"result","extra":true},
            {"role":"user","content":[{"type":"text","text":"continue","extra":true}],"reasoning_content":42}
        ],
        "tools":[{"type":"function","extra":true,"function":{"name":"vendor.lookup/path",
            "parameters":{"type":"object","extra":{"const":null}},"strict":"false","description":42}}]
    })).unwrap();
    assert!(request.stream);
    assert!(request.include_usage);
    assert_eq!(request.body["store"], false);
    assert_eq!(request.body["instructions"], "first\n\nsecond");
    assert_eq!(request.body["input"][1]["arguments"], args);
    assert_eq!(request.body["input"][2]["call_id"], "orphan");
    assert_eq!(request.body["tools"][0]["strict"], "false");
    assert_eq!(
        request.body["tools"][0]["parameters"]["extra"]["const"],
        Value::Null
    );
    assert!(!request.body.to_string().contains("synthetic-private"));
    for field in [
        "max_tokens",
        "temperature",
        "top_p",
        "private-extension",
        "n",
    ] {
        assert!(request.body.get(field).is_none());
    }
}

#[test]
fn native_responses_history_and_options_are_trusted_without_enum_or_link_checks() {
    let input = json!([
        {"type":"reasoning","status":"vendor-status","encrypted_content":"opaque","extra":true},
        {"type":"function_call","call_id":"c1","name":"vendor.lookup/path","arguments":"partial{"},
        {"type":"function_call_output","call_id":"orphan","output":{"result":42}},
        {"type":"message","role":"vendor-role","content":[{"type":"vendor-part","payload":42}],"extra":true}
    ]);
    let request = normalize_responses(json!({"model":"fixture","input":input,
        "temperature":0.8,"max_output_tokens":-1,"private-extension":true,
        "reasoning":{"effort":"vendor-effort","summary":42,"extra":true},
        "include":["reasoning.encrypted_content","vendor-include"],
        "tool_choice":{"type":"function","name":"not-in-tools"},"parallel_tool_calls":"true"
    }))
    .unwrap();
    assert_eq!(request.body["input"], input);
    assert_eq!(request.body["reasoning"]["summary"], 42);
    assert_eq!(request.body["include"][1], "vendor-include");
    assert_eq!(request.body["tool_choice"]["name"], "not-in-tools");
    assert_eq!(request.body["parallel_tool_calls"], true);
    assert!(request.body.get("temperature").is_none());
    assert!(request.body.get("private-extension").is_none());
}

#[test]
fn required_conversion_structure_still_returns_a_safe_location() {
    for (chat, body, path) in [
        (
            true,
            json!({"model":null,"messages":[],"synthetic-private":true}),
            "model",
        ),
        (true, json!({"model":"fixture","messages":{}}), "messages"),
        (
            true,
            json!({"model":"fixture","messages":[{"role":"tool","content":"result"}]}),
            "messages[0].tool_call_id",
        ),
        (false, json!({"model":"fixture","input":42}), "input"),
    ] {
        let result = if chat {
            normalize_chat(body)
        } else {
            normalize_responses(body)
        };
        let error = result
            .err()
            .expect("required structure cannot be converted");
        assert_eq!(error.param, path);
        assert!(!error.diagnostic_message().contains("synthetic-private"));
    }
}

#[test]
fn tool_count_names_and_argument_json_are_not_prevalidated() {
    let definitions = (0..130)
        .map(|_| json!({"function":{"name":"vendor.lookup/path","parameters":null}}))
        .collect::<Vec<_>>();
    let request = normalize_chat(json!({"model":"fixture","messages":[
        {"role":"assistant","tool_calls":[{"id":"c1","function":{"name":"vendor.lookup/path","arguments":{"key":null}}}]}
    ],"tools":definitions})).unwrap();
    assert_eq!(request.body["tools"].as_array().unwrap().len(), 130);
    assert_eq!(request.body["tools"][0]["parameters"], Value::Null);
    assert_eq!(
        serde_json::from_str::<Value>(request.body["input"][0]["arguments"].as_str().unwrap())
            .unwrap(),
        json!({"key":null})
    );
}

#[tokio::test]
async fn trusted_requests_reach_upstream_for_both_protocols_and_response_modes() {
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let received = seen.clone();
    let upstream = Router::new()
        .route("/models", get(|| async { Json(json!({"models":[{"slug":"fixture"}]})) }))
        .route("/responses", post(move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
            assert!(!headers.contains_key("session-id"));
            assert!(!headers.contains_key("thread-id"));
            assert_eq!(body["prompt_cache_key"], "cache\nmetadata");
            let reject = body["reasoning"]["effort"] == "reject-me";
            received.lock().unwrap().push(body);
            async move {
                if reject { return (axum::http::StatusCode::BAD_REQUEST, "synthetic-private-upstream-message").into_response(); }
                let events = [
                    json!({"type":"response.created","response":{"id":"r","model":"fixture","created_at":100,"status":"in_progress"}}),
                    json!({"type":"response.completed","response":{"id":"r","model":"fixture","created_at":100,"status":"completed","output":[]}}),
                ];
                ([("content-type","text/event-stream")], events.iter().map(|e| format!("data: {e}\n\n")).collect::<String>()).into_response()
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let bundle = json!({"access_token":"fixture","refresh_token":"fixture","account_id":"fixture","subject":"fixture","expires_at":now()+3600});
    store
        .save_codex_credentials(
            0,
            &protection::protect(bundle.to_string().as_bytes()).unwrap(),
            now() + 3600,
        )
        .await
        .unwrap();
    let service = Service::for_test(store.clone(), address).unwrap();
    let key = store.create_key("trusted fixture").await.unwrap();
    let mut gateway = Gateway::start("127.0.0.1:0".parse().unwrap(), store.clone(), service)
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let mut request_ids = vec![];
    for chat in [true, false] {
        for stream in [false, true] {
            let route = if chat {
                "chat/completions"
            } else {
                "responses"
            };
            let mut body = if chat {
                json!({"model":"fixture","messages":[{"role":"user","content":"hello","extension":true}],"reasoning_effort":"vendor-effort"})
            } else {
                json!({"model":"fixture","input":[{"role":"user","content":"hello","extension":true}],"reasoning":{"effort":"vendor-effort"}})
            };
            body["stream"] = json!(if stream { "true" } else { "false" });
            body["temperature"] = json!(0.7);
            body["max_tokens"] = json!(-1);
            body["max_output_tokens"] = json!("2048");
            body["synthetic-private-extension"] = json!("synthetic-private-value");
            body["prompt_cache_key"] = json!("cache\nmetadata");
            let url = format!("http://{}/v1/{route}", gateway.address);
            assert_eq!(
                client.post(&url).json(&body).send().await.unwrap().status(),
                401
            );
            let response = client
                .post(&url)
                .bearer_auth(key.value.as_str())
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let id = response.headers()["x-request-id"]
                .to_str()
                .unwrap()
                .to_owned();
            assert!(
                response.headers()["x-tiny-proxy-ignored-parameters"]
                    .to_str()
                    .unwrap()
                    .contains("temperature")
            );
            let text = response.text().await.unwrap();
            assert!(text.contains(if stream && chat {
                "[DONE]"
            } else if chat {
                "chat.completion"
            } else {
                "completed"
            }));
            request_ids.push((id, stream));
        }
    }
    assert_eq!(seen.lock().unwrap().len(), 4);
    for body in seen.lock().unwrap().iter() {
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["reasoning"]["effort"], "vendor-effort");
        assert!(body.get("temperature").is_none());
        assert!(body.get("synthetic-private-extension").is_none());
    }
    let response = client.post(format!("http://{}/v1/responses", gateway.address)).bearer_auth(key.value.as_str())
        .json(&json!({"model":"fixture","input":"hello","reasoning":{"effort":"reject-me"},"prompt_cache_key":"cache\nmetadata"}))
        .send().await.unwrap();
    assert_eq!(response.status(), 400);
    let failure_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let error: Value = response.json().await.unwrap();
    assert_eq!(error["error"]["code"], "upstream_rejected_request");
    assert!(
        !error
            .to_string()
            .contains("synthetic-private-upstream-message")
    );
    assert_eq!(seen.lock().unwrap().len(), 5);
    gateway.stop().await;
    assert_eq!(
        store
            .request_record(&failure_id)
            .await
            .unwrap()
            .unwrap()
            .phase,
        "waiting_upstream"
    );
    assert_eq!(
        store
            .failure_record(&failure_id)
            .await
            .unwrap()
            .unwrap()
            .code,
        "upstream_rejected_request"
    );
    for (id, stream) in request_ids {
        let record = store.request_record(&id).await.unwrap().unwrap();
        assert_eq!(record.outcome, "completed");
        assert_eq!(record.stream, Some(stream));
        assert!(store.failure_record(&id).await.unwrap().is_none());
    }
    store.close().await;
    let raw = std::fs::read(dir.path().join("app.db")).unwrap();
    for secret in [
        "synthetic-private-extension",
        "synthetic-private-value",
        "synthetic-private-upstream-message",
    ] {
        assert!(!raw.windows(secret.len()).any(|w| w == secret.as_bytes()));
    }
    task.abort();
}
