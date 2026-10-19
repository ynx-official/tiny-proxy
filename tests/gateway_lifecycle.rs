use axum::{
    Json, Router,
    body::{Body, Bytes},
    response::IntoResponse,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tiny_proxy::{
    auth::codex::{Service, now},
    gateway::Gateway,
    platform::protection,
    storage::Store,
};

struct Open(Arc<AtomicUsize>);
impl Drop for Open {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
fn frame(v: Value) -> Bytes {
    Bytes::from(format!("data: {v}\n\n"))
}
fn created() -> Value {
    json!({"type":"response.created","response":{"id":"r","created_at":100,"model":"fixture","status":"in_progress"}})
}

fn diagnostic_body(mode: &str) -> Option<Body> {
    let mut bytes = frame(created()).to_vec();
    let tail = match mode {
        "diagnostic-eof" => Vec::new(),
        "diagnostic-truncated" => b"data: {\"private-field\":\"private reply\"}".to_vec(),
        "diagnostic-json" => b"data: {private reply\n\n".to_vec(),
        "diagnostic-utf8" => b"data: \xff\n\n".to_vec(),
        "diagnostic-event-name" => {
            let mut tail = b"event: response.completed\n".to_vec();
            tail.extend_from_slice(&frame(created()));
            tail
        }
        "diagnostic-unknown-event" => frame(json!({"type":"private-event-name","private-field":"private reply"})).to_vec(),
        "diagnostic-completed" => frame(json!({"type":"response.completed","response":{"id":"r","status":"in_progress","output":[],"private-field":"private reply"}})).to_vec(),
        "diagnostic-tool" => frame(json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":{"private-field":"private arguments"}})).to_vec(),
        "diagnostic-early-done" => b"data: [DONE]\n\n".to_vec(),
        "diagnostic-oversized" => {
            let mut tail = b"data: ".to_vec();
            tail.resize(1024 * 1024 + 1, b'x');
            tail
        }
        "diagnostic-network" => {
            let stream = stream::iter([Ok::<_, std::io::Error>(Bytes::from(bytes))])
                .chain(stream::once(async {
                    // Deliver response headers and the first event before the reset.
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Err(std::io::Error::other("private transport failure"))
                }));
            return Some(Body::from_stream(stream));
        }
        _ => return None,
    };
    bytes.extend_from_slice(&tail);
    Some(Body::from(bytes))
}

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
    gateway: Gateway,
    task: tokio::task::JoinHandle<()>,
    key: tiny_proxy::storage::CreatedKey,
    active: Arc<AtomicUsize>,
}
impl Fixture {
    async fn new() -> Self {
        let active = Arc::new(AtomicUsize::new(0));
        let count = active.clone();
        let app=Router::new().route("/models",get(||async{Json(json!({"models":[{"slug":"fixture"}]}))}))
            .route("/responses",post(move|Json(body):Json<Value>|{let count=count.clone();async move{
                let mode=body["input"][0]["content"][0]["text"].as_str().or_else(||body["input"][0]["content"].as_str()).unwrap_or("normal");
                if let Some(body)=diagnostic_body(mode) {return ([("content-type","text/event-stream")],body).into_response();}
                if mode=="401" {return (axum::http::StatusCode::UNAUTHORIZED,"secret upstream body").into_response();}
                if mode=="bad" {return ([("content-type","text/event-stream")],Body::from(frame(created()))).into_response();}
                if mode=="failed-event" {return ([("content-type","text/event-stream")],Body::from([created(),json!({"type":"response.failed","response":{"id":"r","status":"failed","error":{"message":"private upstream failure detail"}}})].into_iter().flat_map(|v|frame(v).to_vec()).collect::<Vec<_>>())).into_response();}
                if mode.starts_with("cache-") {
                    let mut usage=json!({"input_tokens":100,"output_tokens":1});
                    let cached=match mode {"cache-hit"=>Some(json!(80)),"cache-zero"=>Some(json!(0)),"cache-negative"=>Some(json!(-1)),"cache-text"=>Some(json!("80")),"cache-float"=>Some(json!(80.5)),"cache-null"=>Some(Value::Null),_=>None};
                    if let Some(cached)=cached {usage["input_tokens_details"]=json!({"cached_tokens":cached});}
                    let terminal=json!({"type":"response.completed","response":{"id":"r","created_at":100,"model":"fixture","status":"completed","output":[],"usage":usage}});
                    return ([("content-type","text/event-stream")],Body::from([created(),terminal].into_iter().flat_map(|v|frame(v).to_vec()).collect::<Vec<_>>())).into_response();
                }
                if mode=="terminal-open" || mode=="terminal-partial-tail" {
                    let mut terminal=frame(json!({"type":"response.completed","response":{"id":"r","created_at":100,"model":"fixture","status":"completed","output":[],"usage":{"input_tokens":2,"output_tokens":1}}})).to_vec();
                    if mode=="terminal-partial-tail" {terminal.extend_from_slice(b"data: [DO");}
                    let stream=stream::iter([frame(created()),Bytes::from(terminal)].map(Ok::<_,std::convert::Infallible>))
                        .chain(stream::pending());
                    return ([("content-type","text/event-stream")],Body::from_stream(stream)).into_response();
                }
                if mode=="tools" {
                    let call=json!({"type":"function_call","id":"item1","call_id":"call1","name":"read_file","arguments":"{\"path\":\"a.rs\"}"});
                    let data=[created(),json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"item1","call_id":"call1","name":"read_file","arguments":""}}),json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"path\":"}),json!({"type":"response.completed","response":{"id":"r","created_at":100,"model":"fixture","status":"completed","output":[call]}})];
                    return ([("content-type","text/event-stream")],Body::from(data.into_iter().flat_map(|v|frame(v).to_vec()).collect::<Vec<_>>())).into_response();
                }
                count.fetch_add(1,Ordering::SeqCst);let open=Open(count);
                let stream=stream::unfold((0,open),|(n,open)|async move{
                    if n>0 {tokio::time::sleep(Duration::from_millis(50)).await;}
                    let v=if n==0 {created()} else {json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"x"})};
                    Some((Ok::<_,std::convert::Infallible>(frame(v)),(n+1,open)))
                });
                ([("content-type","text/event-stream")],Body::from_stream(stream)).into_response()
            }}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("app.db")).await.unwrap();
        let token=format!("e30.{}.sig",URL_SAFE_NO_PAD.encode(json!({"sub":"fixture-sub","exp":now()+3600,"https://api.openai.com/auth":{"chatgpt_account_id":"fixture-account"}}).to_string()));
        let bundle = json!({"access_token":token,"refresh_token":"fixture","account_id":"fixture-account","subject":"fixture-sub","expires_at":now()+3600});
        store
            .save_codex_credentials(
                0,
                &protection::protect(bundle.to_string().as_bytes()).unwrap(),
                now() + 3600,
            )
            .await
            .unwrap();
        let service = Service::for_test(store.clone(), addr).unwrap();
        let key = store.create_key("fixture").await.unwrap();
        let gateway = Gateway::start("127.0.0.1:0".parse().unwrap(), store.clone(), service)
            .await
            .unwrap();
        Self {
            _dir: dir,
            store,
            gateway,
            task,
            key,
            active,
        }
    }
    async fn post(&self, mode: &str) -> reqwest::Response {
        reqwest::Client::new().post(format!("http://{}/v1/chat/completions",self.gateway.address)).bearer_auth(self.key.value.as_str())
            .json(&json!({"model":"fixture","messages":[{"role":"user","content":mode}],"stream":true})).send().await.unwrap()
    }
    async fn drained(&self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.active.load(Ordering::SeqCst) > 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn upstream_cache_details_survive_both_apis_and_stream_modes() {
    let f = Fixture::new().await;
    for (mode, expected) in [
        ("cache-hit", Some(80)),
        ("cache-zero", Some(0)),
        ("cache-missing", None),
        ("cache-null", None),
        ("cache-negative", None),
        ("cache-text", None),
        ("cache-float", None),
    ] {
        for chat in [true, false] {
            for stream in [true, false] {
                let route = if chat {
                    "chat/completions"
                } else {
                    "responses"
                };
                let body = if chat {
                    json!({"model":"fixture","messages":[{"role":"user","content":mode}],"stream":stream})
                } else {
                    json!({"model":"fixture","input":mode,"stream":stream})
                };
                let response = reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap()
                    .post(format!("http://{}/v1/{route}", f.gateway.address))
                    .bearer_auth(f.key.value.as_str())
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                assert!(response.status().is_success());
                let id = response.headers()["x-request-id"]
                    .to_str()
                    .unwrap()
                    .to_owned();
                response.text().await.unwrap();
                let saved = f.store.request_record(&id).await.unwrap().unwrap();
                assert_eq!(
                    saved.cached_input_tokens, expected,
                    "{mode}, chat={chat}, stream={stream}"
                );
                assert_eq!(saved.input_tokens, Some(100));
                assert_eq!(saved.outcome, "completed");
            }
        }
    }
    let window = tiny_proxy::overview::OverviewWindow::at(
        tiny_proxy::overview::OverviewPeriod::Last24Hours,
        chrono::Utc::now() + chrono::Duration::seconds(1),
    )
    .unwrap();
    let snapshot = f.store.overview(window, &[]).await.unwrap();
    assert_eq!(snapshot.totals.requests, 28);
    assert_eq!(snapshot.totals.cache_samples, 8);
    assert_eq!(snapshot.totals.cached_input_tokens, Some(320));
    assert_eq!(snapshot.totals.cache_input_tokens, Some(800));
    assert_eq!(snapshot.totals.cache_hit_rate(), Some(0.4));
}

#[tokio::test]
async fn a_valid_terminal_finishes_without_waiting_for_the_upstream_socket_to_close() {
    let f = Fixture::new().await;
    for mode in ["terminal-open", "terminal-partial-tail"] {
        for chat in [true, false] {
            for stream in [true, false] {
                tokio::time::timeout(Duration::from_secs(2), async {
                    let route = if chat { "chat/completions" } else { "responses" };
                    let body = if chat { json!({"model":"fixture","messages":[{"role":"user","content":mode}],"stream":stream}) } else { json!({"model":"fixture","input":mode,"stream":stream}) };
                    let response = reqwest::Client::new().post(format!("http://{}/v1/{route}", f.gateway.address))
                        .bearer_auth(f.key.value.as_str()).json(&body).send().await.unwrap();
                    let id = response.headers()["x-request-id"].to_str().unwrap().to_owned();
                    let text = response.text().await.unwrap();
                    assert!(text.contains(if chat && stream { "[DONE]" } else if chat { "completion_tokens" } else { "completed" }));
                    let saved = f.store.request_record(&id).await.unwrap().unwrap();
                    assert_eq!(saved.outcome, "completed");
                    assert_eq!(saved.input_tokens, Some(2));
                    assert_eq!(saved.output_tokens, Some(1));
                    assert!(saved.ended_at.is_some());
                }).await.expect("valid terminal was held until upstream EOF");
            }
        }
    }
}

#[tokio::test]
async fn live_metadata_and_disconnect_are_persisted_without_request_contents() {
    let mut f = Fixture::new().await;
    let response = f.post("private-user-prompt-should-not-be-saved").await;
    let id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let mut stream = response.bytes_stream();
    stream.next().await.unwrap().unwrap();
    let live = f
        .gateway
        .live_records()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    assert_eq!(live.outcome, "running");
    assert!(live.ended_at.is_none());
    assert_eq!(live.key_id.as_deref(), Some(f.key.id.as_str()));
    assert_eq!(live.stream, Some(true));
    assert_eq!(live.message_count, Some(1));
    assert!(live.body_bytes.unwrap() > 0);
    assert!(live.first_byte_at.is_some());
    assert!(f.store.request_record(&id).await.unwrap().is_some());
    drop(stream);
    f.drained().await;
    f.gateway.stop().await;
    let saved = f.store.request_record(&id).await.unwrap().unwrap();
    assert_eq!(saved.outcome, "cancelled");
    assert!(saved.ended_at.unwrap() >= saved.started_at);
    assert_eq!(
        f.store.failure_records(0, 100).await.unwrap()[0].code,
        "client_disconnected"
    );
    f.store.close().await;
    let raw = std::fs::read(f._dir.path().join("app.db")).unwrap();
    for secret in [
        "private-user-prompt-should-not-be-saved",
        f.key.value.as_str(),
    ] {
        assert!(!raw.windows(secret.len()).any(|w| w == secret.as_bytes()));
    }
}

#[tokio::test]
async fn validation_auth_and_stream_failures_have_separate_exception_rows() {
    let mut f = Fixture::new().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{}/v1/responses", f.gateway.address))
        .bearer_auth(f.key.value.as_str())
        .json(&json!({"model":"fixture","input":42,"private-field-name":"private input"}))
        .send()
        .await
        .unwrap();
    let id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(response.status(), 400);
    let r = f.store.request_record(&id).await.unwrap().unwrap();
    assert_eq!(r.http_status, Some(400));
    assert_eq!(r.phase, "validating");
    assert_eq!(
        f.store.failure_records(0, 100).await.unwrap()[0].code,
        "unsupported_or_invalid_parameter"
    );
    let response = client
        .get(format!("http://{}/v1/models", f.gateway.address))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    assert!(
        f.store
            .failure_records(0, 100)
            .await
            .unwrap()
            .iter()
            .any(|e| e.code == "invalid_gateway_key")
    );
    let response = f.post("failed-event").await;
    let id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let text = response.text().await.unwrap();
    assert!(text.contains("upstream_failed"));
    assert!(!text.contains("[DONE]"));
    f.gateway.stop().await;
    let r = f.store.request_record(&id).await.unwrap().unwrap();
    assert_eq!(r.outcome, "failed");
    assert_eq!(r.http_status, Some(200));
    assert_eq!(r.upstream_status, Some(200));
    let errors = f.store.failure_records(0, 100).await.unwrap();
    assert_eq!(errors.len(), 3);
    assert!(errors.iter().all(|e| !e.message.contains("private")));
    assert_eq!(
        errors.iter().find(|e| e.request_id == id).unwrap().code,
        "upstream_failed"
    );
}

#[tokio::test]
async fn parameter_error_response_and_persisted_diagnostic_include_the_same_known_path() {
    let mut f = Fixture::new().await;
    let client = reqwest::Client::new();
    for (route, body, path, rule) in [
        (
            "chat/completions",
            json!({"model":"fixture","messages":{},"private-field-name":"private prompt"}),
            "messages",
            "数组",
        ),
        (
            "chat/completions",
            json!({"model":"fixture","messages":[{"role":"assistant","content":"private reply","tool_calls":[{"function":{"name":"lookup","arguments":"{}"}}]}]}),
            "messages[0].tool_calls[0].id",
            "非空字符串",
        ),
        (
            "responses",
            json!({"model":"fixture","input":42,"private-field-name":"private input","max_output_tokens":"1024"}),
            "input",
            "输入对象",
        ),
        (
            "responses",
            json!({"model":"fixture","input":[{"role":"user","content":[{"type":"input_image","image_url":"https://example.test/private-image"}]}]}),
            "input[0].content[0].image_url",
            "Base64",
        ),
    ] {
        let response = client
            .post(format!("http://{}/v1/{route}", f.gateway.address))
            .bearer_auth(f.key.value.as_str())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        let id = response.headers()["x-request-id"]
            .to_str()
            .unwrap()
            .to_owned();
        let error: Value = response.json().await.unwrap();
        assert_eq!(error["error"]["code"], "unsupported_or_invalid_parameter");
        assert_eq!(error["error"]["param"], path);
        let record = f.store.request_record(&id).await.unwrap().unwrap();
        assert_eq!(record.phase, "validating");
        assert_eq!(record.upstream_status, None);
        assert_eq!(record.first_byte_at, None);
        let failure = f.store.failure_record(&id).await.unwrap().unwrap();
        assert!(failure.message.contains(path));
        assert!(failure.message.contains(rule));
        assert!(failure.message.contains("建议"));
        assert_eq!(f.active.load(Ordering::SeqCst), 0);
    }
    for (route, mut body) in [
        (
            "chat/completions",
            json!({"model":"fixture","messages":[{"role":"user","content":"private prompt"}]}),
        ),
        (
            "responses",
            json!({"model":"fixture","input":"private input"}),
        ),
    ] {
        body["private-field-name"] = json!("private field value");
        body["model"] = Value::Null;
        let response = client
            .post(format!("http://{}/v1/{route}", f.gateway.address))
            .bearer_auth(f.key.value.as_str())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        let id = response.headers()["x-request-id"]
            .to_str()
            .unwrap()
            .to_owned();
        let error: Value = response.json().await.unwrap();
        assert_eq!(error["error"]["param"], "model");
        let failure = f.store.failure_record(&id).await.unwrap().unwrap();
        assert!(failure.message.contains("model"));
        assert!(!failure.message.contains("private-field-name"));
    }
    f.gateway.stop().await;
    f.store.close().await;
    let raw = std::fs::read(f._dir.path().join("app.db")).unwrap();
    for secret in [
        "private prompt",
        "private reply",
        "private input",
        "private-image",
        "private-field-name",
        "private field value",
        f.key.value.as_str(),
    ] {
        assert!(!raw.windows(secret.len()).any(|w| w == secret.as_bytes()));
    }
}

#[tokio::test]
async fn stream_failure_diagnostics_identify_the_branch_without_saving_upstream_payloads() {
    let mut f = Fixture::new().await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for (mode, reason) in [
        (
            "diagnostic-eof",
            "连接已结束，但未收到有效的 response.completed/response.incomplete 终态",
        ),
        ("diagnostic-truncated", "连接在 SSE 事件尚未完整接收时结束"),
        ("diagnostic-json", "SSE data 不是有效 JSON"),
        ("diagnostic-utf8", "SSE 事件包含无效 UTF-8"),
        ("diagnostic-event-name", "SSE event 名称与 JSON type 不一致"),
        ("diagnostic-unknown-event", "收到未支持或缺失类型的上游事件"),
        (
            "diagnostic-completed",
            "response.completed 终态未通过完整性校验",
        ),
        ("diagnostic-tool", "函数工具调用事件未通过完整性校验"),
        ("diagnostic-early-done", "在有效终态之前收到 [DONE]"),
        ("diagnostic-oversized", "SSE 单事件超过 1 MiB 限制"),
        ("diagnostic-network", "读取上游响应时发生网络或传输错误"),
    ] {
        for chat in [true, false] {
            for stream in [true, false] {
                let route = if chat {
                    "chat/completions"
                } else {
                    "responses"
                };
                let body = if chat {
                    json!({"model":"fixture","messages":[{"role":"user","content":mode}],"stream":stream})
                } else {
                    json!({"model":"fixture","input":mode,"stream":stream})
                };
                let response = client
                    .post(format!("http://{}/v1/{route}", f.gateway.address))
                    .bearer_auth(f.key.value.as_str())
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status().as_u16(), if stream { 200 } else { 502 });
                let id = response.headers()["x-request-id"]
                    .to_str()
                    .unwrap()
                    .to_owned();
                let text = response.text().await.unwrap();
                let error: Value = if stream {
                    text.lines()
                        .filter_map(|line| line.strip_prefix("data: "))
                        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
                        .find(|event| event.get("error").is_some() || event["type"] == "error")
                        .expect("stream must contain an error event")
                } else {
                    serde_json::from_str(&text).unwrap()
                };
                let error = error.get("error").unwrap_or(&error);
                assert_eq!(error["code"], "upstream_stream_interrupted");
                let message = error["message"].as_str().unwrap();
                assert!(message.contains(reason), "{mode}: {message}");
                let record = f.store.request_record(&id).await.unwrap().unwrap();
                assert_eq!(record.outcome, "failed");
                assert_eq!(record.upstream_status, Some(200));
                assert!(record.first_output_at.is_none());
                let failure = f.store.failure_record(&id).await.unwrap().unwrap();
                assert_eq!(failure.code, "upstream_stream_interrupted");
                assert_eq!(failure.http_status, 502);
                assert_eq!(failure.message, message);
                assert!(!text.contains("data: [DONE]"));
                assert!(!text.contains("private"));
            }
        }
    }
    f.gateway.stop().await;
    f.store.close().await;
    let database = std::fs::read(f._dir.path().join("app.db")).unwrap();
    for secret in [
        "private-field",
        "private reply",
        "private-event-name",
        "private arguments",
        "private transport failure",
        f.key.value.as_str(),
    ] {
        assert!(
            !database
                .windows(secret.len())
                .any(|bytes| bytes == secret.as_bytes())
        );
    }
}

#[tokio::test]
async fn http_auth_failure_and_truncated_stream_never_look_successful() {
    let f = Fixture::new().await;
    let response = f.post("401").await;
    assert_eq!(response.status(), 503);
    assert!(
        !response
            .text()
            .await
            .unwrap()
            .contains("secret upstream body")
    );
    let response = f.post("bad").await;
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    assert!(text.contains("upstream_stream_interrupted"));
    assert!(!text.contains("[DONE]"));
}
#[tokio::test]
async fn tool_arguments_are_sent_as_chat_deltas_and_finish_as_tool_calls() {
    let f = Fixture::new().await;
    let response = f.post("tools").await;
    let text = response.text().await.unwrap();
    assert!(text.contains("read_file"));
    assert!(text.contains("tool_calls"));
    assert!(text.ends_with("data: [DONE]\n\n"));
    let mut args = String::new();
    for line in text.lines().filter_map(|l| l.strip_prefix("data: ")) {
        if let Ok(v) = serde_json::from_str::<Value>(line)
            && let Some(s) =
                v["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str()
        {
            args.push_str(s);
        }
    }
    assert_eq!(args, "{\"path\":\"a.rs\"}");
}
#[tokio::test]
async fn revoking_one_key_keeps_another_keys_stream_running() {
    let mut f = Fixture::new().await;
    let other = f.store.create_key("other-client").await.unwrap();
    let response = f.post("slow").await;
    let other_response = reqwest::Client::new()
        .post(format!("http://{}/v1/chat/completions", f.gateway.address))
        .bearer_auth(other.value.as_str())
        .json(
            &json!({"model":"fixture","messages":[{"role":"user","content":"slow"}],"stream":true}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(other_response.status(), 200);
    let mut other_stream = other_response.bytes_stream();
    other_stream.next().await.unwrap().unwrap();
    assert_eq!(f.active.load(Ordering::SeqCst), 2);

    f.store.revoke_key(&f.key.id).await.unwrap();
    f.gateway.cancel_key(&f.key.id);
    let cancelled = tokio::time::timeout(Duration::from_secs(5), response.text())
        .await
        .unwrap()
        .unwrap();
    assert!(cancelled.contains("request_cancelled"));
    assert!(!cancelled.contains("[DONE]"));
    tokio::time::timeout(Duration::from_secs(5), async {
        while f.active.load(Ordering::SeqCst) != 1 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), other_stream.next())
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    assert_eq!(f.store.list_keys().await.unwrap()[0].id, other.id);
    drop(other_stream);
    f.drained().await;
    f.gateway.stop().await;
}

#[tokio::test]
async fn disconnect_revocation_and_stop_release_upstream_streams() {
    let mut f = Fixture::new().await;
    let mut stream = f.post("slow").await.bytes_stream();
    stream.next().await.unwrap().unwrap();
    drop(stream);
    f.drained().await;
    let response = f.post("slow").await;
    f.store.revoke_key(&f.key.id).await.unwrap();
    f.gateway.cancel_key(&f.key.id);
    let result = tokio::time::timeout(Duration::from_secs(5), response.text())
        .await
        .unwrap()
        .unwrap();
    assert!(result.contains("request_cancelled"));
    assert!(!result.contains("[DONE]"));
    f.drained().await;
    assert!(f.store.list_keys().await.unwrap().is_empty());
    assert!(f.store.reveal_key(&f.key.id).await.is_err());
    assert_eq!(f.post("slow").await.status(), 401);
    assert_eq!(f.active.load(Ordering::SeqCst), 0);
    f.key = f.store.create_key("replacement").await.unwrap();
    let response = f.post("slow").await;
    f.gateway.stop().await;
    let text = tokio::time::timeout(Duration::from_secs(5), response.text())
        .await
        .unwrap()
        .unwrap();
    assert!(!text.contains("[DONE]"));
    f.drained().await;
}
