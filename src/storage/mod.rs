use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{path::Path, time::Duration};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroizing;
mod desktop;
mod migrations;
pub mod recovery;
mod requests;
mod updates;
pub use desktop::{AccountSummary, ModelTest};
pub use requests::{FailureRecord, RequestRecord};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("local storage failed")]
    Unavailable,
    #[error("此旧密钥仅保存了摘要，无法还原；请创建新密钥")]
    KeyUnavailable,
    #[error("密钥不存在或已撤销")]
    KeyNotFound,
    #[error("credential version changed")]
    Conflict,
    #[error("invalid label")]
    InvalidLabel,
    #[error("invalid theme preference")]
    InvalidThemePreference,
    #[error("invalid issued client ID")]
    InvalidRegistration,
    #[error("local database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("local database migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

impl Error {
    pub(crate) fn startup_message(&self, path: &Path) -> String {
        use sqlx::migrate::MigrateError;
        let reason = match self {
            Self::Migration(MigrateError::VersionMismatch(version)) => {
                format!("迁移 {version} 校验不一致，请使用兼容版本并核对迁移来源")
            }
            Self::Migration(MigrateError::VersionMissing(version)) => {
                format!("当前程序不支持数据库中的迁移 {version}，请使用支持该数据库的版本")
            }
            Self::Migration(MigrateError::Dirty(version)) => {
                format!("迁移 {version} 未完整完成，需要检查迁移记录或一致性备份")
            }
            Self::Database(error) | Self::Migration(MigrateError::Execute(error)) => {
                database_startup_reason(error).into()
            }
            Self::Migration(MigrateError::ExecuteMigration(error, version)) => {
                format!(
                    "执行迁移 {version} 失败：{}",
                    database_startup_reason(error)
                )
            }
            Self::Migration(_) => "数据库迁移失败，请检查程序版本和迁移记录".into(),
            _ => "本地存储初始化失败".into(),
        };
        format!(
            "本地数据库初始化失败：{reason}。\n数据库：{}\n请保留原有数据库，勿删除后重试。",
            path.display()
        )
    }
}

// Do not display raw SQL errors: they may contain saved values or SQL text.
fn database_startup_reason(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::Database(error) => {
            match error.code().and_then(|code| code.parse::<u32>().ok()) {
                Some(code) => match code & 0xff {
                    5 | 6 => "数据库正被占用，请退出其他使用同一数据目录的实例",
                    8 => "数据库或数据目录为只读，请检查访问权限",
                    11 | 26 => "数据库文件损坏或格式无效，请检查一致性备份",
                    13 => "数据库所在磁盘空间不足",
                    14 => "无法打开数据库文件，请检查数据目录路径与访问权限",
                    _ => "SQLite 数据库操作失败，请检查数据库状态与程序版本",
                },
                None => "数据库操作失败，请检查数据库状态与程序版本",
            }
        }
        sqlx::Error::Io(_) => "数据库文件访问失败，请检查数据目录与磁盘状态",
        sqlx::Error::PoolTimedOut => "数据库连接超时，请检查占用情况与磁盘状态",
        sqlx::Error::Configuration(_) => "数据库连接配置无效，请检查数据目录路径",
        _ => "数据库操作失败，请检查数据库状态与程序版本",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStatus {
    Valid(String),
    Invalid,
}

pub struct CreatedKey {
    pub id: String,
    pub value: Zeroizing<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeySummary {
    pub id: String,
    pub label: String,
    pub public_prefix: String,
    pub recoverable: bool,
}

#[derive(Debug, Clone)]
pub struct Registration {
    pub id: String,
    pub client_id: String,
    pub label: String,
    pub permission_state: String,
    pub active: bool,
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}
impl Store {
    pub async fn codex_credentials(&self) -> Result<Option<(i64, Vec<u8>)>, Error> {
        if let Some(id) = self.active_account().await? {
            return self.account_credentials(&id).await;
        }
        Ok(sqlx::query(
            "SELECT generation, encrypted_blob FROM codex_session WHERE singleton_id = 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|r| (r.get("generation"), r.get("encrypted_blob"))))
    }
    pub async fn save_codex_credentials(
        &self,
        generation: i64,
        blob: &[u8],
        expires_at: i64,
    ) -> Result<(), Error> {
        if blob.is_empty() {
            return Err(Error::InvalidRegistration);
        }
        let count = if generation == 0 {
            sqlx::query("INSERT INTO codex_session (singleton_id, encrypted_blob, generation, expires_at) VALUES (1, ?, 1, ?) ON CONFLICT DO NOTHING")
                .bind(blob).bind(expires_at).execute(&self.pool).await?.rows_affected()
        } else {
            sqlx::query("UPDATE codex_session SET encrypted_blob = ?, expires_at = ?, generation = generation + 1, updated_at = unixepoch() WHERE singleton_id = 1 AND generation = ?")
                .bind(blob).bind(expires_at).bind(generation).execute(&self.pool).await?.rows_affected()
        };
        if count != 1 {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    pub async fn open(path: &Path) -> Result<Self, Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        if let Err(error) = migrations::run(&pool).await {
            pool.close().await;
            return Err(error.into());
        }
        sqlx::query("INSERT OR IGNORE INTO app_settings (singleton_id, host_id) VALUES (1, ?)")
            .bind(format!("urn:uuid:{}", Uuid::new_v4()))
            .execute(&pool)
            .await?;
        Ok(Self { pool })
    }
    pub async fn host_id(&self) -> Result<String, Error> {
        Ok(
            sqlx::query_scalar("SELECT host_id FROM app_settings WHERE singleton_id = 1")
                .fetch_one(&self.pool)
                .await?,
        )
    }
    pub async fn create_key(&self, label: &str) -> Result<CreatedKey, Error> {
        let label = label.trim();
        if label.is_empty() || label.chars().count() > 80 || label.chars().any(char::is_control) {
            return Err(Error::InvalidLabel);
        }
        let id = Uuid::new_v4().simple().to_string();
        let secret = Zeroizing::new(URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>()));
        let digest = Sha256::digest(secret.as_bytes()).to_vec();
        let value = Zeroizing::new(format!("tp_{id}.{}", secret.as_str()));
        let encrypted =
            crate::platform::protection::protect_async(Zeroizing::new(value.as_bytes().to_vec()))
                .await
                .map_err(|_| Error::Unavailable)?;
        sqlx::query(
            "INSERT INTO client_keys (id, label, public_prefix, secret_digest,encrypted_value) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(label)
        .bind(format!("tp_{}", &id[..8]))
        .bind(digest)
        .bind(encrypted)
        .execute(&self.pool)
        .await?;
        Ok(CreatedKey { value, id })
    }
    pub async fn authenticate(&self, value: &str) -> Result<KeyStatus, Error> {
        let Some((prefix, secret)) = value.split_once('.') else {
            return Ok(KeyStatus::Invalid);
        };
        let Some(id) = prefix.strip_prefix("tp_") else {
            return Ok(KeyStatus::Invalid);
        };
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) || secret.len() != 43 {
            return Ok(KeyStatus::Invalid);
        }
        let Some(row) = sqlx::query(
            "SELECT secret_digest FROM client_keys WHERE id = ? AND revoked_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(KeyStatus::Invalid);
        };
        let digest: Vec<u8> = row.get("secret_digest");
        if !bool::from(digest.as_slice().ct_eq(&Sha256::digest(secret.as_bytes()))) {
            return Ok(KeyStatus::Invalid);
        }
        Ok(KeyStatus::Valid(id.into()))
    }
    /// Delete the record before the application cancels this key's requests.
    /// Repeating a successful revocation is harmless; storage failures keep it usable.
    pub async fn revoke_key(&self, id: &str) -> Result<(), Error> {
        sqlx::query("DELETE FROM client_keys WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn list_keys(&self) -> Result<Vec<KeySummary>, Error> {
        Ok(sqlx::query(
            "SELECT id, label, public_prefix, encrypted_value IS NOT NULL AS recoverable FROM client_keys WHERE revoked_at IS NULL ORDER BY created_at, id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| KeySummary {
            id: row.get("id"),
            label: row.get("label"),
            public_prefix: row.get("public_prefix"),
            recoverable: row.get("recoverable"),
        })
        .collect())
    }
    pub async fn retain_registration(&self, client_id: &str) -> Result<String, Error> {
        if !client_id.starts_with("oaiapp_")
            || client_id.len() > 256
            || client_id
                .bytes()
                .any(|c| !c.is_ascii_alphanumeric() && c != b'_' && c != b'-')
        {
            return Err(Error::InvalidRegistration);
        }
        sqlx::query("INSERT OR IGNORE INTO account_registrations (id, client_id) VALUES (?, ?)")
            .bind(Uuid::new_v4().to_string())
            .bind(client_id)
            .execute(&self.pool)
            .await?;
        Ok(
            sqlx::query_scalar("SELECT id FROM account_registrations WHERE client_id = ?")
                .bind(client_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }
    /// Caller provides an OS-protected bundle; the version check and replacement
    /// are one SQLite statement, so a stale refresh can never overwrite a newer one.
    pub async fn save_credentials(
        &self,
        id: &str,
        expected_generation: i64,
        blob: &[u8],
        expires_at: i64,
    ) -> Result<i64, Error> {
        let affected = if expected_generation == 0 {
            sqlx::query("INSERT INTO credential_bundles (registration_id, encrypted_blob, generation, expires_at) VALUES (?, ?, 1, ?) ON CONFLICT(registration_id) DO NOTHING")
                .bind(id).bind(blob).bind(expires_at).execute(&self.pool).await?.rows_affected()
        } else {
            sqlx::query("UPDATE credential_bundles SET encrypted_blob = ?, generation = generation + 1, expires_at = ?, updated_at = unixepoch() WHERE registration_id = ? AND generation = ?")
                .bind(blob).bind(expires_at).bind(id).bind(expected_generation).execute(&self.pool).await?.rows_affected()
        };
        if affected != 1 {
            return Err(Error::Conflict);
        }
        Ok(expected_generation + 1)
    }
    pub async fn load_credentials(&self, id: &str) -> Result<Option<(i64, Vec<u8>)>, Error> {
        Ok(sqlx::query(
            "SELECT generation, encrypted_blob FROM credential_bundles WHERE registration_id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| (r.get("generation"), r.get("encrypted_blob"))))
    }
    pub async fn close(&self) {
        self.pool.close().await;
    }
    pub async fn registrations(&self) -> Result<Vec<Registration>, Error> {
        Ok(sqlx::query("SELECT a.id, a.client_id, a.label, a.permission_state, a.id = s.active_registration_id AS active FROM account_registrations a CROSS JOIN app_settings s ORDER BY a.created_at")
            .fetch_all(&self.pool).await?.into_iter().map(|r| Registration {
                id: r.get("id"), client_id: r.get("client_id"), label: r.get("label"), permission_state: r.get("permission_state"), active: r.get::<Option<bool>, _>("active").unwrap_or(false),
            }).collect())
    }
    pub async fn complete_sign_in(
        &self,
        id: &str,
        subject: &str,
        inference: bool,
        blob: &[u8],
        expires_at: i64,
    ) -> Result<(), Error> {
        if subject.is_empty() || blob.is_empty() {
            return Err(Error::InvalidRegistration);
        }
        let mut tx = self.pool.begin().await?;
        let permission = if inference {
            "connected"
        } else {
            "signed_in_without_inference"
        };
        let count = sqlx::query("UPDATE account_registrations SET subject = ?, permission_state = ?, label = 'ChatGPT 连接', updated_at = unixepoch() WHERE id = ? AND (subject IS NULL OR subject = ?)")
            .bind(subject).bind(permission).bind(id).bind(subject).execute(&mut *tx).await?.rows_affected();
        if count != 1 {
            return Err(Error::InvalidRegistration);
        }
        sqlx::query("INSERT INTO credential_bundles (registration_id, encrypted_blob, generation, expires_at) VALUES (?, ?, 1, ?) ON CONFLICT(registration_id) DO UPDATE SET encrypted_blob = excluded.encrypted_blob, generation = credential_bundles.generation + 1, expires_at = excluded.expires_at, updated_at = unixepoch()")
            .bind(id).bind(blob).bind(expires_at).execute(&mut *tx).await?;
        sqlx::query("UPDATE app_settings SET active_registration_id = ? WHERE singleton_id = 1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
