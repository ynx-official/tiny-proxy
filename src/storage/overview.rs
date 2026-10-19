use super::{Error, RequestRecord, Store};
use crate::overview::{OverviewSnapshot, OverviewWindow, aggregate};

impl Store {
    /// Read the full selected interval, independent from diagnostic pagination.
    pub async fn overview(
        &self,
        window: OverviewWindow,
        live: &[RequestRecord],
    ) -> Result<OverviewSnapshot, Error> {
        let records = sqlx::query_as::<_, RequestRecord>(
            "SELECT * FROM request_records WHERE method='POST' AND route IN ('/v1/responses','/v1/chat/completions') AND started_at>=? AND started_at<?",
        ).bind(window.start_ms).bind(window.end_ms).fetch_all(&self.pool).await?;
        Ok(aggregate(window, records, live))
    }
}
