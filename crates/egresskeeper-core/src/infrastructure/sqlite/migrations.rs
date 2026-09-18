//! Версионированные миграции схемы хранилища.
//!
//! Версия схемы хранится в `PRAGMA user_version`: это часть самого файла базы,
//! поэтому состояние схемы не может разойтись с состоянием данных. Миграции
//! применяются по одной, каждая — в отдельной транзакции.
//!
//! Уникальность имени профиля обеспечивается колонкой `name_folded` с
//! Unicode-приведением к нижнему регистру: `COLLATE NOCASE` в SQLite работает
//! только с ASCII и не различал бы «Работа» и «работа».

use rusqlite::Connection;

use crate::domain::error::EgressError;
use crate::infrastructure::sqlite::store::storage_error;

/// Описание миграции.
struct Migration {
    /// Версия, в которую переводит миграция.
    version: i64,
    /// Краткое описание для логов.
    description: &'static str,
    /// SQL миграции.
    sql: &'static str,
}

/// Все миграции схемы в порядке применения.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "policies: profiles and rules",
        sql: SCHEMA_V1,
    },
    Migration {
        version: 2,
        description: "proxy: listeners",
        sql: SCHEMA_V2,
    },
    Migration {
        version: 3,
        description: "shell: application settings",
        sql: SCHEMA_V3,
    },
];

/// Начальная схема: профили политики и их правила.
const SCHEMA_V1: &str = r"
CREATE TABLE profiles (
    id                 TEXT    PRIMARY KEY NOT NULL,
    name               TEXT    NOT NULL,
    name_folded        TEXT    NOT NULL UNIQUE,
    default_action     TEXT    NOT NULL CHECK (default_action IN ('allow', 'deny')),
    created_at_unix_ms INTEGER NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL
);

CREATE TABLE rules (
    id            TEXT    PRIMARY KEY NOT NULL,
    profile_id    TEXT    NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    -- Верхняя граница покрывает временный сдвиг позиций при перестановке:
    -- см. POSITION_SHIFT и POSITION_LIMIT в policy_repository.rs.
    position      INTEGER NOT NULL CHECK (position >= 0 AND position < 2000000),
    action        TEXT    NOT NULL CHECK (action IN ('allow', 'deny')),
    matcher_kind  TEXT    NOT NULL CHECK (matcher_kind IN ('exact', 'subdomains')),
    matcher_value TEXT    NOT NULL,
    port_kind     TEXT    NOT NULL CHECK (port_kind IN ('any', 'exactly', 'range')),
    port_start    INTEGER,
    port_end      INTEGER,
    -- Проверки `IS NOT NULL` обязательны: в SQL сравнение с NULL даёт NULL, а не
    -- FALSE, и CHECK с NULL-результатом считается выполненным.
    CHECK (
        (port_kind = 'any' AND port_start IS NULL AND port_end IS NULL)
        OR (
            port_kind = 'exactly'
            AND port_start IS NOT NULL
            AND port_start BETWEEN 1 AND 65535
            AND port_end IS NULL
        )
        OR (
            port_kind = 'range'
            AND port_start IS NOT NULL
            AND port_end IS NOT NULL
            AND port_start BETWEEN 1 AND 65535
            AND port_end >= port_start
        )
    ),
    UNIQUE (profile_id, position)
);
";

/// Конфигурация listeners proxy.
const SCHEMA_V2: &str = r"
CREATE TABLE listeners (
    id                 TEXT    PRIMARY KEY NOT NULL,
    port               INTEGER NOT NULL CHECK (port >= 1024 AND port <= 65535),
    profile_id         TEXT    NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    enabled            INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    created_at_unix_ms INTEGER NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL,
    UNIQUE (port)
);
";

/// Последняя известная версия схемы.
///
/// Тесты сравнивают с этим значением, а не с числом: добавление миграции не
/// должно требовать правки каждого теста.
#[must_use]
pub fn latest_version() -> i64 {
    MIGRATIONS.last().map_or(0, |migration| migration.version)
}

/// Настройки приложения: пары «ключ — значение».
///
/// Отдельные колонки под каждую настройку не нужны: набор настроек расширяется
/// без новой миграции, а значение по умолчанию задаётся в коде.
const SCHEMA_V3: &str = r"
CREATE TABLE settings (
    key                TEXT    PRIMARY KEY NOT NULL,
    value              TEXT    NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL
);
";

/// Применяет неприменённые миграции.
///
/// # Errors
///
/// Возвращает [`EgressError::Storage`], если миграцию не удалось применить.
pub(crate) fn apply(connection: &mut Connection) -> Result<(), EgressError> {
    let current: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|source| storage_error("read schema version", source))?;

    for migration in MIGRATIONS
        .iter()
        .filter(|migration| migration.version > current)
    {
        let transaction = connection
            .transaction()
            .map_err(|source| storage_error("begin migration", source))?;

        transaction
            .execute_batch(migration.sql)
            .map_err(|source| storage_error("apply migration", source))?;

        transaction
            .pragma_update(None, "user_version", migration.version)
            .map_err(|source| storage_error("store schema version", source))?;

        transaction
            .commit()
            .map_err(|source| storage_error("commit migration", source))?;

        tracing::info!(
            version = migration.version,
            description = migration.description,
            "database migration applied"
        );
    }

    Ok(())
}
