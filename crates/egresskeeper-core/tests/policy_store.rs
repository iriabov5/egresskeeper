//! Интеграционные тесты хранилища политик на SQLite.
//!
//! Проверяют не только поведение через порт, но и инварианты самой схемы:
//! миграции, режим журнала, внешние ключи и ограничения `CHECK`. Для этого тесты
//! открывают файл базы напрямую.

use std::fs;
use std::path::Path;

use egresskeeper_core::infrastructure::sqlite::{DATABASE_FILE_NAME, latest_schema_version};
use egresskeeper_core::{
    Action, ErrorCode, HostKind, HostMatcher, PolicyRepository, PortSpec, ProfileDraft, ProfileId,
    RuleDraft, RuleId, SqliteRepository,
};
use rusqlite::Connection;

fn draft(name: &str) -> ProfileDraft {
    ProfileDraft {
        name: name.to_owned(),
        default_action: Action::Deny,
    }
}

fn rule_draft(host_kind: HostKind, host: &str, port: PortSpec) -> RuleDraft {
    RuleDraft {
        action: Action::Allow,
        host: HostMatcher::from_parts(host_kind, host).expect("host is valid"),
        port,
    }
}

fn open_raw(state_dir: &Path) -> Connection {
    Connection::open(state_dir.join(DATABASE_FILE_NAME)).expect("raw connection")
}

#[test]
fn fresh_store_creates_a_denying_default_profile() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");

    let profiles = repository.list_profiles().expect("profiles");

    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].default_action, Action::Deny);
    assert_eq!(repository.count_rules(&profiles[0].id).expect("rules"), 0);
    assert_eq!(
        repository.schema_version().expect("schema version"),
        latest_schema_version()
    );
}

#[test]
fn reinitialization_does_not_duplicate_the_default_profile() {
    let directory = tempfile::tempdir().expect("temporary directory");

    {
        let first = SqliteRepository::open_in_state_dir(directory.path()).expect("first open");
        assert_eq!(first.list_profiles().expect("profiles").len(), 1);
    }

    let second = SqliteRepository::open_in_state_dir(directory.path()).expect("second open");

    assert_eq!(second.list_profiles().expect("profiles").len(), 1);
    assert_eq!(
        second.schema_version().expect("schema version"),
        latest_schema_version()
    );
}

#[test]
fn policy_survives_reopening_the_store() {
    let directory = tempfile::tempdir().expect("temporary directory");

    let (profile_id, rule_ids) = {
        let repository =
            SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");
        let profile = repository
            .create_profile(&draft("Работа"))
            .expect("profile");

        let first = repository
            .add_rule(
                &profile.id,
                &rule_draft(HostKind::Exact, "api.example.com", PortSpec::any()),
            )
            .expect("first rule");
        let second = repository
            .add_rule(
                &profile.id,
                &rule_draft(
                    HostKind::Subdomains,
                    "example.com",
                    PortSpec::range(8000, 8100).expect("range"),
                ),
            )
            .expect("second rule");

        (profile.id, vec![first.id, second.id])
    };

    let reopened = SqliteRepository::open_in_state_dir(directory.path()).expect("store reopens");

    let profile = reopened
        .find_profile(&profile_id)
        .expect("read profile")
        .expect("profile exists");
    assert_eq!(profile.name, "Работа");

    let rules = reopened.list_rules(&profile_id).expect("rules");
    assert_eq!(
        rules.iter().map(|rule| rule.id.clone()).collect::<Vec<_>>(),
        rule_ids
    );
    assert_eq!(rules[1].host.kind(), HostKind::Subdomains);
    assert_eq!(rules[1].port, PortSpec::range(8000, 8100).expect("range"));
}

#[test]
fn profile_names_are_unique_ignoring_case() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");
    repository
        .create_profile(&draft("Работа"))
        .expect("profile");

    let error = repository
        .create_profile(&draft("работа"))
        .expect_err("duplicate name");

    assert_eq!(error.code(), ErrorCode::Validation);
    assert_eq!(error.invalid_field(), Some("name"));

    assert!(
        repository
            .find_profile_by_name("РАБОТА")
            .expect("lookup")
            .is_some(),
        "lookup must ignore case"
    );
}

#[test]
fn deleting_a_profile_cascades_to_its_rules() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");

    let first = repository.create_profile(&draft("Работа")).expect("first");
    let second = repository.create_profile(&draft("Личное")).expect("second");

    repository
        .add_rule(
            &first.id,
            &rule_draft(HostKind::Exact, "a.example.com", PortSpec::any()),
        )
        .expect("rule for first");
    repository
        .add_rule(
            &second.id,
            &rule_draft(HostKind::Exact, "b.example.com", PortSpec::any()),
        )
        .expect("rule for second");

    repository.delete_profile(&first.id).expect("deleted");

    let connection = open_raw(directory.path());
    let orphan_rules: i64 = connection
        .query_row("SELECT COUNT(*) FROM rules", [], |row| row.get(0))
        .expect("count rules");

    assert_eq!(
        orphan_rules, 1,
        "cascade must remove rules of the deleted profile"
    );
    assert_eq!(repository.list_rules(&second.id).expect("rules").len(), 1);
}

#[test]
fn reorder_assigns_dense_unique_positions() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");
    let profile = repository
        .create_profile(&draft("Работа"))
        .expect("profile");

    let mut rules = Vec::new();
    for host in ["a.example.com", "b.example.com", "c.example.com"] {
        rules.push(
            repository
                .add_rule(
                    &profile.id,
                    &rule_draft(HostKind::Exact, host, PortSpec::any()),
                )
                .expect("rule"),
        );
    }

    let reversed: Vec<RuleId> = rules.iter().rev().map(|rule| rule.id.clone()).collect();
    let reordered = repository
        .reorder_rules(&profile.id, &reversed)
        .expect("reorder");

    assert_eq!(
        reordered
            .iter()
            .map(|rule| rule.id.clone())
            .collect::<Vec<_>>(),
        reversed
    );
    assert_eq!(
        reordered
            .iter()
            .map(|rule| rule.position)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
}

#[test]
fn failed_reorder_keeps_the_previous_order() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");
    let profile = repository
        .create_profile(&draft("Работа"))
        .expect("profile");

    let first = repository
        .add_rule(
            &profile.id,
            &rule_draft(HostKind::Exact, "a.example.com", PortSpec::any()),
        )
        .expect("first");
    let second = repository
        .add_rule(
            &profile.id,
            &rule_draft(HostKind::Exact, "b.example.com", PortSpec::any()),
        )
        .expect("second");

    let unknown = RuleId::new();
    let error = repository
        .reorder_rules(&profile.id, &[unknown, second.id.clone()])
        .expect_err("unknown rule");

    assert_eq!(error.code(), ErrorCode::Validation);

    let rules = repository.list_rules(&profile.id).expect("rules");
    assert_eq!(
        rules.iter().map(|rule| rule.id.clone()).collect::<Vec<_>>(),
        vec![first.id, second.id],
        "positions must be rolled back together with the failed reorder"
    );
    assert_eq!(
        rules.iter().map(|rule| rule.position).collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[test]
fn adding_a_rule_to_unknown_profile_reports_not_found() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");
    let unknown = ProfileId::new();

    let error = repository
        .add_rule(
            &unknown,
            &rule_draft(HostKind::Exact, "a.example.com", PortSpec::any()),
        )
        .expect_err("unknown profile");

    assert_eq!(error.code(), ErrorCode::NotFound);
}

#[test]
fn updates_and_deletes_of_missing_entities_report_not_found() {
    let repository = SqliteRepository::open_in_memory().expect("store opens");
    let unknown_profile = ProfileId::new();
    let unknown_rule = RuleId::new();

    assert_eq!(
        repository
            .rename_profile(&unknown_profile, "Куда-то")
            .expect_err("rename")
            .code(),
        ErrorCode::NotFound
    );
    assert_eq!(
        repository
            .set_default_action(&unknown_profile, Action::Allow)
            .expect_err("default action")
            .code(),
        ErrorCode::NotFound
    );
    assert_eq!(
        repository
            .delete_profile(&unknown_profile)
            .expect_err("delete")
            .code(),
        ErrorCode::NotFound
    );
    assert_eq!(
        repository
            .update_rule(
                &unknown_rule,
                &rule_draft(HostKind::Exact, "a.example.com", PortSpec::any())
            )
            .expect_err("update rule")
            .code(),
        ErrorCode::NotFound
    );
    assert_eq!(
        repository
            .delete_rule(&unknown_rule)
            .expect_err("delete rule")
            .code(),
        ErrorCode::NotFound
    );
}

#[test]
fn store_enables_wal_and_foreign_keys() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let _repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");

    let connection = open_raw(directory.path());

    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("journal mode");
    assert_eq!(journal_mode, "wal");

    let foreign_keys: i64 = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("foreign keys");
    assert_eq!(foreign_keys, 1);

    let schema_version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("schema version");
    assert_eq!(schema_version, latest_schema_version());
}

#[test]
fn schema_rejects_invalid_rule_rows() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");
    let profile = repository
        .create_profile(&draft("Работа"))
        .expect("profile");

    let connection = open_raw(directory.path());

    let invalid_port = connection.execute(
        "INSERT INTO rules (id, profile_id, position, action, matcher_kind, matcher_value, port_kind, port_start, port_end)
         VALUES (?1, ?2, 0, 'allow', 'exact', 'a.example.com', 'exactly', NULL, NULL)",
        rusqlite::params![RuleId::new().as_str(), profile.id.as_str()],
    );
    assert!(
        invalid_port.is_err(),
        "schema must reject a rule without a port"
    );

    let invalid_matcher = connection.execute(
        "INSERT INTO rules (id, profile_id, position, action, matcher_kind, matcher_value, port_kind, port_start, port_end)
         VALUES (?1, ?2, 1, 'allow', 'regex', 'a.example.com', 'any', NULL, NULL)",
        rusqlite::params![RuleId::new().as_str(), profile.id.as_str()],
    );
    assert!(
        invalid_matcher.is_err(),
        "schema must reject an unknown matcher"
    );

    let invalid_action = connection.execute(
        "INSERT INTO rules (id, profile_id, position, action, matcher_kind, matcher_value, port_kind, port_start, port_end)
         VALUES (?1, ?2, 2, 'maybe', 'exact', 'a.example.com', 'any', NULL, NULL)",
        rusqlite::params![RuleId::new().as_str(), profile.id.as_str()],
    );
    assert!(
        invalid_action.is_err(),
        "schema must reject an unknown action"
    );
}

#[test]
fn corrupted_rows_are_reported_as_storage_errors() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");
    let profile = repository
        .create_profile(&draft("Работа"))
        .expect("profile");

    let connection = open_raw(directory.path());
    connection
        .execute(
            "INSERT INTO rules (id, profile_id, position, action, matcher_kind, matcher_value, port_kind, port_start, port_end)
             VALUES ('not-a-uuid', ?1, 0, 'allow', 'exact', 'a.example.com', 'any', NULL, NULL)",
            rusqlite::params![profile.id.as_str()],
        )
        .expect("corrupted row is inserted for the test");

    let error = repository
        .list_rules(&profile.id)
        .expect_err("corrupted data");

    assert_eq!(error.code(), ErrorCode::StorageUnavailable);
}

#[test]
fn unusable_database_reports_storage_error_without_leaking_path() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join(DATABASE_FILE_NAME);
    fs::write(&database_path, b"this is not a database").expect("garbage file");

    let error = SqliteRepository::open_in_state_dir(directory.path())
        .expect_err("database cannot be opened");

    assert_eq!(error.code(), ErrorCode::StorageUnavailable);

    let leaked = directory.path().to_string_lossy();
    assert!(
        !error.public_message().contains(leaked.as_ref()),
        "public message must not contain the state directory path"
    );
    assert!(
        !error.public_message().contains(DATABASE_FILE_NAME),
        "public message must not contain the database file name"
    );
}

#[test]
fn create_profile_rejects_unknown_default_action_in_storage() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let _repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");

    let connection = open_raw(directory.path());
    let inserted = connection.execute(
        "INSERT INTO profiles (id, name, name_folded, default_action, created_at_unix_ms, updated_at_unix_ms)
         VALUES (?1, 'Сломанный', 'сломанный', 'maybe', 1, 1)",
        rusqlite::params![ProfileId::new().as_str()],
    );

    assert!(inserted.is_err(), "schema must reject an unknown action");
}
