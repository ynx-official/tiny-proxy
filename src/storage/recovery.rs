use crate::platform::single_instance::InstanceGuard;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("旧数据迁移失败，原库已保留；请检查数据目录权限")]
    Io(#[from] std::io::Error),
    #[error("旧数据库无法读取，原库已保留；未创建空库")]
    Database(#[from] sqlx::Error),
    #[error("发现多份有数据的旧数据库，未自动选择或覆盖；请先核对数据来源")]
    MultipleSources,
    #[error("旧数据库完整性检查失败，原库已保留")]
    Integrity,
    #[error("旧版 Tiny Proxy 仍在运行，请先关闭旧窗口再启动")]
    InUse,
}

async fn open_read_only(path: &Path) -> Result<SqliteConnection, Error> {
    Ok(
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(path).read_only(true))
            .await?,
    )
}

async fn check_integrity(connection: &mut SqliteConnection) -> Result<(), Error> {
    let checks: Vec<String> = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_all(connection)
        .await?;
    if checks != ["ok"] {
        return Err(Error::Integrity);
    }
    Ok(())
}

async fn contains_history(connection: &mut SqliteConnection) -> Result<bool, Error> {
    check_integrity(connection).await?;
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table'")
            .fetch_all(&mut *connection)
            .await?;
    for table in [
        "client_keys",
        "codex_session",
        "codex_accounts",
        "model_tests",
        "account_registrations",
        "credential_bundles",
    ] {
        if tables.iter().any(|name| name == table) {
            let query = format!("SELECT EXISTS(SELECT 1 FROM {table})");
            if sqlx::query_scalar::<_, bool>(&query)
                .fetch_one(&mut *connection)
                .await?
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Copy an unambiguous legacy database including WAL. Caller holds the target
/// instance lock. Source contents remain intact; an existing target always wins.
pub async fn recover_legacy(
    database: &Path,
    legacy_directories: &[PathBuf],
) -> Result<Option<PathBuf>, Error> {
    if database.try_exists()? {
        return Ok(None);
    }
    let mut seen = HashSet::new();
    let mut guards = Vec::new();
    let mut source = None;
    for directory in legacy_directories {
        let path = directory.join("app.db");
        if !path.try_exists()? {
            continue;
        }
        // AppData and its explicit LocalCache path may be aliases of one file.
        let path = std::fs::canonicalize(path)?;
        if !seen.insert(path.clone()) {
            continue;
        }
        let lock_path = path
            .parent()
            .expect("database has a parent")
            .join("instance.lock");
        guards.push(InstanceGuard::acquire(&lock_path).map_err(|_| Error::InUse)?);
        let mut connection = open_read_only(&path).await?;
        if contains_history(&mut connection).await? {
            if source.is_some() {
                return Err(Error::MultipleSources);
            }
            source = Some((path, connection));
        } else {
            connection.close().await?;
        }
    }
    let Some((path, mut connection)) = source else {
        return Ok(None);
    };
    let staging = database.with_file_name(format!("recovery-{}.db", uuid::Uuid::new_v4()));
    // SQLite's logical snapshot includes committed WAL pages, unlike copying app.db.
    sqlx::query("VACUUM INTO ?")
        .bind(staging.to_string_lossy().as_ref())
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let mut restored = open_read_only(&staging).await?;
    check_integrity(&mut restored).await?;
    restored.close().await?;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&staging)?
        .sync_all()?;
    // Atomically publish without ever replacing a concurrently created target.
    std::fs::hard_link(&staging, database)?;
    let _ = std::fs::remove_file(staging);
    Ok(Some(path))
}
