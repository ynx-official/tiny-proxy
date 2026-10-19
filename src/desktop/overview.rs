use super::overview_format::{cache_read, duration, number, percentage, token_total};
use super::*;
use gpui_kit::component::tooltip::Tooltip;
use tiny_proxy::overview::OverviewPeriod;

impl Desktop {
    pub(super) fn refresh_overview(
        &mut self,
        period: Option<OverviewPeriod>,
        cx: &mut Context<Self>,
    ) {
        let period = period.unwrap_or(self.overview.period);
        if self.pending.installing || (self.overview.loading && period == self.overview.period) {
            return;
        }
        let request_id = self.overview.begin(period, Instant::now());
        self.send(Command::LoadOverview { request_id, period }, cx);
    }

    pub(super) fn overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.colors;
        let mut periods = div()
            .h_flex()
            .rounded(px(6.))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(colors.border));
        for (index, period) in OverviewPeriod::ALL.into_iter().enumerate() {
            let selected = self.overview.period == period;
            let style = ButtonCustomVariant::new(cx)
                .color(
                    rgb(if selected {
                        colors.selected
                    } else {
                        colors.surface
                    })
                    .into(),
                )
                .foreground(rgb(colors.ink).into())
                .hover(rgb(colors.button_hover).into())
                .active(rgb(colors.button_active).into());
            periods = periods.child(
                button(SharedString::from(format!("overview-period-{index}")))
                    .custom(style)
                    .rounded(ButtonRounded::None)
                    .h(px(34.))
                    .px(px(14.))
                    .label(period.label())
                    .toggled(selected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.overview.period != period {
                            this.refresh_overview(Some(period), cx);
                        }
                    })),
            );
        }
        let refreshed = self
            .overview
            .data
            .as_ref()
            .map(|snapshot| {
                chrono::DateTime::from_timestamp_millis(snapshot.window.end_ms)
                    .map(|at| {
                        at.with_timezone(&chrono::Local)
                            .format("%H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_else(|| "—".into())
            })
            .unwrap_or_else(|| "尚未读取".into());
        let header = div()
            .h_flex()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .gap(px(16.))
            .child(layout::page_title("概览"))
            .child(
                div()
                    .h_flex()
                    .gap(px(12.))
                    .flex_shrink_0()
                    .child(periods)
                    .child(
                        button("overview-refresh")
                            .icon(IconName::RefreshCw)
                            .w(px(36.))
                            .px(px(0.))
                            .accessibility_label("刷新统计")
                            .tooltip(format!("刷新统计 · 上次 {refreshed}"))
                            .disabled(self.overview.loading || !self.theme_loaded)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.refresh_overview(None, cx)),
                            ),
                    )
                    .child(self.gateway_button("overview-gateway", cx)),
            );
        let content = div()
            .v_flex()
            .w_full()
            .min_w_0()
            .gap(px(12.))
            .child(header)
            .child(self.overview_connection(cx))
            .child(self.overview_trend())
            .child(self.overview_metrics());
        self.scroll_page("overview-scroll", content)
            .into_any_element()
    }

    fn overview_connection(&self, cx: &mut Context<Self>) -> Div {
        let colors = self.colors;
        let active = self
            .accounts
            .iter()
            .find(|account| account.active)
            .map(|account| account.display_name.clone())
            .unwrap_or_else(|| "尚未连接账号".into());
        let value = self
            .gateway_address
            .as_ref()
            .map(|address| format!("http://{address}/v1"));
        let display = value.clone().unwrap_or_else(|| "服务尚未启动".into());
        self.card()
            .p(px(16.))
            .gap(px(12.))
            .child(
                div()
                    .h_flex()
                    .gap(px(18.))
                    .flex_wrap()
                    .child(
                        self.badge(
                            if self.gateway_address.is_some() {
                                "服务运行中"
                            } else {
                                "服务已停止"
                            },
                            if self.gateway_address.is_some() {
                                colors.success
                            } else {
                                colors.muted
                            },
                        )
                        .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(div().w(px(1.)).h(px(20.)).bg(rgb(colors.border)))
                    .child(
                        self.badge(
                            if self.codex_connected {
                                "账号已登录"
                            } else {
                                "账号未登录"
                            },
                            if self.codex_connected {
                                colors.success
                            } else {
                                colors.muted
                            },
                        )
                        .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(div().max_w(px(240.)).min_w_0().truncate().child(active))
                    .child(
                        button("overview-account")
                            .link()
                            .label(if self.codex_connected {
                                "管理账号"
                            } else {
                                "连接账号"
                            })
                            .h(px(24.))
                            .px(px(0.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(1);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .gap(px(12.))
                    .child(self.caption("连接地址").flex_shrink_0())
                    .child(
                        div()
                            .w(px(450.))
                            .min_w_0()
                            .max_w(px(480.))
                            .h(px(40.))
                            .h_flex()
                            .px(px(12.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(rgb(colors.border))
                            .bg(rgb(colors.canvas))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_family("Consolas")
                                    .text_size(px(17.))
                                    .child(display),
                            ),
                    )
                    .child(
                        button("overview-copy-url")
                            .label("复制")
                            .disabled(value.is_none())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(value) = &value {
                                    this.copy_text(value.clone(), "连接地址已复制", cx);
                                }
                            })),
                    )
                    .child(div().flex_1().min_w_0())
                    .child(
                        button("overview-clients")
                            .link()
                            .label(if self.keys.is_empty() {
                                "创建客户端密钥 →"
                            } else {
                                "配置客户端 →"
                            })
                            .px(px(0.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_client_tab(if this.keys.is_empty() { 1 } else { 0 });
                                cx.notify();
                            })),
                    ),
            )
    }

    fn overview_metric_label(&self, id: &'static str, label: &'static str, hint: String) -> Div {
        div()
            .h_flex()
            .gap(px(6.))
            .justify_center()
            .child(self.caption(label))
            .child(
                div()
                    .id(id)
                    .text_size(px(13.))
                    .text_color(rgb(self.colors.muted))
                    .child("ⓘ")
                    .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx)),
            )
    }

    fn overview_metrics(&self) -> Div {
        let totals = self.overview.data.as_ref().map(|snapshot| &snapshot.totals);
        let rate = totals.and_then(|totals| totals.cache_hit_rate());
        let cache_hint = totals.map(|t| format!(
            "缓存命中率 = 有效样本的缓存输入 Token 合计 ÷ 同批样本的输入 Token 合计。\n缓存 {} / 输入 {}，有效样本 {} / {}。\n旧记录、缺失/负值或缓存超过输入的记录不参与；零缓存有效，分母为零或合计超出计数范围显示未知。",
            t.cached_input_tokens.map_or("未知".into(), |n| n.to_string()),
            t.cache_input_tokens.map_or("未知".into(), |n| n.to_string()),
            t.cache_samples, t.requests
        )).unwrap_or_else(|| "使用上游实际提供的缓存输入明细，不用请求成功率代替；正在读取所选时段。".into());
        let cache_note = totals
            .map(|t| {
                format!(
                    "缓存 {} / 输入 {}",
                    number(t.cached_input_tokens),
                    number(t.cache_input_tokens)
                )
            })
            .unwrap_or_else(|| "缓存 — / 输入 —".into());
        let cache_column = div()
            .v_flex()
            .flex_1()
            .min_w_0()
            .px(px(6.))
            .items_center()
            .justify_center()
            .gap(px(6.))
            .child(
                self.overview_metric_label("overview-cache-info", "缓存命中率", cache_hint.clone())
                    .w_full()
                    .justify_start(),
            )
            .child(super::overview_chart::cache_gauge(rate, self.colors))
            .child(
                div()
                    .id("overview-cache-note")
                    .w_full()
                    .text_center()
                    .truncate()
                    .text_size(px(13.))
                    .line_height(relative(1.4))
                    .text_color(rgb(self.colors.muted))
                    .child(cache_note)
                    .tooltip(move |window, cx| Tooltip::new(cache_hint.clone()).build(window, cx)),
            )
            .when_some(
                totals.filter(|t| !t.cache_usage_complete() && t.requests > 0),
                |column, t| {
                    column.child(
                        self.caption(format!("有效样本 {} / {}", t.cache_samples, t.requests))
                            .text_size(px(12.))
                            .line_height(relative(1.2))
                            .truncate(),
                    )
                },
            );
        let mut strip = self
            .card()
            .h(px(182.))
            .flex_shrink_0()
            .p(px(12.))
            .h_flex()
            .items_stretch()
            .gap(px(0.))
            .child(cache_column);
        let retention_hint = self.overview.data.as_ref().map(|snapshot| {
            let time = |at| chrono::DateTime::from_timestamp_millis(at).map(|date| date.with_timezone(&chrono::Local).format("%m/%d %H:%M:%S").to_string()).unwrap_or_default();
            format!("生成请求 {} 条，范围 {}–{}。\n成功率 = 完整完成 ÷（完整完成 + 失败/中断/受限结束）；取消与进行中不参与。\n排除模型目录查询；仅统计本地保留记录（最多7天或10000条已结束请求）。", snapshot.totals.requests, time(snapshot.window.start_ms), time(snapshot.window.end_ms))
        }).unwrap_or_else(|| "正在读取所选时段的生成请求".into());
        let mut request_notes = totals
            .map(|t| {
                vec![
                    format!("成功率 {}", percentage(t.success_rate())),
                    format!(
                        "成功 {} · 失败 {} · 取消 {}",
                        t.completed, t.failed, t.cancelled
                    ),
                ]
            })
            .unwrap_or_else(|| vec!["成功率 —".into(), "成功 — · 失败 — · 取消 —".into()]);
        if let Some(t) = totals.filter(|t| t.running > 0) {
            request_notes.push(format!("进行中 {}", t.running));
        }
        let metrics = [
            ("overview-count-info", "生成请求", totals.map(|t| number(Some(t.requests))).unwrap_or_else(|| "—".into()), request_notes, retention_hint),
            ("overview-token-info", "Token 用量", totals.map(token_total).unwrap_or_else(|| "—".into()),
                totals.map(|t| vec![format!("输入 {} · 输出 {}", number(t.input_tokens), number(t.output_tokens)), format!("缓存读取 {}", cache_read(t))]).unwrap_or_else(|| vec!["输入 — · 输出 —".into(), "缓存读取 —".into()]),
                totals.map(|t| format!("完整输入/输出样本 {} / {}；缓存成对样本 {} / {}。\n精确输入 {} / 输出 {}；有效样本缓存读取 {}。\n缓存读取属于输入用量，已包含在总用量中，不再重复相加。≥ 表示已知用量下限；缺失不补零。", t.usage_samples, t.requests, t.cache_samples, t.requests,
                    t.input_tokens.map_or("未知".into(), |n| n.to_string()), t.output_tokens.map_or("未知".into(), |n| n.to_string()), t.cached_input_tokens.map_or("未知".into(), |n| n.to_string()))).unwrap_or_else(|| "仅使用上游实际提供的输入/输出及缓存用量".into())),
            ("overview-latency-info", "首个输出 P50", duration(totals.and_then(|t| t.p50_ms)), vec![format!("P95  {}", duration(totals.and_then(|t| t.p95_ms)))],
                totals.map(|t| format!("已观察到首个输出的流式请求：{} 个样本。\nP50、P95 均按最近秩计算；不使用首批上游字节代替。", t.latency_samples)).unwrap_or_else(|| "未观察到首个输出时显示未知".into())),
        ];
        for (id, label, value, notes, hint) in metrics {
            let mut detail = div().v_flex().w_full().min_w_0().gap(px(4.));
            for (index, note) in notes.into_iter().enumerate() {
                let note_hint = format!("{note}\n{hint}");
                detail = detail.child(
                    div()
                        .id(SharedString::from(format!("{id}-note-{index}")))
                        .w_full()
                        .text_center()
                        .truncate()
                        .text_size(px(13.))
                        .line_height(relative(1.4))
                        .text_color(rgb(self.colors.muted))
                        .child(note)
                        .tooltip(move |window, cx| {
                            Tooltip::new(note_hint.clone()).build(window, cx)
                        }),
                );
            }
            strip = strip.child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .justify_center()
                    .gap(px(12.))
                    .px(px(8.))
                    .border_l_1()
                    .border_color(rgb(self.colors.border))
                    .child(self.overview_metric_label(id, label, hint))
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_size(px(34.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(relative(1.2))
                            .truncate()
                            .child(value),
                    )
                    .child(detail),
            );
        }
        strip
    }
}
