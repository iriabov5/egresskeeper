//! Тесты use-cases конфигурации listeners.

mod support;

use std::sync::{Arc, Mutex};

use egresskeeper_core::{ErrorCode, ListenerId, ListenerRuntimeState, ListenerService, ProfileId};
use support::InMemoryListenerRepository;

/// Рантайм-заглушка: запоминает запросы и умеет притворяться работающим.
#[derive(Debug, Clone, Default)]
struct RecordingRuntime {
    calls: Arc<RuntimeCalls>,
}

#[derive(Debug, Default)]
struct RuntimeCalls {
    started: Mutex<Vec<ListenerId>>,
    stopped: Mutex<Vec<ListenerId>>,
    /// Если задано, любой listener считается работающим.
    always_active: bool,
}

impl RecordingRuntime {
    /// Рантайм, в котором listener всегда считается работающим.
    fn always_active() -> Self {
        Self {
            calls: Arc::new(RuntimeCalls {
                always_active: true,
                ..RuntimeCalls::default()
            }),
        }
    }

    fn started(&self) -> Vec<ListenerId> {
        self.calls.started.lock().expect("lock").clone()
    }

    fn stopped(&self) -> Vec<ListenerId> {
        self.calls.stopped.lock().expect("lock").clone()
    }
}

impl ListenerRuntimeState for RecordingRuntime {
    fn is_active(&self, _id: &ListenerId) -> bool {
        self.calls.always_active
    }

    fn request_start(&self, listener: &egresskeeper_core::Listener) {
        self.calls
            .started
            .lock()
            .expect("lock")
            .push(listener.id.clone());
    }

    fn request_stop(&self, id: &ListenerId) {
        self.calls.stopped.lock().expect("lock").push(id.clone());
    }
}

/// Собирает сервис с известным профилем и записывающим рантаймом.
fn service(
    profile: &ProfileId,
    runtime: RecordingRuntime,
) -> ListenerService<InMemoryListenerRepository, RecordingRuntime> {
    ListenerService::new(
        InMemoryListenerRepository::with_profiles(vec![profile.clone()]),
        runtime,
    )
}

#[test]
fn listener_is_created_disabled() {
    let profile = ProfileId::new();
    let service = service(&profile, RecordingRuntime::default());

    let listener = service.create(8787, profile.as_str()).expect("listener");

    assert_eq!(listener.port.get(), 8787);
    assert_eq!(listener.profile_id, profile);
    assert!(
        !listener.enabled,
        "создание не включает listener автоматически"
    );
    assert_eq!(service.list().expect("list").len(), 1);
}

#[test]
fn privileged_port_is_rejected() {
    let profile = ProfileId::new();
    let service = service(&profile, RecordingRuntime::default());

    let error = service
        .create(80, profile.as_str())
        .expect_err("privileged port");

    assert_eq!(error.code(), ErrorCode::Validation);
    assert_eq!(error.invalid_field(), Some("port"));
    assert!(service.list().expect("list").is_empty());
}

#[test]
fn duplicate_port_is_rejected() {
    let profile = ProfileId::new();
    let service = service(&profile, RecordingRuntime::default());
    service.create(8787, profile.as_str()).expect("first");

    let error = service
        .create(8787, profile.as_str())
        .expect_err("duplicate port");

    assert_eq!(error.invalid_field(), Some("port"));
    assert_eq!(service.list().expect("list").len(), 1);
}

#[test]
fn unknown_profile_is_reported_as_not_found() {
    let service = ListenerService::new(
        InMemoryListenerRepository::with_profiles(Vec::new()),
        RecordingRuntime::default(),
    );

    let error = service
        .create(8787, ProfileId::new().as_str())
        .expect_err("unknown profile");

    assert_eq!(error.code(), ErrorCode::NotFound);
}

#[test]
fn invalid_profile_id_is_rejected() {
    let service = ListenerService::new(
        InMemoryListenerRepository::with_profiles(Vec::new()),
        RecordingRuntime::default(),
    );

    let error = service.create(8787, "not-a-uuid").expect_err("invalid id");

    assert_eq!(error.invalid_field(), Some("profile_id"));
}

#[test]
fn enabling_listener_asks_the_runtime_to_start() {
    let profile = ProfileId::new();
    let runtime = RecordingRuntime::default();
    let service = service(&profile, runtime.clone());
    let listener = service.create(8787, profile.as_str()).expect("listener");

    let enabled = service
        .set_enabled(listener.id.as_str(), true)
        .expect("enabled");

    assert!(enabled.enabled);
    assert_eq!(runtime.started(), vec![listener.id]);
    assert!(runtime.stopped().is_empty());
}

#[test]
fn disabling_listener_asks_the_runtime_to_stop() {
    let profile = ProfileId::new();
    let runtime = RecordingRuntime::default();
    let service = service(&profile, runtime.clone());
    let listener = service.create(8787, profile.as_str()).expect("listener");

    let disabled = service
        .set_enabled(listener.id.as_str(), false)
        .expect("disabled");

    assert!(!disabled.enabled);
    assert_eq!(runtime.stopped(), vec![listener.id]);
}

#[test]
fn running_listener_cannot_be_reconfigured() {
    let profile = ProfileId::new();
    let service = service(&profile, RecordingRuntime::always_active());
    let listener = service.create(8787, profile.as_str()).expect("listener");

    let error = service
        .update(listener.id.as_str(), 9999, profile.as_str())
        .expect_err("running listener");

    assert_eq!(error.code(), ErrorCode::Validation);
    assert_eq!(error.invalid_field(), Some("listener_id"));
    assert_eq!(service.list().expect("list")[0].port.get(), 8787);
}

#[test]
fn stopped_listener_can_be_reconfigured() {
    let profile = ProfileId::new();
    let service = service(&profile, RecordingRuntime::default());
    let listener = service.create(8787, profile.as_str()).expect("listener");

    let updated = service
        .update(listener.id.as_str(), 9999, profile.as_str())
        .expect("updated");

    assert_eq!(updated.port.get(), 9999);
}

#[test]
fn reconfiguration_to_used_port_is_rejected() {
    let profile = ProfileId::new();
    let service = service(&profile, RecordingRuntime::default());
    let first = service.create(8787, profile.as_str()).expect("first");
    service.create(9999, profile.as_str()).expect("second");

    let error = service
        .update(first.id.as_str(), 9999, profile.as_str())
        .expect_err("port is taken");

    assert_eq!(error.invalid_field(), Some("port"));
}

#[test]
fn updating_unknown_listener_reports_not_found() {
    let profile = ProfileId::new();
    let service = service(&profile, RecordingRuntime::default());

    let error = service
        .update(ListenerId::new().as_str(), 8787, profile.as_str())
        .expect_err("unknown listener");

    assert_eq!(error.code(), ErrorCode::NotFound);
}

#[test]
fn deleting_listener_stops_it() {
    let profile = ProfileId::new();
    let runtime = RecordingRuntime::default();
    let service = service(&profile, runtime.clone());
    let listener = service.create(8787, profile.as_str()).expect("listener");

    service.delete(listener.id.as_str()).expect("deleted");

    assert_eq!(runtime.stopped(), vec![listener.id]);
    assert!(service.list().expect("list").is_empty());
}

#[test]
fn deleting_unknown_listener_reports_not_found_without_stopping_anything() {
    let profile = ProfileId::new();
    let runtime = RecordingRuntime::default();
    let service = service(&profile, runtime.clone());

    let error = service
        .delete(ListenerId::new().as_str())
        .expect_err("unknown listener");

    assert_eq!(error.code(), ErrorCode::NotFound);
    assert!(runtime.stopped().is_empty());
}

#[test]
fn profile_existence_is_checked_through_the_repository() {
    let profile = ProfileId::new();
    let repository = InMemoryListenerRepository::with_profiles(vec![profile.clone()]);
    let service = ListenerService::new(repository, RecordingRuntime::default());

    assert!(service.create(8787, profile.as_str()).is_ok());
    assert_eq!(
        service
            .create(9999, ProfileId::new().as_str())
            .expect_err("unknown profile")
            .code(),
        ErrorCode::NotFound
    );
}
