use sqlx::{
    SqlitePool,
    migrate::{MigrateError, Migration, Migrator},
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{borrow::Cow, path::Path, time::Duration};
use tiny_proxy::{
    app::{Backend, Event},
    storage::{Error, Store},
};

// Use real SQLx migrations so the fixture records the checksums produced by
// Windows release checkouts, rather than merely changing a database marker.
async fn legacy_database(path: &Path, latest: i64, crlf: impl Fn(i64) -> bool) -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    let migrator = fixture_migrator(latest, crlf);
    migrator.run(&pool).await.unwrap();
    sqlx::query("INSERT INTO app_settings(singleton_id,host_id,gateway_bind) VALUES(1,'fixture-host','127.0.0.1:9876')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO codex_accounts(id,display_name,encrypted_blob,generation,expires_at) VALUES('fixture','Fixture',X'010203',2,100)")
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE app_settings SET active_codex_account_id='fixture'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO client_keys(id,label,public_prefix,secret_digest,encrypted_value) VALUES('fixture-key','Fixture key','fixture',?,X'040506')")
        .bind(vec![0_u8; 32]).execute(&pool).await.unwrap();
    pool
}

fn fixture_migrator(latest: i64, crlf: impl Fn(i64) -> bool) -> Migrator {
    let source = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            source
                .iter()
                .filter(|migration| migration.version <= latest)
                .map(|migration| {
                    let lf = migration.sql.replace("\r\n", "\n");
                    let sql = if crlf(migration.version) {
                        lf.replace('\n', "\r\n")
                    } else {
                        lf
                    };
                    Migration::new(
                        migration.version,
                        migration.description.clone(),
                        migration.migration_type,
                        Cow::Owned(sql),
                        migration.no_tx,
                    )
                })
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
}

async fn migration_records(pool: &SqlitePool) -> Vec<(i64, bool, Vec<u8>)> {
    sqlx::query_as("SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version")
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn assert_saved_data(store: &Store, pool: &SqlitePool) {
    assert_eq!(store.host_id().await.unwrap(), "fixture-host");
    assert_eq!(store.gateway_bind().await.unwrap(), "127.0.0.1:9876");
    assert_eq!(
        store.active_account().await.unwrap().as_deref(),
        Some("fixture")
    );
    assert_eq!(
        store.account_credentials("fixture").await.unwrap().unwrap(),
        (2, vec![1, 2, 3])
    );
    assert_eq!(store.list_keys().await.unwrap().len(), 1);
    let encrypted: Vec<u8> =
        sqlx::query_scalar("SELECT encrypted_value FROM client_keys WHERE id='fixture-key'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(encrypted, [4, 5, 6]);
}

#[tokio::test]
async fn windows_checksums_reopen_without_rewriting_history_or_saved_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let pool = legacy_database(&path, 8, |_| true).await;
    let before = migration_records(&pool).await;
    // This was the exact release/startup failure before compatibility handling.
    assert!(matches!(
        fixture_migrator(8, |_| false).run(&pool).await,
        Err(MigrateError::VersionMismatch(1))
    ));
    for _ in 0..2 {
        let store = Store::open(&path).await.unwrap();
        assert_saved_data(&store, &pool).await;
        assert_eq!(migration_records(&pool).await, before);
        store.close().await;
    }
    pool.close().await;
}

#[tokio::test]
async fn mixed_line_ending_history_reopens_and_stays_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let pool = legacy_database(&path, 8, |version| version % 2 == 0).await;
    let before = migration_records(&pool).await;
    let store = Store::open(&path).await.unwrap();
    assert_saved_data(&store, &pool).await;
    assert_eq!(migration_records(&pool).await, before);
    store.close().await;
    pool.close().await;
}

#[tokio::test]
async fn windows_history_upgrades_with_canonical_new_checksums() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let pool = legacy_database(&path, 7, |_| true).await;
    let before = migration_records(&pool).await;
    let store = Store::open(&path).await.unwrap();
    assert_saved_data(&store, &pool).await;
    let after = migration_records(&pool).await;
    assert_eq!(&after[..before.len()], before.as_slice());
    let last = fixture_migrator(8, |_| false);
    let theme = last
        .iter()
        .find(|migration| migration.version == 8)
        .unwrap();
    assert_eq!(after[7].2, theme.checksum.as_ref());
    store.close().await;
    pool.close().await;
}

#[tokio::test]
async fn fresh_database_records_lf_checksums() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = Store::open(&path).await.unwrap();
    let pool = SqlitePoolOptions::new()
        .connect_with(SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    let records = migration_records(&pool).await;
    let source = sqlx::migrate!("./migrations");
    assert_eq!(records.len(), source.iter().len());
    for (record, migration) in records.iter().zip(source.iter()) {
        let canonical = Migration::new(
            migration.version,
            migration.description.clone(),
            migration.migration_type,
            Cow::Owned(migration.sql.replace("\r\n", "\n")),
            migration.no_tx,
        );
        assert_eq!(record.0, canonical.version);
        assert!(record.1);
        assert_eq!(record.2, canonical.checksum.as_ref());
    }
    store.close().await;
    let reopened = Store::open(&path).await.unwrap();
    assert_eq!(migration_records(&pool).await, records);
    reopened.close().await;
    pool.close().await;
}

#[tokio::test]
async fn changed_sql_unknown_versions_and_dirty_history_still_fail_closed() {
    for scenario in ["changed", "newer", "dirty"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.db");
        let pool = legacy_database(&path, 8, |_| true).await;
        match scenario {
            "changed" => {
                sqlx::query("UPDATE _sqlx_migrations SET checksum=? WHERE version=8")
                    .bind(vec![0_u8; 48])
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            "newer" => {
                sqlx::query("INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES(99,'future',1,?,0)")
                    .bind(vec![0_u8; 48]).execute(&pool).await.unwrap();
            }
            "dirty" => {
                sqlx::query("UPDATE _sqlx_migrations SET success=0 WHERE version=8")
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before = migration_records(&pool).await;
        let error = Store::open(&path).await.err().expect("startup must fail");
        assert!(match scenario {
            "changed" => matches!(error, Error::Migration(MigrateError::VersionMismatch(8))),
            "newer" => matches!(error, Error::Migration(MigrateError::VersionMissing(99))),
            "dirty" => matches!(error, Error::Migration(MigrateError::Dirty(8))),
            _ => unreachable!(),
        });
        assert_eq!(migration_records(&pool).await, before);
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT host_id FROM app_settings")
                .fetch_one(&pool)
                .await
                .unwrap(),
            "fixture-host"
        );
        pool.close().await;
    }
}

async fn startup_error(path: &Path) -> String {
    let mut backend = Backend::start(path.to_path_buf()).unwrap();
    let mut events = backend.events.take().unwrap();
    let event = tokio::time::timeout(Duration::from_secs(10), events.recv())
        .await
        .unwrap()
        .expect("startup must report its failure");
    let Event::Error(message) = event else {
        panic!("startup failure must not be reported as ready");
    };
    assert!(
        tokio::time::timeout(Duration::from_secs(10), events.recv())
            .await
            .expect("failed startup must close its event channel")
            .is_none()
    );
    backend.stop();
    assert!(message.contains(path.to_string_lossy().as_ref()));
    message
}

#[tokio::test]
async fn backend_reports_migration_conflict_instead_of_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let pool = legacy_database(&path, 8, |_| false).await;
    sqlx::query("UPDATE _sqlx_migrations SET checksum=? WHERE version=8")
        .bind(vec![0_u8; 48])
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let message = startup_error(&path).await;
    assert!(message.contains("迁移 8 校验不一致"), "{message}");
    assert!(!message.contains("检查数据目录权限"), "{message}");
}

#[tokio::test]
async fn backend_distinguishes_unopenable_and_corrupt_databases() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing-parent/app.db");
    let message = startup_error(&missing).await;
    assert!(message.contains("无法打开数据库文件"), "{message}");
    assert!(!missing.exists());

    let corrupt = dir.path().join("app.db");
    let content = b"not a SQLite database; private fixture content";
    std::fs::write(&corrupt, content).unwrap();
    let message = startup_error(&corrupt).await;
    assert!(message.contains("数据库文件损坏或格式无效"), "{message}");
    assert!(!message.contains("private fixture content"));
    assert_eq!(std::fs::read(&corrupt).unwrap(), content);
}
