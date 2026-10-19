mod accounts;
mod branding;
mod catalog;
mod clients;
mod components;
mod interactions;
mod layout;
mod navigation;
mod overview;
mod overview_chart;
mod overview_format;
mod overview_state;
mod presentation;
mod requests;
mod shell;
mod theme;
mod updates;

use gpui_kit::component::{
    button::*,
    input::{Input, InputState},
    *,
};
use gpui_kit::{prelude::*, *};
use std::time::{Duration, Instant};
use tiny_proxy::{
    app::{Command, Event, appearance::ThemePreference},
    auth::codex::{Model, VERIFICATION_URL},
    auth::quota::Quota,
    gateway::Record,
    storage::{AccountSummary, CreatedKey, FailureRecord, KeySummary, ModelTest},
};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

#[cfg(windows)]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(windows)]
use std::rc::Rc;
#[cfg(windows)]
use tiny_proxy::platform::tray::{Tray, TrayEvent};

const PAGES: [&str; 6] = [
    "概览",
    "账号连接",
    "模型",
    "客户端接入",
    "请求记录",
    "系统设置",
];

use components::{button, input};

fn clear_removed_key_display(
    keys: &[KeySummary],
    displayed: &mut Option<(CreatedKey, Instant)>,
    confirming: &mut Option<String>,
) {
    // Only the committed snapshot removes local plaintext; a failed deletion
    // must keep the key and its controls available.
    if displayed
        .as_ref()
        .is_some_and(|(key, _)| !keys.iter().any(|item| item.id == key.id))
    {
        *displayed = None;
    }
    if confirming
        .as_ref()
        .is_some_and(|id| !keys.iter().any(|key| &key.id == id))
    {
        *confirming = None;
    }
}

fn format_time(value: Option<i64>) -> String {
    value
        .and_then(|n| chrono::DateTime::from_timestamp(n, 0))
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "未提供".into())
}
fn test_status(value: &str) -> &'static str {
    match value {
        "completed" => "成功",
        "failed" => "失败",
        "interrupted" => "中断",
        "running" => "测试中",
        _ => "未知",
    }
}
fn request_time(value: Option<i64>) -> String {
    value
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string()
        })
        .unwrap_or_else(|| "未提供".into())
}
fn request_status(value: &str) -> &'static str {
    match value {
        "running" => "执行中",
        "completed" => "完成",
        "failed" => "失败",
        "cancelled" => "已取消",
        "interrupted" => "应用中断",
        "incomplete" => "受限结束",
        _ => "未知",
    }
}
fn request_phase(value: &str) -> &'static str {
    match value {
        "authenticating" => "网关鉴权",
        "reading_request" => "读取客户端请求",
        "validating" => "校验参数",
        "waiting_upstream" => "等待上游（凭据/目录/响应头）",
        "receiving" => "接收上游",
        "sending" => "发送给客户端",
        "finished" => "已结束",
        _ => "未知阶段",
    }
}
fn latency(start: i64, time: Option<i64>) -> String {
    time.map(|time| request_duration((time - start).max(0)))
        .unwrap_or_else(|| "尚未收到".into())
}

fn request_duration(milliseconds: i64) -> String {
    let milliseconds = milliseconds.max(0);
    if milliseconds < 10 {
        format!("{milliseconds} 毫秒")
    } else {
        format!("{:.2} 秒", milliseconds as f64 / 1000.)
    }
}

// Rendering and clipboard export share these lines so copied diagnostics match
// the visible snapshot, including unknown values and recovery-time semantics.
fn request_detail_lines(record: &Record, failure: Option<&FailureRecord>) -> Vec<String> {
    let failure = failure.filter(|failure| failure.request_id == record.id);
    let mut lines = vec![
        request_status(&record.outcome).into(),
        record.id.clone(),
        format!("{} {}", record.method, record.route),
        format!(
            "模型：{}",
            if record.model.is_empty() {
                "未提供"
            } else {
                &record.model
            }
        ),
    ];
    if let Some(failure) = failure {
        lines.push(presentation::failure_label(&failure.code).into());
        lines.push(failure.code.clone());
        lines.push(failure.message.clone());
    }
    lines.push("请求时间线".into());
    for (label, time) in presentation::timeline(record) {
        lines.push(format!("{label} · {}", request_time(Some(time))));
    }
    lines.push(format!(
        "{} {}",
        if record.outcome == "interrupted" {
            "中断前已观察耗时"
        } else {
            "总耗时"
        },
        request_duration(record.elapsed_ms)
    ));
    for (label, value) in presentation::request_summary(record) {
        lines.push(format!("{label}：{value}"));
    }
    if presentation::needs_stream_warning(record) {
        lines.push("HTTP 200 不代表流已完整结束".into());
    }
    lines.push("完整诊断明细".into());
    lines.extend(request_metadata_lines(record, failure));
    lines.push("不保存提示词、回答、工具参数或认证信息。".into());
    lines
}

fn request_metadata_lines(record: &Record, failure: Option<&FailureRecord>) -> Vec<String> {
    let mut metadata = vec![
        format!(
            "{} {} · 模型 {} · {}",
            record.method,
            record.route,
            if record.model.is_empty() {
                "未提供"
            } else {
                &record.model
            },
            request_status(&record.outcome)
        ),
        format!("开始：{}", request_time(Some(record.started_at))),
        format!(
            "{}：{} · {} {}",
            if record.outcome == "interrupted" {
                "恢复确认时间"
            } else {
                "结束"
            },
            if record.ended_at.is_none() {
                "执行中".into()
            } else {
                request_time(record.ended_at)
            },
            if record.outcome == "interrupted" {
                "中断前已观察耗时"
            } else {
                "耗时"
            },
            request_duration(record.elapsed_ms)
        ),
        format!(
            "{}：{}",
            if record.ended_at.is_none() {
                "当前阶段"
            } else {
                "结束阶段"
            },
            request_phase(&record.phase)
        ),
        format!(
            "首批上游数据：{} · 首个输出：{}",
            latency(record.started_at, record.first_byte_at),
            latency(record.started_at, record.first_output_at)
        ),
        format!("最近上游活动：{}", request_time(record.last_activity_at)),
        format!(
            "HTTP：{} · 上游 HTTP：{}",
            record
                .http_status
                .map_or("未返回".into(), |s| s.to_string()),
            record
                .upstream_status
                .map_or("未提供".into(), |s| s.to_string())
        ),
        format!(
            "流式：{} · 请求大小：{} 字节 · 消息/输入项：{} · 工具定义：{}",
            record
                .stream
                .map_or("未提供", |s| if s { "是" } else { "否" }),
            record.body_bytes.map_or("未提供".into(), |n| n.to_string()),
            record
                .message_count
                .map_or("未提供".into(), |n| n.to_string()),
            record.tool_count.map_or("未提供".into(), |n| n.to_string())
        ),
        format!(
            "输入/输出 tokens：{} / {}",
            record
                .input_tokens
                .map_or("未提供".into(), |n| n.to_string()),
            record
                .output_tokens
                .map_or("未提供".into(), |n| n.to_string())
        ),
        format!(
            "缓存输入 tokens：{}",
            record
                .cached_input_tokens
                .map_or("未提供".into(), |n| n.to_string())
        ),
        format!("密钥 ID：{}", record.key_id.as_deref().unwrap_or("未鉴权")),
        format!(
            "账号 ID：{}",
            record.account_id.as_deref().unwrap_or("未就绪")
        ),
        format!("请求 ID：{}", record.id),
    ];
    if record.phase == "validating"
        && record.http_status == Some(400)
        && record.upstream_status.is_none()
    {
        metadata.push("处理位置：本地参数校验，尚未发送到上游；请按异常原因修改客户端请求".into());
    }
    if let Some(failure) = failure.filter(|failure| failure.request_id == record.id) {
        metadata.push(format!("异常代码：{}", failure.code));
        metadata.push(format!("异常原因：{}", failure.message));
        metadata.push(format!(
            "异常时间：{} · 异常阶段：{} · 异常 HTTP：{}",
            request_time(Some(failure.occurred_at)),
            request_phase(&failure.phase),
            failure.http_status
        ));
    }
    metadata
}

struct Desktop {
    #[cfg(windows)]
    tray: Option<Rc<Tray>>,
    tray_notice: Option<String>,
    update: tiny_proxy::updater::Snapshot,
    update_interval: Entity<InputState>,
    update_notice: Option<String>,
    confirm_update: bool,
    page: usize,
    client_tab: usize,
    login_open: bool,
    device_countdown: u64,
    compact: bool,
    selected_key: Option<String>,
    selected_model: Option<String>,
    account_details: Option<String>,
    key_menu_open: bool,
    model_menu_open: bool,
    details_expanded: bool,
    test_history_open: bool,
    toast: Option<(String, Instant)>,
    request_search: Entity<InputState>,
    model_search: Entity<InputState>,
    theme_preference: ThemePreference,
    theme_loaded: bool,
    theme_saving: bool,
    colors: theme::Palette,
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<Event>,
    status: String,
    error: bool,
    busy: bool,
    keys: Vec<KeySummary>,
    codex_connected: bool,
    accounts: Vec<AccountSummary>,
    usage_busy: Option<String>,
    tests: Vec<ModelTest>,
    models_checked_at: Option<i64>,
    device_code: Option<(Zeroizing<String>, Instant)>,
    gateway_address: Option<String>,
    pending: interactions::Pending,
    overview: overview_state::OverviewState,
    records: Vec<Record>,
    failures: Vec<FailureRecord>,
    request_count: i64,
    failure_count: i64,
    request_page: i64,
    failure_page: i64,
    show_failures: bool,
    selected_request: Option<Record>,
    selected_failure: Option<FailureRecord>,
    requested_detail: Option<String>,
    log_write_failures: u64,
    bind_ip: Entity<InputState>,
    bind_port: Entity<InputState>,
    models: Vec<Model>,
    key_name: Entity<InputState>,
    new_key: Option<(CreatedKey, Instant)>,
    revoke_id: Option<String>,
    test_result: Option<String>,
}

impl Desktop {
    fn new(
        commands: mpsc::Sender<Command>,
        events: mpsc::Receiver<Event>,
        _data_directory: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        #[cfg(windows)]
        let (tray, tray_notice) = {
            let native_window = window.window_handle().ok().and_then(|handle| {
                if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                    Some(handle.hwnd.get() as windows_sys::Win32::Foundation::HWND)
                } else {
                    None
                }
            });
            let result = native_window
                .ok_or_else(|| std::io::Error::other("无法读取 Windows 窗口句柄"))
                .and_then(|hwnd| Tray::new(hwnd, _data_directory));
            let (tray, notice) = match result {
                Ok(tray) => (Some(Rc::new(tray)), None),
                Err(error) => (
                    None,
                    Some(format!(
                        "托盘初始化失败：{error}。窗口保持打开，可在设置中退出应用。"
                    )),
                ),
            };
            let close_tray = tray.clone();
            window.on_window_should_close(cx, move |_, _| {
                if let Some(tray) = &close_tray {
                    if let Err(error) = tray.hide() {
                        tiny_proxy::platform::tray::show_error(&error.to_string());
                    }
                } else {
                    tiny_proxy::platform::tray::show_error(
                        "托盘不可用，窗口保持打开。请在设置中选择“退出应用”。",
                    );
                }
                // A titlebar close never stops the gateway or destroys the
                // view. Explicit quit/update still uses GPUI's quit path.
                false
            });
            (tray, notice)
        };
        #[cfg(not(windows))]
        let tray_notice = None;
        let colors = theme::apply(ThemePreference::default(), window.appearance(), cx);
        cx.observe_window_appearance(window, |this, window, cx| {
            if this.theme_preference == ThemePreference::System {
                this.update_theme(window, cx);
                cx.notify();
            }
        })
        .detach();
        // The background runtime owns networking and SQLite. GPUI only drains a
        // bounded event queue; no Tokio runtime is created per click or render.
        cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if view
                    .update_in(cx, |view, window, cx| view.poll(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let request_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索本页请求 ID、模型或客户端"));
        let model_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索模型 ID 或名称"));
        cx.observe(&request_search, |_, _, cx| cx.notify()).detach();
        cx.observe(&model_search, |_, _, cx| cx.notify()).detach();
        let key_name = cx.new(|cx| InputState::new(window, cx).placeholder("例如：工作电脑 Trae"));
        cx.observe(&key_name, |_, _, cx| cx.notify()).detach();
        let bind_ip = cx.new(|cx| InputState::new(window, cx).placeholder("127.0.0.1"));
        let bind_port = cx.new(|cx| InputState::new(window, cx).placeholder("8787"));
        cx.observe(&bind_ip, |_, _, cx| cx.notify()).detach();
        cx.observe(&bind_port, |_, _, cx| cx.notify()).detach();
        Self {
            #[cfg(windows)]
            tray,
            tray_notice,
            update: tiny_proxy::updater::Snapshot {
                preferences: Default::default(),
                status: Default::default(),
            },
            update_interval: cx.new(|cx| InputState::new(window, cx).placeholder("24")),
            update_notice: None,
            confirm_update: false,
            page: 0,
            client_tab: 0,
            login_open: false,
            device_countdown: 0,
            compact: false,
            selected_key: None,
            selected_model: None,
            account_details: None,
            key_menu_open: false,
            model_menu_open: false,
            details_expanded: false,
            test_history_open: false,
            toast: None,
            request_search,
            model_search,
            theme_preference: ThemePreference::default(),
            theme_loaded: false,
            theme_saving: false,
            colors,
            commands,
            events,
            status: "正在打开本地存储…".into(),
            error: false,
            busy: true,
            keys: vec![],
            codex_connected: false,
            accounts: vec![],
            usage_busy: None,
            tests: vec![],
            models_checked_at: None,
            device_code: None,
            gateway_address: None,
            pending: interactions::Pending::default(),
            overview: overview_state::OverviewState::default(),
            records: vec![],
            failures: vec![],
            request_count: 0,
            failure_count: 0,
            request_page: 0,
            failure_page: 0,
            show_failures: false,
            selected_request: None,
            selected_failure: None,
            requested_detail: None,
            log_write_failures: 0,
            bind_ip,
            bind_port,
            models: vec![],
            key_name,
            new_key: None,
            revoke_id: None,
            test_result: None,
        }
    }

    fn set_bind_inputs(&mut self, address: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (ip, port) = presentation::gateway_bind_fields(address);
        self.bind_ip
            .update(cx, |input, cx| input.set_value(ip, window, cx));
        self.bind_port
            .update(cx, |input, cx| input.set_value(port, window, cx));
    }

    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut changed = false;
        #[cfg(windows)]
        if let Some(tray) = &self.tray {
            while let Ok(event) = tray.try_recv() {
                changed = true;
                match event {
                    TrayEvent::Restore => {}
                    TrayEvent::Exit => {
                        // main() calls Backend::stop after the GPUI loop ends;
                        // this releases listening ports and drains records.
                        cx.quit();
                        return;
                    }
                    TrayEvent::Unavailable => {
                        self.tray_notice =
                            Some("托盘恢复失败，已重新显示窗口；可在设置中退出应用。".into());
                    }
                }
            }
        }
        while let Ok(event) = self.events.try_recv() {
            changed = true;
            let reveal_key = self.pending.reveal_requested;
            self.pending.receive(&event);
            match event {
                Event::Overview {
                    request_id,
                    period,
                    result,
                } => {
                    self.overview
                        .receive(request_id, period, result, Instant::now());
                }
                Event::Update(update) => {
                    if !update.preferences.notify {
                        self.update_notice = None;
                    }
                    if update.preferences != self.update.preferences {
                        self.update_interval.update(cx, |input, cx| {
                            input.set_value(
                                update.preferences.interval_hours.to_string(),
                                window,
                                cx,
                            )
                        });
                    }
                    if !matches!(
                        update.status,
                        tiny_proxy::updater::UpdateStatus::ReadyToRestart(..)
                    ) {
                        self.confirm_update = false;
                    }
                    self.update = *update;
                }
                Event::UpdateNotice(message) => self.update_notice = Some(message),
                Event::UpdateProgress(done, total) => {
                    if let tiny_proxy::updater::UpdateStatus::Downloading(
                        _,
                        current,
                        total_current,
                    ) = &mut self.update.status
                    {
                        *current = done;
                        *total_current = total;
                    }
                }
                Event::QuitForUpdate => {
                    cx.quit();
                    return;
                }
                Event::Snapshot {
                    keys,
                    codex_connected,
                    accounts,
                    models,
                    models_checked_at,
                    tests,
                    theme_preference,
                } => {
                    if !self.theme_loaded || self.theme_preference != theme_preference {
                        self.theme_preference = theme_preference;
                        self.theme_loaded = true;
                        self.update_theme(window, cx);
                    }
                    if self.accounts.iter().find(|a| a.active).map(|a| &a.id)
                        != accounts.iter().find(|a| a.active).map(|a| &a.id)
                    {
                        self.test_result = None;
                    }
                    clear_removed_key_display(&keys, &mut self.new_key, &mut self.revoke_id);
                    if self
                        .selected_key
                        .as_ref()
                        .is_none_or(|id| !keys.iter().any(|key| &key.id == id))
                    {
                        self.selected_key = keys.first().map(|key| key.id.clone());
                    }
                    self.keys = keys;
                    self.codex_connected = codex_connected;
                    self.accounts = accounts;
                    self.models = models;
                    self.models_checked_at = models_checked_at;
                    self.tests = tests;
                }
                Event::Status { message, busy } => {
                    self.status = message;
                    self.busy = busy;
                    self.error = false;
                    if !busy {
                        self.login_open = false;
                        self.device_code = None;
                    }
                }
                Event::Error(message) => {
                    self.status = message;
                    self.busy = false;
                    self.error = true;
                    self.device_code = None;
                    if self.selected_request.is_none() {
                        self.requested_detail = None;
                    }
                }
                Event::DeviceCode { code, expires_in } => {
                    // A late response must not reopen a cancelled login drawer.
                    if self.login_open {
                        self.device_code = Some((code, Instant::now() + expires_in));
                    }
                }
                Event::Gateway { address } => {
                    if let Some(address) = &address {
                        self.set_bind_inputs(address, window, cx);
                    }
                    self.gateway_address = address;
                }
                Event::History(history) => {
                    let mut selected_finished = false;
                    if let Some(selected) = &mut self.selected_request
                        && let Some(current) = history.requests.iter().find(|r| r.id == selected.id)
                    {
                        selected_finished =
                            selected.ended_at.is_none() && current.ended_at.is_some();
                        *selected = current.clone();
                    }
                    if let Some(selected) = &self.selected_request
                        && self.requested_detail.as_deref() == Some(selected.id.as_str())
                    {
                        if let Some(failure) = history
                            .failures
                            .iter()
                            .find(|f| f.request_id == selected.id)
                        {
                            self.selected_failure = Some(failure.clone());
                        } else if selected_finished
                            && self.selected_failure.is_none()
                            && matches!(
                                selected.outcome.as_str(),
                                "failed" | "cancelled" | "incomplete" | "interrupted"
                            )
                        {
                            // A live selection may finish outside the current exception
                            // page. Fetch its own failure rather than rely on pagination.
                            let id = selected.id.clone();
                            self.send(Command::RequestDetail(id), cx);
                        }
                    }
                    self.records = history.requests;
                    self.failures = history.failures;
                    self.request_count = history.request_count;
                    self.failure_count = history.failure_count;
                    self.request_page = history.request_page;
                    self.failure_page = history.failure_page;
                    self.log_write_failures = history.write_failures;
                }
                Event::RequestDetail { record, failure } => {
                    if self.requested_detail.as_deref() == Some(record.id.as_str()) {
                        self.selected_request = Some(*record);
                        self.selected_failure = failure;
                    }
                }
                Event::GatewayBind(address) => {
                    self.set_bind_inputs(&address, window, cx);
                }
                Event::ThemePreferenceSaved(result) => {
                    self.theme_saving = false;
                    match result {
                        Ok(preference) => {
                            self.theme_preference = preference;
                            self.update_theme(window, cx);
                            self.status = "主题设置已保存".into();
                            self.error = false;
                        }
                        Err(message) => {
                            self.status = message;
                            self.error = true;
                        }
                    }
                }
                Event::OpenBrowser(url) => cx.open_url(&url),
                Event::KeyCreated(key) => {
                    self.selected_key = Some(key.id.clone());
                    if reveal_key && self.page == 3 && self.client_tab == 1 {
                        self.new_key = Some((key, Instant::now()));
                    }
                    self.toast = Some(("密钥已创建".into(), Instant::now()));
                    self.status = "密钥已加密保存，之后可随时查看或复制".into();
                    self.error = false;
                }
                Event::KeyRevealed { key, copy } => {
                    self.selected_key = Some(key.id.clone());
                    if copy {
                        cx.write_to_clipboard(ClipboardItem::new_string(key.value.to_string()));
                        self.status = "密钥已复制到系统剪贴板".into();
                        self.toast = Some(("密钥已复制".into(), Instant::now()));
                    } else if reveal_key && self.page == 3 && self.client_tab == 1 {
                        self.new_key = Some((key, Instant::now()));
                        self.status = "密钥已显示，两分钟后隐藏；可随时再次查看".into();
                    }
                    self.error = false;
                }
                Event::UsageBusy(id) => self.usage_busy = id,
                Event::Models(models) => {
                    self.models = models;
                    self.test_result = None;
                }
                Event::TestResult(result) => {
                    let usage =
                        |n: Option<u64>| n.map(|n| n.to_string()).unwrap_or_else(|| "未知".into());
                    self.test_result = Some(format!(
                        "{}\n\n已收到 response.completed · 输入 {} / 输出 {} tokens",
                        result.text,
                        usage(result.input_tokens),
                        usage(result.output_tokens)
                    ));
                }
            }
        }
        if self
            .new_key
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= Duration::from_secs(120))
        {
            self.new_key = None;
            changed = true;
        }
        if self
            .device_code
            .as_ref()
            .is_some_and(|(_, until)| Instant::now() >= *until)
        {
            self.device_code = None;
            if self.login_open && self.busy {
                self.send(Command::Cancel, cx);
                self.status = "授权已过期，请重新申请".into();
                self.error = true;
            }
            changed = true;
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= Duration::from_secs(4))
        {
            self.toast = None;
            changed = true;
        }
        // Device-code countdown is presentation only; the backend owns expiry/cancellation.
        let remaining = self
            .device_code
            .as_ref()
            .map(|(_, until)| until.saturating_duration_since(Instant::now()).as_secs())
            .unwrap_or(0);
        if remaining != self.device_countdown {
            self.device_countdown = remaining;
            changed = true;
        }
        if changed {
            cx.notify();
        }
        if self.page == 0
            && self.theme_loaded
            && !self.pending.installing
            && self.overview.should_refresh(Instant::now())
        {
            self.refresh_overview(None, cx);
        }
    }

    fn update_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.colors = theme::apply(self.theme_preference, window.appearance(), cx);
    }

    fn set_theme_preference(&mut self, preference: ThemePreference, cx: &mut Context<Self>) {
        if !self.theme_loaded || self.theme_saving || preference == self.theme_preference {
            return;
        }
        if self
            .commands
            .try_send(Command::SetThemePreference(preference))
            .is_ok()
        {
            self.theme_saving = true;
        } else {
            self.status = "主题设置未保存，后台忙碌或已退出，请重试".into();
            self.error = true;
        }
        cx.notify();
    }

    fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        if !self.pending.allows(&command) {
            return;
        }
        let after_enqueue = self.pending.after_enqueue(&command);
        let overview_request = match &command {
            Command::LoadOverview { request_id, period } => Some((*request_id, *period)),
            _ => None,
        };
        let reveals_key = matches!(&command, Command::RevealKey { copy: false, .. });
        let starts_busy = matches!(
            &command,
            Command::SignIn | Command::RefreshModels | Command::TestModel(_) | Command::Cancel
        );
        let detail_id = match &command {
            Command::RequestDetail(id) => Some(id.clone()),
            _ => None,
        };
        if self.commands.try_send(command).is_err() {
            if let Some((id, period)) = overview_request {
                self.overview.receive(
                    id,
                    period,
                    Err("后台忙碌或已经退出，请重试".into()),
                    Instant::now(),
                );
                cx.notify();
                return;
            }
            if starts_busy {
                self.busy = false;
            }
            self.status = "后台忙碌或已经退出，请稍后重试".into();
            self.error = true;
        } else {
            self.pending = after_enqueue;
            if overview_request.is_none() {
                self.error = false;
            }
            if reveals_key {
                self.new_key = None;
            }
            if let Some(id) = detail_id {
                if self.requested_detail.as_ref() != Some(&id) {
                    self.details_expanded = false;
                }
                if self.selected_request.as_ref().is_some_and(|r| r.id != id) {
                    self.selected_request = None;
                    self.selected_failure = None;
                }
                self.requested_detail = Some(id);
            }
        }
        cx.notify();
    }
}

pub fn run(
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<Event>,
    data_directory: std::path::PathBuf,
) {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::apply(ThemePreference::default(), cx.window_appearance(), cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.), px(820.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Tiny Proxy · Codex 网关".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(960.), px(640.))),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Desktop::new(commands, events, &data_directory, window, cx))
            })
            .expect("Tiny Proxy window could not be opened");
            cx.activate(true);
        });
}

#[cfg(test)]
mod model_status_tests {
    use super::test_status;

    #[test]
    fn labels_remain_valid_after_source_status_is_dropped() {
        for (status, expected) in [
            ("completed", "成功"),
            ("failed", "失败"),
            ("interrupted", "中断"),
            ("running", "测试中"),
            ("unrecognized", "未知"),
        ] {
            let label: &'static str = {
                let stored_status = String::from(status);
                test_status(&stored_status)
            };
            assert_eq!(label, expected);
        }
    }
}

#[cfg(test)]
mod request_detail_tests {
    use super::{FailureRecord, Record, request_detail_lines};

    #[test]
    fn copied_drawer_keeps_timeline_warning_and_metadata_in_display_order() {
        let mut record = record();
        record.outcome = "failed".into();
        record.stream = Some(true);
        record.http_status = Some(200);
        record.first_byte_at = Some(1_100);
        record.first_output_at = None;
        record.ended_at = Some(2_000);
        let text = request_detail_lines(&record, None).join("\n");
        let timeline = text.find("请求时间线").unwrap();
        let warning = text.find("HTTP 200 不代表流已完整结束").unwrap();
        let metadata = text.find("完整诊断明细").unwrap();
        assert!(timeline < warning && warning < metadata);
        assert!(!text[timeline..warning].contains("首个输出"));
        assert!(text[metadata..].contains("首个输出：尚未收到"));
    }

    fn record() -> Record {
        Record::new(
            "request-fixture".into(),
            "POST".into(),
            "/v1/chat/completions".into(),
            1_000,
        )
    }

    #[test]
    fn fast_validation_details_preserve_milliseconds_and_parameter_guidance() {
        let mut record = record();
        record.outcome = "failed".into();
        record.phase = "validating".into();
        record.ended_at = Some(1_005);
        record.elapsed_ms = 5;
        record.http_status = Some(400);
        let failure = FailureRecord {
            request_id: record.id.clone(),
            occurred_at: 1_005,
            phase: "validating".into(),
            code: "unsupported_or_invalid_parameter".into(),
            message:
                "参数 messages[1].tool_calls[0].id：调用 ID 必须是非空字符串；建议：提供调用 ID"
                    .into(),
            http_status: 400,
        };
        let text = request_detail_lines(&record, Some(&failure)).join("\n");
        assert!(text.contains("耗时 5 毫秒"));
        assert!(!text.contains("耗时 0.00 秒"));
        assert!(text.contains(&format!("异常原因：{}", failure.message)));
        assert!(text.contains("尚未发送到上游"));
    }

    #[test]
    fn failed_request_details_include_413_and_visible_diagnostics() {
        // Historical failures remain readable after the request limit changes.
        let mut record = record();
        record.outcome = "failed".into();
        record.phase = "reading_request".into();
        record.ended_at = Some(1_010);
        record.elapsed_ms = 10;
        record.http_status = Some(413);
        record.key_id = Some("key-fixture".into());
        record.account_id = Some("account-fixture".into());
        let failure = FailureRecord {
            request_id: record.id.clone(),
            occurred_at: 1_010,
            phase: "reading_request".into(),
            code: "invalid_request".into(),
            message: "请求 JSON 无效或超过 2 MiB".into(),
            http_status: 413,
        };
        let lines = request_detail_lines(&record, Some(&failure));
        let text = lines.join("\n");
        for expected in [
            "POST /v1/chat/completions · 模型 未提供 · 失败",
            "耗时 0.01 秒",
            "结束阶段：读取客户端请求",
            "首批上游数据：尚未收到 · 首个输出：尚未收到",
            "最近上游活动：未提供",
            "HTTP：413 · 上游 HTTP：未提供",
            "流式：未提供 · 请求大小：未提供 字节 · 消息/输入项：未提供 · 工具定义：未提供",
            "输入/输出 tokens：未提供 / 未提供",
            "密钥 ID：key-fixture",
            "账号 ID：account-fixture",
            "请求 ID：request-fixture",
            "异常代码：invalid_request",
            "异常原因：请求 JSON 无效或超过 2 MiB",
            "异常阶段：读取客户端请求 · 异常 HTTP：413",
        ] {
            assert!(text.contains(expected), "missing detail: {expected}");
        }
        assert!(lines.iter().any(|line| line.starts_with("开始：")));
        assert!(lines.iter().any(|line| line.starts_with("结束：")));
        assert!(lines.iter().any(|line| line.starts_with("异常时间：")));
    }

    #[test]
    fn completed_request_details_preserve_known_metadata_and_zero_values() {
        let mut record = record();
        record.model = "model-fixture".into();
        record.outcome = "completed".into();
        record.phase = "finished".into();
        record.ended_at = Some(3_500);
        record.elapsed_ms = 2_500;
        record.first_byte_at = Some(1_250);
        record.first_output_at = Some(1_500);
        record.last_activity_at = Some(3_500);
        record.http_status = Some(200);
        record.upstream_status = Some(200);
        record.stream = Some(false);
        record.body_bytes = Some(512);
        record.message_count = Some(2);
        record.tool_count = Some(0);
        record.input_tokens = Some(100);
        record.output_tokens = Some(0);
        record.cached_input_tokens = Some(80);
        let text = request_detail_lines(&record, None).join("\n");
        for expected in [
            "模型 model-fixture · 完成",
            "耗时 2.50 秒",
            "结束阶段：已结束",
            "首批上游数据：0.25 秒 · 首个输出：0.50 秒",
            "HTTP：200 · 上游 HTTP：200",
            "流式：否 · 请求大小：512 字节 · 消息/输入项：2 · 工具定义：0",
            "输入/输出 tokens：100 / 0",
            "缓存输入 tokens：80",
        ] {
            assert!(text.contains(expected), "missing detail: {expected}");
        }
        assert!(!text.contains("最近上游活动：未提供"));
        assert!(!text.contains("异常代码："));
    }

    #[test]
    fn running_and_recovered_request_details_keep_time_semantics() {
        let mut record = record();
        record.phase = "receiving".into();
        record.elapsed_ms = 1_500;
        record.stream = Some(true);
        let text = request_detail_lines(&record, None).join("\n");
        assert!(text.contains("结束：执行中 · 耗时 1.50 秒"));
        assert!(text.contains("缓存输入 tokens：未提供"));
        assert!(text.contains("当前阶段：接收上游"));
        assert!(text.contains("HTTP：未返回"));
        assert!(text.contains("流式：是"));

        record.outcome = "interrupted".into();
        record.ended_at = Some(60_000);
        let text = request_detail_lines(&record, None).join("\n");
        assert!(text.contains("恢复确认时间："));
        assert!(text.contains("中断前已观察耗时 1.50 秒"));
        assert!(!text.contains("结束："));
    }

    #[test]
    fn request_details_do_not_include_another_requests_failure() {
        let failure = FailureRecord {
            request_id: "another-request".into(),
            occurred_at: 1_010,
            phase: "waiting_upstream".into(),
            code: "upstream_timeout".into(),
            message: "unrelated failure marker".into(),
            http_status: 504,
        };
        let text = request_detail_lines(&record(), Some(&failure)).join("\n");
        assert!(!text.contains("异常代码："));
        assert!(!text.contains("upstream_timeout"));
        assert!(!text.contains("unrelated failure marker"));
        assert!(text.contains("请求 ID：request-fixture"));
    }
}

#[cfg(test)]
mod key_display_tests {
    use super::{CreatedKey, Instant, KeySummary, Zeroizing, clear_removed_key_display};

    fn key(id: &str) -> KeySummary {
        KeySummary {
            id: id.into(),
            label: id.into(),
            public_prefix: "tp_fixture".into(),
            recoverable: true,
        }
    }

    #[test]
    fn removed_key_display_is_cleared_after_successful_snapshot() {
        let mut displayed = Some((
            CreatedKey {
                id: "removed".into(),
                value: Zeroizing::new("fixture".into()),
            },
            Instant::now(),
        ));
        let mut confirming = Some("removed".into());
        clear_removed_key_display(&[key("retained")], &mut displayed, &mut confirming);
        assert!(displayed.is_none());
        assert!(confirming.is_none());
    }

    #[test]
    fn removed_key_display_preserves_values_when_deletion_fails_or_other_keys_change() {
        let mut displayed = Some((
            CreatedKey {
                id: "retained".into(),
                value: Zeroizing::new("fixture".into()),
            },
            Instant::now(),
        ));
        let mut confirming = Some("retained".into());
        clear_removed_key_display(&[key("retained")], &mut displayed, &mut confirming);
        assert_eq!(displayed.as_ref().unwrap().0.id, "retained");
        assert_eq!(confirming.as_deref(), Some("retained"));

        confirming = Some("removed".into());
        clear_removed_key_display(&[key("retained")], &mut displayed, &mut confirming);
        assert!(displayed.is_some());
        assert!(confirming.is_none());
    }
}
