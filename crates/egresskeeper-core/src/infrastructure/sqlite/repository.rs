//! Хранилище приложения на SQLite: реализация портов `PolicyRepository` и
//! `ListenerRepository`.
//!
//! Одно соединение на процесс: запись идёт короткими транзакциями, а режим WAL
//! даёт параллельное чтение. Пул понадобится, если появится конкурентный писатель
//! (например, audit-писатель proxy).

use std::path::Path;
use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::application::ports::PolicyRepository;
use crate::domain::error::EgressError;
use crate::domain::policy::action::Action;
use crate::domain::policy::entities::{Profile, ProfileDraft, ProfileId, Rule, RuleDraft, RuleId};
use crate::domain::policy::host::{HostKind, HostMatcher};
use crate::domain::policy::port::PortSpec;
use crate::domain::time::now_unix_ms;
use crate::infrastructure::sqlite::store::{SqliteStore, is_unique_violation, storage_error};

/// Сдвиг позиций перед перестановкой.
///
/// `UNIQUE(profile_id, position)` не позволяет переставлять правила «на месте»,
/// поэтому в одной транзакции позиции сначала уводятся в свободный диапазон,
/// а затем назначаются заново.
const POSITION_SHIFT: i64 = 1_000_000;

/// Верхняя граница обычных позиций правил.
///
/// Ограничение схемы задано как `POSITION_SHIFT + POSITION_LIMIT`: во время
/// перестановки позиции временно выходят за пределы обычного диапазона. Если
/// профиль дорос до этого предела, новые правила добавлять нельзя — иначе
/// перестановка перестала бы работать.
const POSITION_LIMIT: i64 = 1_000_000;

const PROFILE_COLUMNS: &str = "id, name, default_action, created_at_unix_ms, updated_at_unix_ms";
const RULE_COLUMNS: &str = "id, profile_id, position, action, matcher_kind, matcher_value, port_kind, port_start, port_end";

/// Локальное хранилище приложения на SQLite.
///
/// Клонирование дешёвое: все клоны используют одно соединение, поэтому
/// сервисы политики, listeners и источник политики для proxy работают с одним и
/// тем же состоянием.
#[derive(Debug, Clone)]
pub struct SqliteRepository {
    store: Arc<SqliteStore>,
}

impl SqliteRepository {
    /// Открывает хранилище в каталоге состояния приложения.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку каталога состояния или хранилища.
    pub fn open_in_state_dir(state_dir: &Path) -> Result<Self, EgressError> {
        Ok(Self {
            store: Arc::new(SqliteStore::open(state_dir)?),
        })
    }

    /// Открывает хранилище в памяти.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища.
    pub fn open_in_memory() -> Result<Self, EgressError> {
        Ok(Self {
            store: Arc::new(SqliteStore::open_in_memory()?),
        })
    }

    /// Возвращает версию схемы хранилища.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища.
    pub fn schema_version(&self) -> Result<i64, EgressError> {
        self.store.schema_version()
    }

    /// Выполняет операцию над соединением.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если соединение недоступно.
    pub(crate) fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut rusqlite::Connection) -> Result<T, EgressError>,
    ) -> Result<T, EgressError> {
        self.store.with_connection(operation)
    }

    /// Находит профиль по идентификатору.
    pub(crate) fn read_profile(
        connection: &Connection,
        id: &ProfileId,
    ) -> Result<Option<Profile>, EgressError> {
        connection
            .query_row(
                &format!("SELECT {PROFILE_COLUMNS} FROM profiles WHERE id = ?1"),
                params![id.as_str()],
                read_raw_profile,
            )
            .optional()
            .map_err(|source| storage_error("read profile", source))?
            .map(RawProfile::into_profile)
            .transpose()
    }

    /// Требует существования профиля и возвращает его.
    fn require_profile(&self, id: &ProfileId) -> Result<Profile, EgressError> {
        self.store.with_connection(|connection| {
            Self::read_profile(connection, id)?.ok_or_else(|| EgressError::not_found("profile"))
        })
    }

    /// Читает правила профиля в рамках соединения.
    pub(crate) fn read_rules_with(
        connection: &Connection,
        profile_id: &ProfileId,
    ) -> Result<Vec<Rule>, EgressError> {
        {
            let mut statement = connection
                .prepare(&format!(
                    "SELECT {RULE_COLUMNS} FROM rules WHERE profile_id = ?1 ORDER BY position"
                ))
                .map_err(|source| storage_error("prepare rule query", source))?;

            let rows = statement
                .query_map(params![profile_id.as_str()], read_raw_rule)
                .map_err(|source| storage_error("read rules", source))?;

            let mut raw = Vec::new();
            for row in rows {
                raw.push(row.map_err(|source| storage_error("read rule row", source))?);
            }

            raw.into_iter().map(RawRule::into_rule).collect()
        }
    }

    /// Читает правила профиля.
    fn read_rules(&self, profile_id: &ProfileId) -> Result<Vec<Rule>, EgressError> {
        self.store
            .with_connection(|connection| Self::read_rules_with(connection, profile_id))
    }
}

impl PolicyRepository for SqliteRepository {
    fn list_profiles(&self) -> Result<Vec<Profile>, EgressError> {
        self.store.with_connection(|connection| {
            let mut statement = connection
                .prepare(&format!(
                    "SELECT {PROFILE_COLUMNS} FROM profiles ORDER BY created_at_unix_ms, name"
                ))
                .map_err(|source| storage_error("prepare profile query", source))?;

            let rows = statement
                .query_map([], read_raw_profile)
                .map_err(|source| storage_error("read profiles", source))?;

            let mut raw = Vec::new();
            for row in rows {
                raw.push(row.map_err(|source| storage_error("read profile row", source))?);
            }

            raw.into_iter().map(RawProfile::into_profile).collect()
        })
    }

    fn find_profile(&self, id: &ProfileId) -> Result<Option<Profile>, EgressError> {
        self.store
            .with_connection(|connection| Self::read_profile(connection, id))
    }

    fn find_profile_by_name(&self, name: &str) -> Result<Option<Profile>, EgressError> {
        self.store.with_connection(|connection| {
            connection
                .query_row(
                    &format!("SELECT {PROFILE_COLUMNS} FROM profiles WHERE name_folded = ?1"),
                    params![Profile::fold_name(name)],
                    read_raw_profile,
                )
                .optional()
                .map_err(|source| storage_error("find profile by name", source))?
                .map(RawProfile::into_profile)
                .transpose()
        })
    }

    fn count_profiles(&self) -> Result<u64, EgressError> {
        self.store.with_connection(|connection| {
            let count: i64 = connection
                .query_row("SELECT COUNT(*) FROM profiles", [], |row| row.get(0))
                .map_err(|source| storage_error("count profiles", source))?;

            u64::try_from(count)
                .map_err(|_| EgressError::storage_message("profile count is invalid"))
        })
    }

    fn create_profile(&self, draft: &ProfileDraft) -> Result<Profile, EgressError> {
        let now = now_unix_ms()?;
        let profile = Profile {
            id: ProfileId::new(),
            name: draft.name.clone(),
            default_action: draft.default_action,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        self.store.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO profiles (id, name, name_folded, default_action, created_at_unix_ms, updated_at_unix_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        profile.id.as_str(),
                        profile.name,
                        Profile::fold_name(&profile.name),
                        profile.default_action.as_str(),
                        profile.created_at_unix_ms,
                        profile.updated_at_unix_ms
                    ],
                )
                .map_err(|source| {
                    if is_unique_violation(&source) {
                        EgressError::validation("name", "must be unique")
                    } else {
                        storage_error("insert profile", source)
                    }
                })?;

            Ok(())
        })?;

        Ok(profile)
    }

    fn rename_profile(&self, id: &ProfileId, name: &str) -> Result<Profile, EgressError> {
        let updated = now_unix_ms()?;

        let affected = self.store.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE profiles SET name = ?2, name_folded = ?3, updated_at_unix_ms = ?4 WHERE id = ?1",
                    params![id.as_str(), name, Profile::fold_name(name), updated],
                )
                .map_err(|source| {
                    if is_unique_violation(&source) {
                        EgressError::validation("name", "must be unique")
                    } else {
                        storage_error("rename profile", source)
                    }
                })
        })?;

        if affected == 0 {
            return Err(EgressError::not_found("profile"));
        }

        self.require_profile(id)
    }

    fn set_default_action(&self, id: &ProfileId, action: Action) -> Result<Profile, EgressError> {
        let updated = now_unix_ms()?;

        let affected = self.store.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE profiles SET default_action = ?2, updated_at_unix_ms = ?3 WHERE id = ?1",
                    params![id.as_str(), action.as_str(), updated],
                )
                .map_err(|source| storage_error("update default action", source))
        })?;

        if affected == 0 {
            return Err(EgressError::not_found("profile"));
        }

        self.require_profile(id)
    }

    fn delete_profile(&self, id: &ProfileId) -> Result<(), EgressError> {
        let affected = self.store.with_connection(|connection| {
            connection
                .execute("DELETE FROM profiles WHERE id = ?1", params![id.as_str()])
                .map_err(|source| storage_error("delete profile", source))
        })?;

        if affected == 0 {
            return Err(EgressError::not_found("profile"));
        }

        Ok(())
    }

    fn list_rules(&self, profile_id: &ProfileId) -> Result<Vec<Rule>, EgressError> {
        self.read_rules(profile_id)
    }

    fn count_rules(&self, profile_id: &ProfileId) -> Result<u32, EgressError> {
        self.store.with_connection(|connection| {
            let count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM rules WHERE profile_id = ?1",
                    params![profile_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(|source| storage_error("count rules", source))?;

            u32::try_from(count).map_err(|_| EgressError::storage_message("rule count is invalid"))
        })
    }

    fn add_rule(&self, profile_id: &ProfileId, draft: &RuleDraft) -> Result<Rule, EgressError> {
        let rule_id = RuleId::new();
        let (matcher_kind, matcher_value) = matcher_columns(&draft.host);
        let (port_kind, port_start, port_end) = port_columns(draft.port);

        let position = self.store.with_connection(|connection| {
            let transaction = connection
                .transaction()
                .map_err(|source| storage_error("begin rule insert", source))?;

            let exists = transaction
                .query_row(
                    "SELECT 1 FROM profiles WHERE id = ?1",
                    params![profile_id.as_str()],
                    |_row| Ok(()),
                )
                .optional()
                .map_err(|source| storage_error("check profile", source))?
                .is_some();

            if !exists {
                return Err(EgressError::not_found("profile"));
            }

            let next: i64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(position) + 1, 0) FROM rules WHERE profile_id = ?1",
                    params![profile_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(|source| storage_error("read next rule position", source))?;

            if next >= POSITION_LIMIT {
                return Err(EgressError::validation(
                    "profile_id",
                    "profile cannot hold more rules",
                ));
            }

            transaction
                .execute(
                    "INSERT INTO rules (id, profile_id, position, action, matcher_kind, matcher_value, port_kind, port_start, port_end)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        rule_id.as_str(),
                        profile_id.as_str(),
                        next,
                        draft.action.as_str(),
                        matcher_kind,
                        matcher_value,
                        port_kind,
                        port_start,
                        port_end
                    ],
                )
                .map_err(|source| storage_error("insert rule", source))?;

            transaction
                .commit()
                .map_err(|source| storage_error("commit rule insert", source))?;

            Ok(next)
        })?;

        Ok(Rule {
            id: rule_id,
            profile_id: profile_id.clone(),
            position: u32::try_from(position)
                .map_err(|_| EgressError::storage_message("rule position is invalid"))?,
            action: draft.action,
            host: draft.host.clone(),
            port: draft.port,
        })
    }

    fn update_rule(&self, id: &RuleId, draft: &RuleDraft) -> Result<Rule, EgressError> {
        let (matcher_kind, matcher_value) = matcher_columns(&draft.host);
        let (port_kind, port_start, port_end) = port_columns(draft.port);

        let rule = self.store.with_connection(|connection| {
            let affected = connection
                .execute(
                    "UPDATE rules
                     SET action = ?2, matcher_kind = ?3, matcher_value = ?4, port_kind = ?5, port_start = ?6, port_end = ?7
                     WHERE id = ?1",
                    params![
                        id.as_str(),
                        draft.action.as_str(),
                        matcher_kind,
                        matcher_value,
                        port_kind,
                        port_start,
                        port_end
                    ],
                )
                .map_err(|source| storage_error("update rule", source))?;

            if affected == 0 {
                return Err(EgressError::not_found("rule"));
            }

            connection
                .query_row(
                    &format!("SELECT {RULE_COLUMNS} FROM rules WHERE id = ?1"),
                    params![id.as_str()],
                    read_raw_rule,
                )
                .map_err(|source| storage_error("read updated rule", source))?
                .into_rule()
        })?;

        Ok(rule)
    }

    fn delete_rule(&self, id: &RuleId) -> Result<(), EgressError> {
        let affected = self.store.with_connection(|connection| {
            connection
                .execute("DELETE FROM rules WHERE id = ?1", params![id.as_str()])
                .map_err(|source| storage_error("delete rule", source))
        })?;

        if affected == 0 {
            return Err(EgressError::not_found("rule"));
        }

        Ok(())
    }

    fn reorder_rules(
        &self,
        profile_id: &ProfileId,
        ordered: &[RuleId],
    ) -> Result<Vec<Rule>, EgressError> {
        self.store.with_connection(|connection| {
            let transaction = connection
                .transaction()
                .map_err(|source| storage_error("begin reorder", source))?;

            let existing: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM rules WHERE profile_id = ?1",
                    params![profile_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(|source| storage_error("count profile rules", source))?;

            let requested = i64::try_from(ordered.len())
                .map_err(|_| EgressError::validation("rule_ids", "is too long"))?;

            if existing != requested {
                return Err(EgressError::validation(
                    "rule_ids",
                    "must list exactly the rules of the profile",
                ));
            }

            transaction
                .execute(
                    "UPDATE rules SET position = position + ?2 WHERE profile_id = ?1",
                    params![profile_id.as_str(), POSITION_SHIFT],
                )
                .map_err(|source| storage_error("shift rule positions", source))?;

            for (index, rule_id) in ordered.iter().enumerate() {
                let position = i64::try_from(index)
                    .map_err(|_| EgressError::validation("rule_ids", "is too long"))?;

                let affected = transaction
                    .execute(
                        "UPDATE rules SET position = ?3 WHERE id = ?2 AND profile_id = ?1",
                        params![profile_id.as_str(), rule_id.as_str(), position],
                    )
                    .map_err(|source| storage_error("assign rule position", source))?;

                if affected == 0 {
                    return Err(EgressError::validation(
                        "rule_ids",
                        "must list exactly the rules of the profile",
                    ));
                }
            }

            transaction
                .commit()
                .map_err(|source| storage_error("commit reorder", source))?;

            Ok(())
        })?;

        self.read_rules(profile_id)
    }
}

/// Сырая строка таблицы профилей до валидации.
struct RawProfile {
    id: String,
    name: String,
    default_action: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl RawProfile {
    /// Преобразует строку в сущность домена.
    fn into_profile(self) -> Result<Profile, EgressError> {
        let default_action = Action::parse(&self.default_action)
            .map_err(|_| EgressError::storage_message("stored profile has an invalid action"))?;

        Ok(Profile {
            id: ProfileId::from_stored(&self.id)?,
            name: self.name,
            default_action,
            created_at_unix_ms: self.created_at_unix_ms,
            updated_at_unix_ms: self.updated_at_unix_ms,
        })
    }
}

/// Сырая строка таблицы правил до валидации.
struct RawRule {
    id: String,
    profile_id: String,
    position: i64,
    action: String,
    matcher_kind: String,
    matcher_value: String,
    port_kind: String,
    port_start: Option<i64>,
    port_end: Option<i64>,
}

impl RawRule {
    /// Преобразует строку в сущность домена.
    fn into_rule(self) -> Result<Rule, EgressError> {
        let host_kind = match self.matcher_kind.as_str() {
            "exact" => HostKind::Exact,
            "subdomains" => HostKind::Subdomains,
            _ => {
                return Err(EgressError::storage_message(
                    "stored rule has an unknown host matcher kind",
                ));
            }
        };

        let host = HostMatcher::from_parts(host_kind, &self.matcher_value)
            .map_err(|_| EgressError::storage_message("stored rule has an invalid host matcher"))?;

        let port = match self.port_kind.as_str() {
            "any" => PortSpec::any(),
            "exactly" => PortSpec::exactly(stored_port(self.port_start)?)
                .map_err(|_| EgressError::storage_message("stored rule has an invalid port"))?,
            "range" => PortSpec::range(stored_port(self.port_start)?, stored_port(self.port_end)?)
                .map_err(|_| {
                    EgressError::storage_message("stored rule has an invalid port range")
                })?,
            _ => {
                return Err(EgressError::storage_message(
                    "stored rule has an unknown port kind",
                ));
            }
        };

        let action = Action::parse(&self.action)
            .map_err(|_| EgressError::storage_message("stored rule has an invalid action"))?;

        Ok(Rule {
            id: RuleId::from_stored(&self.id)?,
            profile_id: ProfileId::from_stored(&self.profile_id)?,
            position: u32::try_from(self.position)
                .map_err(|_| EgressError::storage_message("stored rule has an invalid position"))?,
            action,
            host,
            port,
        })
    }
}

/// Читает строку профиля.
fn read_raw_profile(row: &Row<'_>) -> rusqlite::Result<RawProfile> {
    Ok(RawProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        default_action: row.get(2)?,
        created_at_unix_ms: row.get(3)?,
        updated_at_unix_ms: row.get(4)?,
    })
}

/// Читает строку правила.
fn read_raw_rule(row: &Row<'_>) -> rusqlite::Result<RawRule> {
    Ok(RawRule {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        position: row.get(2)?,
        action: row.get(3)?,
        matcher_kind: row.get(4)?,
        matcher_value: row.get(5)?,
        port_kind: row.get(6)?,
        port_start: row.get(7)?,
        port_end: row.get(8)?,
    })
}

/// Преобразует сохранённый порт в `u16`.
fn stored_port(value: Option<i64>) -> Result<u16, EgressError> {
    value
        .and_then(|raw| u16::try_from(raw).ok())
        .ok_or_else(|| EgressError::storage_message("stored rule has an invalid port"))
}

/// Разбирает matcher на колонки схемы.
fn matcher_columns(host: &HostMatcher) -> (&'static str, &str) {
    match host {
        HostMatcher::Exact(value) => ("exact", value.as_str()),
        HostMatcher::Subdomains(value) => ("subdomains", value.as_str()),
    }
}

/// Разбирает ограничение порта на колонки схемы.
fn port_columns(port: PortSpec) -> (&'static str, Option<i64>, Option<i64>) {
    match port {
        PortSpec::Any => ("any", None, None),
        PortSpec::Exactly { port } => ("exactly", Some(i64::from(port)), None),
        PortSpec::Range { start, end } => ("range", Some(i64::from(start)), Some(i64::from(end))),
    }
}
