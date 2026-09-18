//! Решения proxy.
//!
//! Доменный [`Decision`] описывает решение по правилу политики. У proxy есть
//! отказы, которые правилом не описываются — политика не прочитана или целью
//! соединения является сам proxy, — поэтому здесь определён собственный тип
//! решения с более широким набором причин.

use serde::{Deserialize, Serialize};

use crate::domain::policy::decision::{DecisionReason, EvaluationTarget, Policy};
use crate::domain::policy::entities::{Profile, Rule};
use crate::domain::policy::{Action, NormalizedHost};
use crate::domain::proxy::listener::ListenerId;
use crate::domain::time::now_unix_ms;

/// Причина решения proxy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProxyDecisionReason {
    /// Решение принято политикой профиля.
    Policy {
        /// Решение домена с указанием сработавшего правила или default action.
        decision: DecisionReason,
    },
    /// Политику профиля не удалось прочитать: соединение запрещено (fail-closed).
    PolicyUnavailable,
    /// Целью соединения является сам listener: соединение запрещено.
    LoopbackTarget,
}

/// Снимок политики профиля, с которым работает proxy.
///
/// Снимок берётся в момент установления соединения и применяется к нему целиком:
/// изменения политики действуют на новые соединения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicySnapshot {
    /// Политика прочитана и готова к применению.
    Loaded {
        /// Профиль политики.
        profile: Profile,
        /// Правила профиля.
        rules: Vec<Rule>,
    },
    /// Политику прочитать не удалось или профиль отсутствует.
    Unavailable,
}

/// Решение proxy по конкретному соединению.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyDecision {
    /// Listener, принявший соединение.
    pub listener_id: ListenerId,
    /// Нормализованный host цели.
    pub host: String,
    /// Порт цели.
    pub port: u16,
    /// Итоговое действие.
    pub action: Action,
    /// Причина решения.
    pub reason: ProxyDecisionReason,
    /// Момент решения в миллисекундах Unix epoch.
    pub at_unix_ms: i64,
}

impl ProxyDecision {
    /// Возвращает `true`, если соединение запрещено.
    #[must_use]
    pub const fn is_denied(&self) -> bool {
        !self.action.allows()
    }
}

/// Принимает решение по соединению.
///
/// Проверка петли выполняется до обращения к политике: соединение, целью которого
/// является сам listener, отклоняется независимо от правил.
#[must_use]
pub fn decide(
    listener_id: &ListenerId,
    listener_port: u16,
    snapshot: &PolicySnapshot,
    target: &EvaluationTarget,
) -> ProxyDecision {
    let (action, reason) = if is_loopback_target(target.host(), target.port(), listener_port) {
        (Action::Deny, ProxyDecisionReason::LoopbackTarget)
    } else {
        match snapshot {
            PolicySnapshot::Loaded { profile, rules } => {
                let decision = Policy::new(profile, rules).evaluate(target);

                (
                    decision.action,
                    ProxyDecisionReason::Policy {
                        decision: decision.reason,
                    },
                )
            }
            PolicySnapshot::Unavailable => (Action::Deny, ProxyDecisionReason::PolicyUnavailable),
        }
    };

    ProxyDecision {
        listener_id: listener_id.clone(),
        host: target.host().as_str().to_owned(),
        port: target.port(),
        action,
        reason,
        at_unix_ms: now_unix_ms().unwrap_or(i64::MAX),
    }
}

/// Возвращает `true`, если целью соединения является сам listener.
///
/// Петля возможна только при совпадении порта цели с портом listener'а: обращение
/// к другому локальному сервису — обычный сценарий разработки и запрещать его
/// нельзя. Проверка host текстовая: распознаются loopback-литералы (`localhost`,
/// `127.0.0.1`, `[::1]`, `0.0.0.0`); имя, которое разрешается в loopback через
/// DNS, здесь не распознаётся — это ограничение зафиксировано в дизайне change'а.
#[must_use]
pub fn is_loopback_target(host: &NormalizedHost, target_port: u16, listener_port: u16) -> bool {
    target_port == listener_port && is_loopback_host(host)
}

/// Возвращает `true`, если host указывает на loopback-адрес.
fn is_loopback_host(host: &NormalizedHost) -> bool {
    matches!(host.as_str(), "localhost" | "[::1]" | "0.0.0.0" | "[::]")
        || host.as_str().starts_with("127.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policy::entities::{ProfileId, RuleId};
    use crate::domain::policy::host::HostMatcher;
    use crate::domain::policy::port::PortSpec;

    fn listener_id() -> ListenerId {
        ListenerId::new()
    }

    fn profile(default_action: Action) -> Profile {
        Profile {
            id: ProfileId::new(),
            name: "Тест".to_owned(),
            default_action,
            created_at_unix_ms: 0,
            updated_at_unix_ms: 0,
        }
    }

    fn rule(profile: &Profile, action: Action, host: &str, port: PortSpec) -> Rule {
        Rule {
            id: RuleId::new(),
            profile_id: profile.id.clone(),
            position: 0,
            action,
            host: HostMatcher::exact(host).expect("host"),
            port,
        }
    }

    fn target(host: &str, port: u16) -> EvaluationTarget {
        EvaluationTarget::parse(host, port).expect("target")
    }

    #[test]
    fn allowed_connection_carries_domain_reason() {
        let profile = profile(Action::Deny);
        let rule = rule(&profile, Action::Allow, "api.example.com", PortSpec::any());
        let snapshot = PolicySnapshot::Loaded {
            profile: profile.clone(),
            rules: vec![rule.clone()],
        };

        let decision = decide(
            &listener_id(),
            8787,
            &snapshot,
            &target("api.example.com", 443),
        );

        assert_eq!(decision.action, Action::Allow);
        assert!(!decision.is_denied());
        assert_eq!(decision.host, "api.example.com");
        assert_eq!(decision.port, 443);
        assert_eq!(
            decision.reason,
            ProxyDecisionReason::Policy {
                decision: DecisionReason::MatchedRule {
                    rule_id: rule.id,
                    position: 0
                }
            }
        );
    }

    #[test]
    fn default_action_denies_unmatched_connection() {
        let snapshot = PolicySnapshot::Loaded {
            profile: profile(Action::Deny),
            rules: Vec::new(),
        };

        let decision = decide(
            &listener_id(),
            8787,
            &snapshot,
            &target("unknown.example.org", 443),
        );

        assert!(decision.is_denied());
        assert!(matches!(
            decision.reason,
            ProxyDecisionReason::Policy {
                decision: DecisionReason::DefaultAction { .. }
            }
        ));
    }

    #[test]
    fn unavailable_policy_denies_connection() {
        let decision = decide(
            &listener_id(),
            8787,
            &PolicySnapshot::Unavailable,
            &target("api.example.com", 443),
        );

        assert!(decision.is_denied());
        assert_eq!(decision.reason, ProxyDecisionReason::PolicyUnavailable);
    }

    #[test]
    fn loopback_target_is_denied_even_when_policy_allows_everything() {
        let profile = profile(Action::Allow);
        let snapshot = PolicySnapshot::Loaded {
            profile,
            rules: Vec::new(),
        };

        for host in ["localhost", "127.0.0.1", "127.1.2.3", "[::1]"] {
            let decision = decide(&listener_id(), 8787, &snapshot, &target(host, 8787));

            assert!(decision.is_denied(), "host {host} must be denied");
            assert_eq!(
                decision.reason,
                ProxyDecisionReason::LoopbackTarget,
                "host {host}"
            );
        }
    }

    #[test]
    fn loopback_target_on_another_port_is_not_a_loop() {
        let profile = profile(Action::Allow);
        let snapshot = PolicySnapshot::Loaded {
            profile,
            rules: Vec::new(),
        };

        let decision = decide(&listener_id(), 8787, &snapshot, &target("127.0.0.1", 8080));

        assert_eq!(decision.action, Action::Allow);
        assert!(matches!(
            decision.reason,
            ProxyDecisionReason::Policy { .. }
        ));
    }

    #[test]
    fn host_is_normalized_in_the_decision() {
        let profile = profile(Action::Allow);
        let snapshot = PolicySnapshot::Loaded {
            profile,
            rules: Vec::new(),
        };

        let decision = decide(
            &listener_id(),
            8787,
            &snapshot,
            &target("API.Example.com.", 443),
        );

        assert_eq!(decision.host, "api.example.com");
    }

    #[test]
    fn decision_serializes_with_reason_kind() {
        let decision = decide(
            &listener_id(),
            8787,
            &PolicySnapshot::Unavailable,
            &target("api.example.com", 443),
        );

        let json = serde_json::to_value(&decision).expect("serialize");

        assert_eq!(json["action"], "deny");
        assert_eq!(json["reason"]["kind"], "policy_unavailable");
        assert_eq!(json["host"], "api.example.com");
        assert_eq!(json["port"], 443);
    }
}
