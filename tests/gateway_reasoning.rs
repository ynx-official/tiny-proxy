use axum::{
    Json, Router,
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

const REASONING: &str = "synthetic-private-reasoning\n不应进入提示词";
const IGNORED: &str = "messages[].reasoning_content";
const WARNING: &str = "Assistant reasoning_content is not replayed to the Codex upstream";
const LIMIT_WARNING: &str = "Output token limits are not enforced by the Codex upstream";

fn history(reasoning: Value) -> Value {
    json!({"model":"fixture","messages":[
        {"role":"system","content":"Keep instructions unchanged."},
        {"role":"user","content":"Read the file."},
        {"role":"assistant","content":null,"reasoning_content":reasoning,
         "tool_calls":[{"id":"c1","type":"function","function":{"name":"read_file","arguments":"{\"path\": \"hello.txt\"}"}}]},
        {"role":"tool","tool_call_id":"c1","content":"file text"},
        {"role":"assistant","content":[{"type":"text","text":"Read complete."}],"reasoning_content":reasoning},
        {"role":"user","content":"Continue."}
    ]})
}

fn without_reasoning(mut body: Value) -> Value {
    for message in body["messages"].as_array_mut().unwrap() {
        message.as_object_mut().unwrap().remove("reasoning_content");
    }
    body
}

#[test]
fn assistant_reasoning_metadata_preserves_text_and_tool_history_without_replay() {
    for reasoning in [Value::Null, json!(""), json!(REASONING)] {
        let body = history(reasoning.clone());
        let expected = normalize_chat(without_reasoning(body.clone())).unwrap();
        let actual = normalize_chat(body).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(actual.body, expected.body);
        assert_eq!(actual.body["input"][1]["call_id"], "c1");
        assert_eq!(
            actual.body["input"][1]["arguments"],
            "{\"path\": \"hello.txt\"}"
        );
        assert_eq!(actual.body["input"][2]["output"], "file text");
        assert_eq!(
            actual.body["input"][3]["content"][0]["text"],
            "Read complete."
        );
        assert_eq!(
            actual.ignored_parameters,
            if reasoning == REASONING {
                vec![IGNORED]
            } else {
                vec![]
            }
        );
    }
}

fn invalid_requests() -> Vec<Value> {
    let mut requests = vec![];
    for value in [json!(false), json!(7), json!([]), json!({"text":REASONING})] {
        requests.push(history(value));
    }
    for index in [0, 1, 3] {
        let mut body = history(Value::Null);
        body["messages"][index]["reasoning_content"] = Value::Null;
        requests.push(body);
    }
    let mut body = history(Value::Null);
    body["reasoning_content"] = json!(REASONING);
    requests.push(body);
    let mut body = history(json!(REASONING));
    body["messages"][2]["reasoning_details"] = json!([]);
    requests.push(body);
    let mut body = history(json!(REASONING));
    body["temperature"] = json!(0.2);
    requests.push(body);
    let mut body = history(json!(REASONING));
    body["messages"][3]["tool_call_id"] = json!("orphan");
    requests.push(body);
    let mut body = history(json!(REASONING));
    body["messages"].as_array_mut().unwrap().remove(3);
    requests.push(body);
    requests
}

#[test]
fn reasoning_compatibility_does_not_relax_types_roles_unknown_fields_or_tool_links() {
    for body in invalid_requests() {
        let error = normalize_chat(body)
            .err()
            .expect("invalid history must fail");
        assert!(!error.to_string().contains(REASONING));
    }
}

#[test]
fn responses_keeps_native_reasoning_replay_and_rejects_chat_metadata() {
    let input = json!([
        {"type":"reasoning","id":"r1","summary":[],"encrypted_content":"synthetic-ciphertext"},
        {"type":"function_call","call_id":"c1","name":"read_file","arguments":"{}"},
        {"type":"function_call_output","call_id":"c1","output":"file text"}
    ]);
    let request = normalize_responses(json!({"model":"fixture","input":input})).unwrap();
    assert_eq!(request.body["input"], input);
    assert!(request.ignored_parameters.is_empty());
    assert!(
        normalize_responses(json!({"model":"fixture","input":"hi","reasoning_content":null}))
            .is_err()
    );
    assert!(normalize_responses(json!({"model":"fixture","input":[{"role":"assistant","content":"hello","reasoning_content":null}]})).is_err());
}

#[tokio::test]
async fn reasoning_history_works_over_http_with_accurate_warnings_for_json_sse_and_errors() {
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let received = seen.clone();
    let upstream = Router::new()
        .route("/models", get(|| async { Json(json!({"models":[{"slug":"fixture"}]})) }))
        .route("/responses", post(move |Json(body): Json<Value>| {
            received.lock().unwrap().push(body);
            async {
                let events = [
                    json!({"type":"response.created","response":{"id":"r","model":"fixture","created_at":100,"status":"in_progress"}}),
                    json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"continued"}),
                    json!({"type":"response.completed","response":{"id":"r","model":"fixture","created_at":100,"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"continued"}]}]}})
                ];
                ([("content-type", "text/event-stream")], events.iter().map(|e| format!("data: {e}\n\n")).collect::<String>())
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
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
    let service = Service::for_test(store.clone(), address).unwrap();
    let key = store.create_key("reasoning fixture").await.unwrap();
    let mut gateway = Gateway::start("127.0.0.1:0".parse().unwrap(), store, service)
        .await
        .unwrap();
    let url = format!("http://{}/v1/chat/completions", gateway.address);
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    let expected = normalize_chat(without_reasoning(history(Value::Null)))
        .unwrap()
        .body;
    for stream in [false, true] {
        for reasoning in [Value::Null, json!(""), json!(REASONING)] {
            for limit in [false, true] {
                let mut body = history(reasoning.clone());
                body["stream"] = json!(stream);
                if limit {
                    body["max_tokens"] = json!(1);
                }
                let response = http
                    .post(&url)
                    .bearer_auth(key.value.as_str())
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let mut ignored = vec![];
                let mut warnings = vec![];
                if limit {
                    ignored.push("max_tokens");
                    warnings.push(LIMIT_WARNING);
                }
                if reasoning == REASONING {
                    ignored.push(IGNORED);
                    warnings.push(WARNING);
                }
                if ignored.is_empty() {
                    assert!(
                        !response
                            .headers()
                            .contains_key("x-tiny-proxy-ignored-parameters")
                    );
                    assert!(!response.headers().contains_key("x-tiny-proxy-warning"));
                } else {
                    assert_eq!(
                        response.headers()["x-tiny-proxy-ignored-parameters"],
                        ignored.join(", ")
                    );
                    assert_eq!(
                        response.headers()["x-tiny-proxy-warning"],
                        warnings.join("; ")
                    );
                }
                if stream {
                    let text = response.text().await.unwrap();
                    assert!(text.contains("continued"));
                    assert!(text.contains("\"finish_reason\":\"stop\""));
                    assert!(text.ends_with("data: [DONE]\n\n"));
                } else {
                    let value: Value = response.json().await.unwrap();
                    assert_eq!(value["choices"][0]["message"]["content"], "continued");
                    assert_eq!(value["choices"][0]["finish_reason"], "stop");
                }
                assert_eq!(*seen.lock().unwrap().last().unwrap(), expected);
            }
        }
    }
    let before = seen.lock().unwrap().len();
    assert_eq!(before, 12);
    for body in invalid_requests() {
        let response = http
            .post(&url)
            .bearer_auth(key.value.as_str())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        let error: Value = response.json().await.unwrap();
        assert_eq!(error["error"]["code"], "unsupported_or_invalid_parameter");
        assert!(!error.to_string().contains("synthetic-private"));
    }
    let body = history(json!(REASONING));
    assert_eq!(
        http.post(&url).json(&body).send().await.unwrap().status(),
        401
    );
    let mut body = body;
    body["model"] = json!("missing");
    let response = http
        .post(&url)
        .bearer_auth(key.value.as_str())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
    assert_eq!(
        response.headers()["x-tiny-proxy-ignored-parameters"],
        IGNORED
    );
    assert_eq!(response.headers()["x-tiny-proxy-warning"], WARNING);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["code"],
        "model_not_found"
    );
    assert_eq!(seen.lock().unwrap().len(), before);
    gateway.stop().await;
    task.abort();
}
