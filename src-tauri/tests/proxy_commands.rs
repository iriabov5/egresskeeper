//! Интеграционные тесты IPC-слоя proxy.
//!
//! Проверяют связку «composition root → хранилище + рантайм → контракт»: команды
//! в `src-tauri` — тонкие адаптеры, поэтому их логика проверяется через тот же
//! `AppState`, с которым работают обработчики. Реальность состояния проверяется на
//! живом сокете: listener действительно принимает соединения.

use std::time::Duration;

use egresskeeper_app::{AppState, IpcError, ListenerStateView};
use egresskeeper_core::{Action, ListenerRuntimeState, PortSpec, RuleDraft, SqliteRepository};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Создаёт состояние приложения на временном каталоге.
fn initialize_state(directory: &tempfile::TempDir) -> AppState {
    AppState::initialize(directory.path(), "0.1.0")
}

/// Занимает свободный порт.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    port
}

/// Возвращает идентификатор профиля по умолчанию.
fn default_profile_id(state: &AppState) -> String {
    state
        .policy()
        .expect("policy is ready")
        .list_profiles()
        .expect("profiles")[0]
        .profile
        .id
        .as_str()
        .to_owned()
}

/// Ждёт фактического состояния listener'а в рантайме.
///
/// Состояние меняет задача listener'а, поэтому переходы асинхронны и факт нужно
/// ожидать, а не читать мгновенно.
async fn wait_for_runtime_state(
    state: &AppState,
    listener_id: &egresskeeper_core::ListenerId,
    expected: fn(&egresskeeper_core::ListenerState) -> bool,
) {
    for _ in 0..200 {
        let proxy = state.proxy().expect("runtime");

        if proxy
            .health()
            .iter()
            .any(|health| &health.listener_id == listener_id && expected(&health.state))
        {
            return;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("listener did not reach the expected runtime state");
}

/// Ждёт состояния listener'а.
async fn wait_for_state(
    state: &AppState,
    listener_id: &str,
    expected: fn(&ListenerStateView) -> bool,
) {
    for _ in 0..200 {
        let proxy = state.proxy().expect("runtime");
        let listeners = state.listeners().expect("service").list().expect("list");
        let view = egresskeeper_app::proxy_view_for_tests(listeners, proxy);

        if view.listeners.iter().any(|listener| {
            listener.listener.id.as_str() == listener_id && expected(&listener.state)
        }) {
            return;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("listener did not reach the expected state");
}

#[test]
fn listener_is_created_disabled_and_appears_in_status() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let profile_id = default_profile_id(&state);

    let service = state.listeners().expect("service is ready");
    let listener = service
        .create(8787, &profile_id)
        .expect("listener is created");

    assert!(!listener.enabled);
    assert_eq!(service.list().expect("list").len(), 1);
}

#[test]
fn privileged_port_is_reported_through_the_contract() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let profile_id = default_profile_id(&state);

    let error = IpcError::from_result(
        state
            .listeners()
            .expect("service is ready")
            .create(80, &profile_id),
    )
    .expect_err("privileged port");

    assert_eq!(error.code, "validation");
    assert_eq!(
        error.details.map(|details| details.field),
        Some("port".to_owned())
    );
}

#[test]
fn unknown_profile_is_reported_as_not_found() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);

    let error = IpcError::from_result(
        state
            .listeners()
            .expect("service is ready")
            .create(8787, egresskeeper_core::ProfileId::new().as_str()),
    )
    .expect_err("unknown profile");

    assert_eq!(error.code, "not_found");
}

#[test]
fn configuration_survives_reopening_the_shell() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let port = 9876;
    let profile_id = {
        let state = initialize_state(&directory);
        let profile_id = default_profile_id(&state);

        state
            .listeners()
            .expect("service is ready")
            .create(port, &profile_id)
            .expect("listener");

        profile_id
    };

    let reopened = initialize_state(&directory);
    let listeners = reopened
        .listeners()
        .expect("service is ready")
        .list()
        .expect("list");

    assert_eq!(listeners.len(), 1);
    assert_eq!(listeners[0].port.get(), port);
    assert_eq!(listeners[0].profile_id.as_str(), profile_id);
}

#[tokio::test]
async fn enabled_listener_accepts_connections_and_denies_by_default() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let profile_id = default_profile_id(&state);
    let port = free_port();

    let service = state.listeners().expect("service is ready");
    let listener = service.create(port, &profile_id).expect("listener");
    let enabled = service
        .set_enabled(listener.id.as_str(), true)
        .expect("listener is enabled");
    assert!(enabled.enabled);

    wait_for_state(&state, listener.id.as_str(), |state| {
        matches!(state, ListenerStateView::Running)
    })
    .await;

    // Профиль по умолчанию запрещает всё: соединение отклоняется без обращения к цели.
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream
        .write_all(b"CONNECT 127.0.0.1:9 HTTP/1.1\r\nHost: 127.0.0.1:9\r\n\r\n")
        .await
        .expect("request");

    let mut buffer = [0_u8; 32];
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
        .await
        .expect("response arrives")
        .expect("response is readable");
    let response = String::from_utf8_lossy(&buffer[..read]);

    // Порт цели совпадает с портом самого listener'а? Нет: 9 — это не наш порт,
    // поэтому решение принимает политика, а не защита от петли.
    assert!(response.starts_with("HTTP/1.1 403"), "response: {response}");

    state.shutdown_proxy();
}

#[tokio::test]
async fn decisions_reach_the_decision_stream() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let profile_id = default_profile_id(&state);
    let port = free_port();

    let mut decisions = state.take_decision_stream().expect("stream is available");

    let service = state.listeners().expect("service is ready");
    let listener = service.create(port, &profile_id).expect("listener");
    service
        .set_enabled(listener.id.as_str(), true)
        .expect("enabled");

    wait_for_state(&state, listener.id.as_str(), |state| {
        matches!(state, ListenerStateView::Running)
    })
    .await;

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream
        .write_all(b"GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n")
        .await
        .expect("request");

    let decision = tokio::time::timeout(Duration::from_secs(5), decisions.recv())
        .await
        .expect("decision arrives")
        .expect("stream is open");

    assert_eq!(decision.host, "example.com");
    assert_eq!(decision.port, 80);
    assert!(decision.is_denied());
    assert_eq!(decision.listener_id, listener.id);

    state.shutdown_proxy();
}

#[tokio::test]
async fn allowed_connection_is_proxied_end_to_end() {
    // Тестовый целевой сервер.
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let upstream_address = upstream.local_addr().expect("addr");
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = upstream.accept().await {
            let mut buffer = [0_u8; 1024];
            if let Ok(read) = stream.read(&mut buffer).await {
                let _ = stream.write_all(&buffer[..read]).await;
            }
        }
    });

    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let port = free_port();

    // Разрешаем обращение к тестовому серверу: профиль по умолчанию запрещает всё.
    let repository = SqliteRepository::open_in_state_dir(directory.path()).expect("store opens");
    let profile = egresskeeper_core::PolicyRepository::list_profiles(&repository)
        .expect("profiles")
        .remove(0);
    egresskeeper_core::PolicyRepository::add_rule(
        &repository,
        &profile.id,
        &RuleDraft {
            action: Action::Allow,
            host: egresskeeper_core::HostMatcher::exact(&upstream_address.ip().to_string())
                .expect("host"),
            port: PortSpec::exactly(upstream_address.port()).expect("port"),
        },
    )
    .expect("rule");

    let service = state.listeners().expect("service is ready");
    let listener = service.create(port, profile.id.as_str()).expect("listener");
    service
        .set_enabled(listener.id.as_str(), true)
        .expect("enabled");

    wait_for_state(&state, listener.id.as_str(), |state| {
        matches!(state, ListenerStateView::Running)
    })
    .await;

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream
        .write_all(
            format!("CONNECT {upstream_address} HTTP/1.1\r\nHost: {upstream_address}\r\n\r\n")
                .as_bytes(),
        )
        .await
        .expect("request");

    let mut buffer = [0_u8; 64];
    let read = stream.read(&mut buffer).await.expect("response");
    let head = String::from_utf8_lossy(&buffer[..read]);
    assert!(head.starts_with("HTTP/1.1 200"), "response: {head}");

    stream.write_all(b"ping").await.expect("tunnel write");
    let mut echo = [0_u8; 4];
    stream.read_exact(&mut echo).await.expect("tunnel read");
    assert_eq!(&echo, b"ping");

    state.shutdown_proxy();
}

#[tokio::test]
async fn deleting_running_listener_stops_it() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let profile_id = default_profile_id(&state);

    let service = state.listeners().expect("service is ready");
    let listener = service.create(free_port(), &profile_id).expect("listener");
    service
        .set_enabled(listener.id.as_str(), true)
        .expect("enabled");

    wait_for_runtime_state(&state, &listener.id, |state| {
        matches!(state, egresskeeper_core::ListenerState::Running)
    })
    .await;

    service.delete(listener.id.as_str()).expect("deleted");

    // Остановка асинхронна: состояние меняет задача listener'а, поэтому факт
    // проверяется ожиданием, а не мгновенным чтением.
    wait_for_runtime_state(&state, &listener.id, |state| {
        matches!(state, egresskeeper_core::ListenerState::Stopped)
    })
    .await;

    assert!(service.list().expect("list").is_empty());
    assert!(
        !ListenerRuntimeState::is_active(state.proxy().expect("runtime"), &listener.id),
        "удалённый listener не должен оставаться активным"
    );
}

#[test]
fn shell_stays_usable_when_storage_is_unavailable() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory
        .path()
        .join(egresskeeper_core::infrastructure::sqlite::DATABASE_FILE_NAME);
    std::fs::write(&database, b"this is not a database").expect("garbage database file");

    let state = initialize_state(&directory);

    assert!(state.overview().is_ok(), "runtime info stays available");

    let error = state.listeners().expect_err("listeners are unavailable");
    assert_eq!(error.code, "storage_unavailable");
    assert!(state.proxy().is_err());
}

#[test]
fn policy_commands_still_work_after_proxy_integration() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let state = initialize_state(&directory);
    let service = state.policy().expect("policy is ready");

    let profile = service.create_profile("Работа").expect("profile");
    service
        .add_rule(
            profile.id.as_str(),
            egresskeeper_core::RuleInput {
                action: Action::Allow,
                host_kind: egresskeeper_core::HostKind::Exact,
                host: "api.example.com".to_owned(),
                port: PortSpec::any(),
            },
        )
        .expect("rule");

    let decision = service
        .evaluate(profile.id.as_str(), "api.example.com", 443)
        .expect("decision");

    assert_eq!(decision.action, Action::Allow);
}

#[tokio::test]
async fn enabled_listeners_resume_when_shell_starts_again() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let port = free_port();

    let listener_id = {
        let state = initialize_state(&directory);
        let profile_id = default_profile_id(&state);
        let service = state.listeners().expect("service is ready");
        let listener = service.create(port, &profile_id).expect("listener");
        service
            .set_enabled(listener.id.as_str(), true)
            .expect("enabled");

        wait_for_runtime_state(&state, &listener.id, |state| {
            matches!(state, egresskeeper_core::ListenerState::Running)
        })
        .await;

        state.shutdown_proxy();
        wait_for_runtime_state(&state, &listener.id, |state| {
            matches!(state, egresskeeper_core::ListenerState::Stopped)
        })
        .await;

        listener.id
    };

    // Новая сессия: рантайм пуст, конфигурация прочитана из хранилища.
    let state = initialize_state(&directory);
    assert!(
        state.proxy().expect("runtime").health().is_empty(),
        "новый рантайм не должен знать о listeners до их запуска"
    );

    state.start_enabled_listeners();

    wait_for_runtime_state(&state, &listener_id, |state| {
        matches!(state, egresskeeper_core::ListenerState::Running)
    })
    .await;

    // Listener действительно принимает соединения: порт занят и отвечает отказом.
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("listener is accepting");
    stream
        .write_all(b"GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n")
        .await
        .expect("request");

    let mut buffer = [0_u8; 32];
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
        .await
        .expect("response arrives")
        .expect("response is readable");
    let response = String::from_utf8_lossy(&buffer[..read]);

    assert!(response.starts_with("HTTP/1.1 403"), "response: {response}");

    state.shutdown_proxy();
}

#[tokio::test]
async fn disabled_listeners_do_not_resume() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let port = free_port();

    let listener_id = {
        let state = initialize_state(&directory);
        let profile_id = default_profile_id(&state);
        let service = state.listeners().expect("service is ready");
        let listener = service.create(port, &profile_id).expect("listener");

        listener.id
    };

    let state = initialize_state(&directory);
    state.start_enabled_listeners();

    assert!(
        !ListenerRuntimeState::is_active(state.proxy().expect("runtime"), &listener_id),
        "выключенный listener не должен запускаться сам"
    );
}
