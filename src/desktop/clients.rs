use super::*;

impl Desktop {
    pub(super) fn clients(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = div().v_flex().gap_4().child(self.title()).child(
            div().h_flex().flex_wrap().gap_2().children(
                ["连接地址", "客户端密钥", "接入指南"]
                    .into_iter()
                    .enumerate()
                    .map(|(tab, label)| {
                        self.tab(
                            SharedString::from(format!("client-tab-{tab}")),
                            label,
                            self.client_tab == tab,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_client_tab(tab);
                            cx.notify();
                        }))
                    }),
            ),
        );
        content = content.child(match self.client_tab {
            1 => self.client_keys(cx),
            2 => self.client_guide(cx),
            _ => self.client_connection(cx),
        });
        self.scroll_page("clients-scroll", content)
            .into_any_element()
    }

    fn network_controls(&self, cx: &mut Context<Self>) -> Div {
        let local = self
            .bind_ip
            .read(cx)
            .value()
            .trim()
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
            || self.bind_ip.read(cx).value().trim().is_empty();
        let disabled = self.gateway_address.is_some()
            || self.busy
            || self.pending.gateway.is_some()
            || self.pending.account.is_some();
        let controls = div().v_flex().gap(px(12.)).child(
            div().h_flex().flex_wrap().gap(px(12.)).child(
                div()
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(self.colors.accent))
                    .text_color(rgb(self.colors.accent))
                    .child(if local {
                        "本机访问"
                    } else {
                        "局域网 / VPN"
                    }),
            ),
        );
        let row = div()
            .h_flex()
            .gap(px(12.))
            .flex_wrap()
            .items_start()
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w(px(160.))
                    .gap_2()
                    .child(self.caption("监听 IP"))
                    .child(input(&self.bind_ip).w_full().disabled(disabled)),
            )
            .child(
                div()
                    .v_flex()
                    .w(px(110.))
                    .flex_shrink_0()
                    .gap_2()
                    .child(self.caption("端口"))
                    .child(input(&self.bind_port).w_full().disabled(disabled)),
            );
        let controls = controls.child(row).child(
            div()
                .h_flex()
                .child(self.gateway_button("clients-gateway", cx)),
        );
        if self.gateway_address.is_some() {
            controls.child(self.caption("停止服务后可修改端口"))
        } else {
            controls
        }
    }

    fn client_connection(&self, cx: &mut Context<Self>) -> Div {
        let model = self
            .models
            .iter()
            .find(|m| self.selected_model.as_deref() == Some(m.slug.as_str()))
            .or(self.models.first());
        let model_id = model.map(|m| m.slug.clone());
        let mut fields = div()
            .v_flex()
            .flex_basis(px(360.))
            .flex_grow(3.)
            .min_w(px(300.))
            .gap(px(18.))
            .child(
                div()
                    .text_size(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("连接信息"),
            )
            .child(self.network_controls(cx))
            .child(
                self.copy_field(
                    "copy-base-url",
                    "Base URL",
                    self.gateway_address
                        .as_ref()
                        .map(|a| format!("http://{a}/v1")),
                    cx,
                ),
            )
            .child(
                self.copy_field(
                    "copy-chat-url",
                    "Chat Completions 完整地址",
                    self.gateway_address
                        .as_ref()
                        .map(|a| format!("http://{a}/v1/chat/completions")),
                    cx,
                ),
            )
            .child(
                self.copy_field(
                    "copy-responses-url",
                    "Responses 完整地址",
                    self.gateway_address
                        .as_ref()
                        .map(|a| format!("http://{a}/v1/responses")),
                    cx,
                ),
            )
            .child(self.caption("模型 ID"))
            .child(
                div()
                    .h_flex()
                    .gap(px(8.))
                    .child(
                        button("choose-model")
                            .dropdown_caret(true)
                            .label(
                                model
                                    .map(|m| m.slug.clone())
                                    .unwrap_or_else(|| "尚未同步目录".into()),
                            )
                            .flex_1()
                            .min_w_0()
                            .h(px(40.))
                            .disabled(self.models.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.model_menu_open = !this.model_menu_open;
                                this.key_menu_open = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("copy-connection-model")
                            .icon(IconName::Copy)
                            .label("ID")
                            .h(px(40.))
                            .disabled(model_id.is_none())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(id) = &model_id {
                                    this.copy_text(id.clone(), "模型 ID 已复制", cx);
                                }
                            })),
                    ),
            );
        if self.model_menu_open {
            fields = fields.child(
                div()
                    .v_flex()
                    .p(px(4.))
                    .rounded(px(6.))
                    .bg(rgb(self.colors.surface))
                    .border_1()
                    .border_color(rgb(self.colors.border))
                    .children(self.models.iter().map(|m| {
                        let id = m.slug.clone();
                        button(SharedString::from(format!("model-option-{id}")))
                            .ghost()
                            .label(id.clone())
                            .w_full()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_model = Some(id.clone());
                                this.model_menu_open = false;
                                cx.notify();
                            }))
                    })),
            );
        }
        let mut guide = div()
            .v_flex()
            .flex_basis(px(260.))
            .flex_grow(2.)
            .min_w(px(260.))
            .gap(px(18.))
            .pl(px(24.))
            .border_l_1()
            .border_color(rgb(self.colors.border))
            .child(
                div()
                    .text_size(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("接入客户端"),
            )
            .child(self.step_heading(1, "选择 OpenAI 兼容接口"))
            .child(self.step_heading(2, "填写 Base URL 与密钥"))
            .child(self.step_heading(3, "填入模型 ID"))
            .child(div().h(px(1.)).bg(rgb(self.colors.border)))
            .child(self.caption("使用的客户端密钥"));
        let selected = self
            .keys
            .iter()
            .find(|k| self.selected_key.as_deref() == Some(k.id.as_str()));
        guide = guide.child(
            button("choose-client-key")
                .dropdown_caret(true)
                .label(
                    selected
                        .map(|k| k.label.clone())
                        .unwrap_or_else(|| "尚无客户端密钥".into()),
                )
                .w_full()
                .h(px(40.))
                .disabled(self.keys.is_empty())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.key_menu_open = !this.key_menu_open;
                    this.model_menu_open = false;
                    cx.notify();
                })),
        );
        if self.key_menu_open {
            guide = guide.child(
                div()
                    .v_flex()
                    .p(px(4.))
                    .rounded(px(6.))
                    .bg(rgb(self.colors.surface))
                    .border_1()
                    .border_color(rgb(self.colors.border))
                    .children(self.keys.iter().map(|k| {
                        let id = k.id.clone();
                        button(SharedString::from(format!("key-option-{id}")))
                            .ghost()
                            .label(k.label.clone())
                            .w_full()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_key = Some(id.clone());
                                this.key_menu_open = false;
                                cx.notify();
                            }))
                    })),
            );
        }
        if let Some(key) = selected {
            let copy_id = key.id.clone();
            let reveal_id = key.id.clone();
            guide = guide
                .child(
                    div()
                        .h_flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .h_flex()
                                .flex_1()
                                .min_w_0()
                                .h(px(40.))
                                .px(px(12.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(rgb(self.colors.border))
                                .bg(rgb(self.colors.surface))
                                .child(
                                    div()
                                        .font_family("Consolas")
                                        .text_ellipsis()
                                        .child(format!("{}••••••••", key.public_prefix)),
                                ),
                        )
                        .child(
                            button("view-selected-key")
                                .ghost()
                                .icon(IconName::Eye)
                                .tooltip("到密钥页查看")
                                .disabled(!key.recoverable || self.pending.key.is_some())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_client_tab(1);
                                    this.send(
                                        Command::RevealKey {
                                            id: reveal_id.clone(),
                                            copy: false,
                                        },
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            button("copy-selected-key")
                                .icon(IconName::Copy)
                                .label("复制密钥")
                                .h(px(40.))
                                .disabled(!key.recoverable || self.pending.key.is_some())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.send(
                                        Command::RevealKey {
                                            id: copy_id.clone(),
                                            copy: true,
                                        },
                                        cx,
                                    )
                                })),
                        ),
                )
                .when(!key.recoverable, |column| {
                    column.child(self.caption("旧密钥仅有摘要，无法恢复原值"))
                });
        }
        guide = guide
            .child(
                button("open-client-guide")
                    .label("打开完整接入指南 →")
                    .w_full()
                    .h(px(40.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_client_tab(2);
                        cx.notify();
                    })),
            )
            .child(
                button("manage-client-keys")
                    .ghost()
                    .label("创建 / 管理客户端密钥")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_client_tab(1);
                        cx.notify();
                    })),
            );
        div().v_flex().gap(px(20.)).child(
            div()
                .h_flex()
                .items_stretch()
                .flex_wrap()
                .gap(px(24.))
                .child(fields)
                .child(guide),
        )
    }

    fn client_keys(&self, cx: &mut Context<Self>) -> Div {
        let mut content = div().v_flex().gap_4().child(
            self.card()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("创建客户端密钥"),
                )
                .child(
                    div()
                        .h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            input(&self.key_name)
                                .w(px(280.))
                                .disabled(self.busy || self.pending.key.is_some()),
                        )
                        .child(
                            self.action_button("create-key")
                                .icon(IconName::Plus)
                                .label(
                                    if self.pending.key == Some(interactions::KeyAction::Create) {
                                        "创建中…"
                                    } else {
                                        "创建密钥"
                                    },
                                )
                                .disabled(
                                    self.busy
                                        || self.new_key.is_some()
                                        || self.pending.key.is_some()
                                        || !presentation::valid_key_name(
                                            &self.key_name.read(cx).value(),
                                        ),
                                )
                                .tooltip("名称需为 1–80 个字符")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let label = this.key_name.read(cx).value().to_string();
                                    this.send(Command::CreateKey(label), cx);
                                })),
                        ),
                ),
        );
        if let Some((key, _)) = &self.new_key {
            content = content.child(
                self.card()
                    .border_color(rgb(self.colors.warning))
                    .child(self.caption("2 分钟后隐藏"))
                    .child(
                        div()
                            .id("revealed-key-scroll")
                            .overflow_x_scroll()
                            .font_family("Consolas")
                            .child(key.value.to_string()),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(
                                button("copy-key")
                                    .icon(IconName::Copy)
                                    .label("复制密钥")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some((key, _)) = &this.new_key {
                                            this.copy_text(key.value.to_string(), "密钥已复制", cx);
                                        }
                                    })),
                            )
                            .child(
                                button("dismiss-key")
                                    .label("隐藏密钥")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.new_key = None;
                                        this.pending.reveal_requested = false;
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        }
        if self.keys.is_empty() {
            return content.child(self.panel("暂无客户端密钥", "创建密钥后连接客户端"));
        }
        let mut table = div()
            .v_flex()
            .min_w(px(720.))
            .bg(rgb(self.colors.surface))
            .child(self.table_header(&[("客户端名称", 190.), ("密钥前缀", 170.), ("操作", 304.)]));
        for key in &self.keys {
            let view_id = key.id.clone();
            let copy_id = key.id.clone();
            let revoke_id = key.id.clone();
            let confirming = self.revoke_id.as_deref() == Some(key.id.as_str());
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
                            .child(self.table_cell(key.label.clone(), 190.))
                            .child(
                                self.table_cell(format!("{}…", key.public_prefix), 170.)
                                    .font_family("Consolas"),
                            )
                            .child(
                                layout::table_column(304.).child(
                                    div()
                                        .h_flex()
                                        .gap(px(8.))
                                        .child(
                                            button(SharedString::from(format!("view-{}", key.id)))
                                                .icon(IconName::Eye)
                                                .label("查看")
                                                .disabled(
                                                    !key.recoverable || self.pending.key.is_some(),
                                                )
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
                                            button(SharedString::from(format!("copy-{}", key.id)))
                                                .label("复制")
                                                .disabled(
                                                    !key.recoverable || self.pending.key.is_some(),
                                                )
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
                                            button(SharedString::from(format!(
                                                "revoke-{}",
                                                key.id
                                            )))
                                            .label(if confirming {
                                                "确认撤销并删除"
                                            } else {
                                                "撤销"
                                            })
                                            .when(confirming, |button| button.danger())
                                            .disabled(self.busy || self.pending.key.is_some())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if this.revoke_id.as_ref() == Some(&revoke_id) {
                                                    this.revoke_id = None;
                                                    this.send(
                                                        Command::RevokeKey(revoke_id.clone()),
                                                        cx,
                                                    );
                                                } else {
                                                    this.revoke_id = Some(revoke_id.clone());
                                                    cx.notify();
                                                }
                                            })),
                                        ),
                                ),
                            ),
                    )
                    .when(!key.recoverable, |row| {
                        row.child(self.caption("旧密钥仅有摘要，无法恢复原值"))
                    })
                    .when(confirming, |row| {
                        row.child(self.caption(
                            "撤销会拒绝新请求、取消这把密钥的在途请求，并删除密钥记录与明文预览",
                        ))
                        .child(
                            button(SharedString::from(format!("cancel-revoke-{}", key.id)))
                                .label("取消撤销")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.revoke_id = None;
                                    cx.notify();
                                })),
                        )
                    }),
            );
        }
        content.child(
            div()
                .id("keys-table-scroll")
                .overflow_x_scroll()
                .rounded_lg()
                .border_1()
                .border_color(rgb(self.colors.border))
                .child(table),
        )
    }

    fn client_guide(&self, cx: &mut Context<Self>) -> Div {
        div()
            .v_flex()
            .gap_4()
            .child(self.panel("1  接口格式", "OpenAI Chat Completions / Responses"))
            .child(self.panel(
                "2  连接信息",
                "复制连接地址与客户端密钥，分别填入 Base URL 和 API Key。",
            ))
            .child(self.panel("3  模型", "从模型页复制模型 ID。"))
            .child(self.panel(
                "4  跨设备",
                "使用本机 LAN / VPN 地址，确认端口可达；127.0.0.1 仅限本机。",
            ))
            .child(self.panel("排查失败请求", "到请求记录查看对应明细。"))
            .child(
                button("guide-back-to-address")
                    .label("查看连接地址")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_client_tab(0);
                        cx.notify();
                    })),
            )
    }
}
