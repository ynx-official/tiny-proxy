use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::time::Duration;
use tiny_proxy::{
    app::{Backend, Command, Event},
    storage::{KeyStatus, Store},
};

#[tokio::test]
async fn revocation_deletes_the_record_and_secret_without_affecting_other_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = Store::open(&path).await.unwrap();
    let removed = store.create_key("remove").await.unwrap();
    let retained = store.create_key("keep").await.unwrap();

    store.revoke_key(&removed.id).await.unwrap();
    store.revoke_key(&removed.id).await.unwrap();
    let keys = store.list_keys().await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].id, retained.id);
    assert!(store.reveal_key(&removed.id).await.is_err());
    assert_eq!(
        store.authenticate(&removed.value).await.unwrap(),
        KeyStatus::Invalid
    );
    store.close().await;

    let pool = SqlitePoolOptions::new()
        .connect_with(SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM client_keys WHERE id=?")
        .bind(&removed.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "revocation must remove the entire database row");
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.list_keys().await.unwrap().len(), 1);
    assert!(store.reveal_key(&removed.id).await.is_err());
    assert_eq!(
        store.authenticate(&removed.value).await.unwrap(),
        KeyStatus::Invalid
    );
    assert_eq!(
        store.authenticate(&retained.value).await.unwrap(),
        KeyStatus::Valid(retained.id.clone())
    );
    assert_eq!(
        store.reveal_key(&retained.id).await.unwrap().as_str(),
        retained.value.as_str()
    );
    store.close().await;
}

#[tokio::test]
async fn upgrade_removes_legacy_revoked_keys_and_preserves_active_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let pool = SqlitePoolOptions::new()
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    let mut legacy = sqlx::migrate!("./migrations");
    legacy.migrations = legacy
        .iter()
        .filter(|migration| migration.version <= 4)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    legacy.run(&pool).await.unwrap();
    sqlx::query("INSERT INTO app_settings (singleton_id, host_id) VALUES (1, 'legacy-host')")
        .execute(&pool)
        .await
        .unwrap();
    for (id, revoked, encrypted) in [
        (
            "revoked-recoverable",
            Some(123_i64),
            Some(b"cipher".as_slice()),
        ),
        ("revoked-digest-only", Some(123_i64), None),
        ("active-recoverable", None, Some(b"cipher".as_slice())),
        ("active-digest-only", None, None),
    ] {
        sqlx::query("INSERT INTO client_keys (id,label,public_prefix,secret_digest,revoked_at,encrypted_value) VALUES (?,?,?, ?,?,?)")
            .bind(id).bind(id).bind(id).bind(vec![0_u8; 32]).bind(revoked).bind(encrypted)
            .execute(&pool).await.unwrap();
    }
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.host_id().await.unwrap(), "legacy-host");
    let keys = store.list_keys().await.unwrap();
    assert_eq!(keys.len(), 2);
    assert!(keys.iter().all(|key| key.id.starts_with("active-")));
    store.close().await;
    let pool = SqlitePoolOptions::new()
        .connect_with(SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    let revoked: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM client_keys WHERE revoked_at IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        revoked, 0,
        "upgrade must delete old rows, not merely hide them"
    );
    pool.close().await;
    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.list_keys().await.unwrap().len(), 2);
    store.close().await;
}

#[tokio::test]
async fn failed_deletion_preserves_key_access_and_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = Store::open(&path).await.unwrap();
    let key = store.create_key("keep-on-failure").await.unwrap();
    let pool = SqlitePoolOptions::new()
        .connect_with(SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_key_deletion BEFORE DELETE ON client_keys BEGIN SELECT RAISE(ABORT, 'fixture deletion failure'); END")
        .execute(&pool).await.unwrap();
    assert!(store.revoke_key(&key.id).await.is_err());
    assert_eq!(store.list_keys().await.unwrap()[0].id, key.id);
    assert_eq!(
        store.authenticate(&key.value).await.unwrap(),
        KeyStatus::Valid(key.id.clone())
    );
    assert_eq!(
        store.reveal_key(&key.id).await.unwrap().as_str(),
        key.value.as_str()
    );
    pool.close().await;
    store.close().await;
}

#[tokio::test]
async fn backend_publishes_a_key_snapshot_without_the_revoked_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Backend::start(dir.path().join("app.db")).unwrap();
    let mut events = backend.events.take().unwrap();
    backend
        .commands
        .send(Command::CreateKey("desktop".into()))
        .await
        .unwrap();
    let key = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match events.recv().await.unwrap() {
                Event::KeyCreated(key) => break key,
                Event::Error(message) => panic!("{message}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    // Drain the creation snapshot before checking the result of revocation.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Event::Snapshot { keys, .. } = events.recv().await.unwrap()
                && keys.iter().any(|summary| summary.id == key.id)
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    backend
        .commands
        .send(Command::RevokeKey(key.id.clone()))
        .await
        .unwrap();
    let keys = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await.unwrap() {
                Event::Snapshot { keys, .. } => break keys,
                Event::Error(message) => panic!("{message}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(
        keys.is_empty(),
        "the desktop snapshot must contain no revoked key"
    );
    backend
        .commands
        .send(Command::RevealKey {
            id: key.id,
            copy: false,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await.unwrap() {
                Event::Error(_) => break,
                Event::KeyRevealed { .. } => panic!("deleted keys must not be revealed or copied"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    backend.stop();
}
