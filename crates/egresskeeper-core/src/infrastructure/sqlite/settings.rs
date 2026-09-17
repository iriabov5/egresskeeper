//! Реализация порта [`SettingsRepository`] поверх SQLite.

use rusqlite::{OptionalExtension, params};

use crate::application::ports::SettingsRepository;
use crate::domain::error::EgressError;
use crate::domain::time::now_unix_ms;
use crate::infrastructure::sqlite::repository::SqliteRepository;
use crate::infrastructure::sqlite::store::storage_error;

impl SettingsRepository for SqliteRepository {
    fn get_setting(&self, key: &str) -> Result<Option<String>, EgressError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|source| storage_error("read setting", source))
        })
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), EgressError> {
        let now = now_unix_ms()?;

        self.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO settings (key, value, updated_at_unix_ms)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT (key) DO UPDATE
                     SET value = excluded.value, updated_at_unix_ms = excluded.updated_at_unix_ms",
                    params![key, value, now],
                )
                .map_err(|source| storage_error("write setting", source))?;

            Ok(())
        })
    }
}
