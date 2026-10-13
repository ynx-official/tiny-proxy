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
    extract::{DefaultBodyLimit, Request, State, rejection::BytesRejection},
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

mod journal;
pub use crate::storage::RequestRecord as Record;
use journal::{FailureInfo, Guard, Journal, Trace, now_ms};
#[derive(Clone)]
struct Streaming;
#[derive(Clone)]
struct Key {
    cancel: CancellationToken,
}
struct Shared {
    store: Store,
    service: Arc<Service>,
    stop: CancellationToken,
    slots: Arc<Semaphore>,
    keys: Mutex<HashMap<String, CancellationToken>>,
    journal: Arc<Journal>,
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
        let journal = Journal::new(store.clone());
        let shared = Arc::new(Shared {
            store,
            service,
            address,
            stop: CancellationToken::new(),
            slots: Arc::new(Semaphore::new(4)),
            keys: Mutex::new(HashMap::new()),
            journal,
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
            // Explicitly disable Axum's default limit: removing this layer would
            // still reject Bytes bodies larger than 2 MiB.
            .layer(DefaultBodyLimit::disable())
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
    pub fn live_records(&self) -> Vec<Record> {
        self.shared.journal.live_records()
    }
    pub fn log_write_failures(&self) -> u64 {
        self.shared.journal.write_failures()
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
        self.shared.journal.flush().await;
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
        let info = FailureInfo::new(self.code, self.status.as_u16());
        let mut response = (self.status, Json(self.value())).into_response();
        response.extensions_mut().insert(info);
        response
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
fn parse_json(v: Result<Bytes, BytesRejection>, trace: &Trace) -> Result<Value, ApiError> {
    let bytes = v.map_err(|e| {
        ApiError::new(
            e.status().as_u16(),
            "invalid_request",
            "Request body could not be read",
        )
    })?;
    trace.update(|r| r.body_bytes = Some(bytes.len() as i64));
    let value = serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::new(400, "invalid_request", "Request must be valid JSON"))?;
    trace.metadata(&value, bytes.len());
    trace.phase("validating");
    Ok(value)
}
/// Finish an ordinary rejected upload before releasing its HTTP/1 connection.
/// Dropping a still-uploading body can reset the Windows socket before the
/// client receives 401/415. Discard chunks without parsing or retaining them;
/// bound both bytes and time so an unauthenticated slow sender cannot hold us.
async fn early_rejection(request: Request, error: ApiError, stop: &CancellationToken) -> Response {
    const MAX_DISCARD: u64 = 8 * 1024 * 1024;
    let expect_continue = request.headers().contains_key("expect");
    let oversized = request
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|size| size > MAX_DISCARD);
    let drained = if expect_continue || oversized {
        false
    } else {
        let mut stream = request.into_body().into_data_stream();
        let discard = async {
            let mut bytes = 0u64;
            while let Some(chunk) = stream.next().await {
                let Ok(chunk) = chunk else {
                    return false;
                };
                bytes = bytes.saturating_add(chunk.len() as u64);
                if bytes > MAX_DISCARD {
                    return false;
                }
            }
            true
        };
        tokio::select! { biased; _=stop.cancelled()=>false, result=tokio::time::timeout(Duration::from_secs(2),discard)=>result.unwrap_or(false) }
    };
    let mut response = error.into_response();
    if !drained {
        response
            .headers_mut()
            .insert("connection", HeaderValue::from_static("close"));
    }
    response
}

async fn authenticate(State(s): State<Arc<Shared>>, mut request: Request, next: Next) -> Response {
    let id = Uuid::new_v4().to_string();
    // Never retain query strings or arbitrary unknown paths: they can contain secrets.
    let route = match request.uri().path() {
        "/v1/models" => "/v1/models",
        "/v1/chat/completions" => "/v1/chat/completions",
        "/v1/responses" => "/v1/responses",
        _ => "unknown",
    };
    let method = match request.method().as_str() {
        "GET" => "GET",
        "POST" => "POST",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        "PATCH" => "PATCH",
        "OPTIONS" => "OPTIONS",
        "HEAD" => "HEAD",
        _ => "OTHER",
    };
    let trace = s
        .journal
        .begin(Record::new(
            id.clone(),
            method.into(),
            route.into(),
            now_ms(),
        ))
        .await;
    let mut guard = Guard(trace.clone(), true);
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
            return early_rejection(
                request,
                ApiError::new(
                    400,
                    "invalid_host",
                    "Host must match the configured gateway address",
                ),
                &s.stop,
            )
            .await;
        }
        if request.headers().contains_key("origin") {
            return early_rejection(
                request,
                ApiError::new(
                    403,
                    "browser_origin_denied",
                    "Browser origins are not enabled",
                ),
                &s.stop,
            )
            .await;
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
            return early_rejection(
                request,
                ApiError::new(
                    401,
                    "invalid_gateway_key",
                    "A single gateway Bearer key is required",
                ),
                &s.stop,
            )
            .await;
        }
        let Some(value) = value else {
            return early_rejection(
                request,
                ApiError::new(401, "invalid_gateway_key", "Gateway Bearer key is required"),
                &s.stop,
            )
            .await;
        };
        let key = match s.store.authenticate(value).await {
            Ok(KeyStatus::Valid(id)) => id,
            Ok(KeyStatus::Invalid) => {
                return early_rejection(
                    request,
                    ApiError::new(401, "invalid_gateway_key", "Invalid gateway key"),
                    &s.stop,
                )
                .await;
            }
            Err(_) => {
                return early_rejection(
                    request,
                    ApiError::new(
                        503,
                        "storage_unavailable",
                        "Local credential store is unavailable",
                    ),
                    &s.stop,
                )
                .await;
            }
        };
        trace.update(|r| r.key_id = Some(key.clone()));
        let account = s.service.active_id().await.ok();
        trace.update(|r| r.account_id = account);
        let token = s.keys.lock().unwrap().entry(key).or_default().clone();
        if token.is_cancelled() {
            return early_rejection(
                request,
                ApiError::new(403, "key_revoked", "Gateway key was revoked"),
                &s.stop,
            )
            .await;
        }
        request.extensions_mut().insert(Key { cancel: token });
        request.extensions_mut().insert(trace.clone());
        trace.phase("reading_request");
        if request.method() == axum::http::Method::POST
            && matches!(route, "/v1/chat/completions" | "/v1/responses")
        {
            let media = request
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if !(media == "application/json"
                || media.starts_with("application/") && media.ends_with("+json"))
            {
                return early_rejection(
                    request,
                    ApiError::new(
                        415,
                        "invalid_request",
                        "Content-Type must be application/json",
                    ),
                    &s.stop,
                )
                .await;
            }
        }
        next.run(request).await
    }
    .await;
    let mut result = result;
    trace.update(|r| r.http_status = Some(result.status().as_u16().into()));
    if result.extensions().get::<Streaming>().is_some() {
        guard.1 = false;
    } else {
        trace
            .finish(result.extensions().get::<FailureInfo>().cloned())
            .await;
    }
    result
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_str(&id).unwrap());
    result
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    result
}
async fn models(
    State(s): State<Arc<Shared>>,
    Extension(key): Extension<Key>,
    Extension(trace): Extension<Trace>,
) -> Response {
    trace.phase("waiting_upstream");
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
    Extension(trace): Extension<Trace>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let request =
        parse_json(body, &trace).and_then(|v| gateway::normalize_chat(v).map_err(request_error));
    match request {
        Ok(r) => generate(s, key, trace, r, true).await,
        Err(e) => e.into_response(),
    }
}
async fn responses(
    State(s): State<Arc<Shared>>,
    Extension(key): Extension<Key>,
    Extension(trace): Extension<Trace>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let request = parse_json(body, &trace)
        .and_then(|v| gateway::normalize_responses(v).map_err(request_error));
    match request {
        Ok(r) => generate(s, key, trace, r, false).await,
        Err(e) => e.into_response(),
    }
}
async fn generate(
    s: Arc<Shared>,
    key: Key,
    trace: Trace,
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
    let mut response = generate_response(s, key, trace, request, chat).await;
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
    trace: Trace,
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
    trace.phase("waiting_upstream");
    let upstream = tokio::select! { biased; _=s.stop.cancelled()=>Err(cancelled()),_=key.cancel.cancelled()=>Err(cancelled()),r=s.service.open(&request.body)=>r.map_err(ApiError::from) };
    let response = match upstream {
        Ok(r) => r,
        Err(e) => {
            return e.into_response();
        }
    };
    trace.update(|r| r.upstream_status = Some(response.status().as_u16().into()));
    trace.phase("receiving");
    if !request.stream {
        let result = tokio::select! { biased; _=s.stop.cancelled()=>Err(cancelled()),_=key.cancel.cancelled()=>Err(cancelled()),r=consume(response,&request,None,chat,Some(&trace))=>r };
        return match result {
            Ok(result) => {
                trace.result(&result);
                Json(result).into_response()
            }
            Err(e) => e.into_response(),
        };
    }
    let (sender, receiver) = mpsc::channel(4);
    trace.update(|r| r.http_status = Some(200));
    let guard = Guard(trace.clone(), true);
    let task = tokio::spawn(async move {
        let _guard = guard;
        let _slot = slot;
        let result = tokio::select! { biased; _=s.stop.cancelled()=>Err(cancelled()),_=key.cancel.cancelled()=>Err(cancelled()),r=consume(response,&request,Some(&sender),chat,Some(&trace))=>r };
        match result {
            Ok(result) => {
                trace.result(&result);
                trace.finish(None).await;
            }
            Err(e) => {
                trace
                    .finish(Some(FailureInfo::new(e.code, e.status.as_u16())))
                    .await;
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
    let mut response = (
        [
            ("content-type", "text/event-stream"),
            ("x-accel-buffering", "no"),
        ],
        Body::from_stream(stream),
    )
        .into_response();
    response.extensions_mut().insert(Streaming);
    response
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
    trace: Option<&Trace>,
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
        let chunk = chunk.map_err(|e| {
            if e.is_timeout() {
                ApiError::new(
                    504,
                    "upstream_timeout",
                    "Upstream request deadline exceeded",
                )
            } else {
                interrupted()
            }
        })?;
        if let Some(trace) = trace {
            trace.activity(false);
        }
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
            if matches!(value["type"].as_str(), Some("error" | "response.failed")) {
                return Err(ApiError::new(
                    502,
                    "upstream_failed",
                    "Upstream returned a failed response",
                ));
            }
            let chunks = projection.accept(&value).map_err(|_| interrupted())?;
            if let Some(trace) = trace {
                let kind = value["type"].as_str().unwrap_or("");
                let output = matches!(
                    kind,
                    "response.output_text.delta"
                        | "response.function_call_arguments.delta"
                        | "response.refusal.delta"
                ) && value["delta"].as_str().is_some_and(|s| !s.is_empty());
                if output {
                    trace.activity(true);
                }
            }
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
                        if let Some(trace) = trace {
                            trace.phase("sending");
                        }
                        send(sender, frame).await?;
                        if let Some(trace) = trace {
                            trace.phase("receiving");
                        }
                    }
                }
            }
        }
        // A validated terminal is authoritative. Waiting for transport EOF here
        // keeps successful generations alive on persistent upstream connections.
        if projection.terminal().is_some() {
            break;
        }
    }
    // The optional trailing [DONE] can be split across network chunks. A validated
    // response terminal already ends the request; do not wait for that suffix.
    if projection.terminal().is_none() {
        decoder.finish().map_err(|_| interrupted())?;
    }
    let result = if chat {
        projection.finish().map_err(|_| interrupted())?
    } else {
        projection.terminal().cloned().ok_or_else(interrupted)?
    };
    if let Some(sender) = sender {
        for frame in terminal_frames {
            if let Some(trace) = trace {
                trace.phase("sending");
            }
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
    let result = consume(response, &request, None, true, None)
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

#[cfg(test)]
mod rejection_tests {
    use super::{ApiError, early_rejection};
    use axum::{
        body::{Body, Bytes},
        extract::Request,
    };
    use std::{convert::Infallible, time::Duration};
    use tokio_util::sync::CancellationToken;

    fn pending_body() -> Body {
        Body::from_stream(futures_util::stream::pending::<Result<Bytes, Infallible>>())
    }
    #[tokio::test]
    async fn oversized_rejected_upload_does_not_wait_for_untrusted_body() {
        let request = Request::builder()
            .header("content-length", 100 * 1024 * 1024)
            .body(pending_body())
            .unwrap();
        let response = tokio::time::timeout(
            Duration::from_millis(100),
            early_rejection(
                request,
                ApiError::new(401, "invalid_gateway_key", "missing key"),
                &CancellationToken::new(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 401);
        assert_eq!(response.headers()["connection"], "close");
    }
    #[tokio::test]
    async fn expect_continue_rejection_does_not_trigger_an_upload() {
        let request = Request::builder()
            .header("expect", "100-continue")
            .body(pending_body())
            .unwrap();
        let response = tokio::time::timeout(
            Duration::from_millis(100),
            early_rejection(
                request,
                ApiError::new(401, "invalid_gateway_key", "missing key"),
                &CancellationToken::new(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 401);
        assert_eq!(response.headers()["connection"], "close");
    }
    #[tokio::test]
    async fn gateway_shutdown_interrupts_rejected_body_cleanup() {
        let stop = CancellationToken::new();
        stop.cancel();
        let request = Request::new(pending_body());
        let response = tokio::time::timeout(
            Duration::from_millis(100),
            early_rejection(
                request,
                ApiError::new(401, "invalid_gateway_key", "missing key"),
                &stop,
            ),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 401);
        assert_eq!(response.headers()["connection"], "close");
    }
}
