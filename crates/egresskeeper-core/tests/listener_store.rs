//! Интеграционные тесты хранения listeners на SQLite.
//!
//! Отдельно проверяется миграция схемы: тест создаёт базу версии 1 «замороженным»
//! SQL предыдущего изменения и убеждается, что обновление добавляет listeners, не
//! трогая профили и правила.

use std::path::Path;

use egresskeeper_core::infrastructure::sqlite::{DATABASE_FILE_NAME, latest_schema_version};
use egresskeeper_core::{
    Action, ErrorCode, ListenerDraft, ListenerId, ListenerRepository, PolicyRepository, PortSpec,
    ProfileDraft, ProfileId, ProxyPort, RuleDraft, SqliteRepository,
};
use rusqlite::Connection;

/// Схема версии 1 в том виде, в котором её создавало изменение `add-policy-engine`.
///
/// Копия намеренно заморожена: тест должен ловить ситуацию, когда правка текущей
/// версии схемы ломает обновление с уже существующей базы.
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
    position      INTEGER NOT NULL CHECK (position >= 0 AND position < 2000000),
    action        TEXT    NOT NULL CHECK (action IN ('allow', 'deny')),
    matcher_kind  TEXT    NOT NULL CHECK (matcher_kind IN ('exact', 'subdomains')),
    matcher_value TEXT    NOT NULL,
    port_kind     TEXT    NOT NULL CHECK (port_kind IN ('any', 'exactly', 'range')),
    port_start    INTEGER,
    port_end      INTEGER,
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

fn open_raw(state_dir: &Path) -> Connection {
    Connection::open(state_dir.join(DATABASE_FILE_NAME)).expect("raw connection")
}

/// Создаёт базу версии 1 с профилем и правилом.
fn create_v1_database(state_dir: &Path, profile_id: &ProfileId) -> ProfileId {
    let connection = open_raw(state_dir);
    connection
        .execute_batch(SCHEMA_V1)
        .expect("v1 schema is applied");
    connection
        .pragma_update(None, "user_version", 1)
        .expect("version is stored");
    connection
        .execute(
            "INSERT INTO profiles (id, name, name_folded, default_action, created_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, 'Работа', 'работа', 'deny', 1, 1)",
            rusqlite::params![profile_id.as_str()],
        )
        .expect("profile is inserted");
    connection
        .execute(
            "INSERT INTO rules (id, profile_id, position, action, matcher_kind, matcher_value, port_kind, port_start, port_end)
             VALUES (?1, ?2, 0, 'allow', 'exact', 'api.example.com', 'any', NULL, NULL)",
            rusqlite::params![
                egresskeeper_core::RuleId::new().as_str(),
                profile_id.as_str()
            ],
        )
        .expect("rule is inserted");

    profile_id.clone()
}

fn draft(profile: &ProfileId, port: u16) -> ListenerDraft {
    ListenerDraft {
        port: ProxyPort::parse(port).expect("port is valid"),
        profile_id: profile.clone(),
    }
}

#[test]
fn fresh_store_has_no_listeners_and_schema_version_two() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");

    assert!(repository.list_listeners().expect("list").is_empty());
    assert_eq!(
        repository.schema_version().expect("version"),
        latest_schema_version()
    );
}

#[test]
fn listener_round_trips_through_storage() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");
    let profile = repository
        .create_profile(&ProfileDraft {
            name: "Работа".to_owned(),
            default_action: Action::Deny,
        })
        .expect("profile");

    let created = repository
        .create_listener(&draft(&profile.id, 8787))
        .expect("listener");
    assert!(!created.enabled, "создание не включает listener");

    let enabled = repository
        .set_listener_enabled(&created.id, true)
        .expect("enabled");
    assert!(enabled.enabled);

    let updated = repository
        .update_listener(&created.id, &draft(&profile.id, 9999))
        .expect("updated");
    assert_eq!(updated.port.get(), 9999);

    // Переоткрытие хранилища сохраняет конфигурацию.
    let reopened = SqliteRepository::open_in_state_dir(directory.path()).expect("reopen");
    let listeners = reopened.list_listeners().expect("list");
    assert_eq!(listeners.len(), 1);
    assert_eq!(listeners[0].port.get(), 9999);
    assert!(listeners[0].enabled);

    reopened.delete_listener(&created.id).expect("deleted");
    assert!(reopened.list_listeners().expect("list").is_empty());
}

#[test]
fn duplicate_port_is_rejected_by_storage() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");
    let profile = repository
        .create_profile(&ProfileDraft {
            name: "Работа".to_owned(),
            default_action: Action::Deny,
        })
        .expect("profile");
    repository
        .create_listener(&draft(&profile.id, 8787))
        .expect("first");

    let error = repository
        .create_listener(&draft(&profile.id, 8787))
        .expect_err("duplicate port");

    assert_eq!(error.code(), ErrorCode::Validation);
    assert_eq!(error.invalid_field(), Some("port"));
}

#[test]
fn deleting_profile_removes_its_listeners() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");
    let first = repository
        .create_profile(&ProfileDraft {
            name: "Работа".to_owned(),
            default_action: Action::Deny,
        })
        .expect("first profile");
    let second = repository
        .create_profile(&ProfileDraft {
            name: "Личное".to_owned(),
            default_action: Action::Deny,
        })
        .expect("second profile");
    repository
        .create_listener(&draft(&first.id, 8787))
        .expect("listener of first");
    repository
        .create_listener(&draft(&second.id, 9999))
        .expect("listener of second");

    repository.delete_profile(&first.id).expect("deleted");

    let listeners = repository.list_listeners().expect("list");
    assert_eq!(listeners.len(), 1);
    assert_eq!(listeners[0].profile_id, second.id);
}

#[test]
fn missing_entities_report_not_found() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");
    let unknown = ListenerId::new();

    assert_eq!(
        repository
            .update_listener(&unknown, &draft(&ProfileId::new(), 8787))
            .expect_err("update")
            .code(),
        ErrorCode::NotFound
    );
    assert_eq!(
        repository
            .set_listener_enabled(&unknown, true)
            .expect_err("enable")
            .code(),
        ErrorCode::NotFound
    );
    assert_eq!(
        repository
            .delete_listener(&unknown)
            .expect_err("delete")
            .code(),
        ErrorCode::NotFound
    );
}

#[test]
fn profile_existence_is_reported() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");
    let profile = repository
        .create_profile(&ProfileDraft {
            name: "Работа".to_owned(),
            default_action: Action::Deny,
        })
        .expect("profile");

    assert!(repository.profile_exists(&profile.id).expect("exists"));
    assert!(
        !repository
            .profile_exists(&ProfileId::new())
            .expect("missing")
    );
}

#[test]
fn migration_from_version_one_preserves_policy_and_adds_listeners() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let profile_id = ProfileId::new();
    create_v1_database(directory.path(), &profile_id);

    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store upgrades");

    assert_eq!(
        repository.schema_version().expect("version"),
        latest_schema_version(),
        "миграция должна перевести базу на последнюю версию схемы"
    );

    let profiles = repository.list_profiles().expect("profiles");
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].id, profile_id);
    assert_eq!(profiles[0].name, "Работа");

    let rules = repository.list_rules(&profile_id).expect("rules");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].host.value(), "api.example.com");

    // Новая таблица доступна и связана с существующим профилем.
    let listener = repository
        .create_listener(&draft(&profile_id, 8787))
        .expect("listener after migration");
    assert_eq!(listener.profile_id, profile_id);
}

#[test]
fn listeners_created_after_migration_do_not_duplicate_default_profile() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let profile_id = ProfileId::new();
    create_v1_database(directory.path(), &profile_id);

    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store upgrades");

    assert_eq!(
        repository.list_profiles().expect("profiles").len(),
        1,
        "миграция не должна добавлять профиль по умолчанию в непустую базу"
    );
}

#[test]
fn schema_rejects_out_of_range_ports() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");
    let profile = repository
        .create_profile(&ProfileDraft {
            name: "Работа".to_owned(),
            default_action: Action::Deny,
        })
        .expect("profile");

    let connection = open_raw(directory.path());
    let inserted = connection.execute(
        "INSERT INTO listeners (id, port, profile_id, enabled, created_at_unix_ms, updated_at_unix_ms)
         VALUES (?1, 80, ?2, 0, 1, 1)",
        rusqlite::params![ListenerId::new().as_str(), profile.id.as_str()],
    );

    assert!(
        inserted.is_err(),
        "схема должна отклонять привилегированный порт"
    );
}

#[test]
fn corrupted_listener_rows_are_reported_as_storage_errors() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");
    let profile = repository
        .create_profile(&ProfileDraft {
            name: "Работа".to_owned(),
            default_action: Action::Deny,
        })
        .expect("profile");

    let connection = open_raw(directory.path());
    connection
        .execute(
            "INSERT INTO listeners (id, port, profile_id, enabled, created_at_unix_ms, updated_at_unix_ms)
             VALUES ('not-a-uuid', 8787, ?1, 0, 1, 1)",
            rusqlite::params![profile.id.as_str()],
        )
        .expect("corrupted row is inserted for the test");

    let error = repository.list_listeners().expect_err("corrupted data");

    assert_eq!(error.code(), ErrorCode::StorageUnavailable);
}

#[test]
fn rule_validation_still_works_after_schema_upgrade() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");
    let profile = repository
        .create_profile(&ProfileDraft {
            name: "Работа".to_owned(),
            default_action: Action::Deny,
        })
        .expect("profile");

    let rule = repository
        .add_rule(
            &profile.id,
            &RuleDraft {
                action: Action::Allow,
                host: egresskeeper_core::HostMatcher::subdomains("example.com").expect("host"),
                port: PortSpec::any(),
            },
        )
        .expect("rule");

    assert_eq!(rule.position, 0);
}
