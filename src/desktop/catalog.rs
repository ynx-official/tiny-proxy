use super::*;

impl Desktop {
    pub(super) fn models(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.model_search.read(cx).value().to_string();
        let mut content = div()
            .v_flex()
            .gap(px(18.))
            .child(
                self.page_header(
                    div().h_flex().child(
                        self.action_button("refresh-models")
                            .icon(IconName::RefreshCw)
                            .label(if self.pending.refreshing_models {
                                "刷新中…"
                            } else {
                                "刷新目录"
                            })
                            .disabled(
                                self.busy
                                    || !self.codex_connected
                                    || self.pending.account.is_some(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.busy = true;
                                this.send(Command::RefreshModels, cx);
                            })),
                    ),
                ),
            )
            .child(
                div()
                    .h_flex()
                    .gap(px(16.))
                    .flex_wrap()
                    .child(
                        input(&self.model_search)
                            .prefix(Icon::new(IconName::Search))
                            .cleanable(true)
                            .flex_1()
                            .min_w(px(240.)),
                    )
                    .child(
                        self.caption(format!("最近同步 {}", format_time(self.models_checked_at))),
                    ),
            );
        let models: Vec<_> = self
            .models
            .iter()
            .filter(|m| presentation::matches_query(&query, &[&m.slug, &m.display_name]))
            .collect();
        if models.is_empty() {
            content = content.child(self.panel(
                if self.models.is_empty() {
                    "尚无模型目录"
                } else {
                    "没有匹配的模型"
                },
                "连接账号并刷新目录，或清除搜索。",
            ));
        } else {
            let mut table = div()
                .v_flex()
                .min_w(px(832.))
                .bg(rgb(self.colors.surface))
                .child(self.table_header(&[
                    ("模型", 160.),
                    ("模型 ID", 200.),
                    ("文本验证", 100.),
                    ("操作", 304.),
                ]));
            for model in models {
                let test_id = model.slug.clone();
                let copy_id = model.slug.clone();
                let choose_id = model.slug.clone();
                let last_test = self.tests.iter().find(|t| t.model_id == model.slug);
                table = table.child(
                    div()
                        .h_flex()
                        .gap(px(12.))
                        .px(px(16.))
                        .py(px(12.))
                        .border_b_1()
                        .border_color(rgb(self.colors.soft_border))
                        .child(
                            self.table_cell(model.display_name.clone(), 160.)
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            self.table_cell(model.slug.clone(), 200.)
                                .font_family("Consolas"),
                        )
                        .child(
                            layout::table_column(100.)
                                .v_flex()
                                .gap(px(4.))
                                .child(
                                    last_test
                                        .map(|t| test_status(&t.status))
                                        .unwrap_or("尚未测试"),
                                )
                                .when_some(last_test, |column, test| {
                                    column.child(
                                        self.caption(format_time(
                                            test.finished_at.or(Some(test.started_at)),
                                        ))
                                        .text_size(px(12.))
                                        .text_ellipsis(),
                                    )
                                }),
                        )
                        .child(
                            layout::table_column(304.).child(
                                div()
                                    .h_flex()
                                    .gap(px(8.))
                                    .child(
                                        button(SharedString::from(format!(
                                            "copy-model-{}",
                                            model.slug
                                        )))
                                        .icon(IconName::Copy)
                                        .label("ID")
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.copy_text(
                                                    copy_id.clone(),
                                                    "模型 ID 已复制",
                                                    cx,
                                                )
                                            }),
                                        ),
                                    )
                                    .child(
                                        button(SharedString::from(format!(
                                            "use-model-{}",
                                            model.slug
                                        )))
                                        .label("用于接入")
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.selected_model = Some(choose_id.clone());
                                                this.open_client_tab(0);
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                    .child(
                                        button(SharedString::from(format!("test-{}", model.slug)))
                                            .label(
                                                if self.pending.testing_model.as_ref()
                                                    == Some(&model.slug)
                                                {
                                                    "测试中…"
                                                } else {
                                                    "测试文本"
                                                },
                                            )
                                            .tooltip("调用模型，会消耗账号额度")
                                            .disabled(
                                                self.busy
                                                    || !self.codex_connected
                                                    || self.pending.account.is_some(),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.busy = true;
                                                this.test_result = None;
                                                this.send(Command::TestModel(test_id.clone()), cx);
                                            })),
                                    ),
                            ),
                        ),
                );
            }
            content = content.child(
                div()
                    .id("models-table-scroll")
                    .overflow_x_scroll()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(self.colors.border))
                    .child(table),
            );
        }
        if let Some(result) = &self.test_result {
            content = content.child(self.panel("本次测试结果", result.clone()));
        }
        if !self.tests.is_empty() {
            content = content.child(
                button("toggle-test-history")
                    .ghost()
                    .icon(IconName::ChevronDown)
                    .label(format!("测试记录 ({})", self.tests.len()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.test_history_open = !this.test_history_open;
                        cx.notify();
                    })),
            );
        }
        if self.test_history_open {
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
        }
        self.scroll_page("models-scroll", content)
            .into_any_element()
    }

    pub(super) fn settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let appearance = self
            .card()
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("外观主题"),
            )
            .child(
                div().h_flex().flex_wrap().gap(px(8.)).children(
                    [
                        (ThemePreference::System, "跟随电脑", IconName::Settings),
                        (ThemePreference::Light, "白天模式", IconName::Sun),
                        (ThemePreference::Dark, "夜间模式", IconName::Moon),
                    ]
                    .into_iter()
                    .map(|(preference, label, icon)| {
                        button(SharedString::from(format!("theme-{}", preference.as_str())))
                            .label(label)
                            .icon(icon)
                            .when(preference == self.theme_preference, |button| {
                                button.primary()
                            })
                            .disabled(!self.theme_loaded || self.theme_saving)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_theme_preference(preference, cx)
                            }))
                    }),
                ),
            )
            .when(!self.theme_loaded || self.theme_saving, |card| {
                card.child(self.caption(if !self.theme_loaded {
                    "正在读取主题设置…"
                } else {
                    "正在保存主题设置…"
                }))
            });
        let about = self.card().child(
            div()
                .h_flex()
                .gap(px(12.))
                .child(img(branding::image()).size(px(40.)).flex_shrink_0())
                .child(
                    div()
                        .v_flex()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(20.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Tiny Proxy"),
                        )
                        .child(self.caption(format!("v{}", env!("CARGO_PKG_VERSION")))),
                ),
        );
        let lifecycle = self
            .panel(
                "窗口与退出",
                if cfg!(windows) {
                    "关闭窗口后收起到托盘，服务继续运行。"
                } else {
                    "关闭窗口后退出应用。"
                },
            )
            .when_some(self.tray_notice.as_ref(), |column, notice| {
                column.child(
                    self.caption(notice.clone())
                        .text_color(rgb(self.colors.error)),
                )
            })
            .child(
                button("quit-application")
                    .label("退出应用")
                    .tooltip("停止服务并退出")
                    .on_click(|_, _, cx| cx.quit()),
            );
        let data = self.panel(
            "数据目录",
            if cfg!(target_os = "macos") {
                "~/.tiny-proxy"
            } else {
                "%USERPROFILE%/.tiny-proxy"
            },
        );
        let content = div().v_flex().gap(px(20.)).child(self.title()).child(
            div()
                .h_flex()
                .items_start()
                .flex_wrap()
                .gap(px(20.))
                .child(
                    div()
                        .v_flex()
                        .flex_1()
                        .min_w(px(340.))
                        .gap(px(20.))
                        .child(appearance)
                        .child(self.online_updates(cx)),
                )
                .child(
                    div()
                        .v_flex()
                        .w(px(300.))
                        .min_w(px(260.))
                        .gap(px(20.))
                        .child(about)
                        .child(lifecycle)
                        .child(data),
                ),
        );
        self.scroll_page("settings-scroll", content)
            .into_any_element()
    }
}
