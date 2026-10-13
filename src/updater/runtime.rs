use super::{CheckMode, Preferences, UpdateCheckResult, UpdateInfo, UpdateStatus};
use crate::{app::Event, storage::Store};
use std::{path::PathBuf, time::Duration};
use tokio::{sync::mpsc, task::JoinSet, time::Instant};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub preferences: Preferences,
    pub status: UpdateStatus,
}
pub(crate) enum Completion {
    Checked(UpdateCheckResult),
    Downloaded(PathBuf),
    Installed,
}

pub(crate) struct Updates {
    preferences: Preferences,
    status: UpdateStatus,
    pub jobs: JoinSet<anyhow::Result<Completion>>,
    cancel: CancellationToken,
    shutdown: CancellationToken,
    next_check: Option<Instant>,
    sender: mpsc::Sender<Event>,
}
impl Updates {
    pub async fn new(
        store: &Store,
        sender: mpsc::Sender<Event>,
        shutdown: CancellationToken,
    ) -> Result<Self, String> {
        let preferences = store
            .update_preferences()
            .await
            .map_err(|_| "无法读取更新设置".to_owned())?;
        let mut updates = Self {
            preferences,
            status: UpdateStatus::NotChecked,
            jobs: JoinSet::new(),
            cancel: shutdown.child_token(),
            shutdown,
            next_check: None,
            sender,
        };
        updates.schedule(true);
        updates.emit();
        Ok(updates)
    }
    fn schedule(&mut self, startup: bool) {
        self.next_check = match self.preferences.mode {
            CheckMode::Startup if startup => Some(Instant::now()),
            CheckMode::Interval => Some(
                Instant::now()
                    + super::automatic_update_delay(
                        self.preferences.interval_hours,
                        self.preferences.last_checked_at,
                        chrono::Utc::now().timestamp(),
                    ),
            ),
            _ => None,
        };
    }
    fn emit(&self) {
        let _ = self.sender.try_send(Event::Update(Box::new(Snapshot {
            preferences: self.preferences.clone(),
            status: self.status.clone(),
        })));
    }
    async fn emit_completed(&self) {
        let _ = self
            .sender
            .send(Event::Update(Box::new(Snapshot {
                preferences: self.preferences.clone(),
                status: self.status.clone(),
            })))
            .await;
    }
    pub fn installing(&self) -> bool {
        matches!(self.status, UpdateStatus::Installing)
    }
    pub fn tick(&mut self) {
        if self.next_check.is_some_and(|time| time <= Instant::now()) {
            // Ready downloads remain ready; automatic polling cannot discard
            // a verified payload or an installation awaiting user approval.
            if !self.status.busy() && !matches!(self.status, UpdateStatus::ReadyToRestart(..)) {
                self.check();
            }
            self.next_check = if self.preferences.mode == CheckMode::Interval {
                Some(
                    Instant::now()
                        + Duration::from_secs(u64::from(self.preferences.interval_hours) * 3600),
                )
            } else {
                None
            };
        }
    }
    pub async fn configure(&mut self, store: &Store, mut preferences: Preferences) {
        preferences.last_checked_at = self.preferences.last_checked_at;
        match store.save_update_preferences(&preferences).await {
            Ok(()) => {
                self.preferences = preferences;
                self.schedule(false);
                self.emit_completed().await;
            }
            Err(_) => {
                let _ = self.sender.try_send(Event::Error(
                    "更新设置保存失败；间隔必须为 1–8760 小时".into(),
                ));
            }
        }
    }
    pub fn check(&mut self) {
        if !self.jobs.is_empty() || matches!(self.status, UpdateStatus::ReadyToRestart(..)) {
            return;
        }
        self.cancel = self.shutdown.child_token();
        let cancel = self.cancel.clone();
        self.status = UpdateStatus::Checking;
        self.emit();
        self.jobs.spawn(async move {
            tokio::select! { biased; _ = cancel.cancelled() => anyhow::bail!("检查已取消"), result = super::check_for_update() => result.map(Completion::Checked) }
        });
    }
    pub fn download(&mut self) {
        if !self.jobs.is_empty() {
            return;
        }
        let Some(info) = self.status.available().cloned() else {
            return;
        };
        self.cancel = self.shutdown.child_token();
        let cancel = self.cancel.clone();
        let progress_sender = self.sender.clone();
        let mut last_progress = Instant::now() - Duration::from_millis(100);
        self.status = UpdateStatus::Downloading(info.clone(), 0, info.size);
        self.emit();
        self.jobs.spawn(async move {
            super::download_update(&info, &cancel, move |done, total| {
                if last_progress.elapsed() < Duration::from_millis(100) && done != total {
                    return;
                }
                last_progress = Instant::now();
                let _ = progress_sender.try_send(Event::UpdateProgress(done, total));
            })
            .await
            .map(Completion::Downloaded)
        });
    }
    pub fn cancel_download(&mut self) {
        self.cancel.cancel();
    }
    pub fn ready(&self) -> Option<(UpdateInfo, PathBuf)> {
        match &self.status {
            UpdateStatus::ReadyToRestart(info, path) => Some((info.clone(), path.clone())),
            _ => None,
        }
    }
    pub fn install(&mut self, info: UpdateInfo, path: PathBuf) {
        if !self.jobs.is_empty() {
            return;
        }
        self.status = UpdateStatus::Installing;
        self.emit();
        self.jobs.spawn(async move {
            tokio::task::spawn_blocking(move || super::install_and_restart(&path, &info)).await??;
            Ok(Completion::Installed)
        });
    }
    pub async fn complete(
        &mut self,
        store: &Store,
        result: Option<Result<anyhow::Result<Completion>, tokio::task::JoinError>>,
    ) -> bool {
        let was_installing = self.installing();
        let result = match result {
            Some(Ok(result)) => result,
            Some(Err(error)) => Err(error.into()),
            None => return false,
        };
        self.status = match result {
            Ok(Completion::Checked(result)) => {
                self.preferences.last_checked_at = chrono::Utc::now().timestamp();
                if store
                    .save_update_preferences(&self.preferences)
                    .await
                    .is_err()
                {
                    let _ = self
                        .sender
                        .try_send(Event::Error("更新检查已完成，但保存检查时间失败".into()));
                }
                match result {
                    UpdateCheckResult::UpToDate(info) => UpdateStatus::UpToDate(info),
                    UpdateCheckResult::UpdateAvailable(info) => {
                        if self.preferences.notify {
                            let _ = self.sender.try_send(Event::UpdateNotice(format!(
                                "Tiny Proxy {} 可更新，可在设置中查看说明和下载",
                                info.version
                            )));
                        }
                        UpdateStatus::UpdateAvailable(info)
                    }
                }
            }
            Ok(Completion::Downloaded(path)) => {
                if let UpdateStatus::Downloading(info, ..) = &self.status {
                    if self.cancel.is_cancelled() {
                        UpdateStatus::DownloadCancelled(info.clone())
                    } else {
                        UpdateStatus::ReadyToRestart(info.clone(), path)
                    }
                } else {
                    UpdateStatus::Error("更新下载状态已失效".into())
                }
            }
            Ok(Completion::Installed) => {
                // This event must not be lost behind progress events. The UI
                // quits only after the helper is safely running.
                let _ = self.sender.send(Event::QuitForUpdate).await;
                return true;
            }
            Err(error) => {
                if was_installing {
                    let _ = self
                        .sender
                        .send(Event::Status {
                            message: "更新安装失败，可查看设置中的错误并在客户端接入页重新启动网关"
                                .into(),
                            busy: false,
                        })
                        .await;
                }
                if let UpdateStatus::Downloading(info, ..) = &self.status {
                    if self.cancel.is_cancelled() {
                        UpdateStatus::DownloadCancelled(info.clone())
                    } else {
                        UpdateStatus::DownloadFailed(info.clone(), format!("{error:#}"))
                    }
                } else {
                    UpdateStatus::Error(format!("{error:#}"))
                }
            }
        };
        self.emit_completed().await;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn disabled_checks_do_not_spawn_network_jobs_and_ready_downloads_are_retained() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("app.db")).await.unwrap();
        store
            .save_update_preferences(&Preferences {
                mode: CheckMode::Disabled,
                ..Default::default()
            })
            .await
            .unwrap();
        let (sender, mut events) = mpsc::channel(32);
        let mut updates = Updates::new(&store, sender, CancellationToken::new())
            .await
            .unwrap();
        updates.tick();
        assert!(updates.next_check.is_none());
        assert!(updates.jobs.is_empty());
        let info = UpdateInfo {
            version: "0.2.0".into(),
            notes: String::new(),
            download_url: String::new(),
            size: 1,
            digest: String::new(),
            installation_kind: super::super::InstallationKind::Portable,
        };
        updates.status = UpdateStatus::ReadyToRestart(info, temp.path().join("verified-payload"));
        updates
            .configure(
                &store,
                Preferences {
                    mode: CheckMode::Interval,
                    ..Default::default()
                },
            )
            .await;
        updates.tick();
        assert!(updates.jobs.is_empty());
        assert!(updates.ready().is_some());
        while events.try_recv().is_ok() {}
        store.close().await;
    }

    #[tokio::test]
    async fn successful_check_persists_time_and_emits_terminal_state() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("app.db")).await.unwrap();
        let (sender, mut events) = mpsc::channel(32);
        let mut updates = Updates::new(&store, sender, CancellationToken::new())
            .await
            .unwrap();
        updates
            .complete(
                &store,
                Some(Ok(Ok(Completion::Checked(UpdateCheckResult::UpToDate(
                    super::super::ReleaseInfo {
                        version: "0.1.0".into(),
                        notes: "notes".into(),
                    },
                ))))),
            )
            .await;
        assert!(store.update_preferences().await.unwrap().last_checked_at > 0);
        assert!(matches!(updates.status, UpdateStatus::UpToDate(_)));
        let mut found = false;
        while let Ok(event) = events.try_recv() {
            if let Event::Update(snapshot) = event {
                found |= matches!(snapshot.status, UpdateStatus::UpToDate(_));
            }
        }
        assert!(found);
        store.close().await;
    }
}
impl Drop for Updates {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.jobs.abort_all();
    }
}
