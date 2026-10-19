use super::*;
use gpui_kit::component::tooltip::Tooltip;
use tiny_proxy::overview::{OverviewPeriod, OverviewSnapshot};

impl Desktop {
    pub(super) fn overview_trend(&self) -> Div {
        let snapshot = self.overview.data.as_deref();
        let mut header = div().h_flex().justify_between().gap_3().child(
            div()
                .h_flex()
                .gap_3()
                .child(div().font_weight(FontWeight::SEMIBOLD).child("调用趋势"))
                .when(self.overview.loading, |row| {
                    row.child(self.caption("刷新中…"))
                })
                .when_some(self.overview.error.as_ref(), |row, message| {
                    row.child(
                        div()
                            .id("overview-error")
                            .text_size(px(13.))
                            .text_color(rgb(self.colors.error))
                            .child(if snapshot.is_some() {
                                "刷新失败 · 保留上次结果"
                            } else {
                                "读取失败 · 请刷新重试"
                            })
                            .tooltip({
                                let message = message.clone();
                                move |window, cx| Tooltip::new(message.clone()).build(window, cx)
                            }),
                    )
                }),
        );
        let mut legend = div()
            .h_flex()
            .gap(px(20.))
            .text_size(px(13.))
            .child(self.badge("完成", self.colors.success))
            .child(self.badge("失败", self.colors.error))
            .child(self.badge("取消", self.colors.muted));
        if snapshot.is_some_and(|data| data.totals.running > 0) {
            legend = legend.child(self.badge("进行中", self.colors.accent));
        }
        header = header.child(legend);
        self.card()
            .flex_shrink_0()
            .p(px(16.))
            .gap(px(32.))
            .child(header)
            .child(match snapshot {
                Some(snapshot) => self.overview_plot(snapshot),
                None => div()
                    .h(px(253.))
                    .h_flex()
                    .justify_center()
                    .text_color(rgb(self.colors.muted))
                    .child(if self.overview.error.is_some() {
                        "统计暂不可用"
                    } else {
                        "正在读取统计…"
                    }),
            })
    }

    fn overview_plot(&self, snapshot: &OverviewSnapshot) -> Div {
        let ceiling = super::overview_format::axis_ceiling(
            snapshot
                .buckets
                .iter()
                .map(|bucket| bucket.total())
                .max()
                .unwrap_or(0),
        );
        let colors = self.colors;
        let mut axis = div().relative().w(px(28.)).flex_shrink_0().h(px(228.));
        let mut plot = div().relative().flex_1().min_w_0().h(px(228.));
        for index in 0..=3 {
            let fraction = index as f32 / 3.;
            axis = axis.child(
                div()
                    .absolute()
                    .right_0()
                    .top(relative(fraction))
                    .mt(px(-8.))
                    .text_size(px(12.))
                    .text_color(rgb(colors.muted))
                    .child(super::overview_format::number(Some(
                        ceiling - ceiling / 3 * index,
                    ))),
            );
            plot = plot.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(relative(fraction))
                    .h(px(1.))
                    .bg(rgb(colors.soft_border)),
            );
        }
        let mut bars = div()
            .h_flex()
            .items_end()
            .relative()
            .size_full()
            .gap(px(5.))
            .px(px(5.));
        for (index, bucket) in snapshot.buckets.iter().enumerate() {
            let mut stack = div()
                .v_flex()
                .w_full()
                .h(relative(bucket.total() as f32 / ceiling as f32));
            // Bottom to top: completed, failed, cancelled, then active observations.
            for (count, color) in [
                (bucket.running, colors.accent),
                (bucket.cancelled, colors.muted),
                (bucket.failed, colors.error),
                (bucket.completed, colors.progress),
            ] {
                if count > 0 {
                    stack = stack.child(
                        div()
                            .flex_grow(count as f32)
                            .flex_basis(px(0.))
                            .min_h_0()
                            .w_full()
                            .bg(rgb(color)),
                    );
                }
            }
            let start = chrono::DateTime::from_timestamp_millis(bucket.start_ms)
                .map(|t| {
                    t.with_timezone(&chrono::Local)
                        .format("%m/%d %H:%M")
                        .to_string()
                })
                .unwrap_or_default();
            let end = super::overview_format::tick(bucket.end_ms, OverviewPeriod::Last24Hours);
            let hint = if bucket.start_ms >= snapshot.window.end_ms {
                format!("{start}–{end}\n尚未到达此时段")
            } else {
                format!(
                    "{start}–{end}\n请求 {} · 完成 {} · 失败 {} · 取消 {} · 进行中 {}",
                    bucket.total(),
                    bucket.completed,
                    bucket.failed,
                    bucket.cancelled,
                    bucket.running
                )
            };
            bars = bars.child(
                div()
                    .id(SharedString::from(format!("overview-hour-{index}")))
                    .v_flex()
                    .justify_end()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .hover(move |style| style.bg(rgb(colors.selected)))
                    .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
                    .child(stack),
            );
        }
        plot = plot.child(bars);
        if snapshot.totals.requests == 0 {
            plot = plot.child(
                div()
                    .absolute()
                    .inset_0()
                    .h_flex()
                    .justify_center()
                    .text_color(rgb(colors.muted))
                    .child("暂无生成请求"),
            );
        }
        let mut labels = div()
            .h_flex()
            .justify_between()
            .flex_1()
            .min_w_0()
            .text_size(px(12.))
            .text_color(rgb(colors.muted));
        for index in 0..=6 {
            let at = snapshot.window.start_ms
                + (snapshot.window.chart_end_ms - snapshot.window.start_ms) * index / 6;
            let label = if snapshot.window.period == OverviewPeriod::Today && index == 6 {
                "24:00".into()
            } else {
                super::overview_format::tick(at, snapshot.window.period)
            };
            labels = labels.child(div().child(label));
        }
        div()
            .v_flex()
            .gap(px(8.))
            .child(
                div()
                    .h_flex()
                    .items_stretch()
                    .gap(px(10.))
                    .child(axis)
                    .child(plot),
            )
            .child(
                div()
                    .h_flex()
                    .gap(px(10.))
                    .child(div().w(px(28.)).flex_shrink_0())
                    .child(labels),
            )
    }
}

pub(super) fn cache_gauge(fraction: Option<f64>, colors: theme::Palette) -> Div {
    let fraction = fraction.filter(|value| value.is_finite() && (0. ..=1.).contains(value));
    let label = super::overview_format::percentage(fraction);
    div()
        .relative()
        .w(px(152.))
        .h(px(84.))
        .flex_shrink_0()
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    let width = f32::from(bounds.size.width);
                    let height = f32::from(bounds.size.height);
                    let radius = (width / 2. - 7.).min(height - 14.).max(0.);
                    let center = (
                        f32::from(bounds.origin.x) + width / 2.,
                        f32::from(bounds.origin.y) + height - 7.,
                    );
                    let draw = |fraction: f32, color: u32, window: &mut Window| {
                        let mut path = PathBuilder::stroke(px(10.));
                        for index in 0..=64 {
                            let angle = std::f32::consts::PI * (1. + fraction * index as f32 / 64.);
                            let at = point(
                                px(center.0 + radius * angle.cos()),
                                px(center.1 + radius * angle.sin()),
                            );
                            if index == 0 {
                                path.move_to(at);
                            } else {
                                path.line_to(at);
                            }
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, rgb(color));
                        }
                        for progress in [0., fraction] {
                            let angle = std::f32::consts::PI * (1. + progress);
                            let endpoint = (
                                center.0 + radius * angle.cos(),
                                center.1 + radius * angle.sin(),
                            );
                            let mut cap = PathBuilder::fill();
                            for index in 0..16 {
                                let angle = std::f32::consts::TAU * index as f32 / 16.;
                                let at = point(
                                    px(endpoint.0 + 5. * angle.cos()),
                                    px(endpoint.1 + 5. * angle.sin()),
                                );
                                if index == 0 {
                                    cap.move_to(at);
                                } else {
                                    cap.line_to(at);
                                }
                            }
                            cap.close();
                            if let Ok(cap) = cap.build() {
                                window.paint_path(cap, rgb(color));
                            }
                        }
                    };
                    draw(1., colors.soft_border, window);
                    if let Some(fraction) = fraction.filter(|value| *value > 0.) {
                        draw(fraction.clamp(0., 1.) as f32, colors.progress, window);
                    }
                },
            )
            .absolute()
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .text_center()
                .text_size(px(24.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(label),
        )
}
