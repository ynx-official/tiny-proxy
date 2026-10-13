use super::*;
use tiny_proxy::updater::{CURRENT_VERSION, CheckMode, RELEASES_URL, UpdateStatus};

fn status_text(status: &UpdateStatus) -> String {
    match status {
        UpdateStatus::NotChecked => "尚未检查更新".into(),
        UpdateStatus::Checking => "正在检查更新…".into(),
        UpdateStatus::UpToDate(info) => format!("当前版本已是最新 · 最新正式版本 {}", info.version),
        UpdateStatus::UpdateAvailable(info) => format!("发现新版本 {}", info.version),
        UpdateStatus::Downloading(_, done, total) => {
            if *total == 0 {
                format!("正在下载 · {:.1} MiB", *done as f64 / 1048576.)
            } else {
                format!(
                    "正在下载 · {}% · {:.1} / {:.1} MiB",
                    done.saturating_mul(100)
                        .checked_div(*total)
                        .unwrap_or(0)
                        .min(100),
                    *done as f64 / 1048576.,
                    *total as f64 / 1048576.
                )
            }
        }
        UpdateStatus::DownloadCancelled(_) => "下载已取消，可重新下载".into(),
        UpdateStatus::DownloadFailed(_, error) => format!("下载失败：{error}"),
        UpdateStatus::ReadyToRestart(info, _) => {
            format!("{} 已下载并校验，等待安装重启", info.version)
        }
        UpdateStatus::Installing => "正在安装更新；完成后将退出并启动新版本…".into(),
        UpdateStatus::Error(error) => format!("更新失败：{error}"),
    }
}

impl Desktop {
    pub(super) fn update_sidebar_label(&self) -> Option<&'static str> {
        match &self.update.status {
            UpdateStatus::Checking => Some("正在检查更新"),
            UpdateStatus::Downloading(..) => Some("正在下载更新"),
            UpdateStatus::ReadyToRestart(..) => Some("更新已就绪"),
            UpdateStatus::Installing => Some("正在安装更新"),
            _ if self.update_notice.is_some() => Some("有可用更新"),
            _ => None,
        }
    }
    pub(super) fn online_updates(&self, cx: &mut Context<Self>) -> AnyElement {
        let preferences = &self.update.preferences;
        let status = &self.update.status;
        let ready = matches!(status, UpdateStatus::ReadyToRestart(..));
        let downloading = matches!(status, UpdateStatus::Downloading(..));
        let mut content = div().v_flex().gap_3().p_5().rounded_lg().bg(rgb(self.colors.surface)).border_1().border_color(rgb(self.colors.border))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("在线更新"))
            .child(div().child(format!("Tiny Proxy {CURRENT_VERSION} · {}", tiny_proxy::updater::runtime_environment_label())))
            .child(div().text_color(rgb(self.colors.muted)).child(format!("最近成功检查：{}", if preferences.last_checked_at <= 0 { "尚未检查".into() } else { format_time(Some(preferences.last_checked_at)) })))
            .child(div().h_flex().flex_wrap().gap_2().children([
                (CheckMode::Startup, "启动时检查"), (CheckMode::Interval, "定时检查"), (CheckMode::Disabled, "关闭自动检查")
            ].into_iter().map(|(mode,label)| Button::new(SharedString::from(format!("update-mode-{mode:?}"))).label(label)
                .disabled(matches!(status, UpdateStatus::Installing))
                .when(preferences.mode == mode, |button| button.primary())
                .on_click(cx.listener(move |this,_,_,cx| { let mut preferences = this.update.preferences.clone(); preferences.mode = mode; this.send(Command::ConfigureUpdates(preferences),cx); })))))
            .when(preferences.mode == CheckMode::Interval, |column| column.child(div().h_flex().flex_wrap().items_center().gap_2()
                .child(div().child("检查间隔（小时）"))
                .child(Input::new(&self.update_interval).w(px(96.)).disabled(matches!(status, UpdateStatus::Installing)))
                .child(Button::new("save-update-interval").label("保存间隔").disabled(matches!(status, UpdateStatus::Installing))
                    .on_click(cx.listener(|this,_,_,cx| {
                        let value = this.update_interval.read(cx).value();
                        match value.parse::<u32>() {
                            Ok(hours) if (1..=8760).contains(&hours) => { let mut prefs=this.update.preferences.clone();prefs.interval_hours=hours;this.send(Command::ConfigureUpdates(prefs),cx); },
                            _ => { this.status="请输入 1–8760 之间的整数小时".into();this.error=true;cx.notify(); }
                        }
                    })))
                .child(div().text_color(rgb(self.colors.muted)).child("1–8760 小时 · 应用关闭时暂停"))))
            .child(Button::new("update-notifications").label(if preferences.notify { "新版本提醒：开启" } else { "新版本提醒：关闭" })
                .disabled(matches!(status, UpdateStatus::Installing))
                .on_click(cx.listener(|this,_,_,cx|{let mut prefs=this.update.preferences.clone();prefs.notify = !prefs.notify;this.send(Command::ConfigureUpdates(prefs),cx);})))
            .child(div().text_color(rgb(if matches!(status, UpdateStatus::Error(_) | UpdateStatus::DownloadFailed(..)) { self.colors.error } else { self.colors.ink })).child(status_text(status)))
            .when(downloading, |column| {
                let fraction = if let UpdateStatus::Downloading(_,done,total)=status { if *total > 0 { (*done as f32 / *total as f32).clamp(0.,1.) } else { 0. } } else { 0. };
                column.child(div().w_full().h(px(6.)).rounded_md().bg(rgb(self.colors.border)).child(div().h_full().w(relative(fraction)).bg(rgb(self.colors.ink))))
            })
            .child(div().h_flex().flex_wrap().gap_2()
                .child(Button::new("check-update").label("检查更新").disabled(status.busy() || ready).on_click(cx.listener(|this,_,_,cx|this.send(Command::CheckUpdate,cx))))
                .when(status.available().is_some(), |row| row.child(Button::new("download-update").label(if matches!(status, UpdateStatus::DownloadFailed(..) | UpdateStatus::DownloadCancelled(_)) { "重新下载" } else { "下载更新" }).primary().on_click(cx.listener(|this,_,_,cx|this.send(Command::DownloadUpdate,cx)))))
                .when(downloading, |row| row.child(Button::new("cancel-update").label("取消下载").on_click(cx.listener(|this,_,_,cx|this.send(Command::CancelUpdateDownload,cx)))))
                .when(ready, |row| row.child(Button::new("restart-update").label("安装并重启").primary().on_click(cx.listener(|this,_,_,cx|{this.confirm_update=true;cx.notify();}))))
                .child(Button::new("open-releases").label("GitHub 版本页").on_click(|_,_,cx|cx.open_url(RELEASES_URL))))
            .when(self.confirm_update && ready, |column| column.child(div().v_flex().gap_3().p_4().border_1().border_color(rgb(self.colors.warning))
                .child(div().child("安装会停止网关并中断进行中的请求，然后退出并启动新版本。macOS 安装包可能请求系统管理员授权。"))
                .child(div().h_flex().gap_2()
                    .child(Button::new("confirm-install-update").label("确认安装并重启").primary().on_click(cx.listener(|this,_,_,cx|{this.confirm_update=false;this.send(Command::InstallUpdate,cx);})))
                    .child(Button::new("dismiss-install-update").label("稍后安装").on_click(cx.listener(|this,_,_,cx|{this.confirm_update=false;cx.notify();}))))))
            .child(div().text_color(rgb(self.colors.muted)).child("检查只读取最新正式版发布信息。下载与安装需手动点击，不会静默替换应用。"));
        if !status.notes().is_empty() {
            content = content.child(
                div()
                    .v_flex()
                    .gap_2()
                    .border_t_1()
                    .border_color(rgb(self.colors.border))
                    .pt_3()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("版本更新说明"),
                    )
                    .children(
                        status
                            .notes()
                            .lines()
                            .map(|line| div().text_sm().child(line.to_owned())),
                    ),
            );
        }
        content.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::status_text;
    use tiny_proxy::updater::UpdateStatus;
    #[test]
    fn cancellation_installation_and_errors_have_explicit_feedback() {
        assert!(status_text(&UpdateStatus::Installing).contains("安装"));
        assert!(status_text(&UpdateStatus::Error("网络不可用".into())).contains("网络不可用"));
        assert!(status_text(&UpdateStatus::NotChecked).contains("尚未"));
    }
}
