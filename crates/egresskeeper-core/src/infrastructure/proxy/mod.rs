//! Локальный egress proxy.
//!
//! Рантайм принимает соединения на loopback-адресе, применяет политику профиля
//! listener'а и публикует решения. Один listener — одна задача с владением своим
//! сокетом; остановка идёт сверху вниз, а соединения живут внутри задачи
//! listener'а, поэтому не могут «утечь» за её пределы.
//!
//! Спавн задач выполняется через `tokio::spawn` внутри текущего рантайма
//! приложения (Tauri использует Tokio), поэтому второй рантайм не создаётся.

mod policy_source;
mod server;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::application::listeners::ListenerRuntimeState;
use crate::application::ports::PolicySource;
use crate::domain::error::{EgressError, ErrorCode};
use crate::domain::proxy::listener::{Listener, ListenerId};
use crate::domain::proxy::outcome::ProxyDecision;

pub use policy_source::RepositoryPolicySource;

/// Лимиты рантайма proxy.
///
/// Значения вынесены в конфигурацию, чтобы тесты могли сокращать таймауты, а
/// продакшн-значения оставались в одном месте.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyLimits {
    /// Таймаут установления соединения с целевым сервером.
    pub connect_timeout: Duration,
    /// Таймаут чтения заголовков запроса от клиента.
    pub header_read_timeout: Duration,
    /// Максимум одновременных соединений на listener.
    pub max_connections_per_listener: usize,
    /// Ёмкость очереди решений для UI.
    pub decision_queue_capacity: usize,
}

impl Default for ProxyLimits {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            header_read_timeout: Duration::from_secs(15),
            max_connections_per_listener: 128,
            decision_queue_capacity: 512,
        }
    }
}

/// Конфигурация рантайма proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyConfig {
    bind_address: IpAddr,
    limits: ProxyLimits,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self::loopback(ProxyLimits::default())
    }
}

impl ProxyConfig {
    /// Конфигурация с привязкой к loopback-адресу.
    #[must_use]
    pub const fn loopback(limits: ProxyLimits) -> Self {
        Self {
            bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            limits,
        }
    }

    /// Конфигурация с явно заданным адресом привязки.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`], если адрес не является
    /// loopback-адресом: proxy без аутентификации клиентов не должен быть
    /// доступен из сети.
    pub fn with_bind_address(address: IpAddr, limits: ProxyLimits) -> Result<Self, EgressError> {
        if !address.is_loopback() {
            return Err(EgressError::validation(
                "bind_address",
                "must be a loopback address",
            ));
        }

        Ok(Self {
            bind_address: address,
            limits,
        })
    }

    /// Адрес прослушивания.
    #[must_use]
    pub const fn bind_address(&self) -> IpAddr {
        self.bind_address
    }

    /// Лимиты рантайма.
    #[must_use]
    pub const fn limits(&self) -> ProxyLimits {
        self.limits
    }
}

/// Состояние listener'а в рантайме.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerState {
    /// Listener запускается.
    Starting,
    /// Listener принимает соединения.
    Running,
    /// Listener остановлен.
    Stopped,
    /// Listener не смог запуститься.
    Failed(ListenerFailure),
}

/// Причина отказа запуска listener'а.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerFailure {
    /// Machine-readable код ошибки.
    pub code: ErrorCode,
    /// Сообщение, безопасное для показа пользователю.
    pub message: String,
}

/// Наблюдаемое состояние listener'а.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerHealth {
    /// Идентификатор listener'а.
    pub listener_id: ListenerId,
    /// Состояние.
    pub state: ListenerState,
    /// Количество активных соединений.
    pub active_connections: u32,
}

/// Рантайм proxy: владеет задачами listeners и публикует решения.
///
/// Клонирование дешёвое: клоны разделяют одни и те же задачи listeners.
#[derive(Debug, Clone)]
pub struct ProxyRuntime {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    config: ProxyConfig,
    policy: Arc<dyn PolicySource>,
    listeners: Mutex<HashMap<ListenerId, ListenerHandle>>,
    decisions: mpsc::Sender<ProxyDecision>,
    dropped_decisions: AtomicU64,
    state_generation: watch::Sender<u64>,
    /// Способ запуска фоновых задач.
    ///
    /// Рантайм proxy не создаёт собственный async-рантайм: handle передаёт
    /// composition root, поэтому в приложении остаётся один рантайм, а запуск
    /// задач не зависит от того, есть ли контекст рантайма в текущем потоке
    /// (команды выполняются в блокирующем пуле).
    runtime: Handle,
}

#[derive(Debug)]
struct ListenerHandle {
    shutdown: watch::Sender<bool>,
    state: Arc<Mutex<ListenerState>>,
    active: Arc<AtomicU32>,
    task: Option<JoinHandle<()>>,
}

impl ProxyRuntime {
    /// Создаёт рантайм и поток решений для UI.
    ///
    /// Решения передаются через канал ограниченной ёмкости: при переполнении
    /// старые отбрасываются, а счётчик пропущенных увеличивается, поэтому поток
    /// решений не становится backpressure для обработки соединений.
    #[must_use]
    pub fn new(
        config: ProxyConfig,
        policy: Arc<dyn PolicySource>,
        runtime: Handle,
    ) -> (Self, mpsc::Receiver<ProxyDecision>) {
        let (sender, receiver) = mpsc::channel(config.limits().decision_queue_capacity);
        let (state_generation, _) = watch::channel(0_u64);

        (
            Self {
                inner: Arc::new(Inner {
                    config,
                    policy,
                    listeners: Mutex::new(HashMap::new()),
                    decisions: sender,
                    dropped_decisions: AtomicU64::new(0),
                    state_generation,
                    runtime,
                }),
            },
            receiver,
        )
    }

    /// Конфигурация рантайма.
    #[must_use]
    pub fn config(&self) -> &ProxyConfig {
        &self.inner.config
    }

    /// Наблюдаемое состояние listeners, известных рантайму.
    #[must_use]
    pub fn health(&self) -> Vec<ListenerHealth> {
        let Ok(listeners) = self.inner.listeners.lock() else {
            return Vec::new();
        };

        listeners
            .iter()
            .map(|(id, handle)| ListenerHealth {
                listener_id: id.clone(),
                state: handle.state(),
                active_connections: handle.active.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// Подписка на изменения состояния listeners.
    ///
    /// Значение — счётчик изменений: он увеличивается при каждом переходе
    /// состояния, поэтому подписчик может перечитать состояние и передать его в
    /// UI. Состояние остаётся доступным и через [`ProxyRuntime::health`], поэтому
    /// потеря уведомления не приводит к неверному отображению.
    #[must_use]
    pub fn state_changes(&self) -> watch::Receiver<u64> {
        self.inner.state_generation.subscribe()
    }

    /// Количество решений, не попавших в поток для UI.
    #[must_use]
    pub fn dropped_decisions(&self) -> u64 {
        self.inner.dropped_decisions.load(Ordering::Relaxed)
    }

    /// Останавливает все listeners.
    ///
    /// Вызывается при завершении приложения: сигнал остановки не блокирует
    /// выключение.
    pub fn shutdown(&self) {
        let Ok(listeners) = self.inner.listeners.lock() else {
            return;
        };

        for handle in listeners.values() {
            let _ = handle.shutdown.send(true);
        }
    }

    /// Запускает listener.
    fn start_listener(&self, listener: &Listener) {
        let mut listeners = match self.inner.listeners.lock() {
            Ok(listeners) => listeners,
            Err(_) => return,
        };

        if listeners
            .get(&listener.id)
            .is_some_and(|handle| handle.is_running())
        {
            return;
        }

        // Предыдущая задача могла ещё не освободить порт: новая задача дождётся
        // её завершения, иначе быстрый цикл «выключить-включить» упирался бы в
        // занятый порт.
        let predecessor = listeners
            .remove(&listener.id)
            .and_then(|handle| handle.task);

        let (shutdown, shutdown_receiver) = watch::channel(false);
        let state = Arc::new(Mutex::new(ListenerState::Starting));
        let active = Arc::new(AtomicU32::new(0));

        let task = server::spawn_listener(server::ListenerContext {
            listener_id: listener.id.clone(),
            profile_id: listener.profile_id.clone(),
            port: listener.port,
            config: self.inner.config.clone(),
            policy: Arc::clone(&self.inner.policy),
            inner: Arc::clone(&self.inner),
            runtime: self.inner.runtime.clone(),
            state: Arc::clone(&state),
            active: Arc::clone(&active),
            shutdown: shutdown_receiver,
            predecessor,
        });

        listeners.insert(
            listener.id.clone(),
            ListenerHandle {
                shutdown,
                state,
                active,
                task: Some(task),
            },
        );
    }

    /// Останавливает listener.
    fn stop_listener(&self, id: &ListenerId) {
        let Ok(listeners) = self.inner.listeners.lock() else {
            return;
        };

        if let Some(handle) = listeners.get(id) {
            let _ = handle.shutdown.send(true);
        }
    }
}

impl ListenerRuntimeState for ProxyRuntime {
    fn is_active(&self, id: &ListenerId) -> bool {
        self.inner
            .listeners
            .lock()
            .ok()
            .and_then(|listeners| listeners.get(id).map(ListenerHandle::is_running))
            .unwrap_or(false)
    }

    fn request_start(&self, listener: &Listener) {
        self.start_listener(listener);
    }

    fn request_stop(&self, id: &ListenerId) {
        self.stop_listener(id);
    }
}

impl Inner {
    /// Сообщает подписчикам, что состояние listeners изменилось.
    pub(super) fn notify_state_change(&self) {
        let current = *self.state_generation.borrow();
        let _ = self.state_generation.send(current.wrapping_add(1));
    }

    /// Публикует решение для UI, не блокируя обработку соединений.
    fn publish(&self, decision: ProxyDecision) {
        // Решение фиксируется в логе: это единственный след трафика, пока нет
        // audit trail (он появится отдельным change'ом).
        tracing::debug!(
            listener = %decision.listener_id,
            host = %decision.host,
            port = decision.port,
            action = decision.action.as_str(),
            "proxy decision"
        );

        match self.decisions.try_send(decision) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.dropped_decisions.fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::debug!("decision stream is closed; decisions are no longer delivered");
            }
        }
    }
}

impl ListenerHandle {
    /// Возвращает `true`, если listener принимает соединения или запускается.
    fn is_running(&self) -> bool {
        matches!(
            self.state(),
            ListenerState::Starting | ListenerState::Running
        )
    }

    /// Текущее состояние listener'а.
    fn state(&self) -> ListenerState {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or(ListenerState::Stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policy::entities::ProfileId;
    use crate::domain::proxy::outcome::PolicySnapshot;

    #[derive(Debug)]
    struct EmptyPolicySource;

    impl PolicySource for EmptyPolicySource {
        fn snapshot(&self, _profile_id: &ProfileId) -> Result<Option<PolicySnapshot>, EgressError> {
            Ok(Some(PolicySnapshot::Unavailable))
        }
    }

    fn runtime() -> (ProxyRuntime, mpsc::Receiver<ProxyDecision>) {
        ProxyRuntime::new(
            ProxyConfig::default(),
            Arc::new(EmptyPolicySource),
            Handle::current(),
        )
    }

    #[test]
    fn non_loopback_bind_address_is_rejected() {
        let error = ProxyConfig::with_bind_address(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            ProxyLimits::default(),
        )
        .expect_err("wildcard address must be rejected");

        assert_eq!(error.code(), ErrorCode::Validation);
        assert_eq!(error.invalid_field(), Some("bind_address"));
    }

    #[test]
    fn loopback_addresses_are_accepted() {
        for address in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            "::1".parse::<IpAddr>().expect("ipv6 loopback"),
        ] {
            let config =
                ProxyConfig::with_bind_address(address, ProxyLimits::default()).expect("loopback");

            assert_eq!(config.bind_address(), address);
        }
    }

    #[tokio::test]
    async fn fresh_runtime_has_no_listeners_and_no_dropped_decisions() {
        let (runtime, _decisions) = runtime();

        assert!(runtime.health().is_empty());
        assert_eq!(runtime.dropped_decisions(), 0);
    }

    #[tokio::test]
    async fn publish_reports_dropped_decisions_when_queue_is_full() {
        let (sender, mut receiver) = mpsc::channel(1);
        let inner = Inner {
            config: ProxyConfig::default(),
            policy: Arc::new(EmptyPolicySource),
            listeners: Mutex::new(HashMap::new()),
            decisions: sender,
            dropped_decisions: AtomicU64::new(0),
            state_generation: watch::channel(0_u64).0,
            runtime: Handle::current(),
        };
        let decision = ProxyDecision {
            listener_id: ListenerId::new(),
            host: "api.example.com".to_owned(),
            port: 443,
            action: crate::domain::policy::Action::Deny,
            reason: crate::domain::proxy::outcome::ProxyDecisionReason::PolicyUnavailable,
            at_unix_ms: 0,
        };

        inner.publish(decision.clone());
        inner.publish(decision);

        assert_eq!(inner.dropped_decisions.load(Ordering::Relaxed), 1);
        assert!(receiver.try_recv().is_ok());
    }
}
