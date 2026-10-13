use crate::storage::{FailureRecord, RequestRecord, Store};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Clone, Debug)]
pub struct FailureInfo {
    pub code: &'static str,
    pub message: &'static str,
    pub status: u16,
}
impl FailureInfo {
    pub fn new(code: &'static str, status: u16) -> Self {
        let message = match code {
            "invalid_gateway_key" | "key_revoked" => "网关密钥无效或已撤销",
            "invalid_host" => "Host 与监听地址不一致",
            "browser_origin_denied" => "拒绝浏览器来源请求",
            "invalid_request" => "请求正文读取失败、JSON 无效或 Content-Type 不正确",
            "unsupported_or_invalid_parameter" => "请求含未支持字段或参数格式不正确",
            "gateway_concurrency_limit" => "网关已达到 4 个并发请求上限",
            "account_not_ready" | "upstream_auth_required" => "账号未就绪或上游授权需要重新登录",
            "model_not_found" => "请求的模型不在当前账号目录中",
            "upstream_timeout" => "等待上游超时；请结合阶段及最近上游活动时间排查",
            "upstream_stream_interrupted" => "上游流中断或完成事件校验失败",
            "upstream_failed" => "上游返回生成失败事件",
            "upstream_incomplete" => "上游返回受限结束事件",
            "upstream_rate_limit" => "上游返回额度或速率限制",
            "upstream_forbidden" => "上游拒绝访问",
            "upstream_rejected_request" => "上游拒绝请求参数",
            "request_cancelled" => "网关停止或密钥撤销，已取消请求",
            "client_disconnected" => "客户端断开，请求及上游已取消",
            "slow_consumer" => "客户端超过 30 秒未读取响应",
            "response_too_large" => "上游响应超过 16 MiB",
            "storage_unavailable" => "本地存储不可用",
            "not_found" | "method_not_allowed" => "接口或 HTTP 方法不支持",
            _ => "上游网络、协议或服务错误",
        };
        Self {
            code,
            message,
            status,
        }
    }
}

enum Write {
    Finish(
        Box<RequestRecord>,
        Option<FailureRecord>,
        Option<oneshot::Sender<()>>,
    ),
    Flush(oneshot::Sender<()>),
}
pub struct Journal {
    store: Store,
    sender: mpsc::Sender<Write>,
    active: Arc<Mutex<HashMap<String, Weak<TraceState>>>>,
    failures: Arc<AtomicU64>,
}
impl Journal {
    pub fn new(store: Store) -> Arc<Self> {
        let (sender, mut receiver) = mpsc::channel(128);
        let book = Arc::new(Self {
            store: store.clone(),
            sender,
            active: Arc::new(Mutex::new(HashMap::new())),
            failures: Arc::new(AtomicU64::new(0)),
        });
        let active = book.active.clone();
        let failures = book.failures.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    command = receiver.recv() => {
                        let Some(command) = command else { break };
                        match command {
                            Write::Finish(record, error, ack) => {
                                if store.finish_request(&record, error.as_ref()).await.is_err() { write_failed(&failures); }
                                if let Some(ack) = ack { let _ = ack.send(()); }
                            }
                            Write::Flush(ack) => { let _ = ack.send(()); }
                        }
                    }
                    _ = tick.tick() => {
                        for record in live_records(&active) {
                            if store.checkpoint_request(&record).await.is_err() { write_failed(&failures); }
                        }
                    }
                }
            }
        });
        book
    }
    fn write_failed(&self) {
        write_failed(&self.failures);
    }
    pub fn write_failures(&self) -> u64 {
        self.failures.load(Ordering::Relaxed)
    }
    pub fn live_records(&self) -> Vec<RequestRecord> {
        live_records(&self.active)
    }
    pub async fn begin(self: &Arc<Self>, record: RequestRecord) -> Trace {
        let start = Instant::now();
        if self.store.start_request(&record).await.is_err() {
            self.write_failed();
        }
        let trace = Trace(Arc::new(TraceState {
            record: Mutex::new(record),
            start,
            book: self.clone(),
            done: AtomicBool::new(false),
        }));
        self.active
            .lock()
            .unwrap()
            .insert(trace.id(), Arc::downgrade(&trace.0));
        trace
    }
    pub async fn flush(&self) {
        let (ack, received) = oneshot::channel();
        if self.sender.send(Write::Flush(ack)).await.is_ok() {
            let _ = received.await;
        }
    }
}
fn write_failed(failures: &AtomicU64) {
    failures.fetch_add(1, Ordering::Relaxed);
    eprintln!("request_record_write_failed");
}
fn live_records(active: &Mutex<HashMap<String, Weak<TraceState>>>) -> Vec<RequestRecord> {
    let mut active = active.lock().unwrap();
    active.retain(|_, weak| weak.strong_count() > 0);
    active
        .values()
        .filter_map(Weak::upgrade)
        .filter_map(|state| {
            let record = state.record.lock().unwrap();
            // Check under the terminal lock so checkpoints cannot commit ended_at
            // before completion and its exception have committed together.
            if state.done.load(Ordering::Acquire) {
                return None;
            }
            let mut r = record.clone();
            r.elapsed_ms = state.start.elapsed().as_millis().min(i64::MAX as u128) as i64;
            Some(r)
        })
        .collect()
}
struct TraceState {
    record: Mutex<RequestRecord>,
    start: Instant,
    book: Arc<Journal>,
    done: AtomicBool,
}
#[derive(Clone)]
pub struct Trace(Arc<TraceState>);
impl Trace {
    pub fn id(&self) -> String {
        self.0.record.lock().unwrap().id.clone()
    }
    pub fn update(&self, update: impl FnOnce(&mut RequestRecord)) {
        let mut record = self.0.record.lock().unwrap();
        if !self.0.done.load(Ordering::Acquire) {
            update(&mut record);
        }
    }
    pub fn phase(&self, phase: &str) {
        self.update(|r| r.phase = phase.into());
    }
    pub fn metadata(&self, body: &Value, bytes: usize) {
        self.update(|r| {
            // Only a constrained model identifier is metadata. Never serialize body.
            r.model = body["model"]
                .as_str()
                .filter(|s| {
                    s.len() <= 160
                        && s.bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"-_.:/".contains(&c))
                })
                .unwrap_or("")
                .into();
            r.stream = match body.get("stream") {
                None | Some(Value::Null) => Some(false),
                Some(value) => value.as_bool(),
            };
            r.body_bytes = Some(bytes as i64);
            r.message_count = body["messages"]
                .as_array()
                .or_else(|| body["input"].as_array())
                .map(|a| a.len() as i64)
                .or_else(|| body["input"].as_str().map(|_| 1));
            r.tool_count = Some(body["tools"].as_array().map_or(0, |a| a.len() as i64));
        });
    }
    pub fn activity(&self, output: bool) {
        self.update(|r| {
            let now = now_ms();
            r.first_byte_at.get_or_insert(now);
            r.last_activity_at = Some(now);
            if output {
                r.first_output_at.get_or_insert(now);
            }
        });
    }
    pub fn result(&self, result: &Value) {
        self.update(|r| {
            r.outcome = if result["status"] == "incomplete"
                || matches!(
                    result["choices"][0]["finish_reason"].as_str(),
                    Some("length" | "content_filter")
                ) {
                "incomplete"
            } else {
                "completed"
            }
            .into();
            r.input_tokens = result["usage"]["input_tokens"]
                .as_i64()
                .or_else(|| result["usage"]["prompt_tokens"].as_i64());
            r.output_tokens = result["usage"]["output_tokens"]
                .as_i64()
                .or_else(|| result["usage"]["completion_tokens"].as_i64());
            r.phase = "finished".into();
        });
    }
    fn terminal(
        &self,
        error: Option<FailureInfo>,
    ) -> Option<(RequestRecord, Option<FailureRecord>)> {
        let mut record = self.0.record.lock().unwrap();
        if self.0.done.swap(true, Ordering::AcqRel) {
            return None;
        }
        record.ended_at = Some(now_ms().max(record.started_at));
        record.elapsed_ms = self.0.start.elapsed().as_millis().min(i64::MAX as u128) as i64;
        let error = error.or_else(|| {
            (record.outcome == "incomplete").then(|| FailureInfo::new("upstream_incomplete", 200))
        });
        if let Some(error) = &error {
            record.outcome = if matches!(error.code, "request_cancelled" | "client_disconnected") {
                "cancelled"
            } else if record.outcome == "incomplete" {
                "incomplete"
            } else {
                "failed"
            }
            .into();
        } else if record.outcome == "running" {
            record.outcome = "completed".into();
            record.phase = "finished".into();
        }
        let error = error.map(|e| FailureRecord {
            request_id: record.id.clone(),
            occurred_at: record.ended_at.unwrap(),
            phase: record.phase.clone(),
            code: e.code.into(),
            message: e.message.into(),
            http_status: e.status.into(),
        });
        Some((record.clone(), error))
    }
    pub async fn finish(&self, error: Option<FailureInfo>) {
        if let Some((record, error)) = self.terminal(error) {
            let (ack, received) = oneshot::channel();
            if self
                .0
                .book
                .sender
                .try_send(Write::Finish(Box::new(record), error, Some(ack)))
                .is_err()
            {
                self.0.book.write_failed();
            } else {
                let _ = received.await;
            }
        }
    }
}
pub struct Guard(pub Trace, pub bool);
impl Drop for Guard {
    fn drop(&mut self) {
        if self.1
            && let Some((record, error)) = self
                .0
                .terminal(Some(FailureInfo::new("client_disconnected", 499)))
            && self
                .0
                .0
                .book
                .sender
                .try_send(Write::Finish(Box::new(record), error, None))
                .is_err()
        {
            self.0.0.book.write_failed();
        }
    }
}
