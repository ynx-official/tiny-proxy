use super::*;
use gpui_kit::component::Size as ControlSize;

pub(super) fn button(id: impl Into<ElementId>) -> Button {
    Button::new(id).h(px(36.)).px(px(14.))
}

pub(super) fn input(state: &Entity<InputState>) -> Input {
    Input::new(state).with_size(ControlSize::Large).h(px(40.))
}

impl Desktop {
    pub(super) fn navigate(&mut self, page: usize) {
        self.page = page;
        self.new_key = None;
        self.pending.reveal_requested = false;
        self.revoke_id = None;
        self.key_menu_open = false;
        self.model_menu_open = false;
    }

    pub(super) fn open_client_tab(&mut self, tab: usize) {
        self.navigate(3);
        self.client_tab = tab;
    }

    pub(super) fn action_button(&self, id: impl Into<ElementId>) -> Button {
        button(id).when(self.colors.canvas == 0x000000, |button| button.primary())
    }

    pub(super) fn gateway_control(&self) -> presentation::GatewayControl {
        presentation::gateway_control(
            self.gateway_address.is_some(),
            self.busy || self.pending.account.is_some(),
            self.codex_connected,
            !self.keys.is_empty(),
            self.pending.gateway,
        )
    }

    pub(super) fn gateway_button(
        &self,
        id: impl Into<ElementId>,
        cx: &mut Context<Self>,
    ) -> Button {
        let control = self.gateway_control();
        button(id)
            .label(control.label)
            .disabled(control.blocked.is_some())
            .when_some(control.blocked, |button, reason| button.tooltip(reason))
            .on_click(cx.listener(|this, _, _, cx| {
                let control = this.gateway_control();
                if control.blocked.is_none() {
                    let ip = this.bind_ip.read(cx).value();
                    let port = this.bind_port.read(cx).value();
                    match control.action.command(&ip, &port) {
                        Ok(command) => this.send(command, cx),
                        Err(message) => {
                            this.status = message;
                            this.error = true;
                            cx.notify();
                        }
                    }
                }
            }))
    }

    pub(super) fn title(&self) -> Div {
        self.page_header(div())
    }

    pub(super) fn page_header(&self, actions: Div) -> Div {
        div()
            .h_flex()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .gap_4()
            .pb(px(12.))
            .border_b_1()
            .border_color(rgb(self.colors.soft_border))
            .child(layout::page_title(PAGES[self.page]))
            .child(actions.flex_shrink_0())
    }

    pub(super) fn caption(&self, text: impl Into<SharedString>) -> Div {
        div()
            .text_size(px(layout::LABEL_SIZE))
            .line_height(relative(1.4))
            .text_color(rgb(self.colors.muted))
            .child(text.into())
    }

    pub(super) fn card(&self) -> Div {
        div()
            .v_flex()
            .min_w_0()
            .gap(px(16.))
            .p(px(20.))
            .rounded(px(8.))
            .bg(rgb(self.colors.surface))
            .border_1()
            .border_color(rgb(self.colors.border))
    }

    pub(super) fn panel(
        &self,
        title: impl Into<SharedString>,
        content: impl Into<SharedString>,
    ) -> Div {
        self.card()
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title.into()),
            )
            .child(self.caption(content))
    }

    pub(super) fn badge(&self, label: impl Into<SharedString>, color: u32) -> Div {
        div()
            .h_flex()
            .gap_2()
            .items_center()
            .min_w_0()
            .text_color(rgb(color))
            .child(
                div()
                    .size(px(9.))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(rgb(color)),
            )
            .child(div().min_w_0().text_ellipsis().child(label.into()))
    }

    pub(super) fn outcome_color(&self, outcome: &str) -> u32 {
        match outcome {
            "completed" => self.colors.success,
            "failed" | "interrupted" => self.colors.error,
            _ => self.colors.warning,
        }
    }

    pub(super) fn scroll_page(&self, id: &'static str, content: Div) -> Div {
        div().v_flex().flex_1().min_w_0().min_h_0().h_full().child(
            div()
                .id(id)
                .v_flex()
                .w_full()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_y_scroll()
                .p_6()
                .child(content.w_full().min_w_0().flex_shrink_0()),
        )
    }

    pub(super) fn drawer(&self, content: Div) -> Div {
        div()
            .v_flex()
            .w(px(440.))
            .flex_shrink_0()
            .h_full()
            .min_h_0()
            .bg(rgb(self.colors.surface))
            .border_l_1()
            .border_color(rgb(self.colors.border))
            .when(self.compact, |view| view.absolute().inset_0().w_full())
            .child(
                div()
                    .id("detail-drawer-scroll")
                    .v_flex()
                    .w_full()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_5()
                    .child(content.w_full().min_w_0().flex_shrink_0()),
            )
    }

    pub(super) fn copy_text(&mut self, value: String, label: &'static str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(value));
        self.status = label.into();
        self.error = false;
        self.toast = Some((label.into(), Instant::now()));
        cx.notify();
    }

    pub(super) fn copy_field(
        &self,
        id: &'static str,
        label: &'static str,
        value: Option<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        let available = value.is_some();
        let display = value
            .clone()
            .unwrap_or_else(|| "未监听 · 启动网关后可复制".into());
        div()
            .v_flex()
            .gap_2()
            .min_w_0()
            .child(self.caption(label))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_3()
                            .h(px(40.))
                            .h_flex()
                            .rounded_md()
                            .bg(rgb(self.colors.canvas))
                            .border_1()
                            .border_color(rgb(self.colors.border))
                            .font_family("Consolas")
                            .text_ellipsis()
                            .child(display),
                    )
                    .child(
                        button(id)
                            .h(px(40.))
                            .icon(IconName::Copy)
                            .label("复制")
                            .disabled(!available)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(value) = &value {
                                    this.copy_text(value.clone(), "连接信息已复制", cx);
                                }
                            })),
                    ),
            )
    }

    pub(super) fn key_label(&self, id: Option<&str>) -> String {
        id.map(|id| {
            self.keys
                .iter()
                .find(|k| k.id == id)
                .map(|k| k.label.clone())
                .unwrap_or_else(|| id.to_owned())
        })
        .unwrap_or_else(|| "未鉴权".into())
    }

    pub(super) fn table_cell(&self, text: impl Into<SharedString>, width: f32) -> Div {
        layout::table_column(width)
            .text_ellipsis()
            .child(text.into())
    }

    pub(super) fn tab(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        selected: bool,
    ) -> Button {
        let label = label.into();
        button(id)
            .ghost()
            .rounded(ButtonRounded::None)
            .h(px(42.))
            .px(px(16.))
            .accessibility_label(label.clone())
            .toggled(selected)
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(if selected {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .child(label),
            )
            .text_color(rgb(if selected {
                self.colors.accent
            } else {
                self.colors.muted
            }))
            .when(selected, |button| {
                button.border_b_2().border_color(rgb(self.colors.accent))
            })
    }

    pub(super) fn table_header(&self, columns: &[(&str, f32)]) -> Div {
        div()
            .h_flex()
            .gap_3()
            .px_4()
            .h(px(if self.page == 0 { 32. } else { 40. }))
            .bg(rgb(self.colors.button_hover))
            .text_color(rgb(self.colors.muted))
            .text_size(px(14.))
            .border_b_1()
            .border_color(rgb(self.colors.soft_border))
            .children(
                columns
                    .iter()
                    .map(|(label, width)| self.table_cell(label.to_string(), *width)),
            )
    }

    pub(super) fn step_heading(&self, number: u8, label: &'static str) -> Div {
        div()
            .h_flex()
            .gap(px(12.))
            .child(
                div()
                    .h_flex()
                    .justify_center()
                    .size(px(28.))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(rgb(self.colors.accent))
                    .text_color(rgb(if self.colors.canvas == 0 {
                        0x000000
                    } else {
                        0xffffff
                    }))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(number.to_string()),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label),
            )
    }

    pub(super) fn request_table(&self, records: &[&Record], cx: &mut Context<Self>) -> Div {
        let condensed = self.page == 4 && self.requested_detail.is_some();
        let mut columns = vec![
            (
                if self.page == 0 {
                    "时间"
                } else {
                    "开始时间"
                },
                86.,
            ),
            ("客户端", 142.),
            ("模型", 160.),
            ("状态", 90.),
        ];
        if !condensed {
            columns.push(("耗时", 82.));
        }
        columns.push(("操作", 54.));
        let mut table = div()
            .v_flex()
            .min_w(px(if condensed { 616. } else { 710. }))
            .bg(rgb(self.colors.surface))
            .child(self.table_header(&columns));
        for record in records {
            let id = record.id.clone();
            let selected = self
                .selected_request
                .as_ref()
                .is_some_and(|r| r.id == record.id);
            let time = chrono::DateTime::from_timestamp_millis(record.started_at)
                .map(|t| {
                    t.with_timezone(&chrono::Local)
                        .format("%H:%M:%S")
                        .to_string()
                })
                .unwrap_or_else(|| "未提供".into());
            let status = if record.ended_at.is_none() && record.phase == "receiving" {
                "接收中"
            } else {
                request_status(&record.outcome)
            };
            let mut row = div()
                .h_flex()
                .gap(px(12.))
                .px(px(16.))
                .h(px(if self.page == 0 { 34. } else { 44. }))
                .border_b_1()
                .border_color(rgb(self.colors.soft_border))
                .when(selected, |row| row.bg(rgb(self.colors.selected)))
                .child(self.table_cell(time, 86.))
                .child(self.table_cell(self.key_label(record.key_id.as_deref()), 142.))
                .child(
                    self.table_cell(
                        if record.model.is_empty() {
                            "未提供"
                        } else {
                            &record.model
                        },
                        160.,
                    )
                    .font_family("Consolas"),
                )
                .child(
                    layout::table_column(90.)
                        .child(self.badge(status, self.outcome_color(&record.outcome))),
                );
            if !condensed {
                row = row.child(self.table_cell(request_duration(record.elapsed_ms), 82.));
            }
            table = table.child(
                row.child(
                    layout::table_column(54.).child(
                        button(SharedString::from(format!("detail-{id}")))
                            .link()
                            .when(self.page == 0, |button| button.h(px(28.)))
                            .label("查看")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.navigate(4);
                                this.send(Command::RequestDetail(id.clone()), cx);
                            })),
                    ),
                ),
            );
        }
        table
    }

    pub(super) fn start_login(&mut self, cx: &mut Context<Self>) {
        if self.busy
            || self.gateway_address.is_some()
            || self.pending.gateway.is_some()
            || self.pending.account.is_some()
        {
            return;
        }
        self.login_open = true;
        self.device_code = None;
        self.busy = true;
        self.send(Command::SignIn, cx);
    }

    pub(super) fn cancel_login(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            self.send(Command::Cancel, cx);
        }
        self.login_open = false;
        self.device_code = None;
        cx.notify();
    }
}
