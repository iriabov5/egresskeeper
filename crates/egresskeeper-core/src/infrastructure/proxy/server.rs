//! Приём соединений и обработка запросов proxy.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::header::{
    CONNECTION, HOST, HeaderMap, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE, TRAILER,
    TRANSFER_ENCODING, UPGRADE,
};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Handle;
use tokio::sync::{Semaphore, watch};
use tokio::task::{AbortHandle, JoinHandle, JoinSet};
use tokio::time::timeout;

use super::{Inner, ListenerFailure, ListenerState, ProxyConfig, ProxyLimits};
use crate::application::ports::PolicySource;
use crate::domain::error::ErrorCode;
use crate::domain::policy::decision::EvaluationTarget;
use crate::domain::policy::entities::ProfileId;
use crate::domain::proxy::listener::{ListenerId, ProxyPort};
use crate::domain::proxy::outcome::{PolicySnapshot, ProxyDecisionReason, decide};

/// Тело ответа proxy.
type ProxyBody = BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>;

/// Ответ при запрете соединения политикой.
const DENIED_BODY: &[u8] = b"EgressKeeper: connection denied by policy\n";
/// Ответ, когда политику не удалось применить.
const POLICY_UNAVAILABLE_BODY: &[u8] = b"EgressKeeper: policy is unavailable, connection denied\n";
/// Ответ, когда цель соединения не разобрана.
const BAD_TARGET_BODY: &[u8] = b"EgressKeeper: proxy requires an absolute URI or CONNECT\n";
/// Ответ, когда целевой сервер недоступен.
const BAD_GATEWAY_BODY: &[u8] = b"EgressKeeper: upstream connection failed\n";

/// Реестр задач туннелей, порождённых listener'ом.
///
/// Туннель `CONNECT` живёт дольше обработки запроса, поэтому он не может быть
/// частью задачи соединения. Реестр делает listener владельцем этих задач:
/// остановка listener'а завершает и их, иначе соединение осталось бы висеть.
#[derive(Debug, Default)]
struct TunnelRegistry {
    handles: Mutex<Vec<AbortHandle>>,
}

impl TunnelRegistry {
    /// Регистрирует задачу туннеля, попутно забывая завершённые.
    fn register(&self, handle: AbortHandle) {
        if let Ok(mut handles) = self.handles.lock() {
            handles.retain(|existing| !existing.is_finished());
            handles.push(handle);
        }
    }

    /// Завершает все зарегистрированные туннели.
    fn abort_all(&self) {
        if let Ok(mut handles) = self.handles.lock() {
            for handle in handles.drain(..) {
                handle.abort();
            }
        }
    }
}

/// Параметры задачи listener'а.
pub(super) struct ListenerContext {
    pub(super) listener_id: ListenerId,
    pub(super) profile_id: ProfileId,
    pub(super) port: ProxyPort,
    pub(super) config: ProxyConfig,
    pub(super) policy: Arc<dyn PolicySource>,
    pub(super) inner: Arc<Inner>,
    pub(super) runtime: Handle,
    pub(super) state: Arc<Mutex<ListenerState>>,
    pub(super) active: Arc<AtomicU32>,
    pub(super) shutdown: watch::Receiver<bool>,
    pub(super) predecessor: Option<JoinHandle<()>>,
}

/// Данные, нужные при обработке одного соединения.
#[derive(Clone)]
struct ConnectionContext {
    listener_id: ListenerId,
    profile_id: ProfileId,
    port: ProxyPort,
    limits: ProxyLimits,
    policy: Arc<dyn PolicySource>,
    inner: Arc<Inner>,
    client: Client<HttpConnector, Incoming>,
    tunnels: Arc<TunnelRegistry>,
    runtime: Handle,
}

/// Запускает задачу listener'а.
pub(super) fn spawn_listener(context: ListenerContext) -> JoinHandle<()> {
    let runtime = context.runtime.clone();

    runtime.spawn(run_listener(context))
}

/// Принимает соединения, пока не придёт сигнал остановки.
async fn run_listener(context: ListenerContext) {
    // Предыдущая задача того же listener'а должна освободить порт до новой
    // попытки привязки, иначе быстрый цикл «выключить-включить» упирался бы в
    // занятый порт.
    if let Some(previous) = context.predecessor {
        let _ = previous.await;
    }

    let address = SocketAddr::new(context.config.bind_address(), context.port.get());
    let listener = match TcpListener::bind(address).await {
        Ok(listener) => listener,
        Err(source) => {
            tracing::error!(listener = %context.listener_id, error = %source, "proxy listener failed to bind");
            set_state(
                &context.state,
                &context.inner,
                ListenerState::Failed(bind_failure(&source)),
            );
            return;
        }
    };

    set_state(&context.state, &context.inner, ListenerState::Running);
    tracing::info!(listener = %context.listener_id, port = %context.port, "proxy listener is running");

    let limits = context.config.limits();
    let semaphore = Arc::new(Semaphore::new(limits.max_connections_per_listener));
    let client = build_client(limits);
    let mut connections: JoinSet<()> = JoinSet::new();
    let tunnels = Arc::new(TunnelRegistry::default());
    let mut shutdown = context.shutdown.clone();

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, peer) = match accepted {
                    Ok(accepted) => accepted,
                    Err(source) => {
                        tracing::warn!(listener = %context.listener_id, error = %source, "accept failed");
                        continue;
                    }
                };

                // Страховка: даже если сокет оказался доступен не только локально,
                // соединения извне не обслуживаются.
                if !peer.ip().is_loopback() {
                    tracing::warn!(%peer, "rejecting non-loopback proxy client");
                    continue;
                }

                let Ok(permit) = Arc::clone(&semaphore).try_acquire_owned() else {
                    tracing::warn!(listener = %context.listener_id, "connection limit reached; closing connection");
                    continue;
                };

                let connection = ConnectionContext {
                    listener_id: context.listener_id.clone(),
                    profile_id: context.profile_id.clone(),
                    port: context.port,
                    limits,
                    policy: Arc::clone(&context.policy),
                    inner: Arc::clone(&context.inner),
                    client: client.clone(),
                    tunnels: Arc::clone(&tunnels),
                    runtime: context.runtime.clone(),
                };
                let active = Arc::clone(&context.active);
                active.fetch_add(1, Ordering::Relaxed);

                connections.spawn(async move {
                    let _permit = permit;
                    serve_connection(stream, connection).await;
                    active.fetch_sub(1, Ordering::Relaxed);
                });
            }
        }
    }

    // Активные соединения и туннели прекращаются вместе с listener'ом: listener
    // владеет всем, что породил.
    tunnels.abort_all();
    connections.abort_all();
    while connections.join_next().await.is_some() {}

    set_state(&context.state, &context.inner, ListenerState::Stopped);
    tracing::info!(listener = %context.listener_id, "proxy listener stopped");
}

/// Обслуживает одно соединение клиента.
async fn serve_connection(stream: TcpStream, context: ConnectionContext) {
    let header_read_timeout = context.limits.header_read_timeout;
    let io = TokioIo::new(stream);

    let service = service_fn(move |request: Request<Incoming>| {
        let context = context.clone();

        async move { handle_request(request, context).await }
    });

    let connection = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(header_read_timeout)
        .serve_connection(io, service)
        .with_upgrades();

    if let Err(error) = connection.await {
        tracing::debug!(error = %error, "proxy connection finished with an error");
    }
}

/// Обрабатывает один запрос клиента.
async fn handle_request(
    request: Request<Incoming>,
    context: ConnectionContext,
) -> Result<Response<ProxyBody>, Infallible> {
    let Some(target) = request_target(&request) else {
        // Решения нет: у запроса не определена цель, поэтому решать нечего.
        // Клиент получает понятный отказ, а факт фиксируется в логе.
        tracing::warn!(
            listener = %context.listener_id,
            method = %request.method(),
            uri = %request.uri(),
            "proxy rejected a request without a target"
        );

        return Ok(response(StatusCode::BAD_REQUEST, BAD_TARGET_BODY));
    };

    let snapshot = load_snapshot(&context, &target).await;
    let decision = decide(&context.listener_id, context.port.get(), &snapshot, &target);
    let denied = decision.is_denied();
    let policy_unavailable = decision.reason == ProxyDecisionReason::PolicyUnavailable;
    context.inner.publish(decision);

    if denied {
        return Ok(if policy_unavailable {
            response(StatusCode::FORBIDDEN, POLICY_UNAVAILABLE_BODY)
        } else {
            response(StatusCode::FORBIDDEN, DENIED_BODY)
        });
    }

    if request.method() == Method::CONNECT {
        Ok(handle_connect(request, target, &context).await)
    } else {
        Ok(handle_forward(request, target, &context).await)
    }
}

/// Устанавливает туннель к цели по `CONNECT`.
async fn handle_connect(
    request: Request<Incoming>,
    target: EvaluationTarget,
    context: &ConnectionContext,
) -> Response<ProxyBody> {
    let address = authority(&target, target.port());

    // Соединение с целью устанавливается до ответа клиенту: иначе клиент считал
    // бы туннель установленным, хотя цели недостижимы.
    let upstream = match timeout(context.limits.connect_timeout, TcpStream::connect(&address)).await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(source)) => {
            tracing::debug!(target = %address, error = %source, "upstream connection failed");
            return response(StatusCode::BAD_GATEWAY, BAD_GATEWAY_BODY);
        }
        Err(_) => {
            tracing::debug!(target = %address, "upstream connection timed out");
            return response(StatusCode::BAD_GATEWAY, BAD_GATEWAY_BODY);
        }
    };

    let tunnel_task = context.runtime.clone().spawn(async move {
        match hyper::upgrade::on(request).await {
            Ok(upgraded) => {
                if let Err(error) = tunnel(upgraded, upstream).await {
                    tracing::debug!(error = %error, "proxy tunnel finished with an error");
                }
            }
            Err(error) => {
                tracing::debug!(error = %error, "proxy tunnel upgrade failed");
            }
        }
    });
    context.tunnels.register(tunnel_task.abort_handle());

    Response::builder()
        .status(StatusCode::OK)
        .body(empty_body())
        .unwrap_or_else(|_| response(StatusCode::INTERNAL_SERVER_ERROR, BAD_GATEWAY_BODY))
}

/// Пересылает обычный HTTP-запрос целевому серверу.
async fn handle_forward(
    request: Request<Incoming>,
    target: EvaluationTarget,
    context: &ConnectionContext,
) -> Response<ProxyBody> {
    let Some(upstream_request) = build_upstream_request(request, &target) else {
        return response(StatusCode::BAD_REQUEST, BAD_TARGET_BODY);
    };

    match context.client.request(upstream_request).await {
        Ok(response) => {
            let (mut parts, body) = response.into_parts();
            strip_hop_by_hop(&mut parts.headers);

            Response::from_parts(
                parts,
                body.map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
                    .boxed(),
            )
        }
        Err(error) => {
            tracing::debug!(target = %authority(&target, target.port()), error = %error, "upstream request failed");
            response(StatusCode::BAD_GATEWAY, BAD_GATEWAY_BODY)
        }
    }
}

/// Передаёт байты между клиентом и целевым сервером.
async fn tunnel(upgraded: Upgraded, upstream: TcpStream) -> std::io::Result<()> {
    let mut upgraded = TokioIo::new(upgraded);
    let mut upstream = upstream;

    tokio::io::copy_bidirectional(&mut upgraded, &mut upstream).await?;

    Ok(())
}

/// Разбирает цель соединения из запроса.
fn request_target<B>(request: &Request<B>) -> Option<EvaluationTarget> {
    if request.method() == Method::CONNECT {
        let authority = request.uri().authority()?;
        let host = normalize_authority_host(authority.host());
        let port = authority.port_u16().unwrap_or(443);

        return EvaluationTarget::parse(&host, port).ok();
    }

    let uri = request.uri();
    let host = normalize_authority_host(uri.host()?);
    let port = uri.port_u16().unwrap_or(80);

    EvaluationTarget::parse(&host, port).ok()
}

/// Приводит host из authority к виду, который понимает нормализация.
///
/// IPv6-литералы в authority записываются в квадратных скобках; если библиотека
/// вернула их без скобок, скобки возвращаются на место.
fn normalize_authority_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        return format!("[{host}]");
    }

    host.to_owned()
}

/// Строит запрос к целевому серверу.
fn build_upstream_request<B>(
    mut request: Request<B>,
    target: &EvaluationTarget,
) -> Option<Request<B>> {
    let path = request
        .uri()
        .path_and_query()
        .map_or_else(|| "/".to_owned(), |path| path.as_str().to_owned());

    let uri = Uri::builder()
        .scheme("http")
        .authority(authority(target, target.port()))
        .path_and_query(path)
        .build()
        .ok()?;

    strip_hop_by_hop(request.headers_mut());

    if !request.headers().contains_key(HOST) {
        let host = authority(target, target.port());
        let value = host.parse().ok()?;
        request.headers_mut().insert(HOST, value);
    }

    *request.uri_mut() = uri;

    Some(request)
}

/// Удаляет заголовки, относящиеся к конкретному соединению с proxy.
///
/// Эти заголовки адресованы proxy, а не целевому серверу; их пересылка нарушает
/// протокол и раскрывает целевому серверу лишние детали.
fn strip_hop_by_hop(headers: &mut HeaderMap) {
    for name in [
        CONNECTION,
        PROXY_AUTHENTICATE,
        PROXY_AUTHORIZATION,
        TE,
        TRAILER,
        TRANSFER_ENCODING,
        UPGRADE,
    ] {
        headers.remove(name);
    }

    for name in ["keep-alive", "proxy-connection"] {
        headers.remove(name);
    }
}

/// Собирает authority для целевого сервера.
fn authority(target: &EvaluationTarget, port: u16) -> String {
    format!("{}:{port}", target.host().as_str())
}

/// Читает снимок политики профиля listener'а.
///
/// Обращение к хранилищу блокирующее, поэтому выполняется в блокирующем пуле:
/// обработка соединений не должна ждать ввода-вывода базы.
async fn load_snapshot(context: &ConnectionContext, target: &EvaluationTarget) -> PolicySnapshot {
    let policy = Arc::clone(&context.policy);
    let profile_id = context.profile_id.clone();
    let host = target.host().as_str().to_owned();

    let snapshot = tokio::task::spawn_blocking(move || policy.snapshot(&profile_id)).await;

    match snapshot {
        Ok(Ok(Some(snapshot))) => snapshot,
        Ok(Ok(None)) => {
            tracing::debug!(host = %host, "policy profile is missing; connection will be denied");
            PolicySnapshot::Unavailable
        }
        Ok(Err(error)) => {
            tracing::error!(error = %error, "policy snapshot failed; connection will be denied");
            PolicySnapshot::Unavailable
        }
        Err(error) => {
            tracing::error!(error = %error, "policy snapshot task failed");
            PolicySnapshot::Unavailable
        }
    }
}

/// Создаёт HTTP-клиент для пересылки запросов.
fn build_client(limits: ProxyLimits) -> Client<HttpConnector, Incoming> {
    let mut connector = HttpConnector::new();
    connector.set_connect_timeout(Some(limits.connect_timeout));
    connector.set_nodelay(true);
    connector.enforce_http(false);

    Client::builder(TokioExecutor::new()).build(connector)
}

/// Строит простой ответ с заданным статусом и телом.
fn response(status: StatusCode, body: &'static [u8]) -> Response<ProxyBody> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(
            Full::new(Bytes::from_static(body))
                .map_err(|never| match never {})
                .boxed(),
        )
        .unwrap_or_else(|_| Response::new(empty_body()))
}

/// Пустое тело ответа.
fn empty_body() -> ProxyBody {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

/// Записывает состояние listener'а и уведомляет подписчиков.
fn set_state(state: &Mutex<ListenerState>, inner: &Inner, value: ListenerState) {
    if let Ok(mut guard) = state.lock() {
        *guard = value;
    }

    inner.notify_state_change();
}

/// Преобразует ошибку привязки в наблюдаемый отказ listener'а.
fn bind_failure(source: &std::io::Error) -> ListenerFailure {
    let message = if source.kind() == std::io::ErrorKind::AddrInUse {
        "Порт уже занят другой программой."
    } else {
        "Не удалось запустить listener proxy."
    };

    ListenerFailure {
        code: ErrorCode::PortUnavailable,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Тестовое тело запроса: конкретный тип тела здесь не важен, проверяются
    /// метод, URI и заголовки.
    type TestBody = Empty<Bytes>;

    fn connect_request(target: &str) -> Request<TestBody> {
        Request::builder()
            .method(Method::CONNECT)
            .uri(target)
            .body(Empty::new())
            .expect("request")
    }

    fn get_request(target: &str) -> Request<TestBody> {
        Request::builder()
            .method(Method::GET)
            .uri(target)
            .body(Empty::new())
            .expect("request")
    }

    #[test]
    fn connect_target_is_parsed_with_default_port() {
        let target = request_target(&connect_request("api.example.com:443")).expect("target");

        assert_eq!(target.host().as_str(), "api.example.com");
        assert_eq!(target.port(), 443);
    }

    #[test]
    fn connect_target_without_port_uses_tls_default() {
        let target = request_target(&connect_request("api.example.com")).expect("target");

        assert_eq!(target.port(), 443);
    }

    #[test]
    fn connect_target_supports_ipv6_literal() {
        let target = request_target(&connect_request("[::1]:8443")).expect("target");

        assert_eq!(target.host().as_str(), "[::1]");
        assert_eq!(target.port(), 8443);
    }

    #[test]
    fn absolute_http_target_is_parsed() {
        let target = request_target(&get_request("http://api.example.com/path")).expect("target");

        assert_eq!(target.host().as_str(), "api.example.com");
        assert_eq!(target.port(), 80);
    }

    #[test]
    fn origin_form_request_has_no_target() {
        assert!(request_target(&get_request("/path")).is_none());
    }

    #[test]
    fn upstream_request_uses_origin_form_and_keeps_host() {
        let target = EvaluationTarget::parse("api.example.com", 80).expect("target");
        let request = get_request("http://api.example.com/some/path?query=1");

        let upstream = build_upstream_request(request, &target).expect("upstream request");

        assert_eq!(upstream.uri().scheme_str(), Some("http"));
        assert_eq!(
            upstream.uri().authority().map(|value| value.as_str()),
            Some("api.example.com:80")
        );
        assert_eq!(
            upstream.uri().path_and_query().map(|v| v.as_str()),
            Some("/some/path?query=1")
        );
        assert_eq!(
            upstream
                .headers()
                .get(HOST)
                .and_then(|value| value.to_str().ok()),
            Some("api.example.com:80")
        );
    }

    #[test]
    fn hop_by_hop_headers_are_removed() {
        let target = EvaluationTarget::parse("api.example.com", 80).expect("target");
        let request = Request::builder()
            .method(Method::GET)
            .uri("http://api.example.com/")
            .header("proxy-connection", "keep-alive")
            .header(CONNECTION, "keep-alive")
            .header("x-custom", "kept")
            .body(Empty::<Bytes>::new())
            .expect("request");

        let upstream = build_upstream_request(request, &target).expect("upstream request");

        assert!(upstream.headers().get("proxy-connection").is_none());
        assert!(upstream.headers().get(CONNECTION).is_none());
        assert_eq!(
            upstream
                .headers()
                .get("x-custom")
                .and_then(|value| value.to_str().ok()),
            Some("kept")
        );
    }

    #[test]
    fn authority_keeps_ipv6_brackets() {
        let target = EvaluationTarget::parse("[::1]", 8080).expect("target");

        assert_eq!(authority(&target, target.port()), "[::1]:8080");
    }

    #[test]
    fn bind_failure_reports_port_code() {
        let failure = bind_failure(&std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            "address in use",
        ));

        assert_eq!(failure.code, ErrorCode::PortUnavailable);
        assert!(failure.message.contains("занят"));
    }
}
