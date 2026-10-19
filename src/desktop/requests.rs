use super::presentation::{matches_query, needs_stream_warning, timeline};
use super::*;

impl Desktop {
    pub(super) fn requests(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.request_search.read(cx).value().to_string();
        let mut content = div()
            .v_flex()
            .gap_4()
            .child(self.title())
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        self.tab(
                            "request-tab",
                            format!("全部请求  {}", self.request_count),
                            !self.show_failures,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_failures = false;
                            cx.notify();
                        })),
                    )
                    .child(
                        self.tab(
                            "failure-tab",
                            format!("失败异常  {}", self.failure_count),
                            self.show_failures,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_failures = true;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                input(&self.request_search)
                    .prefix(Icon::new(IconName::Search))
                    .cleanable(true),
            );
        if self.log_write_failures > 0 {
            content = content.child(
                self.panel(
                    "部分诊断记录未写入",
                    format!(
                        "本次监听有 {} 次记录写入失败，请检查数据目录及磁盘。",
                        self.log_write_failures
                    ),
                )
                .text_color(rgb(self.colors.error)),
            );
        }
        if self.show_failures {
            let filtered: Vec<_> = self
                .failures
                .iter()
                .filter(|failure| {
                    let record = self.records.iter().find(|r| r.id == failure.request_id);
                    let client = record
                        .map(|r| self.key_label(r.key_id.as_deref()))
                        .unwrap_or_default();
                    matches_query(
                        &query,
                        &[
                            &failure.request_id,
                            &failure.code,
                            &failure.message,
                            &failure.phase,
                            record.map(|r| r.model.as_str()).unwrap_or(""),
                            &client,
                        ],
                    )
                })
                .collect();
            let mut table = div()
                .v_flex()
                .min_w(px(628.))
                .bg(rgb(self.colors.surface))
                .child(self.table_header(&[
                    ("时间", 86.),
                    ("客户端", 142.),
                    ("模型", 160.),
                    ("结果", 100.),
                    ("操作", 54.),
                ]));
            for failure in &filtered {
                let id = failure.request_id.clone();
                let selected = self.requested_detail.as_ref() == Some(&id);
                let record = self.records.iter().find(|r| r.id == id);
                let client = record
                    .map(|r| self.key_label(r.key_id.as_deref()))
                    .unwrap_or_else(|| "见明细".into());
                let model = record
                    .map(|r| {
                        if r.model.is_empty() {
                            "未提供"
                        } else {
                            r.model.as_str()
                        }
                    })
                    .unwrap_or("见明细");
                let time = chrono::DateTime::from_timestamp_millis(failure.occurred_at)
                    .map(|t| {
                        t.with_timezone(&chrono::Local)
                            .format("%H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_else(|| "未提供".into());
                table = table.child(
                    div()
                        .h_flex()
                        .gap(px(12.))
                        .px(px(16.))
                        .h(px(48.))
                        .border_b_1()
                        .border_color(rgb(self.colors.soft_border))
                        .when(selected, |row| row.bg(rgb(self.colors.selected)))
                        .child(self.table_cell(time, 86.))
                        .child(self.table_cell(client, 142.))
                        .child(
                            self.table_cell(model.to_owned(), 160.)
                                .font_family("Consolas"),
                        )
                        .child(layout::table_column(100.).child(self.badge(
                            presentation::failure_label(&failure.code),
                            self.colors.error,
                        )))
                        .child(
                            layout::table_column(54.).child(
                                button(SharedString::from(format!("failure-{id}")))
                                    .link()
                                    .label("查看")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.send(Command::RequestDetail(id.clone()), cx)
                                    })),
                            ),
                        ),
                );
            }
            if filtered.is_empty() {
                content = content.child(self.panel(
                    if query.trim().is_empty() {
                        "暂无失败异常"
                    } else {
                        "本页没有匹配项"
                    },
                    "可清除搜索或切换分页",
                ));
            } else {
                content = content.child(
                    div()
                        .id("failure-table-scroll")
                        .overflow_x_scroll()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(self.colors.border))
                        .child(table),
                );
            }
        } else {
            let records: Vec<_> = self
                .records
                .iter()
                .filter(|r| {
                    matches_query(
                        &query,
                        &[
                            &r.id,
                            &r.model,
                            &r.route,
                            &self.key_label(r.key_id.as_deref()),
                            request_status(&r.outcome),
                            request_phase(&r.phase),
                        ],
                    )
                })
                .collect();
            if records.is_empty() {
                content = content.child(self.panel(
                    if query.trim().is_empty() {
                        "暂无请求记录"
                    } else {
                        "本页没有匹配项"
                    },
                    "可清除搜索或切换分页",
                ));
            } else {
                content = content.child(
                    div()
                        .id("request-table-scroll")
                        .overflow_x_scroll()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(self.colors.border))
                        .child(self.request_table(&records, cx)),
                );
            }
        }
        let (page, total) = if self.show_failures {
            (self.failure_page, self.failure_count)
        } else {
            (self.request_page, self.request_count)
        };
        let errors = self.show_failures;
        content = content.child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .gap_3()
                .flex_wrap()
                .child(self.caption(format!(
                    "第 {} / {} 页 · 每页 100 条",
                    page + 1,
                    ((total + 99) / 100).max(1)
                )))
                .child(
                    div()
                        .h_flex()
                        .gap_2()
                        .child(
                            button("history-prev")
                                .label("上一页")
                                .disabled(page == 0 || self.pending.history.is_some())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let (requests, failures) = if errors {
                                        (this.request_page, (this.failure_page - 1).max(0))
                                    } else {
                                        ((this.request_page - 1).max(0), this.failure_page)
                                    };
                                    this.send(Command::HistoryPage { requests, failures }, cx);
                                })),
                        )
                        .child(
                            button("history-next")
                                .label("下一页")
                                .disabled(
                                    (page + 1) * 100 >= total || self.pending.history.is_some(),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let (requests, failures) = if errors {
                                        (this.request_page, this.failure_page + 1)
                                    } else {
                                        (this.request_page + 1, this.failure_page)
                                    };
                                    this.send(Command::HistoryPage { requests, failures }, cx);
                                })),
                        ),
                ),
        );
        let mut frame = layout::page_frame().child(self.scroll_page("requests-scroll", content));
        if self.requested_detail.is_some() {
            frame = frame.child(self.drawer(self.request_drawer(cx)));
        }
        frame.into_any_element()
    }

    fn request_drawer(&self, cx: &mut Context<Self>) -> Div {
        let mut view = div().v_flex().gap(px(18.)).min_w_0().child(
            div()
                .h_flex()
                .justify_between()
                .pb(px(16.))
                .border_b_1()
                .border_color(rgb(self.colors.border))
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("请求详情"),
                )
                .child(
                    button("close-request-detail")
                        .ghost()
                        .icon(IconName::Close)
                        .tooltip("关闭详情")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.selected_request = None;
                            this.selected_failure = None;
                            this.requested_detail = None;
                            cx.notify();
                        })),
                ),
        );
        let Some(record) = &self.selected_request else {
            return view.child(self.caption("正在读取关联请求明细…"));
        };
        let detail_text = format!(
            "请求详情\n{}",
            request_detail_lines(record, self.selected_failure.as_ref()).join("\n")
        );
        view = view
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family("Consolas")
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_ellipsis()
                            .child(record.id.clone()),
                    )
                    .child(self.badge(
                        request_status(&record.outcome),
                        self.outcome_color(&record.outcome),
                    )),
            )
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        self.caption(format!("{} {}", record.method, record.route))
                            .font_family("Consolas")
                            .min_w_0(),
                    )
                    .child(
                        button("copy-request-detail")
                            .label("复制明细")
                            .icon(IconName::Copy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.copy_text(detail_text.clone(), "请求明细已复制", cx)
                            })),
                    ),
            )
            .child(self.caption(format!(
                "模型：{}",
                if record.model.is_empty() {
                    "未提供"
                } else {
                    &record.model
                }
            )));
        if let Some(failure) = self
            .selected_failure
            .as_ref()
            .filter(|f| f.request_id == record.id)
        {
            view = view.child(
                div()
                    .v_flex()
                    .gap(px(8.))
                    .p(px(16.))
                    .rounded(px(6.))
                    .border_1()
                    .border_l_2()
                    .border_color(rgb(self.colors.border))
                    .border_color(rgb(self.colors.error))
                    .bg(rgb(self.colors.canvas))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(self.colors.error))
                            .child(presentation::failure_label(&failure.code)),
                    )
                    .child(self.caption(failure.code.clone()).font_family("Consolas"))
                    .child(self.caption(failure.message.clone())),
            );
        }
        view = view.child(
            div()
                .text_size(px(17.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("请求时间线"),
        );
        let events = timeline(record);
        let mut time_view = div().v_flex();
        for (index, (label, time)) in events.iter().enumerate() {
            let last = index + 1 == events.len();
            let terminal = last && record.ended_at.is_some();
            time_view = time_view.child(
                div()
                    .h_flex()
                    .items_start()
                    .gap(px(12.))
                    .min_h(px(38.))
                    .child(
                        div()
                            .v_flex()
                            .items_center()
                            .w(px(10.))
                            .self_stretch()
                            .pt(px(5.))
                            .child(div().size(px(8.)).rounded_full().bg(rgb(if terminal {
                                self.outcome_color(&record.outcome)
                            } else {
                                self.colors.progress
                            })))
                            .when(!last, |column| {
                                column.child(div().w(px(1.)).flex_1().bg(rgb(self.colors.border)))
                            }),
                    )
                    .child(
                        div()
                            .w(px(106.))
                            .flex_shrink_0()
                            .font_family("Consolas")
                            .text_size(px(13.))
                            .text_color(rgb(self.colors.muted))
                            .child(presentation::short_time(*time)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(14.))
                            .text_color(rgb(if terminal {
                                self.outcome_color(&record.outcome)
                            } else {
                                self.colors.ink
                            }))
                            .child(*label),
                    ),
            );
        }
        view = view
            .child(time_view)
            .child(div().font_weight(FontWeight::SEMIBOLD).child(format!(
                "{} {}",
                if record.outcome == "interrupted" {
                    "中断前已观察耗时"
                } else {
                    "总耗时"
                },
                request_duration(record.elapsed_ms)
            )));
        let summary = presentation::request_summary(record);
        view = view.child(
            div()
                .v_flex()
                .gap(px(12.))
                .py(px(16.))
                .border_t_1()
                .border_color(rgb(self.colors.border))
                .children(summary.chunks(2).map(|pair| {
                    div()
                        .h_flex()
                        .items_start()
                        .gap(px(16.))
                        .children(pair.iter().map(|(label, value)| {
                            div()
                                .v_flex()
                                .flex_1()
                                .gap(px(4.))
                                .child(self.caption(*label))
                                .child(value.clone())
                        }))
                })),
        );
        if needs_stream_warning(record) {
            view = view.child(
                div()
                    .p(px(10.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(self.colors.warning))
                    .child(self.badge("HTTP 200 不代表流已完整结束", self.colors.warning)),
            );
        }
        view = view.child(
            button("toggle-request-metadata")
                .ghost()
                .icon(IconName::ChevronDown)
                .label(if self.details_expanded {
                    "收起完整诊断字段"
                } else {
                    "查看完整诊断字段"
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.details_expanded = !this.details_expanded;
                    cx.notify();
                })),
        );
        if self.details_expanded {
            view = view.child(
                div()
                    .id("request-metadata-scroll")
                    .overflow_x_scroll()
                    .child(
                        div().v_flex().gap(px(8.)).children(
                            request_metadata_lines(record, self.selected_failure.as_ref())
                                .into_iter()
                                .map(|line| self.caption(line)),
                        ),
                    ),
            );
        }
        view
    }
}
