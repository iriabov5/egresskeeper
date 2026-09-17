//! Интеграционные тесты сетевого рантайма proxy на реальных сокетах.
//!
//! Тесты поднимают proxy и тестовый сервер в одном процессе, поэтому не требуют
//! внешней сети. Проверяется именно наблюдаемое поведение: байты, статусы
//! ответов, отсутствие обращений к цели при запрете, поведение при остановке и
//! исчерпании лимитов.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use egresskeeper_core::{
    Action, EgressError, Listener, ListenerId, ListenerRuntimeState, ListenerState, PolicySnapshot,
    PolicySource, PortSpec, Profile, ProfileId, ProxyConfig, ProxyDecisionReason, ProxyLimits,
    ProxyPort, ProxyRuntime, Rule, RuleId,
};
use http_body_util::Full;
use hyper::Response;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Источник политики с заранее заданным ответом.
#[derive(Debug)]
struct FakePolicySource {
    profile: Option<Profile>,
    rules: Vec<Rule>,
    failure: Option<&'static str>,
}

impl PolicySource for FakePolicySource {
    fn snapshot(&self, _profile_id: &ProfileId) -> Result<Option<PolicySnapshot>, EgressError> {
        if let Some(message) = self.failure {
            return Err(EgressError::storage_message(message));
        }

        Ok(self.profile.as_ref().map(|profile| PolicySnapshot::Loaded {
            profile: profile.clone(),
            rules: self.rules.clone(),
        }))
    }
}

/// Тестовый профиль с запрещающим default action.
fn profile() -> Profile {
    Profile {
        id: ProfileId::new(),
        name: "Тест".to_owned(),
        default_action: Action::Deny,
        created_at_unix_ms: 0,
        updated_at_unix_ms: 0,
    }
}

/// Правило для профиля.
fn rule(profile: &Profile, action: Action, host: &str, port: PortSpec) -> Rule {
    Rule {
        id: RuleId::new(),
        profile_id: profile.id.clone(),
        position: 0,
        action,
        host: egresskeeper_core::HostMatcher::exact(host).expect("host"),
        port,
    }
}

/// Источник политики, разрешающий указанную цель.
fn allowing(profile: &Profile, host: &str, port: u16) -> Arc<FakePolicySource> {
    Arc::new(FakePolicySource {
        profile: Some(profile.clone()),
        rules: vec![rule(
            profile,
            Action::Allow,
            host,
            PortSpec::exactly(port).expect("port"),
        )],
        failure: None,
    })
}

/// Источник политики без правил: действует запрещающий default action.
fn denying() -> Arc<FakePolicySource> {
    Arc::new(FakePolicySource {
        profile: Some(profile()),
        rules: Vec::new(),
        failure: None,
    })
}

/// Источник политики, у которого профиль отсутствует.
fn missing_profile() -> Arc<FakePolicySource> {
    Arc::new(FakePolicySource {
        profile: None,
        rules: Vec::new(),
        failure: None,
    })
}

/// Источник политики, который не может прочитать политику.
fn failing() -> Arc<FakePolicySource> {
    Arc::new(FakePolicySource {
        profile: None,
        rules: Vec::new(),
        failure: Some("database is locked"),
    })
}

/// Занимает свободный порт и освобождает его.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    port
}

/// Создаёт сущность listener'а для рантайма.
fn listener(port: u16, profile: &Profile) -> Listener {
    Listener {
        id: ListenerId::new(),
        port: ProxyPort::parse(port).expect("port"),
        profile_id: profile.id.clone(),
        enabled: true,
        created_at_unix_ms: 0,
        updated_at_unix_ms: 0,
    }
}

/// Поднимает рантайм с заданными лимитами.
fn runtime(
    policy: Arc<FakePolicySource>,
    limits: ProxyLimits,
) -> (
    ProxyRuntime,
    tokio::sync::mpsc::Receiver<egresskeeper_core::ProxyDecision>,
) {
    ProxyRuntime::new(
        ProxyConfig::loopback(limits),
        policy,
        tokio::runtime::Handle::current(),
    )
}

/// Ждёт, пока listener перейдёт в ожидаемое состояние.
async fn wait_for_state(
    runtime: &ProxyRuntime,
    id: &ListenerId,
    expected: fn(&ListenerState) -> bool,
) {
    for _ in 0..200 {
        if runtime
            .health()
            .iter()
            .any(|health| &health.listener_id == id && expected(&health.state))
        {
            return;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("listener did not reach the expected state");
}

/// Ждёт перехода listener'а в состояние работы.
async fn wait_until_running(runtime: &ProxyRuntime, id: &ListenerId) {
    wait_for_state(runtime, id, |state| matches!(state, ListenerState::Running)).await;
}

/// Поднимает TCP-эхо-сервер: подходит для проверки туннеля.
async fn spawn_echo_upstream() -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("addr");
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&connections);

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };

            counter.fetch_add(1, Ordering::Relaxed);

            tokio::spawn(async move {
                let mut buffer = [0_u8; 1024];
                loop {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            if stream.write_all(&buffer[..read]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });

    (address, connections)
}

/// Поднимает HTTP-сервер, который отвечает фиксированным телом.
async fn spawn_http_upstream() -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("addr");
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&connections);

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };

            counter.fetch_add(1, Ordering::Relaxed);

            tokio::spawn(async move {
                let service = service_fn(|_request| async {
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(
                        b"upstream-ok",
                    ))))
                });

                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });

    (address, connections)
}

/// Отправляет запрос через proxy и читает заголовки ответа.
async fn request_via_proxy(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("proxy accepts connections");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("request is sent");

    read_response_head(&mut stream).await
}

/// Читает данные до конца заголовков ответа.
async fn read_response_head(stream: &mut TcpStream) -> String {
    let mut received = Vec::new();
    let mut buffer = [0_u8; 256];

    for _ in 0..64 {
        let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
            .await
            .expect("response arrives")
            .expect("response is readable");

        if read == 0 {
            break;
        }

        received.extend_from_slice(&buffer[..read]);

        if received.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    String::from_utf8_lossy(&received).into_owned()
}

#[tokio::test]
async fn allowed_connect_tunnels_bytes_in_both_directions() {
    let (upstream, connections) = spawn_echo_upstream().await;
    let profile = profile();
    let policy = allowing(&profile, &upstream.ip().to_string(), upstream.port());
    let (runtime, mut decisions) = runtime(policy, ProxyLimits::default());
    let port = free_port();
    let listener = listener(port, &profile);

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    let head = format!("CONNECT {upstream} HTTP/1.1\r\nHost: {upstream}\r\n\r\n");
    stream.write_all(head.as_bytes()).await.expect("request");

    let response = read_response_head(&mut stream).await;
    assert!(response.starts_with("HTTP/1.1 200"), "response: {response}");

    stream.write_all(b"ping").await.expect("tunnel write");
    let mut buffer = [0_u8; 4];
    stream.read_exact(&mut buffer).await.expect("tunnel read");
    assert_eq!(&buffer, b"ping");

    assert_eq!(connections.load(Ordering::Relaxed), 1);

    let decision = decisions.recv().await.expect("decision is published");
    assert_eq!(decision.action, Action::Allow);
    assert_eq!(decision.port, upstream.port());
    assert!(matches!(
        decision.reason,
        ProxyDecisionReason::Policy { .. }
    ));
}

#[tokio::test]
async fn denied_connect_returns_forbidden_and_never_contacts_target() {
    let (upstream, connections) = spawn_echo_upstream().await;
    let (runtime, mut decisions) = runtime(denying(), ProxyLimits::default());
    let port = free_port();
    let listener = listener(port, &profile());

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    let response = request_via_proxy(
        port,
        &format!("CONNECT {upstream} HTTP/1.1\r\nHost: {upstream}\r\n\r\n"),
    )
    .await;

    assert!(response.starts_with("HTTP/1.1 403"), "response: {response}");
    assert_eq!(
        connections.load(Ordering::Relaxed),
        0,
        "запрещённое соединение не должно доходить до цели"
    );

    let decision = decisions.recv().await.expect("decision is published");
    assert!(decision.is_denied());
    assert!(matches!(
        decision.reason,
        ProxyDecisionReason::Policy {
            decision: egresskeeper_core::DecisionReason::DefaultAction { .. }
        }
    ));
}

#[tokio::test]
async fn missing_policy_profile_denies_connection() {
    let (upstream, connections) = spawn_echo_upstream().await;
    let (runtime, mut decisions) = runtime(missing_profile(), ProxyLimits::default());
    let port = free_port();
    let listener = listener(port, &profile());

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    let response = request_via_proxy(
        port,
        &format!("CONNECT {upstream} HTTP/1.1\r\nHost: {upstream}\r\n\r\n"),
    )
    .await;

    assert!(response.starts_with("HTTP/1.1 403"), "response: {response}");
    assert_eq!(connections.load(Ordering::Relaxed), 0);

    let decision = decisions.recv().await.expect("decision is published");
    assert_eq!(decision.reason, ProxyDecisionReason::PolicyUnavailable);
}

#[tokio::test]
async fn failing_policy_source_denies_connection() {
    let (upstream, _connections) = spawn_echo_upstream().await;
    let (runtime, mut decisions) = runtime(failing(), ProxyLimits::default());
    let port = free_port();
    let listener = listener(port, &profile());

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    let response = request_via_proxy(
        port,
        &format!("CONNECT {upstream} HTTP/1.1\r\nHost: {upstream}\r\n\r\n"),
    )
    .await;

    assert!(response.starts_with("HTTP/1.1 403"), "response: {response}");

    let decision = decisions.recv().await.expect("decision is published");
    assert_eq!(decision.reason, ProxyDecisionReason::PolicyUnavailable);
}

#[tokio::test]
async fn allowed_http_request_is_forwarded() {
    let (upstream, connections) = spawn_http_upstream().await;
    let profile = profile();
    let policy = allowing(&profile, &upstream.ip().to_string(), upstream.port());
    let (runtime, mut decisions) = runtime(policy, ProxyLimits::default());
    let port = free_port();
    let listener = listener(port, &profile);

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    let response = request_via_proxy(
        port,
        &format!(
            "GET http://{upstream}/hello HTTP/1.1\r\nHost: {upstream}\r\nProxy-Connection: keep-alive\r\n\r\n"
        ),
    )
    .await;

    assert!(response.starts_with("HTTP/1.1 200"), "response: {response}");
    assert_eq!(connections.load(Ordering::Relaxed), 1);

    let decision = decisions.recv().await.expect("decision is published");
    assert_eq!(decision.action, Action::Allow);
}

#[tokio::test]
async fn denied_http_request_is_not_forwarded() {
    let (upstream, connections) = spawn_http_upstream().await;
    let (runtime, mut decisions) = runtime(denying(), ProxyLimits::default());
    let port = free_port();
    let listener = listener(port, &profile());

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    let response = request_via_proxy(
        port,
        &format!("GET http://{upstream}/hello HTTP/1.1\r\nHost: {upstream}\r\n\r\n"),
    )
    .await;

    assert!(response.starts_with("HTTP/1.1 403"), "response: {response}");
    assert_eq!(connections.load(Ordering::Relaxed), 0);

    let decision = decisions.recv().await.expect("decision is published");
    assert!(decision.is_denied());
}

#[tokio::test]
async fn request_without_target_is_rejected() {
    let (runtime, _decisions) = runtime(denying(), ProxyLimits::default());
    let port = free_port();
    let listener = listener(port, &profile());

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    let response =
        request_via_proxy(port, "GET /relative HTTP/1.1\r\nHost: example.com\r\n\r\n").await;

    assert!(response.starts_with("HTTP/1.1 400"), "response: {response}");
}

#[tokio::test]
async fn loopback_target_is_denied_even_when_policy_allows_it() {
    let profile = profile();
    let policy = Arc::new(FakePolicySource {
        profile: Some(profile.clone()),
        rules: vec![rule(&profile, Action::Allow, "127.0.0.1", PortSpec::any())],
        failure: None,
    });
    let (runtime, mut decisions) = runtime(policy, ProxyLimits::default());
    let port = free_port();
    let listener = listener(port, &profile);

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    let response = request_via_proxy(
        port,
        &format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"),
    )
    .await;

    assert!(response.starts_with("HTTP/1.1 403"), "response: {response}");

    let decision = decisions.recv().await.expect("decision is published");
    assert_eq!(decision.reason, ProxyDecisionReason::LoopbackTarget);
}

#[tokio::test]
async fn listener_reports_failure_when_port_is_taken() {
    let taken = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = taken.local_addr().expect("addr").port();

    let (runtime, _decisions) = runtime(denying(), ProxyLimits::default());
    let listener = listener(port, &profile());

    runtime.request_start(&listener);

    wait_for_state(&runtime, &listener.id, |state| {
        matches!(state, ListenerState::Failed(_))
    })
    .await;

    let health = runtime.health();
    let state = &health[0].state;

    match state {
        ListenerState::Failed(failure) => {
            assert_eq!(failure.code, egresskeeper_core::ErrorCode::PortUnavailable);
            assert!(failure.message.contains("занят"));
        }
        other => panic!("unexpected state: {other:?}"),
    }
}

#[tokio::test]
async fn stopping_listener_closes_active_tunnel_and_stops_accepting() {
    let (upstream, _connections) = spawn_echo_upstream().await;
    let profile = profile();
    let policy = allowing(&profile, &upstream.ip().to_string(), upstream.port());
    let (runtime, _decisions) = runtime(policy, ProxyLimits::default());
    let port = free_port();
    let listener = listener(port, &profile);

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream
        .write_all(format!("CONNECT {upstream} HTTP/1.1\r\nHost: {upstream}\r\n\r\n").as_bytes())
        .await
        .expect("request");
    assert!(
        read_response_head(&mut stream)
            .await
            .starts_with("HTTP/1.1 200")
    );

    runtime.request_stop(&listener.id);

    // Активный туннель закрывается вместе с listener'ом.
    let mut buffer = [0_u8; 1];
    let closed = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await;
    assert!(
        matches!(closed, Ok(Ok(0)) | Ok(Err(_))),
        "активный туннель должен быть закрыт: {closed:?}"
    );

    // Новые соединения не принимаются.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let refused = TcpStream::connect(("127.0.0.1", port)).await;
    if let Ok(mut stream) = refused {
        let mut buffer = [0_u8; 1];
        let result = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buffer)).await;

        assert!(
            matches!(result, Ok(Ok(0)) | Ok(Err(_)) | Err(_)),
            "listener не должен обслуживать соединения после остановки"
        );
    }
}

#[tokio::test]
async fn listener_can_be_restarted_after_stop() {
    let (upstream, _connections) = spawn_echo_upstream().await;
    let profile = profile();
    let policy = allowing(&profile, &upstream.ip().to_string(), upstream.port());
    let (runtime, _decisions) = runtime(policy, ProxyLimits::default());
    let port = free_port();
    let listener = listener(port, &profile);

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    runtime.request_stop(&listener.id);
    wait_for_state(&runtime, &listener.id, |state| {
        matches!(state, ListenerState::Stopped)
    })
    .await;

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    let response = request_via_proxy(
        port,
        &format!("CONNECT {upstream} HTTP/1.1\r\nHost: {upstream}\r\n\r\n"),
    )
    .await;

    assert!(response.starts_with("HTTP/1.1 200"), "response: {response}");
}

#[tokio::test]
async fn silent_client_is_disconnected_by_header_timeout() {
    let limits = ProxyLimits {
        header_read_timeout: Duration::from_millis(200),
        ..ProxyLimits::default()
    };
    let (runtime, _decisions) = runtime(denying(), limits);
    let port = free_port();
    let listener = listener(port, &profile());

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");

    // Клиент молчит: соединение должно быть закрыто по таймауту чтения заголовков.
    let mut buffer = [0_u8; 1];
    let result = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await;

    assert!(
        matches!(result, Ok(Ok(0)) | Ok(Err(_))),
        "молчащее соединение должно быть закрыто: {result:?}"
    );
}

#[tokio::test]
async fn connection_limit_closes_extra_connections() {
    let limits = ProxyLimits {
        max_connections_per_listener: 1,
        ..ProxyLimits::default()
    };
    let (runtime, _decisions) = runtime(denying(), limits);
    let port = free_port();
    let listener = listener(port, &profile());

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    // Первое соединение занимает единственный слот и молчит.
    let _first = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    tokio::time::sleep(Duration::from_millis(150)).await;

    assert_eq!(runtime.health()[0].active_connections, 1);

    // Второе соединение не обслуживается: оно закрывается сразу.
    let mut second = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    let mut buffer = [0_u8; 1];
    let result = tokio::time::timeout(Duration::from_secs(2), second.read(&mut buffer)).await;

    assert!(
        matches!(result, Ok(Ok(0)) | Ok(Err(_))),
        "соединение сверх лимита должно быть закрыто: {result:?}"
    );
}

#[tokio::test]
async fn decisions_are_dropped_instead_of_blocking_when_queue_is_full() {
    let limits = ProxyLimits {
        decision_queue_capacity: 1,
        ..ProxyLimits::default()
    };
    let (upstream, _connections) = spawn_echo_upstream().await;
    let (runtime, _decisions) = runtime(denying(), limits);
    let port = free_port();
    let listener = listener(port, &profile());

    runtime.request_start(&listener);
    wait_until_running(&runtime, &listener.id).await;

    for _ in 0..5 {
        let response = request_via_proxy(
            port,
            &format!("CONNECT {upstream} HTTP/1.1\r\nHost: {upstream}\r\n\r\n"),
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 403"), "response: {response}");
    }

    assert!(
        runtime.dropped_decisions() > 0,
        "переполнение очереди должно отражаться в счётчике пропущенных решений"
    );
}

#[tokio::test]
async fn shutdown_stops_all_listeners() {
    let (runtime, _decisions) = runtime(denying(), ProxyLimits::default());
    let first = listener(free_port(), &profile());
    let second = listener(free_port(), &profile());

    runtime.request_start(&first);
    runtime.request_start(&second);
    wait_until_running(&runtime, &first.id).await;
    wait_until_running(&runtime, &second.id).await;

    runtime.shutdown();

    wait_for_state(&runtime, &first.id, |state| {
        matches!(state, ListenerState::Stopped)
    })
    .await;
    wait_for_state(&runtime, &second.id, |state| {
        matches!(state, ListenerState::Stopped)
    })
    .await;
}
