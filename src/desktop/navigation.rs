use super::*;

// Keep page IDs stable: reordering the sidebar must not change action targets.
const GROUPS: [(Option<&str>, &[usize]); 3] = [
    (None, &[0, 4]),
    (Some("模型配置"), &[1, 2]),
    (Some("设置"), &[3, 5]),
];
const LABELS: [&str; 6] = [
    "数据概览",
    "账号连接",
    "模型",
    "客户端接入",
    "请求记录",
    "系统设置",
];

impl Desktop {
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let mut sidebar = layout::sidebar_frame()
            .rounded_lg()
            .bg(rgb(self.colors.sidebar))
            .border_1()
            .border_color(rgb(self.colors.border));
        for (heading, pages) in GROUPS {
            if let Some(heading) = heading {
                sidebar = sidebar.child(
                    self.caption(heading)
                        .text_size(px(13.))
                        .px(px(10.))
                        .mt(px(16.))
                        .mb(px(4.))
                        .flex_shrink_0(),
                );
            }
            for &index in pages {
                sidebar = sidebar.child(self.navigation_button(index, cx));
            }
        }
        sidebar
            .child(div().flex_1().min_h_0())
            .when_some(self.update_sidebar_label(), |column, label| {
                column.child(
                    button("update-notice")
                        .label(label)
                        .w_full()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.navigate(5);
                            this.update_notice = None;
                            cx.notify();
                        })),
                )
            })
            .child(
                div()
                    .h_flex()
                    .gap(px(12.))
                    .flex_shrink_0()
                    .p(px(10.))
                    .mt(px(8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(self.colors.soft_border))
                    .child(img(branding::image()).size(px(32.)).flex_shrink_0())
                    .child(
                        div()
                            .v_flex()
                            .min_w_0()
                            .gap(px(2.))
                            .child(div().text_size(px(16.)).truncate().child("Tiny Proxy"))
                            .child(
                                self.caption(format!("v{}", env!("CARGO_PKG_VERSION")))
                                    .text_size(px(13.)),
                            ),
                    ),
            )
    }

    fn navigation_button(&self, index: usize, cx: &mut Context<Self>) -> Button {
        let selected = index == self.page;
        let dark = self.colors == theme::Palette::for_mode(ThemeMode::Dark);
        let icon = match index {
            0 => IconName::LayoutDashboard,
            1 => IconName::User,
            2 => IconName::Cpu,
            3 => IconName::Network,
            4 => IconName::FileText,
            _ => IconName::Settings,
        };
        let style = ButtonCustomVariant::new(cx)
            .color(
                rgb(if selected {
                    self.colors.selected
                } else {
                    self.colors.sidebar
                })
                .into(),
            )
            .foreground(
                rgb(if selected && dark {
                    self.colors.accent
                } else {
                    self.colors.ink
                })
                .into(),
            )
            .hover(rgb(self.colors.button_hover).into())
            .active(rgb(self.colors.selected).into());
        button(SharedString::from(format!("nav-{index}")))
            .custom(style)
            .accessibility_label(LABELS[index])
            .child(
                div()
                    .h_flex()
                    .gap_3()
                    .w_full()
                    .text_size(px(16.))
                    .child(Icon::new(icon).size(px(20.)))
                    .child(
                        div()
                            .truncate()
                            .when(selected, |label| label.font_weight(FontWeight::SEMIBOLD))
                            .child(LABELS[index]),
                    ),
            )
            .w_full()
            .h(px(44.))
            .flex_shrink_0()
            .justify_start()
            .toggled(selected)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.navigate(index);
                if index == 0 {
                    this.refresh_overview(None, cx);
                }
                cx.notify();
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::{GROUPS, LABELS};

    #[test]
    fn groups_keep_all_six_existing_destinations_once_in_the_approved_order() {
        assert_eq!(GROUPS[0], (None, &[0, 4][..]));
        assert_eq!(GROUPS[1], (Some("模型配置"), &[1, 2][..]));
        assert_eq!(GROUPS[2], (Some("设置"), &[3, 5][..]));
        let mut pages: Vec<_> = GROUPS
            .iter()
            .flat_map(|(_, pages)| pages.iter().copied())
            .collect();
        pages.sort_unstable();
        assert_eq!(pages, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(LABELS[4], "请求记录");
        assert_eq!(LABELS[5], "系统设置");
    }
}
