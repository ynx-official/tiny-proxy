use super::{Error, Store};
use crate::updater::{CheckMode, Preferences};
use sqlx::Row;

impl Store {
    pub async fn update_preferences(&self) -> Result<Preferences, Error> {
        let row = sqlx::query("SELECT check_mode, interval_hours, notify, last_checked_at FROM update_preferences WHERE singleton_id = 1")
            .fetch_one(&self.pool).await?;
        let mode: String = row.get("check_mode");
        Ok(Preferences {
            mode: match mode.as_str() {
                "startup" => CheckMode::Startup,
                "interval" => CheckMode::Interval,
                "disabled" => CheckMode::Disabled,
                _ => return Err(Error::Unavailable),
            },
            interval_hours: row.get::<i64, _>("interval_hours") as u32,
            notify: row.get("notify"),
            last_checked_at: row.get("last_checked_at"),
        })
    }

    pub async fn save_update_preferences(&self, preferences: &Preferences) -> Result<(), Error> {
        if !(1..=8760).contains(&preferences.interval_hours) {
            return Err(Error::Unavailable);
        }
        let mode = match preferences.mode {
            CheckMode::Startup => "startup",
            CheckMode::Interval => "interval",
            CheckMode::Disabled => "disabled",
        };
        sqlx::query("UPDATE update_preferences SET check_mode = ?, interval_hours = ?, notify = ?, last_checked_at = ? WHERE singleton_id = 1")
            .bind(mode).bind(preferences.interval_hours as i64).bind(preferences.notify).bind(preferences.last_checked_at)
            .execute(&self.pool).await?;
        Ok(())
    }
}
