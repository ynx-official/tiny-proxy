use super::{Error, Store};
use crate::platform::protection;
use sqlx::{FromRow, Row};
use zeroize::Zeroizing;

#[derive(Clone, Debug, FromRow)]
pub struct AccountSummary {
    pub id: String,
    pub display_name: String,
    pub active: bool,
    pub expires_at: i64,
    pub models_json: Option<String>,
    pub models_checked_at: Option<i64>,
    pub usage_json: Option<String>,
    pub usage_checked_at: Option<i64>,
    pub usage_attempted_at: Option<i64>,
    pub usage_error: Option<String>,
}
#[derive(Clone, Debug, FromRow)]
pub struct ModelTest {
    pub id: String,
    pub account_id: String,
    pub model_id: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    #[sqlx(rename = "result_text")]
    pub text: Option<String>,
    pub error: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}
impl Store {
    pub async fn gateway_bind(&self) -> Result<String, Error> {
        Ok(
            sqlx::query_scalar("SELECT gateway_bind FROM app_settings WHERE singleton_id=1")
                .fetch_one(&self.pool)
                .await?,
        )
    }
    pub async fn save_gateway_bind(&self, address: std::net::SocketAddr) -> Result<(), Error> {
        sqlx::query("UPDATE app_settings SET gateway_bind=? WHERE singleton_id=1")
            .bind(address.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn reveal_key(&self, id: &str) -> Result<Zeroizing<String>, Error> {
        let row = sqlx::query(
            "SELECT encrypted_value FROM client_keys WHERE id=? AND revoked_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::KeyNotFound)?;
        let blob: Vec<u8> = row
            .get::<Option<Vec<u8>>, _>("encrypted_value")
            .ok_or(Error::KeyUnavailable)?;
        let raw = protection::unprotect(&blob).map_err(|_| Error::Unavailable)?;
        Ok(Zeroizing::new(
            String::from_utf8(raw.to_vec()).map_err(|_| Error::Unavailable)?,
        ))
    }
    pub async fn accounts(&self) -> Result<Vec<AccountSummary>, Error> {
        Ok(sqlx::query_as::<_,AccountSummary>("SELECT a.id, a.display_name, COALESCE(a.id=s.active_codex_account_id,0) AS active, a.expires_at, a.models_json, a.models_checked_at, a.usage_json, a.usage_checked_at, a.usage_attempted_at, a.usage_error FROM codex_accounts a CROSS JOIN app_settings s ORDER BY a.created_at,a.id").fetch_all(&self.pool).await?)
    }
    pub async fn account(&self, id: &str) -> Result<Option<AccountSummary>, Error> {
        Ok(self.accounts().await?.into_iter().find(|a| a.id == id))
    }
    pub async fn active_account(&self) -> Result<Option<String>, Error> {
        Ok(sqlx::query_scalar(
            "SELECT active_codex_account_id FROM app_settings WHERE singleton_id=1",
        )
        .fetch_one(&self.pool)
        .await?)
    }
    pub async fn activate_account(&self, id: &str) -> Result<(), Error> {
        let affected=sqlx::query("UPDATE app_settings SET active_codex_account_id=? WHERE singleton_id=1 AND EXISTS(SELECT 1 FROM codex_accounts WHERE id=?)").bind(id).bind(id).execute(&self.pool).await?.rows_affected();
        if affected != 1 {
            return Err(Error::InvalidRegistration);
        }
        Ok(())
    }
    pub async fn account_credentials(&self, id: &str) -> Result<Option<(i64, Vec<u8>)>, Error> {
        Ok(
            sqlx::query("SELECT generation,encrypted_blob FROM codex_accounts WHERE id=?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .map(|r| (r.get("generation"), r.get("encrypted_blob"))),
        )
    }
    pub async fn upsert_account(
        &self,
        id: &str,
        label: &str,
        blob: &[u8],
        expires: i64,
        activate: bool,
    ) -> Result<(), Error> {
        if id.is_empty() || blob.is_empty() {
            return Err(Error::InvalidRegistration);
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO codex_accounts(id,display_name,encrypted_blob,generation,expires_at) VALUES(?,?,?,1,?) ON CONFLICT(id) DO UPDATE SET display_name=excluded.display_name, encrypted_blob=excluded.encrypted_blob, generation=codex_accounts.generation+1,expires_at=excluded.expires_at,updated_at=unixepoch()")
            .bind(id).bind(label).bind(blob).bind(expires).execute(&mut *tx).await?;
        if activate {
            sqlx::query("UPDATE app_settings SET active_codex_account_id=? WHERE singleton_id=1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn rotate_account(
        &self,
        id: &str,
        generation: i64,
        blob: &[u8],
        expires: i64,
    ) -> Result<(), Error> {
        if blob.is_empty() {
            return Err(Error::InvalidRegistration);
        }
        let affected=sqlx::query("UPDATE codex_accounts SET encrypted_blob=?,generation=generation+1,expires_at=?,updated_at=unixepoch() WHERE id=? AND generation=?")
            .bind(blob).bind(expires).bind(id).bind(generation).execute(&self.pool).await?.rows_affected();
        if affected != 1 {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    pub async fn save_models(&self, id: &str, json: &str, at: i64) -> Result<(), Error> {
        sqlx::query("UPDATE codex_accounts SET models_json=?,models_checked_at=? WHERE id=?")
            .bind(json)
            .bind(at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn mark_usage_attempt(&self, id: &str, at: i64) -> Result<(), Error> {
        sqlx::query(
            "UPDATE codex_accounts SET usage_attempted_at=?,usage_error='正在查询…' WHERE id=?",
        )
        .bind(at)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    pub async fn save_usage(
        &self,
        id: &str,
        json: Option<&str>,
        error: Option<&str>,
        at: i64,
    ) -> Result<(), Error> {
        sqlx::query("UPDATE codex_accounts SET usage_attempted_at=?, usage_json=COALESCE(?,usage_json),usage_checked_at=CASE WHEN ? IS NOT NULL THEN ? ELSE usage_checked_at END,usage_error=? WHERE id=?")
            .bind(at).bind(json).bind(json).bind(at).bind(error).bind(id).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn due_accounts(&self, at: i64) -> Result<Vec<String>, Error> {
        Ok(sqlx::query_scalar("SELECT id FROM codex_accounts WHERE usage_attempted_at IS NULL OR usage_attempted_at+3600<=? ORDER BY created_at,id").bind(at).fetch_all(&self.pool).await?)
    }
    pub async fn start_model_test(
        &self,
        account: &str,
        model: &str,
        at: i64,
    ) -> Result<String, Error> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO model_tests(id,account_id,model_id,status,started_at) VALUES(?,?,?,'running',?)").bind(&id).bind(account).bind(model).bind(at).execute(&self.pool).await?;
        Ok(id)
    }
    pub async fn finish_model_test(
        &self,
        id: &str,
        text: Option<&str>,
        error: Option<&str>,
        input: Option<u64>,
        output: Option<u64>,
        at: i64,
    ) -> Result<(), Error> {
        sqlx::query("UPDATE model_tests SET status=?,finished_at=?,result_text=?,error=?,input_tokens=?,output_tokens=? WHERE id=? AND status='running'")
            .bind(if text.is_some(){"completed"}else{"failed"}).bind(at).bind(text).bind(error).bind(input.and_then(|n|i64::try_from(n).ok())).bind(output.and_then(|n|i64::try_from(n).ok())).bind(id).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn model_tests(&self, id: &str) -> Result<Vec<ModelTest>, Error> {
        // The durable history is retained; the desktop loads the latest 100 rows.
        Ok(sqlx::query_as::<_,ModelTest>("SELECT * FROM model_tests WHERE account_id=? ORDER BY started_at DESC,rowid DESC LIMIT 100").bind(id).fetch_all(&self.pool).await?)
    }
    pub async fn interrupt_model_tests(&self) -> Result<(), Error> {
        sqlx::query("UPDATE model_tests SET status='interrupted',finished_at=unixepoch(),error='应用退出，测试未完成' WHERE status='running'").execute(&self.pool).await?;
        sqlx::query("UPDATE codex_accounts SET usage_attempted_at=NULL,usage_error='上次查询中断，将重新查询' WHERE usage_error='正在查询…'").execute(&self.pool).await?;
        Ok(())
    }
}
