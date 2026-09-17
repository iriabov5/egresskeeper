//! Представления состояния proxy для UI.
//!
//! Конфигурация listener'ов живёт в хранилище, фактическое состояние — в
//! рантайме proxy. UI должен видеть одно согласованное представление, поэтому
//! здесь они объединяются: состояние берётся из рантайма, а признак включения — из
//! конфигурации.

use egresskeeper_core::{Listener, ListenerId, ListenerState, ProxyRuntime};
use serde::{Deserialize, Serialize};

/// Состояние listener'а для UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ListenerStateView {
    /// Listener выключен пользователем или ещё не запускался.
    Stopped,
    /// Listener запускается.
    Starting,
    /// Listener принимает соединения.
    Running,
    /// Listener не смог запуститься.
    Failed {
        /// Machine-readable код ошибки.
        code: String,
        /// Сообщение, безопасное для показа пользователю.
        message: String,
    },
}

impl From<&ListenerState> for ListenerStateView {
    fn from(state: &ListenerState) -> Self {
        match state {
            ListenerState::Starting => Self::Starting,
            ListenerState::Running => Self::Running,
            ListenerState::Stopped => Self::Stopped,
            ListenerState::Failed(failure) => Self::Failed {
                code: failure.code.as_str().to_owned(),
                message: failure.message.clone(),
            },
        }
    }
}

/// Listener вместе с фактическим состоянием.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerView {
    /// Конфигурация listener'а.
    pub listener: Listener,
    /// Фактическое состояние.
    pub state: ListenerStateView,
    /// Количество активных соединений.
    pub active_connections: u32,
}

/// Состояние proxy целиком.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyStatusView {
    /// Listeners вместе с состоянием.
    pub listeners: Vec<ListenerView>,
    /// Количество решений, не попавших в поток для UI.
    pub dropped_decisions: u64,
}

/// Состояние рантайма без обращения к хранилищу.
///
/// Используется в событии изменения состояния: UI объединяет его с уже
/// загруженной конфигурацией по идентификатору listener'а.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyRuntimeView {
    /// Состояние listeners, известных рантайму.
    pub listeners: Vec<ListenerHealthView>,
    /// Количество решений, не попавших в поток для UI.
    pub dropped_decisions: u64,
}

/// Состояние одного listener'а в событии.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerHealthView {
    /// Идентификатор listener'а.
    pub listener_id: ListenerId,
    /// Фактическое состояние.
    pub state: ListenerStateView,
    /// Количество активных соединений.
    pub active_connections: u32,
}

/// Собирает представление состояния из конфигурации и рантайма.
#[must_use]
pub fn build_status(listeners: Vec<Listener>, runtime: &ProxyRuntime) -> ProxyStatusView {
    let health = runtime.health();

    let listeners = listeners
        .into_iter()
        .map(|listener| {
            let current = health
                .iter()
                .find(|health| health.listener_id == listener.id);

            let state = match current {
                Some(health) => ListenerStateView::from(&health.state),
                // Включённый listener, о котором рантайм ещё не знает, находится
                // в процессе запуска; выключенный — просто не запущен.
                None if listener.enabled => ListenerStateView::Starting,
                None => ListenerStateView::Stopped,
            };

            ListenerView {
                listener,
                state,
                active_connections: current.map_or(0, |health| health.active_connections),
            }
        })
        .collect();

    ProxyStatusView {
        listeners,
        dropped_decisions: runtime.dropped_decisions(),
    }
}

/// Собирает состояние рантайма для события.
#[must_use]
pub fn build_runtime_view(runtime: &ProxyRuntime) -> ProxyRuntimeView {
    ProxyRuntimeView {
        listeners: runtime
            .health()
            .into_iter()
            .map(|health| ListenerHealthView {
                listener_id: health.listener_id,
                state: ListenerStateView::from(&health.state),
                active_connections: health.active_connections,
            })
            .collect(),
        dropped_decisions: runtime.dropped_decisions(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egresskeeper_core::{
        EgressError, ListenerRuntimeState, PolicySnapshot, PolicySource, ProfileId, ProxyConfig,
        ProxyPort, ProxyRuntime,
    };

    use super::*;

    /// Источник политики-заглушка: состояние listener'ов от него не зависит.
    #[derive(Debug)]
    struct NoPolicy;

    impl PolicySource for NoPolicy {
        fn snapshot(&self, _profile_id: &ProfileId) -> Result<Option<PolicySnapshot>, EgressError> {
            Ok(None)
        }
    }

    fn runtime() -> ProxyRuntime {
        let (runtime, _decisions) = ProxyRuntime::new(
            ProxyConfig::default(),
            Arc::new(NoPolicy),
            tokio::runtime::Handle::current(),
        );

        runtime
    }

    fn listener(port: u16, enabled: bool) -> Listener {
        Listener {
            id: ListenerId::new(),
            port: ProxyPort::parse(port).expect("port"),
            profile_id: ProfileId::new(),
            enabled,
            created_at_unix_ms: 0,
            updated_at_unix_ms: 0,
        }
    }

    #[tokio::test]
    async fn disabled_listener_without_runtime_state_is_stopped() {
        let runtime = runtime();
        let listener = listener(8787, false);

        let status = build_status(vec![listener.clone()], &runtime);

        assert_eq!(status.listeners.len(), 1);
        assert_eq!(status.listeners[0].listener.id, listener.id);
        assert_eq!(status.listeners[0].state, ListenerStateView::Stopped);
        assert_eq!(status.listeners[0].active_connections, 0);
        assert_eq!(status.dropped_decisions, 0);
    }

    #[tokio::test]
    async fn enabled_listener_without_runtime_state_is_starting() {
        let runtime = runtime();

        let status = build_status(vec![listener(8787, true)], &runtime);

        assert_eq!(status.listeners[0].state, ListenerStateView::Starting);
    }

    #[tokio::test]
    async fn running_listener_reports_running_state() {
        let runtime = runtime();
        let listener = listener(crate::proxy_view::tests::free_port(), true);

        runtime.request_start(&listener);
        for _ in 0..100 {
            if runtime.health().iter().any(|health| {
                health.listener_id == listener.id && health.state == ListenerState::Running
            }) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let status = build_status(vec![listener.clone()], &runtime);
        let view = &status.listeners[0];

        assert_eq!(view.state, ListenerStateView::Running);
        assert_eq!(view.active_connections, 0);

        let runtime_view = build_runtime_view(&runtime);
        assert_eq!(runtime_view.listeners.len(), 1);
        assert_eq!(runtime_view.listeners[0].listener_id, listener.id);

        runtime.shutdown();
    }

    #[tokio::test]
    async fn failed_listener_reports_code_and_message() {
        let runtime = runtime();
        let listener = listener(8787, true);
        let failure =
            ListenerStateView::from(&ListenerState::Failed(egresskeeper_core::ListenerFailure {
                code: egresskeeper_core::ErrorCode::PortUnavailable,
                message: "Порт уже занят другой программой.".to_owned(),
            }));

        match failure {
            ListenerStateView::Failed { code, message } => {
                assert_eq!(code, "port_unavailable");
                assert!(message.contains("занят"));
            }
            other => panic!("unexpected state: {other:?}"),
        }

        // Представление конфигурации без состояния рантайма не ломается.
        assert_eq!(build_status(vec![listener], &runtime).listeners.len(), 1);
    }

    /// Занимает свободный порт для тестов.
    pub(super) fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);

        port
    }
}
