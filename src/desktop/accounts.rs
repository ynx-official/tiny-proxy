use super::presentation::quota_fraction;
use super::*;

impl Desktop {
    fn quota_view(&self, account: &AccountSummary, details: bool) -> Div {
        let mut view = div().v_flex().gap(px(12.)).min_w_0().child(
            div()
                .h_flex()
                .justify_between()
                .gap(px(8.))
                .flex_wrap()
                .child(div().font_weight(FontWeight::SEMIBOLD).child("额度快照"))
                .child(self.caption(format!(
                    "上次查询 {}",
                    format_time(account.usage_checked_at)
                ))),
        );
        if let Some(quota) = account
            .usage_json
            .as_deref()
            .and_then(|json| serde_json::from_str::<Quota>(json).ok())
        {
            let mut windows = div().h_flex().items_stretch().flex_wrap().gap(px(24.));
            let empty = quota.windows.is_empty();
            for window in quota.windows {
                let period = match window.window_seconds {
                    Some(18000) => "5 小时窗口".into(),
                    Some(604800) => "每周额度".into(),
                    Some(n) if n % 3600 == 0 => format!("{} 小时窗口", n / 3600),
                    Some(n) => format!("{n} 秒窗口"),
                    None => window.name,
                };
                let fraction = quota_fraction(window.remaining_percent);
                let remaining = fraction
                    .map(|_| format!("{:.1}%", window.remaining_percent.unwrap_or_default()))
                    .unwrap_or_else(|| "未提供".into());
                let mut column = div().v_flex().flex_1().min_w(px(190.)).gap(px(8.)).child(
                    div()
                        .text_size(px(16.))
                        .child(format!("{period} · 剩余 {remaining}")),
                );
                if let Some(fraction) = fraction {
                    column = column.child(
                        div()
                            .h(px(8.))
                            .w_full()
                            .rounded_full()
                            .bg(rgb(self.colors.soft_border))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(fraction))
                                    .rounded_full()
                                    .bg(rgb(self.colors.progress)),
                            ),
                    );
                }
                windows = windows.child(
                    column
                        .child(self.caption(format!("下次重置 {}", format_time(window.reset_at)))),
                );
            }
            view = if empty {
                view.child(self.caption("上游未提供额度窗口"))
            } else {
                view.child(windows)
            };
            view = view.when(details, |view| {
                view.child(self.caption(format!(
                    "套餐 {} · 可用额度重置次数 {} · 次数更新 {}",
                    quota.plan.as_deref().unwrap_or("未提供"),
                    quota
                        .reset_credits
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "未提供".into()),
                    format_time(quota.reset_checked_at)
                )))
            });
            if quota.limit_reached == Some(true) {
                view = view.child(self.badge("上游提示额度已达限制", self.colors.warning));
            }
            if let Some(error) = quota.reset_error {
                view = view.child(self.caption(error).text_color(rgb(self.colors.warning)));
            }
        } else {
            view = view.child(self.caption("暂无额度快照"));
        }
        view = view.when(details, |view| {
            view.child(
                self.caption(format!(
                    "最近尝试 {} · 下次自动查询 {}",
                    format_time(account.usage_attempted_at),
                    account
                        .usage_attempted_at
                        .map(|at| format_time(Some(at + 3600)))
                        .unwrap_or_else(|| "即将查询".into())
                ))
                .text_size(px(13.)),
            )
        });
        if let Some(error) = &account.usage_error {
            view = view.child(
                self.caption(format!("{error} · 保留上次成功快照"))
                    .text_color(rgb(self.colors.warning)),
            );
        }
        view
    }

    pub(super) fn accounts(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = div()
            .v_flex()
            .gap(px(20.))
            .child(
                self.page_header(
                    div().h_flex().child(
                        self.action_button("device-sign-in")
                            .icon(IconName::Plus)
                            .label("添加账号")
                            .disabled(
                                self.busy
                                    || self.gateway_address.is_some()
                                    || self.pending.gateway.is_some()
                                    || self.pending.account.is_some(),
                            )
                            .when(self.gateway_address.is_some(), |button| {
                                button.tooltip("添加账号前请先停止服务")
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.start_login(cx))),
                    ),
                ),
            )
            .when(self.gateway_address.is_some(), |column| {
                column.child(
                    div()
                        .h_flex()
                        .gap(px(12.))
                        .flex_wrap()
                        .child(self.caption("添加或切换账号前，请先停止网关"))
                        .child(self.gateway_button("account-gateway", cx)),
                )
            });
        if let Some(account) = self.accounts.iter().find(|a| a.active) {
            let id = account.id.clone();
            let checking = self.usage_busy.as_ref() == Some(&account.id)
                || self.pending.usage.as_ref() == Some(&account.id);
            content = content.child(
                self.card()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .flex_wrap()
                            .child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .child(self.caption("当前账号"))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(account.display_name.clone()),
                                    ),
                            )
                            .child(self.badge("当前使用 · 凭据已保存", self.colors.success))
                            .child(
                                button("refresh-active-usage")
                                    .icon(IconName::RefreshCw)
                                    .label(if checking {
                                        "查询中…"
                                    } else {
                                        "刷新额度"
                                    })
                                    .disabled(
                                        self.usage_busy.is_some() || self.pending.usage.is_some(),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.send(Command::RefreshUsage(id.clone()), cx)
                                    })),
                            ),
                    )
                    .child(self.quota_view(account, false)),
            );
        }
        if self.accounts.is_empty() {
            content = content.child(self.panel(
                "尚无已保存账号",
                "使用 Codex 设备码在浏览器完成授权，无需预装 Codex。",
            ));
        } else {
            content = content.child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("已保存账号"),
            );
            let mut table = div()
                .v_flex()
                .min_w(px(660.))
                .bg(rgb(self.colors.surface))
                .child(self.table_header(&[("账号", 190.), ("状态", 100.), ("操作", 304.)]));
            for account in &self.accounts {
                let select_id = account.id.clone();
                let usage_id = account.id.clone();
                let detail_id = account.id.clone();
                let expanded = self.account_details.as_deref() == Some(account.id.as_str());
                table = table.child(
                    div()
                        .v_flex()
                        .gap(px(12.))
                        .p(px(16.))
                        .border_b_1()
                        .border_color(rgb(self.colors.soft_border))
                        .child(
                            div()
                                .h_flex()
                                .gap(px(12.))
                                .child(self.table_cell(account.display_name.clone(), 190.))
                                .child(self.table_cell(
                                    if account.active {
                                        "当前使用"
                                    } else {
                                        "已保存"
                                    },
                                    100.,
                                ))
                                .child(
                                    layout::table_column(304.).child(
                                        div()
                                            .h_flex()
                                            .gap(px(8.))
                                            .child(
                                                button(SharedString::from(format!(
                                                    "select-{}",
                                                    account.id
                                                )))
                                                .label(
                                                    if self.pending.account.as_ref()
                                                        == Some(&account.id)
                                                    {
                                                        "切换中…"
                                                    } else {
                                                        "设为当前"
                                                    },
                                                )
                                                .disabled(
                                                    account.active
                                                        || self.busy
                                                        || self.gateway_address.is_some()
                                                        || self.pending.gateway.is_some()
                                                        || self.pending.account.is_some(),
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.send(
                                                        Command::SelectAccount(select_id.clone()),
                                                        cx,
                                                    )
                                                })),
                                            )
                                            .child(
                                                button(SharedString::from(format!(
                                                    "usage-{}",
                                                    account.id
                                                )))
                                                .label(
                                                    if self.usage_busy.as_ref() == Some(&account.id)
                                                        || self.pending.usage.as_ref()
                                                            == Some(&account.id)
                                                    {
                                                        "查询中…"
                                                    } else {
                                                        "刷新额度"
                                                    },
                                                )
                                                .disabled(
                                                    self.usage_busy.is_some()
                                                        || self.pending.usage.is_some(),
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.send(
                                                        Command::RefreshUsage(usage_id.clone()),
                                                        cx,
                                                    )
                                                })),
                                            )
                                            .child(
                                                button(SharedString::from(format!(
                                                    "account-detail-{}",
                                                    account.id
                                                )))
                                                .ghost()
                                                .label(if expanded {
                                                    "收起"
                                                } else {
                                                    "额度详情"
                                                })
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.account_details =
                                                        if this.account_details.as_ref()
                                                            == Some(&detail_id)
                                                        {
                                                            None
                                                        } else {
                                                            Some(detail_id.clone())
                                                        };
                                                    cx.notify();
                                                })),
                                            ),
                                    ),
                                ),
                        )
                        .when(expanded, |row| row.child(self.quota_view(account, true))),
                );
            }
            content = content.child(
                div()
                    .id("account-table-scroll")
                    .overflow_x_scroll()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(self.colors.border))
                    .child(table),
            );
        }
        let mut frame = layout::page_frame().child(self.scroll_page("accounts-scroll", content));
        if self.login_open || self.device_code.is_some() {
            frame = frame.child(self.drawer(self.login_drawer(cx)));
        }
        frame.into_any_element()
    }

    fn login_drawer(&self, cx: &mut Context<Self>) -> Div {
        let mut view = div()
            .v_flex()
            .gap(px(20.))
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("添加 Codex 账号"),
                    )
                    .child(
                        button("close-login")
                            .ghost()
                            .icon(IconName::Close)
                            .tooltip("取消登录并关闭")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_login(cx))),
                    ),
            )
            .child(self.step_heading(1, "复制设备码"));
        if let Some((code, _)) = &self.device_code {
            view =
                view.child(
                    div()
                        .v_flex()
                        .gap(px(8.))
                        .p(px(16.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(self.colors.border))
                        .bg(rgb(self.colors.canvas))
                        .child(self.caption("设备码"))
                        .child(
                            div()
                                .h_flex()
                                .justify_between()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .text_size(px(26.))
                                        .font_family("Consolas")
                                        .child(code.to_string()),
                                )
                                .child(button("copy-device-code").label("复制").on_click(
                                    cx.listener(|this, _, _, cx| {
                                        if let Some((code, _)) = &this.device_code {
                                            this.copy_text(code.to_string(), "设备码已复制", cx);
                                        }
                                    }),
                                )),
                        ),
                );
        } else {
            view = view.child(self.caption(if self.busy {
                "正在申请设备码…"
            } else {
                "本次授权未完成，可重新申请"
            }));
        }
        view = view
            .child(self.step_heading(2, "打开浏览器并粘贴"))
            .child(
                self.action_button("open-device-page")
                    .icon(IconName::ExternalLink)
                    .label("打开授权页面")
                    .w_full()
                    .h(px(40.))
                    .disabled(self.device_code.is_none())
                    .on_click(|_, _, cx| cx.open_url(VERIFICATION_URL)),
            )
            .child(self.step_heading(3, "等待授权完成"));
        if let Some((_, until)) = &self.device_code {
            let seconds = until.saturating_duration_since(Instant::now()).as_secs();
            view = view.child(
                div()
                    .h_flex()
                    .justify_between()
                    .pt(px(16.))
                    .border_t_1()
                    .border_color(rgb(self.colors.soft_border))
                    .child(self.badge("等待浏览器授权", self.colors.accent))
                    .child(
                        div()
                            .font_family("Consolas")
                            .text_color(rgb(self.colors.accent))
                            .child(format!("{:02}:{:02}", seconds / 60, seconds % 60)),
                    ),
            );
        }
        view.when(self.error, |column| {
            column.child(
                self.caption(self.status.clone())
                    .text_color(rgb(self.colors.error)),
            )
        })
        .when(!self.busy, |column| {
            column.child(
                button("retry-login")
                    .label("重新申请")
                    .disabled(
                        self.gateway_address.is_some()
                            || self.pending.gateway.is_some()
                            || self.pending.account.is_some(),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.start_login(cx))),
            )
        })
        .child(
            div()
                .h_flex()
                .justify_between()
                .gap(px(12.))
                .pt(px(16.))
                .border_t_1()
                .border_color(rgb(self.colors.border))
                .child(
                    button("cancel-login")
                        .label("取消登录")
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_login(cx))),
                ),
        )
    }
}
