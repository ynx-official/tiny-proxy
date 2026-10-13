mod theme;

use gpui_kit::component::{
    button::*,
    input::{Input, InputState},
    *,
};
use gpui_kit::{prelude::*, *};
use std::time::{Duration, Instant};
use tiny_proxy::{
    app::{Command, Event},
    auth::codex::{Model, VERIFICATION_URL},
    auth::quota::Quota,
    gateway::Record,
    storage::{AccountSummary, CreatedKey, KeySummary, ModelTest},
};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

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

struct Desktop {
    page: usize,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
            page: 0,
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
        while let Ok(event) = self.events.try_recv() {
            changed = true;
            match event {
                Event::Snapshot {
                    keys,
                    codex_connected,
                    accounts,
                    models,
                    models_checked_at,
                    tests,
                } => {
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
                Event::Gateway { address, records } => {
                    self.gateway_address = address;
                    self.records = records;
                }
                Event::GatewayBind(address) => {
                    self.bind_address
                        .update(cx, |input, cx| input.set_value(address, window, cx));
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
                    .text_color(rgb(theme::MUTED))
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
                    .text_color(rgb(theme::MUTED))
                    .child(subtitle),
            )
    }

    fn panel(title: impl Into<SharedString>, content: impl Into<SharedString>) -> Div {
        div()
            .v_flex()
            .gap_3()
            .p_5()
            .rounded_lg()
            .bg(rgb(theme::SURFACE))
            .border_1()
            .border_color(rgb(theme::BORDER))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title.into()))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme::MUTED))
                    .child(content.into()),
            )
    }

    fn overview(&self, cx: &mut Context<Self>) -> AnyElement {
        div().v_flex().gap_4()
            .child(self.title("Codex 设备码登录 · 为自己的 Agent 提供模型 API。"))
            .child(Self::panel("服务状态",self.gateway_address.as_ref().map(|a|format!("运行中 · http://{a}/v1")).unwrap_or_else(||"已停止 · 在客户端接入页启动监听".into())))
            .child(Self::panel("Codex 账号",if self.codex_connected {"已有 Codex 授权凭据；请求前自动检查有效期并续期。模型可用性以调用结果为准。"} else {"尚未连接账号。请前往账号连接，使用设备码完成授权。"}))
            .child(div().h_flex().gap_3()
                .child(Button::new("overview-login").primary().label("账号连接").on_click(cx.listener(|this,_,_,cx|{this.page=1;cx.notify();})))
                .child(Button::new("overview-clients").label("配置 Trae 接入").on_click(cx.listener(|this,_,_,cx|{this.page=3;cx.notify();}))))
            .child(Self::panel("接入流程","首次使用：设备码授权 → 模型页测试 → 创建客户端密钥 → 选择 IP:端口并启动 → 配置 Trae。之后打开应用会自动启动网关。"))
            .into_any_element()
    }
    fn accounts(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut view=div().v_flex().gap_4().child(self.title("使用与参考项目一致的 Codex 设备码授权，无需预装 Codex。"))
            .child(div().h_flex().gap_3()
                .child(Button::new("device-sign-in").primary().label(if self.codex_connected {"添加 / 更新账号授权"} else {"使用 Codex 设备码登录"})
                    .disabled(self.busy || self.gateway_address.is_some()).on_click(cx.listener(|this,_,_,cx|{this.busy=true;this.send(Command::SignIn,cx);})))
                .child(Button::new("cancel-login").label("取消等待").disabled(!self.busy).on_click(cx.listener(|this,_,_,cx|this.send(Command::Cancel,cx)))))
            .child(Self::panel("操作方式","获取设备码后，浏览器将打开 OpenAI 的 Codex 设备授权页。在该页面输入下面显示的代码，完成授权后应用会自动接收结果。"));
        if let Some((code, _)) = &self.device_code {
            view = view.child(
                div()
                    .v_flex()
                    .gap_3()
                    .p_5()
                    .bg(rgb(theme::SURFACE))
                    .border_1()
                    .border_color(rgb(theme::WARNING))
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
        view=view.child(Self::panel("本地账号与自动查询","账号逐条加密保存，重复授权更新原账号。应用运行时每小时查询额度，重新打开后补查到期账号；也可手动刷新。切换网关使用的账号前请先停止监听。"));
        if self.accounts.is_empty() {
            view = view.child(Self::panel(
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
                .bg(rgb(theme::SURFACE))
                .border_1()
                .border_color(rgb(theme::BORDER))
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
                            .text_color(rgb(theme::WARNING))
                            .child("上游提示当前额度已达限制"),
                    );
                }
                if let Some(error) = quota.reset_error {
                    card = card.child(div().text_color(rgb(theme::WARNING)).child(error));
                }
            } else {
                card = card.child("尚无额度快照，等待查询或点击刷新额度。");
            }
            card = card.child(div().text_sm().text_color(rgb(theme::MUTED)).child(format!(
                    "上次成功：{}\n最近查询：{}  ·  下次自动查询：{}",
                    format_time(account.usage_checked_at),
                    format_time(account.usage_attempted_at),
                    account
                        .usage_attempted_at
                        .map(|at| format_time(Some(at + 3600)))
                        .unwrap_or_else(|| "即将查询".into())
                )));
            if let Some(error) = &account.usage_error {
                card = card.child(
                    div()
                        .text_color(rgb(theme::WARNING))
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
            .bg(rgb(theme::SURFACE))
            .border_1()
            .border_color(rgb(theme::BORDER))
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
            .child(div().text_sm().text_color(rgb(theme::MUTED)).child(
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
    fn requests(&self) -> AnyElement {
        let mut view = div()
            .v_flex()
            .gap_3()
            .child(self.title("当前监听会话最近 100 条生成请求；仅保存在内存，重启监听后清空。"));
        if self.records.is_empty() {
            view=view.child(Self::panel("暂无请求记录","客户端发起模型生成请求后显示模型、结果、耗时与请求 ID。不记录提示词、回答和工具参数。"));
        }
        for record in &self.records {
            view = view.child(Self::panel(
                format!(
                    "{} · {} · {} ms",
                    record.model, record.outcome, record.elapsed_ms
                ),
                format!("{} · {}", record.route, record.id),
            ));
        }
        view.into_any_element()
    }
    fn models(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = div().v_flex().gap_4().child(self.title("仅显示当前账号返回的模型，不预置虚构名称。"))
            .child(Button::new("refresh-models").primary().label("读取模型目录").disabled(self.busy || !self.codex_connected)
                .on_click(cx.listener(|this, _, _, cx| { this.busy = true; this.send(Command::RefreshModels, cx); })))
            .child(Self::panel("测试说明", "点击“测试文本”会发送固定短提示并使用账号额度。目录、每次测试结果和时间按账号保存到本地，重启后恢复。"))
            .child(format!("目录获取时间：{}",format_time(self.models_checked_at)));
        if self.models.is_empty() {
            content = content.child(Self::panel(
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
                    .border_color(rgb(theme::BORDER))
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .min_w_0()
                            .child(model.display_name.clone())
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(theme::MUTED))
                                    .child(model.slug.clone()),
                            )
                            .child(
                                div().text_sm().text_color(rgb(theme::MUTED)).child(
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
            content = content.child(Self::panel("本次测试结果", result.clone()));
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
            content = content.child(Self::panel(
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
                        .border_color(rgb(theme::WARNING))
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
            content = content.child(Self::panel(
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
                    .border_color(rgb(theme::BORDER))
                    .child(div().v_flex().gap_1().child(key.label.clone()).child(
                        div().text_sm().text_color(rgb(theme::MUTED)).child(format!(
                            "{}…  ·  有效 · {}",
                            key.public_prefix,
                            if key.recoverable {
                                "可再次查看"
                            } else {
                                "旧密钥仅有摘要，无法还原"
                            }
                        )),
                    ))
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
}

impl Render for Desktop {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.page {
            0 => self.overview(cx), 1 => self.accounts(cx), 2 => self.models(cx), 3 => self.clients(cx),
            4 => self.requests(),
            _ => div().v_flex().gap_4().child(self.title("Tiny Proxy 0.1.0 · Codex 网关开发版"))
                .child(Self::panel("桌面与后台", "GPUI Kit 0.7.3 · Tokio · Axum 模型网关 · SQLite · Windows 用户级 DPAPI · Codex 设备码认证"))
                .child(Self::panel("窗口与网络", "打开应用时自动启动网关，沿用上次成功监听地址。需要已登录账号和有效客户端密钥；失败后可在客户端接入页重试。关闭窗口即停止网关并退出。托盘与开机自启尚未实现。"))
                .child(Self::panel("界面风格", "hashicorp · 黑色画布、分层表面与明确状态。核心服务不依赖 UI，未引入 Web 前端。"))
                .child(Self::panel("本地数据", "默认保存在 %USERPROFILE%/.tiny-proxy。首次启动自动复制旧目录中的历史数据，保留原库。账号 Token 与可查看密钥使用用户级 DPAPI 加密；模型目录、测试结果与时间、额度快照保存在 SQLite。不是全库加密。额度每小时查询，退出应用后暂停。"))
                .into_any_element(),
        };
        div()
            .h_flex()
            .size_full()
            .bg(rgb(theme::CANVAS))
            .text_color(rgb(theme::INK))
            .text_sm()
            .child(
                div()
                    .v_flex()
                    .w(px(208.))
                    .h_full()
                    .flex_shrink_0()
                    .border_r_1()
                    .border_color(rgb(theme::BORDER))
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
                            .text_color(rgb(theme::MUTED))
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
                    .child(div().text_xs().text_color(rgb(theme::WARNING)).child(
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
                            .border_color(rgb(theme::BORDER))
                            .text_sm()
                            .text_color(rgb(if self.error {
                                theme::ERROR
                            } else {
                                theme::MUTED
                            }))
                            .child(self.status.clone()),
                    ),
            )
    }
}

pub fn run(commands: mpsc::Sender<Command>, events: mpsc::Receiver<Event>) {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::apply(cx);
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
                cx.new(|cx| Desktop::new(commands, events, window, cx))
            })
            .expect("Tiny Proxy window could not be opened");
            cx.activate(true);
        });
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
