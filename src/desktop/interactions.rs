//! Track acknowledgements so a queued click cannot submit the same operation twice.

use super::presentation::GatewayAction;
use tiny_proxy::app::{Command, Event};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum KeyAction {
    Create,
    Reveal(String),
    Revoke(String),
}

#[derive(Clone, Default)]
pub(super) struct Pending {
    pub gateway: Option<GatewayAction>,
    pub key: Option<KeyAction>,
    pub reveal_requested: bool,
    pub account: Option<String>,
    pub usage: Option<String>,
    pub refreshing_models: bool,
    pub testing_model: Option<String>,
    pub update: bool,
    pub installing: bool,
    pub history: Option<(i64, i64)>,
}

impl Pending {
    pub(super) fn allows(&self, command: &Command) -> bool {
        if self.installing {
            return false;
        }
        match command {
            Command::StartGateway(_) | Command::StopGateway => self.gateway.is_none(),
            Command::CreateKey(_) | Command::RevealKey { .. } | Command::RevokeKey(_) => {
                self.key.is_none()
            }
            Command::SelectAccount(_) => self.account.is_none(),
            Command::RefreshUsage(_) => self.usage.is_none(),
            Command::RefreshModels | Command::TestModel(_) => {
                !self.refreshing_models && self.testing_model.is_none()
            }
            Command::CheckUpdate
            | Command::DownloadUpdate
            | Command::CancelUpdateDownload
            | Command::InstallUpdate
            | Command::ConfigureUpdates(_) => !self.update,
            Command::HistoryPage { .. } => self.history.is_none(),
            _ => true,
        }
    }

    pub(super) fn after_enqueue(&self, command: &Command) -> Self {
        let mut next = self.clone();
        match command {
            Command::StartGateway(_) => next.gateway = Some(GatewayAction::Start),
            Command::StopGateway => next.gateway = Some(GatewayAction::Stop),
            Command::CreateKey(_) => {
                next.key = Some(KeyAction::Create);
                next.reveal_requested = true;
            }
            Command::RevealKey { id, copy } => {
                next.key = Some(KeyAction::Reveal(id.clone()));
                next.reveal_requested = !copy;
            }
            Command::RevokeKey(id) => next.key = Some(KeyAction::Revoke(id.clone())),
            Command::SelectAccount(id) => next.account = Some(id.clone()),
            Command::RefreshUsage(id) => next.usage = Some(id.clone()),
            Command::RefreshModels => next.refreshing_models = true,
            Command::TestModel(id) => next.testing_model = Some(id.clone()),
            Command::CheckUpdate
            | Command::DownloadUpdate
            | Command::CancelUpdateDownload
            | Command::InstallUpdate
            | Command::ConfigureUpdates(_) => next.update = true,
            Command::HistoryPage { requests, failures } => {
                next.history = Some((*requests, *failures))
            }
            _ => {}
        }
        if matches!(command, Command::InstallUpdate) {
            next.installing = true;
        }
        next
    }

    pub(super) fn receive(&mut self, event: &Event) {
        match event {
            Event::Gateway { .. } => self.gateway = None,
            Event::Snapshot { accounts, keys, .. } => {
                if self
                    .account
                    .as_ref()
                    .is_some_and(|id| accounts.iter().any(|a| a.active && &a.id == id))
                {
                    self.account = None;
                }
                if matches!(&self.key, Some(KeyAction::Revoke(id)) if !keys.iter().any(|k| &k.id == id))
                {
                    self.key = None;
                }
            }
            Event::KeyCreated(_) if self.key == Some(KeyAction::Create) => {
                self.key = None;
                self.reveal_requested = false;
            }
            Event::KeyRevealed { key, .. }
                if self.key == Some(KeyAction::Reveal(key.id.clone())) =>
            {
                self.key = None;
                self.reveal_requested = false;
            }
            Event::UsageBusy(None) => self.usage = None,
            Event::Models(_) => self.refreshing_models = false,
            Event::TestResult(_) => self.testing_model = None,
            Event::Update(snapshot) => {
                self.update = false;
                self.installing = matches!(
                    snapshot.status,
                    tiny_proxy::updater::UpdateStatus::Installing
                );
            }
            Event::History(snapshot) => {
                if let Some((requests, failures)) = self.history {
                    let last_page = |count: i64| count.saturating_sub(1).max(0) / 100;
                    let target = (
                        requests.clamp(0, last_page(snapshot.request_count)),
                        failures.clamp(0, last_page(snapshot.failure_count)),
                    );
                    if target == (snapshot.request_page, snapshot.failure_page) {
                        self.history = None;
                    }
                }
            }
            // The backend's common error event has no operation ID. Release
            // local guards with its existing error recovery instead of trapping a button.
            Event::Error(_) => *self = Self::default(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_start_stop_and_failed_retry_form_a_complete_cycle() {
        let mut pending = Pending::default();
        let start = Command::StartGateway("127.0.0.1:8787".into());
        assert!(pending.allows(&start));
        pending = pending.after_enqueue(&start);
        assert!(!pending.allows(&start));
        assert!(!pending.allows(&Command::StopGateway));
        pending.receive(&Event::Error("port unavailable".into()));
        assert!(pending.allows(&start));
        pending = pending.after_enqueue(&start);
        pending.receive(&Event::Gateway {
            address: Some("127.0.0.1:8787".into()),
        });
        assert!(pending.allows(&Command::StopGateway));
        pending = pending.after_enqueue(&Command::StopGateway);
        pending.receive(&Event::Gateway { address: None });
        assert!(pending.allows(&start));
    }

    #[test]
    fn key_creation_and_revocation_do_not_accept_repeat_clicks() {
        let create = Command::CreateKey("Fixture".into());
        let revoke = Command::RevokeKey("fixture-key".into());
        let mut pending = Pending::default().after_enqueue(&create);
        assert!(!pending.allows(&create));
        assert!(!pending.allows(&revoke));
        pending.receive(&Event::Status {
            message: "unrelated".into(),
            busy: false,
        });
        assert!(!pending.allows(&create));
        pending.receive(&Event::Error("retry".into()));
        assert!(pending.allows(&create));
    }

    #[test]
    fn model_and_update_controls_unlock_on_their_own_result() {
        let refresh = Command::RefreshModels;
        let mut pending = Pending::default().after_enqueue(&refresh);
        assert!(!pending.allows(&Command::TestModel("fixture".into())));
        pending.receive(&Event::Gateway { address: None });
        assert!(!pending.allows(&refresh));
        pending.receive(&Event::Models(vec![]));
        assert!(pending.allows(&refresh));

        let pending = Pending::default().after_enqueue(&Command::CheckUpdate);
        assert!(!pending.allows(&Command::CheckUpdate));
        assert!(!pending.allows(&Command::DownloadUpdate));
        assert!(pending.allows(&Command::StopGateway));
        let mut pending = pending;
        pending.receive(&Event::Update(Box::new(tiny_proxy::updater::Snapshot {
            preferences: Default::default(),
            status: Default::default(),
        })));
        assert!(pending.allows(&Command::CheckUpdate));
        pending = pending.after_enqueue(&Command::InstallUpdate);
        assert!(pending.installing);
        assert!(!pending.allows(&Command::StartGateway("127.0.0.1:8787".into())));
        pending.receive(&Event::Error("install failed".into()));
        assert!(!pending.installing);
        assert!(pending.allows(&Command::InstallUpdate));
    }

    #[test]
    fn hiding_plaintext_prevents_a_late_key_response_from_reopening_it() {
        let mut pending = Pending::default().after_enqueue(&Command::RevealKey {
            id: "fixture".into(),
            copy: false,
        });
        assert!(pending.reveal_requested);
        pending.reveal_requested = false;
        pending.receive(&Event::KeyRevealed {
            key: tiny_proxy::storage::CreatedKey {
                id: "fixture".into(),
                value: zeroize::Zeroizing::new("fixture-secret".into()),
            },
            copy: false,
        });
        assert!(!pending.reveal_requested);
        assert!(pending.key.is_none());
    }

    #[test]
    fn unrelated_history_refreshes_do_not_unlock_a_requested_page() {
        let command = Command::HistoryPage {
            requests: 2,
            failures: 1,
        };
        let mut pending = Pending::default().after_enqueue(&command);
        let history = |requests, failures| {
            Event::History(Box::new(tiny_proxy::app::HistorySnapshot {
                requests: vec![],
                failures: vec![],
                request_count: 300,
                failure_count: 200,
                request_page: requests,
                failure_page: failures,
                write_failures: 0,
            }))
        };
        pending.receive(&history(1, 1));
        assert!(!pending.allows(&command));
        pending.receive(&history(2, 1));
        assert!(pending.allows(&command));
    }

    #[test]
    fn a_page_clamped_after_history_retention_still_unlocks_pagination() {
        let command = Command::HistoryPage {
            requests: 2,
            failures: 1,
        };
        let mut pending = Pending::default().after_enqueue(&command);
        pending.receive(&Event::History(Box::new(
            tiny_proxy::app::HistorySnapshot {
                requests: vec![],
                failures: vec![],
                request_count: 0,
                failure_count: 0,
                request_page: 0,
                failure_page: 0,
                write_failures: 0,
            },
        )));
        assert!(pending.allows(&command));
    }

    #[test]
    fn switching_accounts_waits_for_the_requested_account_to_be_active() {
        let command = Command::SelectAccount("fixture".into());
        let mut pending = Pending::default().after_enqueue(&command);
        let snapshot = |active| Event::Snapshot {
            accounts: vec![tiny_proxy::storage::AccountSummary {
                id: "fixture".into(),
                display_name: "Fixture".into(),
                active,
                expires_at: 0,
                models_json: None,
                models_checked_at: None,
                usage_json: None,
                usage_checked_at: None,
                usage_attempted_at: None,
                usage_error: None,
            }],
            keys: vec![],
            codex_connected: true,
            models: vec![],
            models_checked_at: None,
            tests: vec![],
            theme_preference: Default::default(),
        };
        pending.receive(&snapshot(false));
        assert!(!pending.allows(&command));
        pending.receive(&snapshot(true));
        assert!(pending.allows(&command));
    }

    #[test]
    fn quota_refresh_waits_for_completion_and_revoke_waits_for_committed_removal() {
        let refresh = Command::RefreshUsage("fixture".into());
        let mut pending = Pending::default().after_enqueue(&refresh);
        pending.receive(&Event::UsageBusy(Some("fixture".into())));
        assert!(!pending.allows(&refresh));
        pending.receive(&Event::UsageBusy(None));
        assert!(pending.allows(&refresh));

        let revoke = Command::RevokeKey("fixture".into());
        let mut pending = Pending::default().after_enqueue(&revoke);
        let snapshot = |keys| Event::Snapshot {
            keys,
            accounts: vec![],
            codex_connected: false,
            models: vec![],
            models_checked_at: None,
            tests: vec![],
            theme_preference: Default::default(),
        };
        pending.receive(&snapshot(vec![tiny_proxy::storage::KeySummary {
            id: "fixture".into(),
            label: "Fixture".into(),
            public_prefix: "fixture".into(),
            recoverable: true,
        }]));
        assert!(!pending.allows(&revoke));
        pending.receive(&snapshot(vec![]));
        assert!(pending.allows(&revoke));
    }
}
