//! Проверка IPC-контракта proxy против общих fixtures.
//!
//! Те же файлы читает frontend, поэтому дрейф между Rust и TypeScript становится
//! падающим тестом.

use egresskeeper_app::{ListenerStateView, ProxyStatusView};
use egresskeeper_core::{Action, ProxyDecision, ProxyDecisionReason};

const LISTENER_FIXTURE: &str = include_str!("../../contracts/ipc/proxy_listener.sample.json");
const STATUS_FIXTURE: &str = include_str!("../../contracts/ipc/proxy_status.sample.json");
const DECISION_FIXTURE: &str = include_str!("../../contracts/ipc/proxy_decision.sample.json");

#[test]
fn listener_fixture_deserializes_and_round_trips() {
    let listener: egresskeeper_app::ListenerView =
        serde_json::from_str(LISTENER_FIXTURE).expect("fixture matches the contract");

    assert_eq!(listener.listener.port.get(), 8787);
    assert!(listener.listener.enabled);
    assert_eq!(listener.active_connections, 2);
    assert_eq!(
        serde_json::to_value(&listener).expect("serialize"),
        serde_json::from_str::<serde_json::Value>(LISTENER_FIXTURE).expect("fixture json")
    );
}

#[test]
fn status_fixture_carries_running_and_failed_listeners() {
    let status: ProxyStatusView =
        serde_json::from_str(STATUS_FIXTURE).expect("fixture matches the contract");

    assert_eq!(status.listeners.len(), 2);
    assert_eq!(status.dropped_decisions, 3);
    assert_eq!(status.listeners[0].state, ListenerStateView::Running);

    match &status.listeners[1].state {
        ListenerStateView::Failed { code, message } => {
            assert_eq!(code, "port_unavailable");
            assert!(message.contains("занят"));
        }
        other => panic!("unexpected state: {other:?}"),
    }

    assert_eq!(
        serde_json::to_value(&status).expect("serialize"),
        serde_json::from_str::<serde_json::Value>(STATUS_FIXTURE).expect("fixture json")
    );
}

#[test]
fn decision_fixture_deserializes_and_round_trips() {
    let decision: ProxyDecision =
        serde_json::from_str(DECISION_FIXTURE).expect("fixture matches the contract");

    assert_eq!(decision.host, "api.example.com");
    assert_eq!(decision.port, 443);
    assert_eq!(decision.action, Action::Deny);
    assert!(decision.is_denied());
    assert!(matches!(
        decision.reason,
        ProxyDecisionReason::Policy { .. }
    ));
    assert_eq!(
        serde_json::to_value(&decision).expect("serialize"),
        serde_json::from_str::<serde_json::Value>(DECISION_FIXTURE).expect("fixture json")
    );
}

#[test]
fn technical_reasons_have_no_domain_decision() {
    for reason in [
        ProxyDecisionReason::PolicyUnavailable,
        ProxyDecisionReason::LoopbackTarget,
    ] {
        let json = serde_json::to_value(&reason).expect("serialize");

        assert!(json.get("decision").is_none());
        assert!(json.get("kind").is_some());
    }
}
