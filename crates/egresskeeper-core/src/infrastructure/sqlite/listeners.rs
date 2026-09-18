//! Реализация порта [`ListenerRepository`] поверх SQLite.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::application::ports::ListenerRepository;
use crate::domain::error::EgressError;
use crate::domain::policy::entities::ProfileId;
use crate::domain::proxy::listener::{Listener, ListenerDraft, ListenerId, ProxyPort};
use crate::domain::time::now_unix_ms;
use crate::infrastructure::sqlite::repository::SqliteRepository;
use crate::infrastructure::sqlite::store::{is_unique_violation, storage_error};

/// Колонки таблицы listeners.
pub(super) const LISTENER_COLUMNS: &str =
    "id, port, profile_id, enabled, created_at_unix_ms, updated_at_unix_ms";

impl SqliteRepository {
    /// Находит listener по идентификатору.
    pub(super) fn read_listener(
        connection: &Connection,
        id: &ListenerId,
    ) -> Result<Option<Listener>, EgressError> {
        connection
            .query_row(
                &format!("SELECT {LISTENER_COLUMNS} FROM listeners WHERE id = ?1"),
                params![id.as_str()],
                read_raw_listener,
            )
            .optional()
            .map_err(|source| storage_error("read listener", source))?
            .map(RawListener::into_listener)
            .transpose()
    }

    /// Требует существования listener'а и возвращает его.
    fn require_listener(&self, id: &ListenerId) -> Result<Listener, EgressError> {
        self.with_connection(|connection| {
            Self::read_listener(connection, id)?.ok_or_else(|| EgressError::not_found("listener"))
        })
    }
}

impl ListenerRepository for SqliteRepository {
    fn list_listeners(&self) -> Result<Vec<Listener>, EgressError> {
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare(&format!(
                    "SELECT {LISTENER_COLUMNS} FROM listeners ORDER BY created_at_unix_ms, port"
                ))
                .map_err(|source| storage_error("prepare listener query", source))?;

            let rows = statement
                .query_map([], read_raw_listener)
                .map_err(|source| storage_error("read listeners", source))?;

            let mut raw = Vec::new();
            for row in rows {
                raw.push(row.map_err(|source| storage_error("read listener row", source))?);
            }

            raw.into_iter().map(RawListener::into_listener).collect()
        })
    }

    fn find_listener(&self, id: &ListenerId) -> Result<Option<Listener>, EgressError> {
        self.with_connection(|connection| Self::read_listener(connection, id))
    }

    fn profile_exists(&self, id: &ProfileId) -> Result<bool, EgressError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT 1 FROM profiles WHERE id = ?1",
                    params![id.as_str()],
                    |_row| Ok(()),
                )
                .optional()
                .map(|found| found.is_some())
                .map_err(|source| storage_error("check profile", source))
        })
    }

    fn create_listener(&self, draft: &ListenerDraft) -> Result<Listener, EgressError> {
        let now = now_unix_ms()?;
        let listener = Listener {
            id: ListenerId::new(),
            port: draft.port,
            profile_id: draft.profile_id.clone(),
            enabled: false,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        self.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO listeners (id, port, profile_id, enabled, created_at_unix_ms, updated_at_unix_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        listener.id.as_str(),
                        i64::from(listener.port.get()),
                        listener.profile_id.as_str(),
                        listener.enabled,
                        listener.created_at_unix_ms,
                        listener.updated_at_unix_ms
                    ],
                )
                .map_err(|source| {
                    if is_unique_violation(&source) {
                        EgressError::validation("port", "is already used by another listener")
                    } else {
                        storage_error("insert listener", source)
                    }
                })?;

            Ok(())
        })?;

        Ok(listener)
    }

    fn update_listener(
        &self,
        id: &ListenerId,
        draft: &ListenerDraft,
    ) -> Result<Listener, EgressError> {
        let updated = now_unix_ms()?;

        let affected = self.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE listeners SET port = ?2, profile_id = ?3, updated_at_unix_ms = ?4 WHERE id = ?1",
                    params![
                        id.as_str(),
                        i64::from(draft.port.get()),
                        draft.profile_id.as_str(),
                        updated
                    ],
                )
                .map_err(|source| {
                    if is_unique_violation(&source) {
                        EgressError::validation("port", "is already used by another listener")
                    } else {
                        storage_error("update listener", source)
                    }
                })
        })?;

        if affected == 0 {
            return Err(EgressError::not_found("listener"));
        }

        self.require_listener(id)
    }

    fn set_listener_enabled(
        &self,
        id: &ListenerId,
        enabled: bool,
    ) -> Result<Listener, EgressError> {
        let updated = now_unix_ms()?;

        let affected = self.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE listeners SET enabled = ?2, updated_at_unix_ms = ?3 WHERE id = ?1",
                    params![id.as_str(), enabled, updated],
                )
                .map_err(|source| storage_error("update listener state", source))
        })?;

        if affected == 0 {
            return Err(EgressError::not_found("listener"));
        }

        self.require_listener(id)
    }

    fn delete_listener(&self, id: &ListenerId) -> Result<(), EgressError> {
        let affected = self.with_connection(|connection| {
            connection
                .execute("DELETE FROM listeners WHERE id = ?1", params![id.as_str()])
                .map_err(|source| storage_error("delete listener", source))
        })?;

        if affected == 0 {
            return Err(EgressError::not_found("listener"));
        }

        Ok(())
    }
}

/// Сырая строка таблицы listeners до валидации.
struct RawListener {
    id: String,
    port: i64,
    profile_id: String,
    enabled: i64,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl RawListener {
    /// Преобразует строку в сущность домена.
    fn into_listener(self) -> Result<Listener, EgressError> {
        Ok(Listener {
            id: ListenerId::from_stored(&self.id)?,
            port: ProxyPort::from_stored(self.port)?,
            profile_id: ProfileId::from_stored(&self.profile_id)?,
            enabled: self.enabled != 0,
            created_at_unix_ms: self.created_at_unix_ms,
            updated_at_unix_ms: self.updated_at_unix_ms,
        })
    }
}

/// Читает строку listener'а.
fn read_raw_listener(row: &Row<'_>) -> rusqlite::Result<RawListener> {
    Ok(RawListener {
        id: row.get(0)?,
        port: row.get(1)?,
        profile_id: row.get(2)?,
        enabled: row.get(3)?,
        created_at_unix_ms: row.get(4)?,
        updated_at_unix_ms: row.get(5)?,
    })
}
