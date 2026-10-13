use std::time::Duration;
use tiny_proxy::{
    app::{Backend, Command, Event, appearance::ThemePreference},
    storage::Store,
};
use tokio::sync::mpsc;

#[test]
fn default_follows_system_and_manual_modes_ignore_system_changes() {
    assert_eq!(ThemePreference::default(), ThemePreference::System);
    for system_is_dark in [false, true, false] {
        assert_eq!(
            ThemePreference::System.is_dark(system_is_dark),
            system_is_dark
        );
        assert!(!ThemePreference::Light.is_dark(system_is_dark));
        assert!(ThemePreference::Dark.is_dark(system_is_dark));
    }
}

#[test]
fn preferences_have_stable_storage_values_and_reject_unknown_values() {
    for preference in [
        ThemePreference::System,
        ThemePreference::Light,
        ThemePreference::Dark,
    ] {
        assert_eq!(
            preference.as_str().parse::<ThemePreference>().unwrap(),
            preference
        );
    }
    for value in ["", "auto", "LIGHT", " light", "dark "] {
        assert!(value.parse::<ThemePreference>().is_err());
    }
}

#[tokio::test]
async fn fresh_database_defaults_to_system() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    assert_eq!(
        store.theme_preference().await.unwrap(),
        ThemePreference::System
    );
    store.close().await;
}

#[tokio::test]
async fn upgrading_existing_database_defaults_to_system_and_preserves_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    let all = sqlx::migrate!("./migrations");
    let legacy = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.iter().filter(|m| m.version < 8).cloned().collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy.run(&pool).await.unwrap();
    sqlx::query("INSERT INTO app_settings(singleton_id,host_id,gateway_bind) VALUES(1,'fixture-host','127.0.0.1:9876')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO codex_accounts(id,display_name,encrypted_blob,generation,expires_at) VALUES('fixture','Fixture',X'01',1,1)")
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE app_settings SET active_codex_account_id='fixture'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    assert_eq!(
        store.theme_preference().await.unwrap(),
        ThemePreference::System
    );
    assert_eq!(store.host_id().await.unwrap(), "fixture-host");
    assert_eq!(store.gateway_bind().await.unwrap(), "127.0.0.1:9876");
    assert_eq!(
        store.active_account().await.unwrap().as_deref(),
        Some("fixture")
    );
    assert_eq!(
        store
            .account_credentials("fixture")
            .await
            .unwrap()
            .unwrap()
            .1,
        [1]
    );
    store.close().await;
}

#[tokio::test]
async fn all_modes_survive_restart_and_do_not_change_gateway_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    for preference in [
        ThemePreference::Light,
        ThemePreference::Dark,
        ThemePreference::System,
    ] {
        let store = Store::open(&path).await.unwrap();
        store
            .save_gateway_bind("127.0.0.1:9876".parse().unwrap())
            .await
            .unwrap();
        let host = store.host_id().await.unwrap();
        store.save_theme_preference(preference).await.unwrap();
        store.close().await;

        let store = Store::open(&path).await.unwrap();
        assert_eq!(store.theme_preference().await.unwrap(), preference);
        assert_eq!(store.gateway_bind().await.unwrap(), "127.0.0.1:9876");
        assert_eq!(store.host_id().await.unwrap(), host);
        store.close().await;
    }
}

#[tokio::test]
async fn database_rejects_invalid_or_null_preferences_and_keeps_previous_choice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = Store::open(&path).await.unwrap();
    store
        .save_theme_preference(ThemePreference::Light)
        .await
        .unwrap();
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    for value in [Some("auto"), None] {
        assert!(
            sqlx::query("UPDATE app_settings SET theme_preference=? WHERE singleton_id=1")
                .bind(value)
                .execute(&pool)
                .await
                .is_err()
        );
        assert_eq!(
            store.theme_preference().await.unwrap(),
            ThemePreference::Light
        );
    }
    pool.close().await;
    store.close().await;
}

async fn next(events: &mut mpsc::Receiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("backend did not respond within five seconds")
        .expect("backend closed unexpectedly")
}

async fn ready(events: &mut mpsc::Receiver<Event>) -> ThemePreference {
    let mut preference = None;
    loop {
        match next(events).await {
            Event::Snapshot {
                theme_preference, ..
            } => preference = Some(theme_preference),
            Event::Status { busy: false, .. } => {
                return preference.expect("startup snapshot missing");
            }
            Event::Error(error) => panic!("{error}"),
            _ => {}
        }
    }
}

async fn saved(events: &mut mpsc::Receiver<Event>) -> Result<ThemePreference, String> {
    loop {
        match next(events).await {
            Event::ThemePreferenceSaved(result) => return result,
            Event::Error(error) => panic!("{error}"),
            _ => {}
        }
    }
}

async fn offline_store(path: &std::path::Path) -> Store {
    let store = Store::open(path).await.unwrap();
    // Keep startup background work local even when an update feed is configured.
    store
        .save_update_preferences(&tiny_proxy::updater::Preferences {
            mode: tiny_proxy::updater::CheckMode::Disabled,
            ..Default::default()
        })
        .await
        .unwrap();
    store
}

#[tokio::test]
async fn backend_saves_choice_and_restores_it_in_startup_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    offline_store(&path).await.close().await;
    let mut backend = Backend::start(path.clone()).unwrap();
    let mut events = backend.events.take().unwrap();
    assert_eq!(ready(&mut events).await, ThemePreference::System);
    backend
        .commands
        .send(Command::SetThemePreference(ThemePreference::Dark))
        .await
        .unwrap();
    assert_eq!(saved(&mut events).await.unwrap(), ThemePreference::Dark);
    drop(events);
    backend.stop();

    let mut backend = Backend::start(path).unwrap();
    let mut events = backend.events.take().unwrap();
    assert_eq!(ready(&mut events).await, ThemePreference::Dark);
    drop(events);
    backend.stop();
}

#[tokio::test]
async fn failed_save_reports_failure_and_preserves_last_saved_choice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = offline_store(&path).await;
    store
        .save_theme_preference(ThemePreference::Light)
        .await
        .unwrap();
    store.close().await;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_theme_update BEFORE UPDATE OF theme_preference ON app_settings BEGIN SELECT RAISE(ABORT, 'fixture'); END")
        .execute(&pool).await.unwrap();

    let mut backend = Backend::start(path.clone()).unwrap();
    let mut events = backend.events.take().unwrap();
    assert_eq!(ready(&mut events).await, ThemePreference::Light);
    backend
        .commands
        .send(Command::SetThemePreference(ThemePreference::Dark))
        .await
        .unwrap();
    assert!(saved(&mut events).await.is_err());
    drop(events);
    backend.stop();

    let store = Store::open(&path).await.unwrap();
    assert_eq!(
        store.theme_preference().await.unwrap(),
        ThemePreference::Light
    );
    pool.close().await;
    store.close().await;
}
