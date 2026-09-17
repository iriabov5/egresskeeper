//! Тесты use-cases управления политикой на реализации порта в памяти.

mod support;

use egresskeeper_core::{
    Action, DecisionReason, ErrorCode, HostKind, PolicyService, PortSpec, RuleInput,
};
use support::InMemoryPolicyRepository;

fn service() -> PolicyService<InMemoryPolicyRepository> {
    PolicyService::new(InMemoryPolicyRepository::new())
}

fn allow_rule(host: &str, port: PortSpec) -> RuleInput {
    RuleInput {
        action: Action::Allow,
        host_kind: HostKind::Exact,
        host: host.to_owned(),
        port,
    }
}

#[test]
fn profile_is_created_with_denying_default_action() {
    let service = service();

    let profile = service
        .create_profile("Работа")
        .expect("profile is created");

    assert_eq!(profile.name, "Работа");
    assert_eq!(profile.default_action, Action::Deny);

    let profiles = service.list_profiles().expect("profiles");
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].rule_count, 0);
}

#[test]
fn profile_name_must_be_unique_ignoring_case() {
    let service = service();
    service.create_profile("Работа").expect("profile");

    let error = service
        .create_profile("работа")
        .expect_err("duplicate name");

    assert_eq!(error.code(), ErrorCode::Validation);
    assert_eq!(error.invalid_field(), Some("name"));
}

#[test]
fn profile_name_must_not_be_empty() {
    let service = service();

    let error = service.create_profile("   ").expect_err("empty name");

    assert_eq!(error.invalid_field(), Some("name"));
}

#[test]
fn profile_can_be_renamed() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");

    let renamed = service
        .rename_profile(profile.id.as_str(), "Личное")
        .expect("renamed");

    assert_eq!(renamed.name, "Личное");
    assert_eq!(renamed.id, profile.id);
}

#[test]
fn rename_rejects_name_of_another_profile() {
    let service = service();
    service.create_profile("Работа").expect("first");
    let second = service.create_profile("Личное").expect("second");

    let error = service
        .rename_profile(second.id.as_str(), "работа")
        .expect_err("duplicate name");

    assert_eq!(error.invalid_field(), Some("name"));
}

#[test]
fn rename_of_unknown_profile_reports_not_found() {
    let service = service();
    let unknown = egresskeeper_core::ProfileId::new();

    let error = service
        .rename_profile(unknown.as_str(), "Куда-то")
        .expect_err("unknown profile");

    assert_eq!(error.code(), ErrorCode::NotFound);
}

#[test]
fn default_action_can_be_changed() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");

    let updated = service
        .set_default_action(profile.id.as_str(), Action::Allow)
        .expect("updated");

    assert_eq!(updated.default_action, Action::Allow);
}

#[test]
fn invalid_profile_id_is_rejected_with_validation_error() {
    let service = service();

    let error = service
        .set_default_action("not-a-uuid", Action::Allow)
        .expect_err("invalid id");

    assert_eq!(error.code(), ErrorCode::Validation);
    assert_eq!(error.invalid_field(), Some("profile_id"));
}

#[test]
fn the_last_profile_cannot_be_deleted() {
    let service = service();
    let first = service.create_profile("Работа").expect("first");

    let error = service
        .delete_profile(first.id.as_str())
        .expect_err("last profile");

    assert_eq!(error.code(), ErrorCode::Validation);
    assert_eq!(error.invalid_field(), Some("profile_id"));
    assert_eq!(service.list_profiles().expect("profiles").len(), 1);
}

#[test]
fn deleting_a_profile_removes_its_rules() {
    let service = service();
    let first = service.create_profile("Работа").expect("first");
    let second = service.create_profile("Личное").expect("second");

    service
        .add_rule(
            first.id.as_str(),
            allow_rule("api.example.com", PortSpec::any()),
        )
        .expect("rule for first");
    service
        .add_rule(
            second.id.as_str(),
            allow_rule("api.example.com", PortSpec::any()),
        )
        .expect("rule for second");

    service.delete_profile(first.id.as_str()).expect("deleted");

    let profiles = service.list_profiles().expect("profiles");
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].profile.id, second.id);
    assert_eq!(profiles[0].rule_count, 1);
}

#[test]
fn rules_are_appended_in_order() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");

    let first = service
        .add_rule(
            profile.id.as_str(),
            allow_rule("a.example.com", PortSpec::any()),
        )
        .expect("first rule");
    let second = service
        .add_rule(
            profile.id.as_str(),
            allow_rule("b.example.com", PortSpec::any()),
        )
        .expect("second rule");

    assert_eq!(first.position, 0);
    assert_eq!(second.position, 1);

    let detail = service.profile_detail(profile.id.as_str()).expect("detail");
    assert_eq!(detail.rules.len(), 2);
    assert_eq!(detail.rules[0].position, 0);
}

#[test]
fn rule_input_is_validated() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");

    let bad_host = service
        .add_rule(
            profile.id.as_str(),
            allow_rule("evil..example.com", PortSpec::any()),
        )
        .expect_err("invalid host");
    assert_eq!(bad_host.invalid_field(), Some("host"));

    let bad_port = service
        .add_rule(
            profile.id.as_str(),
            allow_rule("api.example.com", PortSpec::Exactly { port: 0 }),
        )
        .expect_err("invalid port");
    assert_eq!(bad_port.invalid_field(), Some("port"));

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
fn adding_a_rule_to_unknown_profile_reports_not_found() {
    let service = service();
    let unknown = egresskeeper_core::ProfileId::new();

    let error = service
        .add_rule(
            unknown.as_str(),
            allow_rule("api.example.com", PortSpec::any()),
        )
        .expect_err("unknown profile");

    assert_eq!(error.code(), ErrorCode::NotFound);
}

#[test]
fn rule_can_be_updated_and_keeps_its_position() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");
    let rule = service
        .add_rule(
            profile.id.as_str(),
            allow_rule("api.example.com", PortSpec::any()),
        )
        .expect("rule");

    let updated = service
        .update_rule(
            rule.id.as_str(),
            RuleInput {
                action: Action::Deny,
                host_kind: HostKind::Subdomains,
                host: "example.com".to_owned(),
                port: PortSpec::exactly(443).expect("port"),
            },
        )
        .expect("updated");

    assert_eq!(updated.position, rule.position);
    assert_eq!(updated.action, Action::Deny);
    assert_eq!(updated.host.value(), "example.com");
}

#[test]
fn updating_unknown_rule_reports_not_found() {
    let service = service();
    let unknown = egresskeeper_core::RuleId::new();

    let error = service
        .update_rule(
            unknown.as_str(),
            allow_rule("api.example.com", PortSpec::any()),
        )
        .expect_err("unknown rule");

    assert_eq!(error.code(), ErrorCode::NotFound);
}

#[test]
fn rule_can_be_deleted() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");
    let rule = service
        .add_rule(
            profile.id.as_str(),
            allow_rule("api.example.com", PortSpec::any()),
        )
        .expect("rule");

    service.delete_rule(rule.id.as_str()).expect("deleted");

    assert!(
        service
            .profile_detail(profile.id.as_str())
            .expect("detail")
            .rules
            .is_empty()
    );

    let error = service
        .delete_rule(rule.id.as_str())
        .expect_err("already deleted");
    assert_eq!(error.code(), ErrorCode::NotFound);
}

#[test]
fn rules_can_be_reordered() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");
    let first = service
        .add_rule(
            profile.id.as_str(),
            allow_rule("a.example.com", PortSpec::any()),
        )
        .expect("first");
    let second = service
        .add_rule(
            profile.id.as_str(),
            allow_rule("b.example.com", PortSpec::any()),
        )
        .expect("second");

    let reordered = service
        .reorder_rules(
            profile.id.as_str(),
            &[second.id.as_str().to_owned(), first.id.as_str().to_owned()],
        )
        .expect("reordered");

    assert_eq!(reordered[0].id, second.id);
    assert_eq!(reordered[0].position, 0);
    assert_eq!(reordered[1].id, first.id);
    assert_eq!(reordered[1].position, 1);
}

#[test]
fn reorder_rejects_incomplete_or_duplicated_lists() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");
    let first = service
        .add_rule(
            profile.id.as_str(),
            allow_rule("a.example.com", PortSpec::any()),
        )
        .expect("first");
    let second = service
        .add_rule(
            profile.id.as_str(),
            allow_rule("b.example.com", PortSpec::any()),
        )
        .expect("second");

    let duplicated = service
        .reorder_rules(
            profile.id.as_str(),
            &[first.id.as_str().to_owned(), first.id.as_str().to_owned()],
        )
        .expect_err("duplicates");
    assert_eq!(duplicated.invalid_field(), Some("rule_ids"));

    let incomplete = service
        .reorder_rules(profile.id.as_str(), &[first.id.as_str().to_owned()])
        .expect_err("incomplete");
    assert_eq!(incomplete.invalid_field(), Some("rule_ids"));

    let unknown = service
        .reorder_rules(
            profile.id.as_str(),
            &[
                first.id.as_str().to_owned(),
                egresskeeper_core::RuleId::new().as_str().to_owned(),
            ],
        )
        .expect_err("unknown rule");
    assert_eq!(unknown.invalid_field(), Some("rule_ids"));

    let detail = service.profile_detail(profile.id.as_str()).expect("detail");
    assert_eq!(detail.rules[0].id, first.id);
    assert_eq!(detail.rules[1].id, second.id);
}

#[test]
fn evaluation_reports_the_matching_rule() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");
    let rule = service
        .add_rule(
            profile.id.as_str(),
            allow_rule("api.example.com", PortSpec::exactly(443).expect("port")),
        )
        .expect("rule");

    let decision = service
        .evaluate(profile.id.as_str(), "API.example.com.", 443)
        .expect("decision");

    assert_eq!(decision.action, Action::Allow);
    assert_eq!(
        decision.reason,
        DecisionReason::MatchedRule {
            rule_id: rule.id,
            position: 0
        }
    );
}

#[test]
fn evaluation_falls_back_to_default_action() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");
    service
        .add_rule(
            profile.id.as_str(),
            allow_rule("api.example.com", PortSpec::any()),
        )
        .expect("rule");

    let decision = service
        .evaluate(profile.id.as_str(), "other.example.com", 443)
        .expect("decision");

    assert_eq!(decision.action, Action::Deny);
    assert_eq!(
        decision.reason,
        DecisionReason::DefaultAction {
            profile_id: profile.id.clone(),
            action: Action::Deny
        }
    );
}

#[test]
fn evaluation_validates_target() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");

    let bad_host = service
        .evaluate(profile.id.as_str(), "bad host", 443)
        .expect_err("invalid host");
    assert_eq!(bad_host.invalid_field(), Some("host"));

    let bad_port = service
        .evaluate(profile.id.as_str(), "api.example.com", 0)
        .expect_err("invalid port");
    assert_eq!(bad_port.invalid_field(), Some("port"));
}

#[test]
fn evaluation_of_unknown_profile_reports_not_found() {
    let service = service();
    let unknown = egresskeeper_core::ProfileId::new();

    let error = service
        .evaluate(unknown.as_str(), "api.example.com", 443)
        .expect_err("unknown profile");

    assert_eq!(error.code(), ErrorCode::NotFound);
}

#[test]
fn evaluation_does_not_modify_the_policy() {
    let service = service();
    let profile = service.create_profile("Работа").expect("profile");
    service
        .add_rule(
            profile.id.as_str(),
            allow_rule("api.example.com", PortSpec::any()),
        )
        .expect("rule");

    let before = service.profile_detail(profile.id.as_str()).expect("before");
    service
        .evaluate(profile.id.as_str(), "api.example.com", 443)
        .expect("decision");
    service
        .evaluate(profile.id.as_str(), "blocked.example.com", 443)
        .expect("decision");
    let after = service.profile_detail(profile.id.as_str()).expect("after");

    assert_eq!(before, after);
}
