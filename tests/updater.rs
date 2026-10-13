use std::time::Duration;
use tiny_proxy::updater::{CheckMode, Preferences, automatic_update_delay, is_newer_version};

#[test]
fn versions_compare_numerically_and_respect_prereleases() {
    assert!(is_newer_version("0.1.9", "v0.1.10").unwrap());
    assert!(!is_newer_version("0.1.10", "v0.1.9").unwrap());
    assert!(is_newer_version("0.2.0-beta.1", "v0.2.0").unwrap());
    assert!(!is_newer_version("0.2.0", "v0.2.0-beta.1").unwrap());
    assert!(is_newer_version("0.1.0", "invalid").is_err());
}

#[test]
fn update_checks_default_to_startup_without_automatic_installation() {
    let prefs = Preferences::default();
    assert_eq!(prefs.mode, CheckMode::Startup);
    assert_eq!(prefs.interval_hours, 24);
    assert!(prefs.notify);
    assert_eq!(prefs.last_checked_at, 0);
    assert_eq!(automatic_update_delay(24, 0, 1000), Duration::ZERO);
    assert_eq!(
        automatic_update_delay(24, 1000, 4600),
        Duration::from_secs(23 * 3600)
    );
    assert_eq!(
        automatic_update_delay(24, 1000, 1000 + 24 * 3600),
        Duration::ZERO
    );
}

#[tokio::test]
async fn preferences_survive_restart_and_reject_invalid_intervals() {
    use tiny_proxy::storage::Store;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("app.db");
    let store = Store::open(&path).await.unwrap();
    assert_eq!(
        store.update_preferences().await.unwrap(),
        Preferences::default()
    );
    let mut prefs = Preferences {
        mode: CheckMode::Interval,
        interval_hours: 3,
        notify: false,
        last_checked_at: 1000,
    };
    store.save_update_preferences(&prefs).await.unwrap();
    store.close().await;
    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.update_preferences().await.unwrap(), prefs);
    prefs.interval_hours = 0;
    assert!(store.save_update_preferences(&prefs).await.is_err());
    assert_eq!(store.update_preferences().await.unwrap().interval_hours, 3);
    store.close().await;
}
