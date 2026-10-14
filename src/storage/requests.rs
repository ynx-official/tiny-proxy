use super::{Error, Store};
use sqlx::{FromRow, QueryBuilder, Sqlite};

/// Metadata only: never put request/response bodies or HTTP headers in this type.
#[derive(Clone, Debug, FromRow)]
pub struct RequestRecord {
    pub id: String,
    pub method: String,
    pub route: String,
    pub model: String,
    pub key_id: Option<String>,
    pub account_id: Option<String>,
    pub stream: Option<bool>,
    pub body_bytes: Option<i64>,
    pub message_count: Option<i64>,
    pub tool_count: Option<i64>,
    pub outcome: String,
    pub phase: String,
    pub http_status: Option<i64>,
    pub upstream_status: Option<i64>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub elapsed_ms: i64,
    pub first_byte_at: Option<i64>,
    pub first_output_at: Option<i64>,
    pub last_activity_at: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}
impl RequestRecord {
    pub fn new(id: String, method: String, route: String, started_at: i64) -> Self {
        Self {
            id,
            method,
            route,
            model: String::new(),
            key_id: None,
            account_id: None,
            stream: None,
            body_bytes: None,
            message_count: None,
            tool_count: None,
            outcome: "running".into(),
            phase: "authenticating".into(),
            http_status: None,
            upstream_status: None,
            started_at,
            ended_at: None,
            elapsed_ms: 0,
            first_byte_at: None,
            first_output_at: None,
            last_activity_at: None,
            input_tokens: None,
            output_tokens: None,
        }
    }
}

#[derive(Clone, Debug, FromRow)]
pub struct FailureRecord {
    pub request_id: String,
    pub occurred_at: i64,
    pub phase: String,
    pub code: String,
    /// Code-owned field paths and rules; never arbitrary field names, values or upstream bodies.
    pub message: String,
    pub http_status: i64,
}

fn update(r: &RequestRecord) -> QueryBuilder<'_, Sqlite> {
    let mut q = QueryBuilder::new("UPDATE request_records SET model=");
    q.push_bind(&r.model)
        .push(",key_id=")
        .push_bind(&r.key_id)
        .push(",account_id=")
        .push_bind(&r.account_id)
        .push(",stream=")
        .push_bind(r.stream)
        .push(",body_bytes=")
        .push_bind(r.body_bytes)
        .push(",message_count=")
        .push_bind(r.message_count)
        .push(",tool_count=")
        .push_bind(r.tool_count)
        .push(",outcome=")
        .push_bind(&r.outcome)
        .push(",phase=")
        .push_bind(&r.phase)
        .push(",http_status=")
        .push_bind(r.http_status)
        .push(",upstream_status=")
        .push_bind(r.upstream_status)
        .push(",ended_at=")
        .push_bind(r.ended_at)
        .push(",elapsed_ms=")
        .push_bind(r.elapsed_ms)
        .push(",first_byte_at=")
        .push_bind(r.first_byte_at)
        .push(",first_output_at=")
        .push_bind(r.first_output_at)
        .push(",last_activity_at=")
        .push_bind(r.last_activity_at)
        .push(",input_tokens=")
        .push_bind(r.input_tokens)
        .push(",output_tokens=")
        .push_bind(r.output_tokens)
        .push(" WHERE id=")
        .push_bind(&r.id)
        .push(" AND ended_at IS NULL");
    q
}
impl Store {
    pub async fn start_request(&self, r: &RequestRecord) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO request_records(id,method,route,started_at,outcome,phase) VALUES(?,?,?,?,?,?)")
            .bind(&r.id).bind(&r.method).bind(&r.route).bind(r.started_at)
            .bind(&r.outcome).bind(&r.phase).execute(&mut *tx).await?;
        update(r).build().execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn checkpoint_request(&self, r: &RequestRecord) -> Result<(), Error> {
        update(r).build().execute(&self.pool).await?;
        Ok(())
    }
    /// Completion and its exception commit together. A late checkpoint or second
    /// completion cannot reopen or overwrite a terminal record.
    pub async fn finish_request(
        &self,
        r: &RequestRecord,
        error: Option<&FailureRecord>,
    ) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        let changed = update(r).build().execute(&mut *tx).await?.rows_affected();
        if changed == 1
            && let Some(e) = error
        {
            sqlx::query("INSERT INTO failure_records(request_id,occurred_at,phase,code,message,http_status) VALUES(?,?,?,?,?,?)")
                .bind(&e.request_id).bind(e.occurred_at).bind(&e.phase).bind(&e.code)
                .bind(&e.message).bind(e.http_status).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn request_records(
        &self,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<RequestRecord>, Error> {
        Ok(sqlx::query_as("SELECT * FROM request_records ORDER BY ended_at IS NULL DESC, started_at DESC,id LIMIT ? OFFSET ?")
            .bind(limit.clamp(1, 100)).bind(offset.max(0)).fetch_all(&self.pool).await?)
    }
    pub async fn request_record(&self, id: &str) -> Result<Option<RequestRecord>, Error> {
        Ok(sqlx::query_as("SELECT * FROM request_records WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }
    pub async fn failure_records(
        &self,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<FailureRecord>, Error> {
        Ok(sqlx::query_as(
            "SELECT * FROM failure_records ORDER BY occurred_at DESC,request_id LIMIT ? OFFSET ?",
        )
        .bind(limit.clamp(1, 100))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await?)
    }
    pub async fn failure_record(&self, request_id: &str) -> Result<Option<FailureRecord>, Error> {
        Ok(
            sqlx::query_as("SELECT * FROM failure_records WHERE request_id=?")
                .bind(request_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }
    pub async fn record_counts(&self) -> Result<(i64, i64), Error> {
        Ok(sqlx::query_as(
            "SELECT (SELECT count(*) FROM request_records),(SELECT count(*) FROM failure_records)",
        )
        .fetch_one(&self.pool)
        .await?)
    }
    pub async fn interrupt_requests(&self, ended_at: i64, code: &str) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        // SQL recovery avoids loading arbitrarily many stale rows into memory.
        sqlx::query("INSERT OR IGNORE INTO failure_records(request_id,occurred_at,phase,code,message,http_status) SELECT id,?,phase,?,'Application ended before the request finished',503 FROM request_records WHERE ended_at IS NULL")
            .bind(ended_at).bind(code).execute(&mut *tx).await?;
        // Recovery time is observable, crash time is not. Preserve the last
        // checkpoint's duration instead of including hours while the app was closed.
        sqlx::query("UPDATE request_records SET outcome='interrupted',ended_at=max(started_at,?) WHERE ended_at IS NULL")
            .bind(ended_at).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn prune_request_records(&self, before: i64, max_finished: i64) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM request_records WHERE ended_at IS NOT NULL AND started_at<?")
            .bind(before)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM request_records WHERE id IN (SELECT id FROM request_records WHERE ended_at IS NOT NULL ORDER BY started_at DESC,id LIMIT -1 OFFSET ?)")
            .bind(max_finished.max(0)).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}
