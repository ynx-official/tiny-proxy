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

const PAGES: [&str; 6] = ["概览", "账号连接", "模型", "客户端接入", "请求记录", "设置"];

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
fn test_status(value: &str) -> &str {
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
    time.map(|time| format!("{:.2} 秒", (time - start).max(0) as f64 / 1000.))
        .unwrap_or_else(|| "尚未收到".into())
}

// Rendering and clipboard export share these lines so copied diagnostics match
// the visible snapshot, including unknown values and recovery-time semantics.
fn request_detail_lines(record: &Record, failure: Option<&FailureRecord>) -> Vec<String> {
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
            "{}：{} · {} {:.2} 秒",
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
            record.elapsed_ms as f64 / 1000.
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
        format!("密钥 ID：{}", record.key_id.as_deref().unwrap_or("未鉴权")),
        format!(
            "账号 ID：{}",
            record.account_id.as_deref().unwrap_or("未就绪")
        ),
        format!("请求 ID：{}", record.id),
    ];
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
    records: Vec<Record>,
    failures: Vec<FailureRecord>,
    request_count: i64,
    failure_count: i64,
    request_page: i64,
    failure_page: i64,
    show_failures: bool,
    selected_request: Option<Record>,
    log_write_failures: u64,
    bind_address: Entity<InputState>,
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
            records: vec![],
            failures: vec![],
            request_count: 0,
            failure_count: 0,
            request_page: 0,
            failure_page: 0,
            show_failures: false,
            selected_request: None,
            log_write_failures: 0,
            bind_address: cx.new(|cx| InputState::new(window, cx).placeholder("127.0.0.1:8787")),
            models: vec![],
            key_name: cx.new(|cx| InputState::new(window, cx).placeholder("例如：工作电脑 Trae")),
            new_key: None,
            revoke_id: None,
            test_result: None,
        }
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
            match event {
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
                        self.device_code = None;
                    }
                }
                Event::Error(message) => {
                    self.status = message;
                    self.busy = false;
                    self.error = true;
                    self.device_code = None;
                }
                Event::DeviceCode { code, expires_in } => {
                    self.device_code = Some((code, Instant::now() + expires_in))
                }
                Event::Gateway { address } => {
                    self.gateway_address = address;
                }
                Event::History(history) => {
                    if let Some(selected) = &mut self.selected_request
                        && let Some(current) = history.requests.iter().find(|r| r.id == selected.id)
                    {
                        *selected = current.clone();
                    }
                    self.records = history.requests;
                    self.failures = history.failures;
                    self.request_count = history.request_count;
                    self.failure_count = history.failure_count;
                    self.request_page = history.request_page;
                    self.failure_page = history.failure_page;
                    self.log_write_failures = history.write_failures;
                }
                Event::RequestDetail(record) => self.selected_request = Some(*record),
                Event::GatewayBind(address) => {
                    self.bind_address
                        .update(cx, |input, cx| input.set_value(address, window, cx));
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
                    self.new_key = Some((key, Instant::now()));
                    self.status = "密钥已加密保存，之后可随时查看或复制".into();
                    self.error = false;
                }
                Event::KeyRevealed { key, copy } => {
                    if copy {
                        cx.write_to_clipboard(ClipboardItem::new_string(key.value.to_string()));
                        self.status = "密钥已复制到系统剪贴板".into();
                    } else {
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
            changed = true;
        }
        if changed {
            cx.notify();
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
        if self.commands.try_send(command).is_err() {
            self.status = "后台忙碌或已经退出，请稍后重试".into();
            self.error = true;
        }
        cx.notify();
    }

    fn title(&self, subtitle: &'static str) -> Div {
        div()
            .v_flex()
            .gap_2()
            .mb_6()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(self.colors.muted))
                    .child("TINY PROXY / PERSONAL GATEWAY"),
            )
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(PAGES[self.page]),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(self.colors.muted))
                    .child(subtitle),
            )
    }

    fn panel(&self, title: impl Into<SharedString>, content: impl Into<SharedString>) -> Div {
        div()
            .v_flex()
            .gap_3()
            .p_5()
            .rounded_lg()
            .bg(rgb(self.colors.surface))
            .border_1()
            .border_color(rgb(self.colors.border))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title.into()))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(self.colors.muted))
                    .child(content.into()),
            )
    }

    fn overview(&self, cx: &mut Context<Self>) -> AnyElement {
        div().v_flex().gap_4()
            .child(self.title("Codex 设备码登录 · 为自己的 Agent 提供模型 API。"))
            .child(self.panel("服务状态",self.gateway_address.as_ref().map(|a|format!("运行中 · http://{a}/v1")).unwrap_or_else(||"已停止 · 在客户端接入页启动监听".into())))
            .child(self.panel("Codex 账号",if self.codex_connected {"已有 Codex 授权凭据；请求前自动检查有效期并续期。模型可用性以调用结果为准。"} else {"尚未连接账号。请前往账号连接，使用设备码完成授权。"}))
            .child(div().h_flex().gap_3()
                .child(Button::new("overview-login").primary().label("账号连接").on_click(cx.listener(|this,_,_,cx|{this.page=1;cx.notify();})))
                .child(Button::new("overview-clients").label("配置 Trae 接入").on_click(cx.listener(|this,_,_,cx|{this.page=3;cx.notify();}))))
            .child(self.panel("接入流程","首次使用：设备码授权 → 模型页测试 → 创建客户端密钥 → 选择 IP:端口并启动 → 配置 Trae。之后打开应用会自动启动网关。"))
            .into_any_element()
    }
    fn accounts(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut view=div().v_flex().gap_4().child(self.title("使用与参考项目一致的 Codex 设备码授权，无需预装 Codex。"))
            .child(div().h_flex().gap_3()
                .child(Button::new("device-sign-in").primary().label(if self.codex_connected {"添加 / 更新账号授权"} else {"使用 Codex 设备码登录"})
                    .disabled(self.busy || self.gateway_address.is_some()).on_click(cx.listener(|this,_,_,cx|{this.busy=true;this.send(Command::SignIn,cx);})))
                .child(Button::new("cancel-login").label("取消等待").disabled(!self.busy).on_click(cx.listener(|this,_,_,cx|this.send(Command::Cancel,cx)))))
            .child(self.panel("操作方式","获取设备码后，浏览器将打开 OpenAI 的 Codex 设备授权页。在该页面输入下面显示的代码，完成授权后应用会自动接收结果。"));
        if let Some((code, _)) = &self.device_code {
            view = view.child(
                div()
                    .v_flex()
                    .gap_3()
                    .p_5()
                    .bg(rgb(self.colors.surface))
                    .border_1()
                    .border_color(rgb(self.colors.warning))
                    .child("正在等待设备授权")
                    .child(
                        div()
                            .text_2xl()
                            .font_family("Consolas")
                            .child(code.to_string()),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_3()
                            .child(
                                Button::new("copy-device-code")
                                    .primary()
                                    .label("复制设备码")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some((code, _)) = &this.device_code {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                code.to_string(),
                                            ));
                                        }
                                    })),
                            )
                            .child(
                                Button::new("open-device-page")
                                    .label("打开授权页面")
                                    .on_click(|_, _, cx| cx.open_url(VERIFICATION_URL)),
                            ),
                    ),
            );
        }
        view=view.child(self.panel("本地账号与自动查询","账号逐条加密保存，重复授权更新原账号。应用运行时每小时查询额度，重新打开后补查到期账号；也可手动刷新。切换网关使用的账号前请先停止监听。"));
        if self.accounts.is_empty() {
            view = view.child(self.panel(
                "尚无已保存账号",
                "完成设备码授权后，账号与额度信息会显示在这里。",
            ));
        }
        for account in &self.accounts {
            let select_id = account.id.clone();
            let usage_id = account.id.clone();
            let checking = self.usage_busy.as_ref() == Some(&account.id);
            let mut card = div()
                .v_flex()
                .gap_3()
                .p_5()
                .bg(rgb(self.colors.surface))
                .border_1()
                .border_color(rgb(self.colors.border))
                .child(
                    div()
                        .h_flex()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .min_w_0()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!(
                                    "{}{}",
                                    account.display_name,
                                    if account.active {
                                        " · 当前使用"
                                    } else {
                                        ""
                                    }
                                )),
                        )
                        .child(
                            div()
                                .h_flex()
                                .gap_2()
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "select-{}",
                                        account.id
                                    )))
                                    .label("使用此账号")
                                    .disabled(
                                        account.active
                                            || self.busy
                                            || self.gateway_address.is_some(),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.send(Command::SelectAccount(select_id.clone()), cx)
                                    })),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "usage-{}",
                                        account.id
                                    )))
                                    .label(if checking {
                                        "查询中…"
                                    } else {
                                        "刷新额度"
                                    })
                                    .disabled(checking)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.status = "额度查询已排队".into();
                                        this.send(Command::RefreshUsage(usage_id.clone()), cx);
                                    })),
                                ),
                        ),
                );
            if let Some(quota) = account
                .usage_json
                .as_deref()
                .and_then(|s| serde_json::from_str::<Quota>(s).ok())
            {
                card = card.child(format!(
                    "套餐：{} · 可用额度重置次数：{} · 次数更新：{}",
                    quota.plan.as_deref().unwrap_or("未提供"),
                    quota
                        .reset_credits
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "未提供".into()),
                    format_time(quota.reset_checked_at)
                ));
                for window in quota.windows {
                    let period = match window.window_seconds {
                        Some(18000) => "（5 小时）".to_owned(),
                        Some(604800) => "（7 天）".to_owned(),
                        Some(n) if n % 3600 == 0 => format!("（{} 小时）", n / 3600),
                        Some(n) if n % 60 == 0 => format!("（{} 分钟）", n / 60),
                        Some(n) => format!("（{n} 秒）"),
                        None => String::new(),
                    };
                    card = card.child(format!(
                        "{}{}  ·  剩余 {}  ·  下次重置 {}",
                        window.name,
                        period,
                        window
                            .remaining_percent
                            .map(|n| format!("{n:.1}%"))
                            .unwrap_or_else(|| "未提供".into()),
                        format_time(window.reset_at)
                    ));
                }
                if quota.limit_reached == Some(true) {
                    card = card.child(
                        div()
                            .text_color(rgb(self.colors.warning))
                            .child("上游提示当前额度已达限制"),
                    );
                }
                if let Some(error) = quota.reset_error {
                    card = card.child(div().text_color(rgb(self.colors.warning)).child(error));
                }
            } else {
                card = card.child("尚无额度快照，等待查询或点击刷新额度。");
            }
            card = card.child(
                div()
                    .text_sm()
                    .text_color(rgb(self.colors.muted))
                    .child(format!(
                        "上次成功：{}\n最近查询：{}  ·  下次自动查询：{}",
                        format_time(account.usage_checked_at),
                        format_time(account.usage_attempted_at),
                        account
                            .usage_attempted_at
                            .map(|at| format_time(Some(at + 3600)))
                            .unwrap_or_else(|| "即将查询".into())
                    )),
            );
            if let Some(error) = &account.usage_error {
                card = card.child(
                    div()
                        .text_color(rgb(self.colors.warning))
                        .child(format!("{error} · 保留上次成功快照")),
                );
            }
            view = view.child(card);
        }
        view.into_any_element()
    }
    fn network_controls(&self, cx: &mut Context<Self>) -> Div {
        let mut view = div()
            .v_flex()
            .gap_3()
            .p_5()
            .bg(rgb(self.colors.surface))
            .border_1()
            .border_color(rgb(self.colors.border))
            .child("模型网关监听")
            .child("打开应用时自动恢复上次监听地址，默认 127.0.0.1:8787。修改地址请先停止网关；其他设备使用本机局域网或 VPN 地址。")
            .child(
                div()
                    .h_flex()
                    .gap_3()
                    .child(
                        Input::new(&self.bind_address)
                            .w(px(280.))
                            .disabled(self.gateway_address.is_some() || self.busy),
                    )
                    .child(
                        Button::new("start-gateway")
                            .primary()
                            .label("启动网关")
                            .disabled(
                                self.busy
                                    || self.gateway_address.is_some()
                                    || !self.codex_connected
                                    || self.keys.is_empty(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                let value = this.bind_address.read(cx).value().to_string();
                                this.send(
                                    Command::StartGateway(if value.trim().is_empty() {
                                        "127.0.0.1:8787".into()
                                    } else {
                                        value.trim().into()
                                    }),
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("stop-gateway")
                            .label("停止网关")
                            .disabled(self.gateway_address.is_none())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.send(Command::StopGateway, cx)),
                            ),
                    ),
            )
            .child(div().text_sm().text_color(rgb(self.colors.muted)).child(
                "HTTP 仅供可信局域网或个人 VPN。密钥和请求内容会经过所选网络；启动不会修改防火墙。",
            ));
        if let Some(address) = &self.gateway_address {
            let url = format!("http://{address}/v1");
            view=view.child(div().font_family("Consolas").child(url.clone()))
                .child(Button::new("copy-base-url").label("复制 Base URL").on_click(move|_,_,cx|cx.write_to_clipboard(ClipboardItem::new_string(url.clone()))))
                .child("Trae：选择 OpenAI 兼容接入，填写上面的 Base URL、本页创建的密钥，以及模型页返回的准确模型 ID。支持文本和函数工具；不支持的参数会明确报错。");
        }
        view
    }
    fn requests(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut view = div()
            .v_flex()
            .gap_3()
            .child(self.title("本地持久化元数据，每秒更新。执行中的请求置顶；保留 7 天或最近 10,000 条已结束记录。"))
            .child(div().h_flex().gap_2()
                .child(Button::new("request-tab").label(format!("请求记录 ({})", self.request_count)).when(!self.show_failures, |b|b.primary())
                    .on_click(cx.listener(|this,_,_,cx| {this.show_failures=false;cx.notify();})))
                .child(Button::new("failure-tab").label(format!("失败异常 ({})", self.failure_count)).when(self.show_failures, |b|b.primary())
                    .on_click(cx.listener(|this,_,_,cx| {this.show_failures=true;cx.notify();}))))
            .child(div().text_sm().text_color(rgb(self.colors.muted)).child("详情包含起止时间、阶段、首个输出和上游活动；不保存聊天、代码、工具参数、回答或认证信息。长耗时本身不代表失败。"));
        if self.log_write_failures > 0 {
            view = view.child(div().text_color(rgb(self.colors.error)).child(format!(
                "本次监听有 {} 次记录写入失败，部分记录可能不完整；请检查数据目录及磁盘。",
                self.log_write_failures
            )));
        }
        if let Some(record) = &self.selected_request {
            let failure = self
                .failures
                .iter()
                .find(|failure| failure.request_id == record.id);
            let metadata = request_detail_lines(record, failure);
            let detail_text = format!("请求详情\n{}", metadata.join("\n"));
            view = view.child(
                div()
                    .v_flex()
                    .min_w_0()
                    .p_4()
                    .gap_2()
                    .bg(rgb(self.colors.surface))
                    .rounded_lg()
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child("请求详情")
                            .child(
                                Button::new("copy-request-detail")
                                    .label("复制明细")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            detail_text.clone(),
                                        ));
                                        this.status = "请求明细已复制".into();
                                        this.error = false;
                                        cx.notify();
                                    })),
                            )
                            .child(Button::new("close-request-detail").label("收起").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.selected_request = None;
                                    cx.notify();
                                }),
                            )),
                    )
                    .children(metadata.into_iter().map(|line| div().text_sm().child(line))),
            );
        }
        if self.show_failures {
            if self.failures.is_empty() {
                view = view.child(self.panel(
                    "暂无失败异常",
                    "参数校验失败、上游异常、超时和取消会在这里单独保存，并关联请求 ID。",
                ));
            }
            for failure in &self.failures {
                let id = failure.request_id.clone();
                view = view.child(
                    div()
                        .v_flex()
                        .gap_2()
                        .py_3()
                        .border_b_1()
                        .border_color(rgb(self.colors.border))
                        .child(
                            div()
                                .h_flex()
                                .gap_3()
                                .child(
                                    div()
                                        .text_color(rgb(self.colors.error))
                                        .child(failure.code.clone()),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "failure-{}",
                                        failure.request_id
                                    )))
                                    .label("关联请求详情")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.send(Command::RequestDetail(id.clone()), cx)
                                    })),
                                ),
                        )
                        .child(failure.message.clone())
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(self.colors.muted))
                                .child(format!(
                                    "{} · {} · 异常状态 {} · {}",
                                    request_time(Some(failure.occurred_at)),
                                    request_phase(&failure.phase),
                                    failure.http_status,
                                    failure.request_id
                                )),
                        ),
                );
            }
        } else {
            if self.records.is_empty() {
                view = view.child(self.panel(
                    "暂无请求记录",
                    "客户端 API 请求进入时开始记录；旧版本只有内存记录，无法恢复历史正文或耗时。",
                ));
            }
            for record in &self.records {
                let id = record.id.clone();
                let color = match record.outcome.as_str() {
                    "completed" => self.colors.success,
                    "failed" | "interrupted" => self.colors.error,
                    _ => self.colors.warning,
                };
                view =
                    view.child(
                        div()
                            .v_flex()
                            .gap_2()
                            .py_3()
                            .border_b_1()
                            .border_color(rgb(self.colors.border))
                            .child(
                                div()
                                    .h_flex()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_color(rgb(color))
                                            .child(request_status(&record.outcome)),
                                    )
                                    .child(format!(
                                        "{} · {:.2} 秒{}",
                                        if record.model.is_empty() {
                                            "未提供模型"
                                        } else {
                                            &record.model
                                        },
                                        record.elapsed_ms as f64 / 1000.,
                                        if record.ended_at.is_none() && record.elapsed_ms >= 60000 {
                                            " · 长时间执行"
                                        } else {
                                            ""
                                        }
                                    ))
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "detail-{}",
                                            record.id
                                        )))
                                        .label("详情")
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.send(Command::RequestDetail(id.clone()), cx)
                                            }),
                                        ),
                                    ),
                            )
                            .child(div().text_sm().text_color(rgb(self.colors.muted)).child(
                                format!(
                                    "{} {} · {}",
                                    record.method,
                                    record.route,
                                    request_phase(&record.phase)
                                ),
                            ))
                            .child(div().text_sm().text_color(rgb(self.colors.muted)).child(
                                format!(
                                "开始 {} · {} {}",
                                request_time(Some(record.started_at)),
                                if record.outcome == "interrupted" { "恢复确认" } else { "结束" },
                                record
                                    .ended_at
                                    .map_or("执行中".into(), |t| request_time(Some(t)))
                            ),
                            )),
                    );
            }
        }
        let (page, total) = if self.show_failures {
            (self.failure_page, self.failure_count)
        } else {
            (self.request_page, self.request_count)
        };
        let errors = self.show_failures;
        view = view.child(
            div()
                .h_flex()
                .gap_3()
                .child(
                    Button::new("history-prev")
                        .label("上一页")
                        .disabled(page == 0)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let (requests, failures) = if errors {
                                (this.request_page, (this.failure_page - 1).max(0))
                            } else {
                                ((this.request_page - 1).max(0), this.failure_page)
                            };
                            this.send(Command::HistoryPage { requests, failures }, cx);
                        })),
                )
                .child(format!(
                    "第 {} / {} 页 · 每页 100 条",
                    page + 1,
                    ((total + 99) / 100).max(1)
                ))
                .child(
                    Button::new("history-next")
                        .label("下一页")
                        .disabled((page + 1) * 100 >= total)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let (requests, failures) = if errors {
                                (this.request_page, this.failure_page + 1)
                            } else {
                                (this.request_page + 1, this.failure_page)
                            };
                            this.send(Command::HistoryPage { requests, failures }, cx);
                        })),
                ),
        );
        view.into_any_element()
    }
    fn models(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = div().v_flex().gap_4().child(self.title("仅显示当前账号返回的模型，不预置虚构名称。"))
            .child(Button::new("refresh-models").primary().label("读取模型目录").disabled(self.busy || !self.codex_connected)
                .on_click(cx.listener(|this, _, _, cx| { this.busy = true; this.send(Command::RefreshModels, cx); })))
            .child(self.panel("测试说明", "点击“测试文本”会发送固定短提示并使用账号额度。目录、每次测试结果和时间按账号保存到本地，重启后恢复。"))
            .child(format!("目录获取时间：{}",format_time(self.models_checked_at)));
        if self.models.is_empty() {
            content = content.child(self.panel(
                "尚无模型目录",
                "先连接账号，再读取模型目录。目录失败时查看底部状态信息。",
            ));
        }
        for model in &self.models {
            let id = model.slug.clone();
            content = content.child(
                div()
                    .h_flex()
                    .justify_between()
                    .gap_4()
                    .p_4()
                    .border_b_1()
                    .border_color(rgb(self.colors.border))
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .min_w_0()
                            .child(model.display_name.clone())
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(self.colors.muted))
                                    .child(model.slug.clone()),
                            )
                            .child(
                                div().text_sm().text_color(rgb(self.colors.muted)).child(
                                    self.tests
                                        .iter()
                                        .find(|t| t.model_id == model.slug)
                                        .map(|t| {
                                            format!(
                                                "最近测试：{} · {}",
                                                test_status(&t.status),
                                                format_time(t.finished_at.or(Some(t.started_at)))
                                            )
                                        })
                                        .unwrap_or_else(|| "尚未测试".into()),
                                ),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("test-{}", model.slug)))
                            .label("测试文本")
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.busy = true;
                                this.test_result = None;
                                this.send(Command::TestModel(id.clone()), cx);
                            })),
                    ),
            );
        }
        if let Some(result) = &self.test_result {
            content = content.child(self.panel("本次测试结果", result.clone()));
        }
        if !self.tests.is_empty() {
            content = content.child(div().mt_4().text_lg().child("本地测试记录（最近 100 条）"));
        }
        for test in &self.tests {
            let body = test
                .text
                .as_deref()
                .or(test.error.as_deref())
                .unwrap_or("测试进行中");
            let preview = body.chars().take(4096).collect::<String>();
            content = content.child(self.panel(
                format!(
                    "{} · {} · {}",
                    test.model_id,
                    test_status(&test.status),
                    format_time(Some(test.started_at))
                ),
                format!(
                    "{}{}\n结束：{} · 输入 {} / 输出 {} tokens",
                    preview,
                    if body.chars().count() > 4096 {
                        "\n（界面显示前 4096 字，完整结果已保存在本地）"
                    } else {
                        ""
                    },
                    format_time(test.finished_at),
                    test.input_tokens
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "未提供".into()),
                    test.output_tokens
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "未提供".into())
                ),
            ));
        }
        content.into_any_element()
    }

    fn clients(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = div()
            .v_flex()
            .gap_4()
            .child(self.title("为不同设备分别命名密钥。OAuth 凭据不会提供给客户端。"))
            .child(self.network_controls(cx))
            .child(
                div()
                    .h_flex()
                    .gap_3()
                    .child(Input::new(&self.key_name).w(px(320.)).disabled(self.busy))
                    .child(
                        Button::new("create-key")
                            .primary()
                            .label("创建密钥")
                            .disabled(self.busy || self.new_key.is_some())
                            .on_click(cx.listener(|this, _, _, cx| {
                                let label = this.key_name.read(cx).value().to_string();
                                this.send(Command::CreateKey(label), cx);
                            })),
                    ),
            );
        if let Some((key, _)) = &self.new_key {
            content =
                content.child(
                    div()
                        .v_flex()
                        .gap_3()
                        .p_5()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(self.colors.warning))
                        .child("密钥已加密保存 · 两分钟后隐藏，可随时再次查看或复制")
                        .child(
                            div()
                                .text_sm()
                                .font_family("Consolas")
                                .child(key.value.to_string()),
                        )
                        .child(
                            div()
                                .h_flex()
                                .gap_3()
                                .child(Button::new("copy-key").label("复制密钥").on_click(
                                    cx.listener(|this, _, _, cx| {
                                        if let Some((key, _)) = &this.new_key {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                key.value.to_string(),
                                            ));
                                        }
                                        this.status =
                                            "已复制到系统剪贴板，请粘贴到自己的客户端".into();
                                        cx.notify();
                                    }),
                                ))
                                .child(Button::new("dismiss-key").label("隐藏密钥").on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.new_key = None;
                                        cx.notify();
                                    }),
                                )),
                        ),
                );
        }
        if self.keys.is_empty() {
            content = content.child(self.panel(
                "暂无客户端密钥",
                "创建密钥后可供自己的设备或 Agent 接入；撤销后会删除，不再展示。",
            ));
        }
        for key in &self.keys {
            let id = key.id.clone();
            let view_id = id.clone();
            let copy_id = id.clone();
            let confirming = self.revoke_id.as_ref() == Some(&id);
            content = content.child(
                div()
                    .h_flex()
                    .justify_between()
                    .gap_4()
                    .p_4()
                    .border_b_1()
                    .border_color(rgb(self.colors.border))
                    .child(
                        div().v_flex().gap_1().child(key.label.clone()).child(
                            div()
                                .text_sm()
                                .text_color(rgb(self.colors.muted))
                                .child(format!(
                                    "{}…  ·  有效 · {}",
                                    key.public_prefix,
                                    if key.recoverable {
                                        "可再次查看"
                                    } else {
                                        "旧密钥仅有摘要，无法还原"
                                    }
                                )),
                        ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("view-{id}")))
                            .label("查看")
                            .disabled(!key.recoverable)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.send(
                                    Command::RevealKey {
                                        id: view_id.clone(),
                                        copy: false,
                                    },
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("copy-{id}")))
                            .label("复制")
                            .disabled(!key.recoverable)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.send(
                                    Command::RevealKey {
                                        id: copy_id.clone(),
                                        copy: true,
                                    },
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("revoke-{id}")))
                            .label(if confirming {
                                "确认撤销并删除"
                            } else {
                                "撤销"
                            })
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.revoke_id.as_ref() == Some(&id) {
                                    this.revoke_id = None;
                                    this.send(Command::RevokeKey(id.clone()), cx);
                                } else {
                                    this.revoke_id = Some(id.clone());
                                    cx.notify();
                                }
                            })),
                    ),
            );
        }
        content.into_any_element()
    }
    fn settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let appearance = div()
            .v_flex()
            .gap_3()
            .p_5()
            .rounded_lg()
            .bg(rgb(self.colors.surface))
            .border_1()
            .border_color(rgb(self.colors.border))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("外观主题"))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(self.colors.muted))
                    .child("默认跟随电脑的应用主题；也可以固定使用白天或夜间模式。"),
            )
            .child(
                div().h_flex().flex_wrap().gap_2().children(
                    [
                        (ThemePreference::System, "跟随电脑"),
                        (ThemePreference::Light, "白天模式"),
                        (ThemePreference::Dark, "夜间模式"),
                    ]
                    .into_iter()
                    .map(|(preference, label)| {
                        Button::new(SharedString::from(format!("theme-{}", preference.as_str())))
                            .label(label)
                            .when(preference == self.theme_preference, |button| {
                                button.primary()
                            })
                            .disabled(!self.theme_loaded || self.theme_saving)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_theme_preference(preference, cx);
                            }))
                    }),
                ),
            )
            .child(div().text_sm().text_color(rgb(self.colors.muted)).child(
                if !self.theme_loaded {
                    "正在读取主题设置…"
                } else if self.theme_saving {
                    "正在保存主题设置…"
                } else if self.colors == theme::Palette::for_mode(ThemeMode::Dark) {
                    "当前显示夜间模式 · 点击选项后生效，重启后保留选择。"
                } else {
                    "当前显示白天模式 · 点击选项后生效，重启后保留选择。"
                },
            ));
        div().v_flex().gap_4()
            .child(self.title("应用信息、主题与在线更新。"))
            .child(appearance)
            .child(self.online_updates(cx))
            .child(self.panel("桌面与后台", "GPUI Kit 0.7.3 · Tokio · Axum 模型网关 · SQLite · 本机凭据保护 · Codex 设备码认证"))
            .child(self.panel("窗口与网络", if cfg!(windows) { "打开应用时自动启动网关，沿用上次成功监听地址。关闭窗口默认收起到托盘，网关继续运行；点击托盘图标或再次打开应用可恢复窗口。右键托盘选择退出，或点击下方“退出应用”，将停止网关并结束应用。开机自启尚未实现。" } else { "打开应用时自动启动网关，沿用上次成功监听地址。需要已登录账号和有效客户端密钥；失败后可在客户端接入页重试。关闭窗口即停止网关并退出。托盘与开机自启尚未实现。" }))
            .when_some(self.tray_notice.as_ref(), |column, notice| column.child(div().text_color(rgb(self.colors.error)).child(notice.clone())))
            .child(Button::new("quit-application").label("退出应用").on_click(|_, _, cx| cx.quit()))
            .child(self.panel("界面风格", "hashicorp · 白天与夜间配色、分层表面与明确状态。"))
            .child(self.panel("本地数据", if cfg!(target_os="macos") { "默认 ~/.tiny-proxy。账号 Token 与可查看密钥采用 AES-256-GCM，加密密钥保存在本机钥匙串。SQLite 保存模型、测试与额度，不是全库加密；只复制数据库不能迁移凭据。额度每小时查询，退出暂停。" } else { "默认 %USERPROFILE%/.tiny-proxy。首次启动复制旧目录历史数据并保留原库。账号 Token 与可查看密钥使用 DPAPI，模型、测试与额度保存在 SQLite，不是全库加密。额度每小时查询，退出暂停。" }))
            .into_any_element()
    }
}

impl Render for Desktop {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.page {
            0 => self.overview(cx),
            1 => self.accounts(cx),
            2 => self.models(cx),
            3 => self.clients(cx),
            4 => self.requests(cx),
            _ => self.settings(cx),
        };
        div()
            .h_flex()
            .size_full()
            .bg(rgb(self.colors.canvas))
            .text_color(rgb(self.colors.ink))
            .text_sm()
            .child(
                div()
                    .v_flex()
                    .w(px(208.))
                    .h_full()
                    .flex_shrink_0()
                    .border_r_1()
                    .border_color(rgb(self.colors.border))
                    .p_4()
                    .gap_2()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .mt_3()
                            .mb_1()
                            .child("Tiny Proxy"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(self.colors.muted))
                            .mb_6()
                            .child("个人模型网关"),
                    )
                    .children(PAGES.iter().enumerate().map(|(index, label)| {
                        Button::new(SharedString::from(format!("nav-{index}")))
                            .label(*label)
                            .w_full()
                            .when(index == self.page, |button| button.primary())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.page = index;
                                this.new_key = None;
                                this.revoke_id = None;
                                cx.notify();
                            }))
                    }))
                    .child(div().flex_1())
                    .when_some(self.update_sidebar_label(), |column, label| {
                        column.child(Button::new("update-notice").label(label).w_full().on_click(
                            cx.listener(|this, _, _, cx| {
                                this.page = 5;
                                this.update_notice = None;
                                cx.notify();
                            }),
                        ))
                    })
                    .child(div().text_xs().text_color(rgb(self.colors.warning)).child(
                        if self.gateway_address.is_some() {
                            "网关运行中 · Codex"
                        } else {
                            "网关已停止 · Codex"
                        },
                    )),
            )
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .id("page-scroll")
                            .flex_1()
                            .overflow_y_scroll()
                            .p_8()
                            .child(content),
                    )
                    .child(
                        div()
                            .px_6()
                            .py_3()
                            .border_t_1()
                            .border_color(rgb(self.colors.border))
                            .text_sm()
                            .text_color(rgb(if self.error {
                                self.colors.error
                            } else {
                                self.colors.muted
                            }))
                            .child(self.status.clone()),
                    ),
            )
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
                    size(px(1120.), px(760.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Tiny Proxy · Codex 网关".into()),
                    ..Default::default()
                }),
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
mod request_detail_tests {
    use super::{FailureRecord, Record, request_detail_lines};

    fn record() -> Record {
        Record::new(
            "request-fixture".into(),
            "POST".into(),
            "/v1/chat/completions".into(),
            1_000,
        )
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
        let text = request_detail_lines(&record, None).join("\n");
        for expected in [
            "模型 model-fixture · 完成",
            "耗时 2.50 秒",
            "结束阶段：已结束",
            "首批上游数据：0.25 秒 · 首个输出：0.50 秒",
            "HTTP：200 · 上游 HTTP：200",
            "流式：否 · 请求大小：512 字节 · 消息/输入项：2 · 工具定义：0",
            "输入/输出 tokens：100 / 0",
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
