//! Проверка IPC-контракта политик против общих fixtures.
//!
//! Те же файлы читают frontend-тесты, поэтому дрейф между Rust и TypeScript
//! становится падающим тестом, а не ошибкой времени выполнения.

use egresskeeper_core::{
    Action, Decision, DecisionReason, HostKind, HostMatcher, PortSpec, Profile, ProfileDetail,
    ProfileId, Rule, RuleId,
};

const PROFILE_FIXTURE: &str = include_str!("../../../contracts/ipc/policy_profile.sample.json");
const RULE_FIXTURE: &str = include_str!("../../../contracts/ipc/policy_rule.sample.json");
const DETAIL_FIXTURE: &str =
    include_str!("../../../contracts/ipc/policy_profile_detail.sample.json");
const DECISION_FIXTURE: &str = include_str!("../../../contracts/ipc/policy_decision.sample.json");

fn parse<T: serde::de::DeserializeOwned>(fixture: &str) -> T {
    serde_json::from_str(fixture).expect("fixture matches the contract")
}

#[test]
fn profile_fixture_deserializes_and_round_trips() {
    let profile: Profile = parse(PROFILE_FIXTURE);

    assert_eq!(profile.name, "Работа");
    assert_eq!(profile.default_action, Action::Deny);
    assert_eq!(
        serde_json::to_value(&profile).expect("serialize"),
        serde_json::from_str::<serde_json::Value>(PROFILE_FIXTURE).expect("fixture json")
    );
}

#[test]
fn rule_fixture_deserializes_and_round_trips() {
    let rule: Rule = parse(RULE_FIXTURE);

    assert_eq!(rule.position, 0);
    assert_eq!(rule.action, Action::Allow);
    assert_eq!(
        rule.host,
        HostMatcher::subdomains("example.com").expect("host")
    );
    assert_eq!(rule.port, PortSpec::exactly(443).expect("port"));
    assert_eq!(
        serde_json::to_value(&rule).expect("serialize"),
        serde_json::from_str::<serde_json::Value>(RULE_FIXTURE).expect("fixture json")
    );
}

#[test]
fn profile_detail_fixture_keeps_rule_order() {
    let detail: ProfileDetail = parse(DETAIL_FIXTURE);

    assert_eq!(detail.profile.name, "Работа");
    assert_eq!(detail.rules.len(), 2);
    assert_eq!(detail.rules[0].host.kind(), HostKind::Exact);
    assert_eq!(detail.rules[1].host.kind(), HostKind::Subdomains);
    assert_eq!(detail.rules[1].port, PortSpec::any());
    assert_eq!(
        serde_json::to_value(&detail).expect("serialize"),
        serde_json::from_str::<serde_json::Value>(DETAIL_FIXTURE).expect("fixture json")
    );
}

#[test]
fn decision_fixture_deserializes_and_round_trips() {
    let decision: Decision = parse(DECISION_FIXTURE);

    assert_eq!(decision.action, Action::Allow);
    assert!(matches!(
        decision.reason,
        DecisionReason::MatchedRule { position: 0, .. }
    ));
    assert_eq!(
        serde_json::to_value(&decision).expect("serialize"),
        serde_json::from_str::<serde_json::Value>(DECISION_FIXTURE).expect("fixture json")
    );
}

#[test]
fn default_action_decision_is_serialized_with_profile_id() {
    let profile_id = ProfileId::new();
    let decision = Decision {
        action: Action::Deny,
        reason: DecisionReason::DefaultAction {
            profile_id: profile_id.clone(),
            action: Action::Deny,
        },
    };

    let json = serde_json::to_value(&decision).expect("serialize");

    assert_eq!(
        json,
        serde_json::json!({
            "action": "deny",
            "reason": {
                "kind": "default_action",
                "profile_id": profile_id.as_str(),
                "action": "deny"
            }
        })
    );
}

#[test]
fn rule_ids_are_serialized_as_strings() {
    let rule_id = RuleId::new();

    assert_eq!(
        serde_json::to_value(&rule_id).expect("serialize"),
        serde_json::Value::String(rule_id.as_str().to_owned())
    );
}
