//! Интеграционные тесты хранения настроек.
//!
//! Отдельно проверяется миграция: тест создаёт базу предыдущей версии
//! «замороженным» SQL и убеждается, что обновление добавляет настройки, не
//! трогая существующие данные.

use std::path::Path;

use egresskeeper_core::infrastructure::sqlite::{
    DATABASE_FILE_NAME, SqliteRepository, latest_schema_version,
};
use egresskeeper_core::{
    CloseBehavior, ErrorCode, PolicyRepository, ProfileDraft, ProfileId, SettingsRepository,
    SettingsService, SqliteRepository as Repository,
};
use rusqlite::Connection;

/// Схема версии 2 (политики и listeners) в том виде, в котором её создало
/// изменение `add-egress-proxy-runtime`.
const SCHEMA_V2: &str = r"
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
    position      INTEGER NOT NULL CHECK (position >= 0 AND position < 2000000),
    action        TEXT    NOT NULL CHECK (action IN ('allow', 'deny')),
    matcher_kind  TEXT    NOT NULL CHECK (matcher_kind IN ('exact', 'subdomains')),
    matcher_value TEXT    NOT NULL,
    port_kind     TEXT    NOT NULL CHECK (port_kind IN ('any', 'exactly', 'range')),
    port_start    INTEGER,
    port_end      INTEGER,
    CHECK (
        (port_kind = 'any' AND port_start IS NULL AND port_end IS NULL)
        OR (port_kind = 'exactly' AND port_start IS NOT NULL AND port_start BETWEEN 1 AND 65535 AND port_end IS NULL)
        OR (port_kind = 'range' AND port_start IS NOT NULL AND port_end IS NOT NULL AND port_start BETWEEN 1 AND 65535 AND port_end >= port_start)
    ),
    UNIQUE (profile_id, position)
);

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

fn open_raw(state_dir: &Path) -> Connection {
    Connection::open(state_dir.join(DATABASE_FILE_NAME)).expect("raw connection")
}

/// Создаёт базу версии 2 с профилем и listener'ом.
fn create_v2_database(state_dir: &Path, profile_id: &ProfileId) {
    let connection = open_raw(state_dir);
    connection.execute_batch(SCHEMA_V2).expect("v2 schema");
    connection
        .pragma_update(None, "user_version", 2)
        .expect("version");

    connection
        .execute(
            "INSERT INTO profiles (id, name, name_folded, default_action, created_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, 'Работа', 'работа', 'deny', 1, 1)",
            rusqlite::params![profile_id.as_str()],
        )
        .expect("profile");
    connection
        .execute(
            "INSERT INTO listeners (id, port, profile_id, enabled, created_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, 8787, ?2, 1, 1, 1)",
            rusqlite::params![
                egresskeeper_core::ListenerId::new().as_str(),
                profile_id.as_str()
            ],
        )
        .expect("listener");
}

#[test]
fn settings_round_trip_through_storage() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");
    let service = SettingsService::new(repository);

    assert_eq!(
        service.shell_settings().expect("settings").close_behavior,
        CloseBehavior::HideToTray,
        "по умолчанию окно скрывается"
    );

    service
        .set_close_behavior(CloseBehavior::Quit)
        .expect("saved");

    // Переоткрытие хранилища сохраняет значение.
    let reopened = SqliteRepository::open_in_state_dir(directory.path()).expect("reopen");
    let settings = SettingsService::new(reopened)
        .shell_settings()
        .expect("settings");

    assert_eq!(settings.close_behavior, CloseBehavior::Quit);
}

#[test]
fn missing_setting_is_not_an_error() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");

    assert_eq!(repository.get_setting("unknown.key").expect("read"), None);
}

#[test]
fn corrupted_setting_value_falls_back_to_default() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");
    repository
        .set_setting("shell.close_behavior", "explode")
        .expect("written");

    let settings = SettingsService::new(repository)
        .shell_settings()
        .expect("settings");

    assert_eq!(settings.close_behavior, CloseBehavior::default());
}

#[test]
fn migration_from_version_two_adds_settings_and_keeps_data() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let profile_id = ProfileId::new();
    create_v2_database(directory.path(), &profile_id);

    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store upgrades");

    assert_eq!(
        repository.schema_version().expect("version"),
        latest_schema_version()
    );

    // Существующие данные не тронуты.
    let profiles = repository.list_profiles().expect("profiles");
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].id, profile_id);

    let repository_for_listeners =
        SqliteRepository::open_in_state_dir(directory.path()).expect("store reopens");
    assert_eq!(
        egresskeeper_core::ListenerRepository::list_listeners(&repository_for_listeners)
            .expect("listeners")
            .len(),
        1
    );

    // Настройки доступны после миграции.
    let settings = SettingsService::new(repository)
        .shell_settings()
        .expect("settings");
    assert_eq!(settings.close_behavior, CloseBehavior::default());
}

#[test]
fn settings_are_isolated_from_policy_commands() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");
    let settings = SettingsService::new(repository.clone());

    settings
        .set_close_behavior(CloseBehavior::Quit)
        .expect("saved");
    repository
        .create_profile(&ProfileDraft {
            name: "Работа".to_owned(),
            default_action: egresskeeper_core::Action::Deny,
        })
        .expect("profile");

    assert_eq!(
        settings.shell_settings().expect("settings").close_behavior,
        CloseBehavior::Quit
    );
}

#[test]
fn storage_errors_do_not_leak_paths() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join(DATABASE_FILE_NAME);
    std::fs::write(&database, b"this is not a database").expect("garbage file");

    let error = Repository::open_in_state_dir(directory.path()).expect_err("open fails");

    assert_eq!(error.code(), ErrorCode::StorageUnavailable);
    assert!(!error.public_message().contains("egresskeeper.sqlite3"));
}
