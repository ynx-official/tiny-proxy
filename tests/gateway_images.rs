use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tiny_proxy::{
    auth::codex::{Service, now},
    gateway::Gateway,
    platform::protection,
    protocol::gateway::{normalize_chat, normalize_responses},
    storage::Store,
};

// Synthetic 1x1 PNG: never use a user's screenshot in test fixtures.
const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=";

#[tokio::test]
async fn large_unauthenticated_bodies_receive_401_for_fixed_and_chunked_uploads() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(&directory.path().join("app.db")).await.unwrap();
    let service = Service::for_test(store.clone(), "127.0.0.1:9".parse().unwrap()).unwrap();
    let mut gateway = Gateway::start("127.0.0.1:0".parse().unwrap(), store.clone(), service)
        .await
        .unwrap();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    for route in ["chat/completions", "responses"] {
        for invalid_key in [false, true] {
            for chunked in [false, true] {
                let payload = vec![b' '; 4 * 1024 * 1024];
                let mut request = client
                    .post(format!("http://{}/v1/{route}", gateway.address))
                    .header("content-type", "application/json");
                if invalid_key {
                    request = request.bearer_auth("not-a-valid-key");
                }
                let request = if chunked {
                    request.body(reqwest::Body::wrap_stream(futures_util::stream::iter(
                        payload
                            .chunks(64 * 1024)
                            .map(|part| Ok::<_, std::io::Error>(part.to_vec()))
                            .collect::<Vec<_>>(),
                    )))
                } else {
                    request.body(payload)
                };
                let response = request.send().await.expect(
                    "authentication rejection must deliver an HTTP response instead of a TCP reset",
                );
                assert_eq!(
                    response.status(),
                    401,
                    "route={route}, invalid_key={invalid_key}, chunked={chunked}"
                );
                let id = response.headers()["x-request-id"]
                    .to_str()
                    .unwrap()
                    .to_owned();
                let body: Value = response.json().await.unwrap();
                assert_eq!(body["error"]["code"], "invalid_gateway_key");
                let record = store.request_record(&id).await.unwrap().unwrap();
                assert_eq!(record.http_status, Some(401));
                assert_eq!(
                    record.body_bytes, None,
                    "rejected bodies are discarded without parsing or retention"
                );
            }
        }
    }
    gateway.stop().await;
    store.close().await;
}

fn request(chat: bool, content: Value) -> Value {
    if chat {
        json!({"model":"fixture","messages":[{"role":"user","content":content}]})
    } else {
        json!({"model":"fixture","input":[{"role":"user","content":content}]})
    }
}

fn image(chat: bool, url: &str) -> Value {
    if chat {
        json!({"type":"image_url","image_url":{"url":url}})
    } else {
        json!({"type":"input_image","image_url":url})
    }
}

fn normalize(chat: bool, body: Value) -> Result<Value, tiny_proxy::protocol::gateway::Error> {
    if chat {
        normalize_chat(body)
    } else {
        normalize_responses(body)
    }
    .map(|r| r.body)
}

#[test]
fn chat_images_preserve_text_image_order_and_image_only_messages() {
    let parts = json!([
        {"type":"text","text":"第一张"},
        {"type":"image_url","image_url":{"url":PNG,"detail":"high"}},
        {"type":"text","text":"第二张"},
        {"type":"image_url","image_url":{"url":PNG}}
    ]);
    let body = normalize(true, request(true, parts)).unwrap();
    assert_eq!(
        body["input"][0]["content"],
        json!([
            {"type":"input_text","text":"第一张"},
            {"type":"input_image","image_url":PNG,"detail":"high"},
            {"type":"input_text","text":"第二张"},
            {"type":"input_image","image_url":PNG,"detail":"auto"}
        ])
    );
    let body = normalize(true, request(true, json!([image(true, PNG)]))).unwrap();
    assert_eq!(
        body["input"][0]["content"],
        json!([
            {"type":"input_image","image_url":PNG,"detail":"auto"}
        ])
    );
}

#[test]
fn responses_images_preserve_history_metadata_and_default_detail() {
    let input = json!([
        {"type":"message","role":"user","content":[
            {"type":"input_text","text":"compare"},
            {"type":"input_image","image_url":PNG,"detail":"low"},
            {"type":"input_image","image_url":PNG}
        ]},
        {"type":"message","id":"msg_1","status":"completed","role":"assistant","content":[{"type":"output_text","text":"same","annotations":[]}]},
        {"type":"function_call","call_id":"c1","name":"read","arguments":"{}"},
        {"type":"function_call_output","call_id":"c1","output":"done"}
    ]);
    let body = normalize(false, json!({"model":"fixture","input":input})).unwrap();
    let mut expected = input;
    expected[0]["content"][2]["detail"] = json!("auto");
    assert_eq!(body["input"], expected);
    let body = normalize(false, request(false, json!([image(false, PNG)]))).unwrap();
    assert_eq!(body["input"][0]["content"][0]["image_url"], PNG);
}

#[test]
fn image_detail_is_preserved_and_text_tool_history_stays_valid() {
    for chat in [true, false] {
        for detail in ["auto", "low", "high", "original"] {
            let mut part = image(chat, PNG);
            if chat {
                part["image_url"]["detail"] = json!(detail);
            } else {
                part["detail"] = json!(detail);
            }
            let body = normalize(chat, request(chat, json!([part]))).unwrap();
            assert_eq!(body["input"][0]["content"][0]["detail"], detail);
        }
        for mime in ["image/png", "image/jpeg", "image/webp", "image/gif"] {
            // The gateway validates the envelope, the upstream decodes pixels.
            let url = format!("data:{mime};base64,AQID");
            let body = normalize(chat, request(chat, json!([image(chat, &url)]))).unwrap();
            assert_eq!(body["input"][0]["content"][0]["image_url"], url);
        }
    }
    let body = normalize_chat(json!({"model":"fixture","messages":[
        {"role":"system","content":"instructions"},
        {"role":"user","content":[image(true, PNG)]},
        {"role":"assistant","content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"read","arguments":"{}"}}]},
        {"role":"tool","tool_call_id":"c1","content":"result"}
    ]})).unwrap().body;
    assert_eq!(body["instructions"], "instructions");
    assert_eq!(body["input"][1]["call_id"], "c1");
    assert_eq!(body["input"][2]["output"], "result");
}

#[test]
fn invalid_image_envelopes_are_rejected_without_echoing_data() {
    for chat in [true, false] {
        let mut bad_parts = vec![];
        for url in [
            "",
            "x",
            "https://example.com/private.png",
            "http://127.0.0.1/private.png",
            "file:///C:/private.png",
            "data:image/png;base64,",
            "data:image/png;base64,private!",
            "data:text/plain;base64,AQID",
            "data:image/svg+xml;base64,AQID",
            "data:image/png,AQID",
            "data:image/png;base64,AQ",
            "data:image/png;base64,AQ==\n",
        ] {
            bad_parts.push(image(chat, url));
        }
        for value in [Value::Null, json!(7), json!({})] {
            let mut part = image(chat, PNG);
            if chat {
                part["image_url"]["url"] = value;
            } else {
                part["image_url"] = value;
            }
            bad_parts.push(part);
        }
        for detail in [Value::Null, json!(3), json!("ultra")] {
            let mut part = image(chat, PNG);
            if chat {
                part["image_url"]["detail"] = detail;
            } else {
                part["detail"] = detail;
            }
            bad_parts.push(part);
        }
        let mut unknown = image(chat, PNG);
        unknown["file_id"] = json!("file-private");
        bad_parts.push(unknown);
        if chat {
            let mut unknown = image(true, PNG);
            unknown["image_url"]["extra"] = json!(true);
            bad_parts.push(unknown);
            bad_parts.push(json!({"type":"image_url","image_url":PNG}));
        }
        for part in bad_parts {
            let error = normalize(chat, request(chat, json!([part]))).unwrap_err();
            assert!(!error.to_string().contains("private"));
            assert!(!error.to_string().contains(PNG));
        }
    }
}

#[test]
fn images_do_not_bypass_role_or_tool_history_constraints() {
    for chat in [true, false] {
        for role in ["system", "developer", "assistant"] {
            let mut body = request(chat, json!([image(chat, PNG)]));
            body[if chat { "messages" } else { "input" }][0]["role"] = json!(role);
            assert!(normalize(chat, body).is_err(), "{role}, chat={chat}");
        }
    }
    let chat = json!({"model":"fixture","messages":[
        {"role":"assistant","tool_calls":[{"id":"c1","type":"function","function":{"name":"read","arguments":"{}"}}]},
        {"role":"tool","tool_call_id":"c1","content":[image(true, PNG)]}
    ]});
    assert!(normalize_chat(chat).is_err());
    let responses = json!({"model":"fixture","input":[
        {"type":"function_call","call_id":"c1","name":"read","arguments":"{}"},
        {"type":"function_call_output","call_id":"c1","output":[image(false, PNG)]}
    ]});
    assert!(normalize_responses(responses).is_err());
    for chat in [true, false] {
        let mut body = request(chat, json!([image(chat, PNG)]));
        let items = body[if chat { "messages" } else { "input" }]
            .as_array_mut()
            .unwrap();
        items.insert(0, if chat {
            json!({"role":"assistant","tool_calls":[{"id":"c1","type":"function","function":{"name":"read","arguments":"{}"}}]})
        } else { json!({"type":"function_call","call_id":"c1","name":"read","arguments":"{}"}) });
        assert!(normalize(chat, body).is_err(), "pending call, chat={chat}");
    }
}

#[tokio::test]
async fn images_and_large_requests_reach_http_upstream_with_auth() {
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let received = seen.clone();
    let upstream = Router::new()
        .route("/models", get(|| async { Json(json!({"models":[{"slug":"fixture"}]})) }))
        .route("/responses", post(move |Json(body): Json<Value>| {
            received.lock().unwrap().push(body);
            async {
                let events = [
                    json!({"type":"response.created","response":{"id":"r","model":"fixture","created_at":100,"status":"in_progress"}}),
                    json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"image received"}),
                    json!({"type":"response.completed","response":{"id":"r","model":"fixture","created_at":100,"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"image received"}]}]}})
                ];
                ([("content-type", "text/event-stream")], events.iter().map(|e| format!("data: {e}\n\n")).collect::<String>())
            }
        }))
        // The fake upstream must accept large bodies so this test observes the
        // gateway's behavior rather than Axum's default limit in the fixture.
        .layer(DefaultBodyLimit::disable());
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
    let key = store.create_key("image fixture").await.unwrap();
    let mut gateway = Gateway::start("127.0.0.1:0".parse().unwrap(), store.clone(), service)
        .await
        .unwrap();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    let mut large_records = vec![];
    for chat in [true, false] {
        let url = format!(
            "http://{}/v1/{}",
            gateway.address,
            if chat {
                "chat/completions"
            } else {
                "responses"
            }
        );
        let parts = json!([
            {"type":if chat {"text"} else {"input_text"},"text":"before"},
            image(chat, PNG),
            {"type":if chat {"text"} else {"input_text"},"text":"after"}
        ]);
        let mut body = request(chat, parts);
        for stream in [false, true] {
            body["stream"] = json!(stream);
            let response = http
                .post(&url)
                .bearer_auth(key.value.as_str())
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200, "chat={chat}, stream={stream}");
            if stream {
                let text = response.text().await.unwrap();
                assert!(text.contains("image received"));
                assert!(text.contains(if chat { "[DONE]" } else { "response.completed" }));
            } else {
                let value: Value = response.json().await.unwrap();
                if chat {
                    assert_eq!(value["choices"][0]["message"]["content"], "image received");
                } else {
                    assert_eq!(value["status"], "completed");
                }
            }
            let sent = seen.lock().unwrap().last().unwrap().clone();
            assert_eq!(
                sent["input"][0]["content"],
                json!([
                    {"type":"input_text","text":"before"},
                    {"type":"input_image","image_url":PNG,"detail":"auto"},
                    {"type":"input_text","text":"after"}
                ])
            );
        }
        let before = seen.lock().unwrap().len();
        assert_eq!(
            http.post(&url).json(&body).send().await.unwrap().status(),
            401
        );
        for bad in [
            "http://127.0.0.1/private.png",
            "data:image/png;base64,private!",
        ] {
            let body = request(chat, json!([image(chat, bad)]));
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
            assert!(!error.to_string().contains("private"));
        }
        assert_eq!(seen.lock().unwrap().len(), before);
        let large_image = format!(
            "data:image/png;base64,{}",
            STANDARD.encode(vec![0; 2 * 1024 * 1024])
        );
        let large_text = "large history fixture ".repeat(160_000);
        for (content, expected) in [
            (
                json!([image(chat, &large_image)]),
                json!([{"type":"input_image","image_url":large_image,"detail":"auto"}]),
            ),
            (
                json!(large_text),
                json!([{"type":"input_text","text":large_text}]),
            ),
        ] {
            for stream in [false, true] {
                for chunked in [false, true] {
                    let mut body = request(chat, content.clone());
                    body["stream"] = json!(stream);
                    let encoded = serde_json::to_vec(&body).unwrap();
                    assert!(encoded.len() > 2 * 1024 * 1024);
                    let body_size = encoded.len() as i64;
                    let builder = http
                        .post(&url)
                        .bearer_auth(key.value.as_str())
                        .header("content-type", "application/json");
                    let builder = if chunked {
                        let chunks = encoded
                            .chunks(64 * 1024)
                            .map(|chunk| Ok::<_, std::io::Error>(chunk.to_vec()))
                            .collect::<Vec<_>>();
                        builder.body(reqwest::Body::wrap_stream(futures_util::stream::iter(
                            chunks,
                        )))
                    } else {
                        builder.body(encoded)
                    };
                    let request = builder.build().unwrap();
                    if chunked {
                        assert!(!request.headers().contains_key("content-length"));
                    }
                    let response = http.execute(request).await.unwrap();
                    assert_eq!(
                        response.status(),
                        200,
                        "chat={chat}, stream={stream}, chunked={chunked}"
                    );
                    large_records.push((
                        response.headers()["x-request-id"]
                            .to_str()
                            .unwrap()
                            .to_string(),
                        body_size,
                    ));
                    if stream {
                        let text = response.text().await.unwrap();
                        assert!(text.contains("image received"));
                        assert!(text.contains(if chat { "[DONE]" } else { "response.completed" }));
                    } else {
                        let value: Value = response.json().await.unwrap();
                        if chat {
                            assert_eq!(value["choices"][0]["message"]["content"], "image received");
                        } else {
                            assert_eq!(value["status"], "completed");
                        }
                    }
                    assert_eq!(
                        seen.lock().unwrap().last().unwrap()["input"][0]["content"],
                        expected
                    );
                }
            }
        }
        assert_eq!(seen.lock().unwrap().len(), before + 8);

        // Large inputs still require authentication and valid JSON/content.
        let large_body = request(chat, json!("history ".repeat(400_000)));
        let before_rejected = seen.lock().unwrap().len();
        let response = http.post(&url).json(&large_body).send().await.unwrap();
        assert_eq!(response.status(), 401);
        let response = http
            .post(&url)
            .bearer_auth(key.value.as_str())
            .header("content-type", "application/json")
            .body(format!("{{{}", " ".repeat(3 * 1024 * 1024)))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        let error: Value = response.json().await.unwrap();
        assert_eq!(error["error"]["code"], "invalid_request");
        assert!(!error.to_string().contains("MiB"));
        let response = http
            .post(&url)
            .bearer_auth(key.value.as_str())
            .header("content-type", "text/plain")
            .body(large_body.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 415);
        let mut invalid = large_body;
        invalid["temperature"] = json!(0.2);
        let response = http
            .post(&url)
            .bearer_auth(key.value.as_str())
            .json(&invalid)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        assert_eq!(seen.lock().unwrap().len(), before_rejected);
    }
    assert_eq!(seen.lock().unwrap().len(), 20);
    gateway.stop().await;
    for (id, body_size) in large_records {
        let record = store.request_record(&id).await.unwrap().unwrap();
        assert_eq!(record.body_bytes, Some(body_size));
        assert_eq!(record.outcome, "completed");
        assert_eq!(record.http_status, Some(200));
    }
    task.abort();
}
