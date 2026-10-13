use crate::{
    auth::codex::{self, Service},
    protocol::{
        gateway::{self, Projection},
        sse::Decoder,
    },
    storage::{KeyStatus, Store},
};
use axum::{
    Extension, Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Record {
    pub id: String,
    pub route: String,
    pub model: String,
    pub outcome: String,
    pub elapsed_ms: u128,
}
#[derive(Clone)]
struct Key {
    cancel: CancellationToken,
}
#[derive(Clone)]
struct RequestId(String);
struct Shared {
    store: Store,
    service: Arc<Service>,
    stop: CancellationToken,
    slots: Arc<Semaphore>,
    keys: Mutex<HashMap<String, CancellationToken>>,
    records: Mutex<VecDeque<Record>>,
    address: SocketAddr,
}
pub struct Gateway {
    pub address: SocketAddr,
    shared: Arc<Shared>,
    task: Option<JoinHandle<()>>,
}

pub fn validate_bind(value: &str) -> Result<SocketAddr, String> {
    let address: SocketAddr = value
        .parse()
        .map_err(|_| "请输入明确的 IP:端口，例如 127.0.0.1:8787".to_owned())?;
    if address.port() == 0 || !private(address.ip()) {
        return Err("请选择本机、局域网或个人 VPN 的具体地址；不支持公网或通配监听".into());
    }
    Ok(address)
}
fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
        }
        IpAddr::V6(ip) => ip.is_loopback() || (ip.segments()[0] & 0xfe00 == 0xfc00),
    }
}
impl Gateway {
    pub async fn start(
        address: SocketAddr,
        store: Store,
        service: Arc<Service>,
    ) -> Result<Self, std::io::Error> {
        if !private(address.ip()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "explicit local/private IP required",
            ));
        }
        let listener = tokio::net::TcpListener::bind(address).await?;
        let address = listener.local_addr()?;
        let shared = Arc::new(Shared {
            store,
            service,
            address,
            stop: CancellationToken::new(),
            slots: Arc::new(Semaphore::new(4)),
            keys: Mutex::new(HashMap::new()),
            records: Mutex::new(VecDeque::new()),
        });
        let app = Router::new()
            .route("/v1/models", get(models))
            .route("/v1/chat/completions", post(chat))
            .route("/v1/responses", post(responses))
            .fallback(|| async {
                ApiError::new(404, "not_found", "Unknown API route").into_response()
            })
            .method_not_allowed_fallback(|| async {
                ApiError::new(405, "method_not_allowed", "Unsupported HTTP method").into_response()
            })
            .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
            .layer(middleware::from_fn_with_state(shared.clone(), authenticate))
            .with_state(shared.clone());
        let stop = shared.stop.clone();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(stop.cancelled_owned())
                .await;
        });
        Ok(Self {
            address,
            shared,
            task: Some(task),
        })
    }
    pub fn cancel_key(&self, id: &str) {
        self.shared
            .keys
            .lock()
            .unwrap()
            .entry(id.into())
            .or_default()
            .cancel();
    }
    pub fn records(&self) -> Vec<Record> {
        self.shared
            .records
            .lock()
            .unwrap()
            .iter()
            .rev()
            .cloned()
            .collect()
    }
    pub async fn stop(&mut self) {
        self.shared.stop.cancel();
        if let Some(mut task) = self.task.take()
            && tokio::time::timeout(Duration::from_secs(5), &mut task)
                .await
                .is_err()
        {
            task.abort();
            let _ = task.await;
        }
    }
}
impl Drop for Gateway {
    fn drop(&mut self) {
        self.shared.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}
impl ApiError {
    fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            code,
            message: message.into(),
        }
    }
    fn value(&self) -> Value {
        json!({"error":{"type":if self.status==StatusCode::BAD_REQUEST {"invalid_request_error"} else {"gateway_error"},"code":self.code,"message":self.message}})
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.value())).into_response()
    }
}
impl From<codex::Error> for ApiError {
    fn from(e: codex::Error) -> Self {
        let (status, code) = match e {
            codex::Error::NoAccount => (503, "account_not_ready"),
            codex::Error::AuthRequired => (503, "upstream_auth_required"),
            codex::Error::Model => (404, "model_not_found"),
            codex::Error::Timeout => (504, "upstream_timeout"),
            codex::Error::Http(429) => (429, "upstream_rate_limit"),
            codex::Error::Http(403) => (403, "upstream_forbidden"),
            codex::Error::Http(400) => (400, "upstream_rejected_request"),
            _ => (502, "upstream_error"),
        };
        Self::new(status, code, e.to_string())
    }
}
fn interrupted() -> ApiError {
    ApiError::new(
        502,
        "upstream_stream_interrupted",
        "Upstream did not produce a valid complete stream",
    )
}
fn cancelled() -> ApiError {
    ApiError::new(
        503,
        "request_cancelled",
        "Gateway stopped or client key was revoked",
    )
}
fn request_error(e: gateway::Error) -> ApiError {
    ApiError::new(400, "unsupported_or_invalid_parameter", e.to_string())
}
fn parse_json(v: Result<Json<Value>, JsonRejection>) -> Result<Value, ApiError> {
    v.map(|v| v.0).map_err(|e| {
        ApiError::new(
            e.status().as_u16(),
            "invalid_request",
            "Request must be valid JSON within the 2 MiB limit",
        )
    })
}
async fn authenticate(State(s): State<Arc<Shared>>, mut request: Request, next: Next) -> Response {
    let id = Uuid::new_v4().to_string();
    let result = async {
        // A literal listening address/localhost prevents browser DNS rebinding.
        let host = request
            .headers()
            .get("host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        let expected = s.address.to_string();
        let localhost = format!("localhost:{}", s.address.port());
        if host != expected && !(s.address.ip().is_loopback() && host == localhost) {
            return ApiError::new(
                400,
                "invalid_host",
                "Host must match the configured gateway address",
            )
            .into_response();
        }
        if request.headers().contains_key("origin") {
            return ApiError::new(
                403,
                "browser_origin_denied",
                "Browser origins are not enabled",
            )
            .into_response();
        }
        if s.stop.is_cancelled() {
            return cancelled().into_response();
        }
        let mut auth = request.headers().get_all("authorization").iter();
        let value = auth
            .next()
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split_once(' '))
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
            .map(|(_, v)| v);
        if auth.next().is_some() {
            return ApiError::new(
                401,
                "invalid_gateway_key",
                "A single gateway Bearer key is required",
            )
            .into_response();
        }
        let Some(value) = value else {
            return ApiError::new(401, "invalid_gateway_key", "Gateway Bearer key is required")
                .into_response();
        };
        let key = match s.store.authenticate(value).await {
            Ok(KeyStatus::Valid(id)) => id,
            Ok(KeyStatus::Invalid) => {
                return ApiError::new(401, "invalid_gateway_key", "Invalid gateway key")
                    .into_response();
            }
            Err(_) => {
                return ApiError::new(
                    503,
                    "storage_unavailable",
                    "Local credential store is unavailable",
                )
                .into_response();
            }
        };
        let token = s.keys.lock().unwrap().entry(key).or_default().clone();
        if token.is_cancelled() {
            return ApiError::new(403, "key_revoked", "Gateway key was revoked").into_response();
        }
        request.extensions_mut().insert(Key { cancel: token });
        request.extensions_mut().insert(RequestId(id.clone()));
        next.run(request).await
    }
    .await;
    let mut result = result;
    result
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_str(&id).unwrap());
    result
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    result
}
async fn models(State(s): State<Arc<Shared>>, Extension(key): Extension<Key>) -> Response {
    let Ok(_slot) = s.slots.clone().try_acquire_owned() else {
        return ApiError::new(
            429,
            "gateway_concurrency_limit",
            "At most 4 concurrent requests are allowed",
        )
        .into_response();
    };
    let result = tokio::select! { biased; _=s.stop.cancelled()=>Err(cancelled()),_=key.cancel.cancelled()=>Err(cancelled()), r=s.service.models()=>r.map_err(ApiError::from) };
    match result {
        Ok(models)=>Json(json!({"object":"list","data":models.iter().map(|m|json!({"id":m.slug,"object":"model","created":0,"owned_by":"openai"})).collect::<Vec<_>>()})).into_response(),
        Err(e)=>e.into_response(),
    }
}
async fn chat(
    State(s): State<Arc<Shared>>,
    Extension(key): Extension<Key>,
    Extension(id): Extension<RequestId>,
    body: Result<Json<Value>, JsonRejection>,
) -> Response {
    let request = parse_json(body).and_then(|v| gateway::normalize_chat(v).map_err(request_error));
    match request {
        Ok(r) => generate(s, key, id, r, true).await,
        Err(e) => e.into_response(),
    }
}
async fn responses(
    State(s): State<Arc<Shared>>,
    Extension(key): Extension<Key>,
    Extension(id): Extension<RequestId>,
    body: Result<Json<Value>, JsonRejection>,
) -> Response {
    let request =
        parse_json(body).and_then(|v| gateway::normalize_responses(v).map_err(request_error));
    match request {
        Ok(r) => generate(s, key, id, r, false).await,
        Err(e) => e.into_response(),
    }
}
struct RecordGuard {
    state: Arc<Shared>,
    record: Record,
    start: Instant,
}
impl RecordGuard {
    fn outcome(&mut self, value: impl Into<String>) {
        self.record.outcome = value.into();
    }
}
impl Drop for RecordGuard {
    fn drop(&mut self) {
        self.record.elapsed_ms = self.start.elapsed().as_millis();
        let mut records = self.state.records.lock().unwrap();
        if records.len() == 100 {
            records.pop_front();
        }
        records.push_back(self.record.clone());
    }
}
async fn generate(
    s: Arc<Shared>,
    key: Key,
    id: RequestId,
    request: gateway::Request,
    chat: bool,
) -> Response {
    let ignored = request.ignored_parameters.join(", ");
    let mut warnings = vec![];
    if request.ignored_parameters.iter().any(|parameter| {
        matches!(
            *parameter,
            "max_tokens" | "max_completion_tokens" | "max_output_tokens"
        )
    }) {
        warnings.push("Output token limits are not enforced by the Codex upstream");
    }
    if request
        .ignored_parameters
        .contains(&"messages[].reasoning_content")
    {
        warnings.push("Assistant reasoning_content is not replayed to the Codex upstream");
    }
    let mut response = generate_response(s, key, id, request, chat).await;
    if !ignored.is_empty() {
        response.headers_mut().insert(
            "x-tiny-proxy-ignored-parameters",
            HeaderValue::from_str(&ignored).expect("fixed parameter names are valid ASCII"),
        );
        response.headers_mut().insert(
            "x-tiny-proxy-warning",
            HeaderValue::from_str(&warnings.join("; ")).expect("fixed warnings are valid ASCII"),
        );
    }
    response
}
async fn generate_response(
    s: Arc<Shared>,
    key: Key,
    id: RequestId,
    request: gateway::Request,
    chat: bool,
) -> Response {
    let Ok(slot) = s.slots.clone().try_acquire_owned() else {
        return ApiError::new(
            429,
            "gateway_concurrency_limit",
            "At most 4 concurrent requests are allowed",
        )
        .into_response();
    };
    let mut record = RecordGuard {
        state: s.clone(),
        record: Record {
            id: id.0,
            route: if chat {
                "chat/completions"
            } else {
                "responses"
            }
            .into(),
            model: request.body["model"].as_str().unwrap_or("").into(),
            outcome: "cancelled".into(),
            elapsed_ms: 0,
        },
        start: Instant::now(),
    };
    let upstream = tokio::select! { biased; _=s.stop.cancelled()=>Err(cancelled()),_=key.cancel.cancelled()=>Err(cancelled()),r=s.service.open(&request.body)=>r.map_err(ApiError::from) };
    let response = match upstream {
        Ok(r) => r,
        Err(e) => {
            record.record.outcome = e.code.into();
            return e.into_response();
        }
    };
    if !request.stream {
        let result = tokio::select! { biased; _=s.stop.cancelled()=>Err(cancelled()),_=key.cancel.cancelled()=>Err(cancelled()),r=consume(response,&request,None,chat)=>r };
        return match result {
            Ok(result) => {
                record.record.outcome = result["status"].as_str().unwrap_or("completed").into();
                Json(result).into_response()
            }
            Err(e) => {
                record.record.outcome = e.code.into();
                e.into_response()
            }
        };
    }
    let (sender, receiver) = mpsc::channel(4);
    let task = tokio::spawn(async move {
        let _slot = slot;
        let result = tokio::select! { biased; _=s.stop.cancelled()=>Err(cancelled()),_=key.cancel.cancelled()=>Err(cancelled()),r=consume(response,&request,Some(&sender),chat)=>r };
        match result {
            Ok(result) => record.outcome(result["status"].as_str().unwrap_or("completed")),
            Err(e) => {
                record.outcome(e.code);
                let data = if chat {
                    e.value()
                } else {
                    json!({"type":"error","code":e.code,"message":e.message})
                };
                let _ = send(&sender, encode(&data, chat)).await;
            }
        }
    });
    let receiver = BodyReceiver { receiver, task };
    let stream = stream::unfold(receiver, |mut r| async move {
        r.receiver
            .recv()
            .await
            .map(|bytes| (Ok::<Bytes, std::convert::Infallible>(bytes), r))
    });
    (
        [
            ("content-type", "text/event-stream"),
            ("x-accel-buffering", "no"),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}
struct BodyReceiver {
    receiver: mpsc::Receiver<Bytes>,
    task: JoinHandle<()>,
}
impl Drop for BodyReceiver {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn encode(value: &Value, chat: bool) -> Bytes {
    if chat {
        Bytes::from(format!("data: {value}\n\n"))
    } else {
        Bytes::from(format!(
            "event: {}\ndata: {value}\n\n",
            value["type"].as_str().unwrap_or("error")
        ))
    }
}
async fn send(sender: &mpsc::Sender<Bytes>, bytes: Bytes) -> Result<(), ApiError> {
    tokio::time::timeout(Duration::from_secs(30), sender.send(bytes))
        .await
        .map_err(|_| ApiError::new(504, "slow_consumer", "Client stopped reading the response"))?
        .map_err(|_| cancelled())
}
async fn consume(
    response: reqwest::Response,
    request: &gateway::Request,
    sender: Option<&mpsc::Sender<Bytes>>,
    chat: bool,
) -> Result<Value, ApiError> {
    let mut stream = response.bytes_stream();
    let mut decoder = Decoder::new(1024 * 1024);
    let mut projection = Projection::new(
        request.body["model"].as_str().unwrap_or("").into(),
        request.include_usage,
    );
    let mut terminal_frames = vec![];
    let mut seen_done = false;
    let mut total = 0usize;
    loop {
        let chunk = tokio::time::timeout(Duration::from_secs(90), stream.next())
            .await
            .map_err(|_| ApiError::new(504, "upstream_timeout", "Upstream stream idle timeout"))?;
        let Some(chunk) = chunk else { break };
        let chunk = chunk.map_err(|_| interrupted())?;
        total += chunk.len();
        if total > 16 * 1024 * 1024 {
            return Err(ApiError::new(
                502,
                "response_too_large",
                "Response exceeded the 16 MiB budget",
            ));
        }
        for event in decoder.push(&chunk).map_err(|_| interrupted())? {
            if event.data == "[DONE]" {
                if seen_done || projection.terminal().is_none() {
                    return Err(interrupted());
                }
                seen_done = true;
                continue;
            }
            if seen_done {
                return Err(interrupted());
            }
            let value: Value = serde_json::from_str(&event.data).map_err(|_| interrupted())?;
            if event.event != "message" && Some(event.event.as_str()) != value["type"].as_str() {
                return Err(interrupted());
            }
            let chunks = projection.accept(&value).map_err(|_| interrupted())?;
            if let Some(sender) = sender {
                let frames = if chat {
                    chunks.iter().map(|v| encode(v, true)).collect::<Vec<_>>()
                } else {
                    vec![encode(&value, false)]
                };
                if projection.terminal().is_some() {
                    terminal_frames.extend(frames);
                } else {
                    for frame in frames {
                        send(sender, frame).await?;
                    }
                }
            }
        }
    }
    decoder.finish().map_err(|_| interrupted())?;
    let result = if chat {
        projection.finish().map_err(|_| interrupted())?
    } else {
        projection.terminal().cloned().ok_or_else(interrupted)?
    };
    if let Some(sender) = sender {
        for frame in terminal_frames {
            send(sender, frame).await?;
        }
        if chat {
            send(sender, Bytes::from_static(b"data: [DONE]\n\n")).await?;
        }
    }
    Ok(result)
}

/// Same bounded parser as the public API, used by the desktop's explicit probe.
pub async fn test_text(
    service: &Arc<Service>,
    model: &str,
) -> Result<crate::auth::probe::ProbeResult, String> {
    let account = service.active_id().await.map_err(|e| e.to_string())?;
    test_text_for(service, &account, model).await
}

pub async fn test_text_for(
    service: &Arc<Service>,
    account: &str,
    model: &str,
) -> Result<crate::auth::probe::ProbeResult, String> {
    let request = gateway::normalize_responses(
        json!({"model":model,"input":"Reply with exactly: Tiny Proxy connection OK"}),
    )
    .map_err(|e| e.to_string())?;
    let response = service
        .open_for(account, &request.body)
        .await
        .map_err(|e| e.to_string())?;
    let result = consume(response, &request, None, true)
        .await
        .map_err(|e| e.message)?;
    if result["choices"][0]["finish_reason"] != "stop" {
        return Err("文本探测未正常完成".into());
    }
    Ok(crate::auth::probe::ProbeResult {
        text: result["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .into(),
        input_tokens: result["usage"]["prompt_tokens"].as_u64(),
        output_tokens: result["usage"]["completion_tokens"].as_u64(),
    })
}
