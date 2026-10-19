use super::*;

impl Render for Desktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pending.installing
            || matches!(
                self.update.status,
                tiny_proxy::updater::UpdateStatus::Installing
            )
        {
            return div()
                .v_flex()
                .items_center()
                .justify_center()
                .gap_3()
                .size_full()
                .bg(rgb(self.colors.canvas))
                .text_color(rgb(self.colors.ink))
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("正在安装更新…"),
                )
                .child(self.caption("完成后将自动重启"));
        }
        self.compact = window.bounds().size.width < px(1180.);
        let content = match self.page {
            0 => self.overview(cx),
            1 => self.accounts(cx),
            2 => self.models(cx),
            3 => self.clients(cx),
            4 => self.requests(cx),
            _ => self.settings(cx),
        };
        let sidebar = self.sidebar(cx);
        let dark = self.colors == theme::Palette::for_mode(ThemeMode::Dark);
        let footer = div()
            .h_flex()
            .items_center()
            .flex_wrap()
            .gap_4()
            .px_4()
            .py_2()
            .border_t_1()
            .border_color(rgb(self.colors.border))
            .bg(rgb(self.colors.sidebar))
            .text_xs()
            .child(self.badge(
                if self.gateway_address.is_some() {
                    "服务：运行中"
                } else {
                    "服务：已停止"
                },
                if self.gateway_address.is_some() {
                    self.colors.success
                } else {
                    self.colors.muted
                },
            ))
            .child(self.badge(
                if self.codex_connected {
                    "账号：已登录"
                } else {
                    "账号：未登录"
                },
                if self.codex_connected {
                    self.colors.success
                } else {
                    self.colors.warning
                },
            ))
            .child(
                self.caption(
                    self.gateway_address
                        .clone()
                        .unwrap_or_else(|| "未监听".into()),
                )
                .font_family("Consolas"),
            )
            .child(div().flex_1())
            .child(self.caption(if dark { "夜间模式" } else { "白天模式" }));
        div()
            .v_flex()
            .relative()
            .size_full()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key != "escape" {
                    return;
                }
                if this.page == 1 && (this.login_open || this.device_code.is_some()) {
                    this.cancel_login(cx);
                } else if this.page == 4 {
                    this.selected_request = None;
                    this.selected_failure = None;
                    this.requested_detail = None;
                } else if this.page == 3 {
                    this.key_menu_open = false;
                    this.model_menu_open = false;
                    this.revoke_id = None;
                    this.new_key = None;
                    this.pending.reveal_requested = false;
                } else if this.page == 5 {
                    this.confirm_update = false;
                }
                cx.notify();
            }))
            .bg(rgb(self.colors.canvas))
            .text_color(rgb(self.colors.ink))
            .text_size(px(layout::BODY_SIZE))
            .line_height(relative(1.5))
            .child(
                layout::workspace().child(sidebar).child(
                    div()
                        .v_flex()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .h_full()
                        .child(content),
                ),
            )
            .when(self.error, |root| {
                root.child(
                    div()
                        .h_flex()
                        .items_start()
                        .gap_3()
                        .px_4()
                        .py_2()
                        .border_t_1()
                        .border_color(rgb(self.colors.error))
                        .text_size(px(14.))
                        .text_color(rgb(self.colors.error))
                        .child(div().flex_1().min_w_0().child(self.status.clone()))
                        .child(
                            button("dismiss-error")
                                .ghost()
                                .icon(IconName::Close)
                                .tooltip("关闭提示")
                                .h(px(24.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.error = false;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(footer)
            .when_some(self.toast.as_ref(), |root, (text, _)| {
                root.child(
                    div()
                        .absolute()
                        .bottom(px(52.))
                        .right_6()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .p_3()
                        .rounded_md()
                        .bg(rgb(self.colors.surface))
                        .border_1()
                        .border_color(rgb(self.colors.success))
                        .child(
                            Icon::new(IconName::CircleCheck).text_color(rgb(self.colors.success)),
                        )
                        .child(text.clone()),
                )
            })
    }
}
