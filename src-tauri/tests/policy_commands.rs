//! Интеграционные тесты IPC-слоя политик.
//!
//! Проверяют связку «composition root → хранилище → сервис → контракт ошибок»:
//! команды в `src-tauri` — тонкие адаптеры, поэтому их логика проверяется через
//! тот же `AppState`, с которым работают обработчики. Макрос `#[tauri::command]`
//! добавляет только десериализацию аргументов; это проверяется запуском
//! приложения.

use egresskeeper_app::{AppState, IpcError};
use egresskeeper_core::{Action, HostKind, PortSpec, RuleInput};

fn initialize_state(directory: &tempfile::TempDir) -> AppState {
    AppState::initialize(directory.path(), "0.1.0")
}

fn allow_rule(host: &str) -> RuleInput {
    RuleInput {
        action: Action::Allow,
        host_kind: HostKind::Exact,
        host: host.to_owned(),
        port: PortSpec::any(),
    }
}

#[test]
fn policy_database_is_created_in_the_state_directory() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);

    let service = state.policy().expect("storage is ready");
    let profile = service
        .create_profile("Работа")
        .expect("profile is created");

    assert!(
        directory
            .path()
            .join(egresskeeper_core::infrastructure::sqlite::DATABASE_FILE_NAME)
            .is_file(),
        "database file must live in the state directory"
    );

    // Новый composition root на том же каталоге видит те же данные.
    let reopened = initialize_state(&directory);
    let profiles = reopened
        .policy()
        .expect("storage is ready")
        .list_profiles()
        .expect("profiles");

    assert_eq!(profiles.len(), 2, "default profile plus the created one");
    assert!(
        profiles
            .iter()
            .any(|summary| summary.profile.id == profile.id),
        "created profile must survive reopening"
    );
}

#[test]
fn rule_validation_reports_the_contract_field() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let service = state.policy().expect("storage is ready");
    let profile = service.create_profile("Работа").expect("profile");

    let error = IpcError::from_result(service.add_rule(
        profile.id.as_str(),
        RuleInput {
            action: Action::Allow,
            host_kind: HostKind::Subdomains,
            host: "192.168.0.10".to_owned(),
            port: PortSpec::any(),
        },
    ))
    .expect_err("ip address cannot be a subdomain matcher");

    assert_eq!(error.code, "validation");
    assert_eq!(
        error.details.map(|details| details.field),
        Some("host".to_owned())
    );
    assert_eq!(
        service
            .profile_detail(profile.id.as_str())
            .expect("detail")
            .rules
            .len(),
        0
    );
}

#[test]
fn evaluation_through_the_app_state_reports_the_matching_rule() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let service = state.policy().expect("storage is ready");
    let profile = service.create_profile("Работа").expect("profile");

    let rule = service
        .add_rule(profile.id.as_str(), allow_rule("api.example.com"))
        .expect("rule");

    let decision = service
        .evaluate(profile.id.as_str(), "API.example.com.", 443)
        .expect("decision");

    assert_eq!(decision.action, Action::Allow);
    assert_eq!(
        decision.reason,
        egresskeeper_core::DecisionReason::MatchedRule {
            rule_id: rule.id,
            position: 0
        }
    );
}

#[test]
fn last_profile_protection_is_reported_through_the_contract() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let service = state.policy().expect("storage is ready");
    let profiles = service.list_profiles().expect("profiles");
    let only_profile = &profiles[0].profile;

    let error = IpcError::from_result(service.delete_profile(only_profile.id.as_str()))
        .expect_err("last profile");

    assert_eq!(error.code, "validation");
    assert_eq!(
        error.details.map(|details| details.field),
        Some("profile_id".to_owned())
    );
}

#[test]
fn missing_profile_reports_not_found_without_details() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let service = state.policy().expect("storage is ready");
    let unknown = egresskeeper_core::ProfileId::new();

    let error = IpcError::from_result(service.profile_detail(unknown.as_str()))
        .expect_err("unknown profile");

    assert_eq!(error.code, "not_found");
    assert_eq!(error.details, None);
}

#[test]
fn end_to_end_policy_scenario_through_the_composition_root() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let service = state.policy().expect("storage is ready");

    // Пользователь создаёт профиль и добавляет два правила с противоположными действиями.
    let profile = service
        .create_profile("Работа")
        .expect("profile is created");
    let deny = service
        .add_rule(
            profile.id.as_str(),
            RuleInput {
                action: Action::Deny,
                host_kind: HostKind::Subdomains,
                host: "tracker.example.com".to_owned(),
                port: PortSpec::any(),
            },
        )
        .expect("deny rule");
    let allow = service
        .add_rule(
            profile.id.as_str(),
            RuleInput {
                action: Action::Allow,
                host_kind: HostKind::Subdomains,
                host: "example.com".to_owned(),
                port: PortSpec::any(),
            },
        )
        .expect("allow rule");

    // Первое правило запрещает трекеры, второе разрешает остальные поддомены.
    assert_eq!(
        service
            .evaluate(profile.id.as_str(), "ads.tracker.example.com", 443)
            .expect("decision")
            .action,
        Action::Deny
    );
    assert_eq!(
        service
            .evaluate(profile.id.as_str(), "api.example.com", 443)
            .expect("decision")
            .action,
        Action::Allow
    );

    // Пользователь меняет порядок: разрешение поднимается выше запрета.
    service
        .reorder_rules(
            profile.id.as_str(),
            &[allow.id.as_str().to_owned(), deny.id.as_str().to_owned()],
        )
        .expect("reorder");

    assert_eq!(
        service
            .evaluate(profile.id.as_str(), "ads.tracker.example.com", 443)
            .expect("decision")
            .action,
        Action::Allow,
        "порядок правил определяет решение"
    );

    // Смена default action влияет только на соединения без совпадений.
    service
        .set_default_action(profile.id.as_str(), Action::Allow)
        .expect("default action");
    assert_eq!(
        service
            .evaluate(profile.id.as_str(), "unknown.example.org", 443)
            .expect("decision")
            .action,
        Action::Allow
    );

    // После перезапуска shell политика и порядок сохраняются.
    let restarted = initialize_state(&directory);
    let detail = restarted
        .policy()
        .expect("storage is ready")
        .profile_detail(profile.id.as_str())
        .expect("detail");

    assert_eq!(detail.profile.default_action, Action::Allow);
    assert_eq!(
        detail
            .rules
            .iter()
            .map(|rule| rule.id.clone())
            .collect::<Vec<_>>(),
        vec![allow.id, deny.id]
    );
}
