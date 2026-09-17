//! Оценка соединения по политике профиля.
//!
//! Решение принимает первое совпавшее правило в порядке профиля; если ни одно
//! правило не совпало, применяется default action профиля. Решение всегда
//! содержит причину, чтобы пользователь видел, какое правило сработало.

use serde::{Deserialize, Serialize};

use crate::domain::error::EgressError;
use crate::domain::policy::action::Action;
use crate::domain::policy::entities::{Profile, ProfileId, Rule, RuleId};
use crate::domain::policy::host::NormalizedHost;

/// Цель оценки: нормализованный host и порт назначения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationTarget {
    host: NormalizedHost,
    port: u16,
}

impl EvaluationTarget {
    /// Проверяет и нормализует цель оценки.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] с полем `host` или `port`.
    pub fn parse(host: &str, port: u16) -> Result<Self, EgressError> {
        if port == 0 {
            return Err(EgressError::validation(
                "port",
                "must be between 1 and 65535",
            ));
        }

        Ok(Self {
            host: NormalizedHost::parse(host)?,
            port,
        })
    }

    /// Нормализованный host.
    #[must_use]
    pub fn host(&self) -> &NormalizedHost {
        &self.host
    }

    /// Порт назначения.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }
}

/// Причина решения.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionReason {
    /// Решение принято правилом профиля.
    MatchedRule {
        /// Идентификатор сработавшего правила.
        rule_id: RuleId,
        /// Позиция сработавшего правила.
        position: u32,
    },
    /// Ни одно правило не совпало, применён default action профиля.
    DefaultAction {
        /// Идентификатор профиля.
        profile_id: ProfileId,
        /// Действие по умолчанию.
        action: Action,
    },
}

/// Решение по соединению.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    /// Итоговое действие.
    pub action: Action,
    /// Причина решения.
    pub reason: DecisionReason,
}

/// Политика профиля: профиль вместе с его правилами.
///
/// Правила упорядочиваются при создании, поэтому решение не зависит от порядка
/// элементов во входном срезе.
#[derive(Debug)]
pub struct Policy<'a> {
    profile: &'a Profile,
    rules: Vec<Rule>,
}

impl<'a> Policy<'a> {
    /// Собирает политику профиля из правил.
    #[must_use]
    pub fn new(profile: &'a Profile, rules: &[Rule]) -> Self {
        let mut ordered = rules.to_vec();
        ordered.sort_by_key(|rule| rule.position);

        Self {
            profile,
            rules: ordered,
        }
    }

    /// Оценивает соединение и возвращает решение с причиной.
    #[must_use]
    pub fn evaluate(&self, target: &EvaluationTarget) -> Decision {
        for rule in &self.rules {
            if rule.host.matches(target.host()) && rule.port.matches(target.port()) {
                return Decision {
                    action: rule.action,
                    reason: DecisionReason::MatchedRule {
                        rule_id: rule.id.clone(),
                        position: rule.position,
                    },
                };
            }
        }

        Decision {
            action: self.profile.default_action,
            reason: DecisionReason::DefaultAction {
                profile_id: self.profile.id.clone(),
                action: self.profile.default_action,
            },
        }
    }

    /// Правила политики в порядке применения.
    #[must_use]
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::ErrorCode;
    use crate::domain::policy::host::HostMatcher;
    use crate::domain::policy::port::PortSpec;

    fn profile(default_action: Action) -> Profile {
        Profile {
            id: ProfileId::new(),
            name: "Тест".to_owned(),
            default_action,
            created_at_unix_ms: 0,
            updated_at_unix_ms: 0,
        }
    }

    fn rule(profile: &Profile, position: u32, action: Action, host: &str, port: PortSpec) -> Rule {
        Rule {
            id: RuleId::new(),
            profile_id: profile.id.clone(),
            position,
            action,
            host: HostMatcher::exact(host).expect("host"),
            port,
        }
    }

    fn target(host: &str, port: u16) -> EvaluationTarget {
        EvaluationTarget::parse(host, port).expect("target")
    }

    #[test]
    fn target_normalizes_host_and_rejects_zero_port() {
        assert_eq!(
            target("API.Example.com.", 443).host().as_str(),
            "api.example.com"
        );

        let error = EvaluationTarget::parse("api.example.com", 0).expect_err("zero port");
        assert_eq!(error.code(), ErrorCode::Validation);
        assert_eq!(error.invalid_field(), Some("port"));
    }

    #[test]
    fn matching_rule_decides() {
        let profile = profile(Action::Deny);
        let allow = rule(
            &profile,
            0,
            Action::Allow,
            "api.example.com",
            PortSpec::any(),
        );
        let policy = Policy::new(&profile, std::slice::from_ref(&allow));

        let decision = policy.evaluate(&target("api.example.com", 443));

        assert_eq!(decision.action, Action::Allow);
        assert_eq!(
            decision.reason,
            DecisionReason::MatchedRule {
                rule_id: allow.id,
                position: 0
            }
        );
    }

    #[test]
    fn first_matching_rule_wins() {
        let profile = profile(Action::Deny);
        let deny = rule(
            &profile,
            0,
            Action::Deny,
            "api.example.com",
            PortSpec::any(),
        );
        let allow = rule(
            &profile,
            1,
            Action::Allow,
            "api.example.com",
            PortSpec::any(),
        );
        let policy = Policy::new(&profile, &[allow, deny.clone()]);

        let decision = policy.evaluate(&target("api.example.com", 443));

        assert_eq!(decision.action, Action::Deny);
        assert_eq!(
            decision.reason,
            DecisionReason::MatchedRule {
                rule_id: deny.id,
                position: 0
            }
        );
    }

    #[test]
    fn rule_order_is_applied_regardless_of_input_order() {
        let profile = profile(Action::Deny);
        let allow = rule(
            &profile,
            1,
            Action::Allow,
            "api.example.com",
            PortSpec::any(),
        );
        let deny = rule(
            &profile,
            0,
            Action::Deny,
            "api.example.com",
            PortSpec::any(),
        );

        let policy = Policy::new(&profile, &[allow, deny]);

        assert_eq!(policy.rules()[0].position, 0);
        assert_eq!(
            policy.evaluate(&target("api.example.com", 443)).action,
            Action::Deny
        );
    }

    #[test]
    fn changing_order_changes_decision() {
        let profile = profile(Action::Deny);
        let mut allow = rule(
            &profile,
            0,
            Action::Allow,
            "api.example.com",
            PortSpec::any(),
        );
        let mut deny = rule(
            &profile,
            1,
            Action::Deny,
            "api.example.com",
            PortSpec::any(),
        );

        let before = Policy::new(&profile, &[allow.clone(), deny.clone()])
            .evaluate(&target("api.example.com", 443))
            .action;
        assert_eq!(before, Action::Allow);

        allow.position = 1;
        deny.position = 0;

        let after = Policy::new(&profile, &[allow, deny])
            .evaluate(&target("api.example.com", 443))
            .action;
        assert_eq!(after, Action::Deny);
    }

    #[test]
    fn default_action_applies_when_nothing_matches() {
        let profile = profile(Action::Allow);
        let rule = rule(
            &profile,
            0,
            Action::Deny,
            "api.example.com",
            PortSpec::any(),
        );
        let policy = Policy::new(&profile, &[rule]);

        let decision = policy.evaluate(&target("other.example.com", 443));

        assert_eq!(decision.action, Action::Allow);
        assert_eq!(
            decision.reason,
            DecisionReason::DefaultAction {
                profile_id: profile.id.clone(),
                action: Action::Allow
            }
        );
    }

    #[test]
    fn port_restriction_is_respected() {
        let profile = profile(Action::Deny);
        let rule = rule(
            &profile,
            0,
            Action::Allow,
            "api.example.com",
            PortSpec::exactly(443).expect("port"),
        );
        let policy = Policy::new(&profile, &[rule]);

        assert_eq!(
            policy.evaluate(&target("api.example.com", 443)).action,
            Action::Allow
        );
        assert_eq!(
            policy.evaluate(&target("api.example.com", 8443)).action,
            Action::Deny
        );
    }

    #[test]
    fn subdomains_rule_does_not_allow_the_apex_domain() {
        let profile = profile(Action::Deny);
        let rule = Rule {
            id: RuleId::new(),
            profile_id: profile.id.clone(),
            position: 0,
            action: Action::Allow,
            host: HostMatcher::subdomains("example.com").expect("host"),
            port: PortSpec::any(),
        };
        let policy = Policy::new(&profile, &[rule]);

        assert_eq!(
            policy.evaluate(&target("api.example.com", 443)).action,
            Action::Allow
        );
        assert_eq!(
            policy.evaluate(&target("example.com", 443)).action,
            Action::Deny
        );
        assert_eq!(
            policy.evaluate(&target("evil-example.com", 443)).action,
            Action::Deny
        );
    }

    #[test]
    fn decision_serializes_with_reason_kind() {
        let profile = profile(Action::Deny);
        let policy = Policy::new(&profile, &[]);

        let decision = policy.evaluate(&target("api.example.com", 443));

        assert_eq!(
            serde_json::to_value(&decision).expect("serialize"),
            serde_json::json!({
                "action": "deny",
                "reason": {
                    "kind": "default_action",
                    "profile_id": profile.id.as_str(),
                    "action": "deny"
                }
            })
        );
    }
}
