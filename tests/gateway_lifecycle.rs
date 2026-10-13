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
                if mode=="401" {return (axum::http::StatusCode::UNAUTHORIZED,"secret upstream body").into_response();}
                if mode=="bad" {return ([("content-type","text/event-stream")],Body::from(frame(created()))).into_response();}
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
