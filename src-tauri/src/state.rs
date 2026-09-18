//! Состояние backend, доступное командам IPC.
//!
//! Инициализация выполняется один раз в setup hook, поэтому команды не
//! выполняют инициализацию лениво: подсистема либо готова, либо содержит
//! типизированную ошибку старта, которую UI покажет пользователю.
//!
//! Клонирование handle дешёвое: команды клонируют его, чтобы выполнить работу в
//! блокирующем пуле, не удерживая заимствование managed state.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use egresskeeper_core::{
    CloseBehavior, FsStateDirectory, ListenerService, PolicyService, PolicySource, ProxyConfig,
    ProxyDecision, ProxyRuntime, RepositoryPolicySource, RuntimeInfoService, RuntimeOverview,
    SettingsService, SqliteRepository,
};
use tokio::sync::{mpsc, watch};

use crate::autostart::Autostart;
use crate::ipc::IpcError;
use crate::settings_view::ShellSettingsView;

/// Возвращает handle рантайма приложения.
///
/// Приложение использует один async-рантайм — рантайм Tauri. Handle передаётся в
/// рантайм proxy, поэтому запуск его задач не зависит от того, есть ли контекст
/// рантайма в текущем потоке: команды выполняются в блокирующем пуле.
fn async_runtime_handle() -> tokio::runtime::Handle {
    tokio::runtime::Handle::try_current()
        .unwrap_or_else(|_| tauri::async_runtime::handle().inner().clone())
}

/// Состояние приложения, управляемое Tauri.
#[derive(Debug, Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// Runtime-информация приложения.
    runtime: Result<RuntimeInfoService, IpcError>,
    /// Хранилище политик.
    ///
    /// Ошибка старта хранится уже в виде контракта: причина попадает в лог один
    /// раз при инициализации, а клиент при каждом обращении получает один и тот
    /// же типизированный ответ.
    policy: Result<PolicyService<SqliteRepository>, IpcError>,
    /// Конфигурация listeners proxy; недоступна, если недоступно хранилище.
    listeners: Result<ListenerService<SqliteRepository, ProxyRuntime>, IpcError>,
    /// Настройки приложения; недоступны, если недоступно хранилище.
    settings: Result<SettingsService<SqliteRepository>, IpcError>,
    /// Рантайм proxy; отсутствует, если недоступно хранилище.
    proxy: Option<ProxyRuntime>,
    /// Поток решений для UI; забирается один раз при сборке приложения.
    decisions: Mutex<Option<mpsc::Receiver<ProxyDecision>>>,
    /// Управление автозапуском; появляется после создания окна.
    autostart: Mutex<Option<Arc<dyn Autostart>>>,
    /// Доступен ли значок в трее.
    ///
    /// Если трея нет, окно нельзя скрывать: вернуть его будет нечем.
    tray_available: AtomicBool,
}

impl AppState {
    /// Инициализирует backend.
    ///
    /// Ошибки инициализации не прерывают запуск приложения: они сохраняются и
    /// возвращаются командам как типизированные ошибки, поэтому shell остаётся
    /// работоспособным даже при недоступном хранилище.
    pub fn initialize(state_dir: &Path, app_version: impl Into<String>) -> Self {
        let runtime =
            RuntimeInfoService::initialize(app_version, &FsStateDirectory::new(state_dir))
                .map_err(|error| IpcError::from_domain(&error));

        let repository = SqliteRepository::open_in_state_dir(state_dir).map_err(|error| {
            let ipc_error = IpcError::from_domain(&error);
            tracing::error!(
                code = ipc_error.code,
                "local storage is unavailable; policy and proxy commands will report this error"
            );
            ipc_error
        });

        let (policy, listeners, settings, proxy, decisions) = match repository {
            Ok(repository) => {
                let policy = PolicyService::new(repository.clone());
                let settings = SettingsService::new(repository.clone());
                let source: Arc<dyn PolicySource> =
                    Arc::new(RepositoryPolicySource::new(repository.clone()));
                let (proxy, decisions) =
                    ProxyRuntime::new(ProxyConfig::default(), source, async_runtime_handle());
                let listeners = ListenerService::new(repository, proxy.clone());

                (
                    Ok(policy),
                    Ok(listeners),
                    Ok(settings),
                    Some(proxy),
                    Mutex::new(Some(decisions)),
                )
            }
            Err(error) => (
                Err(error.clone()),
                Err(error.clone()),
                Err(error.clone()),
                None,
                Mutex::new(None),
            ),
        };

        Self {
            inner: Arc::new(Inner {
                runtime,
                policy,
                listeners,
                settings,
                proxy,
                decisions,
                autostart: Mutex::new(None),
                tray_available: AtomicBool::new(false),
            }),
        }
    }

    /// Runtime-информация приложения.
    ///
    /// # Errors
    ///
    /// Возвращает [`IpcError`] с ошибкой инициализации backend.
    pub fn overview(&self) -> Result<RuntimeOverview, IpcError> {
        match &self.inner.runtime {
            Ok(service) => Ok(service.overview()),
            Err(error) => Err(error.clone()),
        }
    }

    /// Сервис политик.
    ///
    /// # Errors
    ///
    /// Возвращает [`IpcError`] с ошибкой открытия хранилища.
    pub fn policy(&self) -> Result<&PolicyService<SqliteRepository>, IpcError> {
        self.inner.policy.as_ref().map_err(Clone::clone)
    }

    /// Сервис конфигурации listeners proxy.
    ///
    /// # Errors
    ///
    /// Возвращает [`IpcError`] с ошибкой открытия хранилища.
    pub fn listeners(&self) -> Result<&ListenerService<SqliteRepository, ProxyRuntime>, IpcError> {
        self.inner.listeners.as_ref().map_err(Clone::clone)
    }

    /// Сервис настроек приложения.
    ///
    /// # Errors
    ///
    /// Возвращает [`IpcError`] с ошибкой открытия хранилища.
    pub fn settings(&self) -> Result<&SettingsService<SqliteRepository>, IpcError> {
        self.inner.settings.as_ref().map_err(Clone::clone)
    }

    /// Возвращает поведение при закрытии главного окна.
    ///
    /// Если настройки недоступны, выбирается завершение: хранилище недоступно
    /// целиком, proxy в таком состоянии политику не применяет, а невидимое
    /// приложение без работающего proxy только запутывает пользователя.
    pub fn close_behavior(&self) -> CloseBehavior {
        let Ok(settings) = self.settings() else {
            return CloseBehavior::Quit;
        };

        match settings.shell_settings() {
            Ok(settings) => settings.close_behavior,
            Err(error) => {
                tracing::warn!(error = %error, "settings are unreadable; quitting on window close");
                CloseBehavior::Quit
            }
        }
    }

    /// Рантайм proxy.
    ///
    /// # Errors
    ///
    /// Возвращает [`IpcError`] с ошибкой открытия хранилища.
    pub fn proxy(&self) -> Result<&ProxyRuntime, IpcError> {
        self.inner
            .proxy
            .as_ref()
            .ok_or_else(|| match &self.inner.listeners {
                Err(error) => error.clone(),
                Ok(_) => IpcError::internal(std::io::Error::other("proxy runtime is missing")),
            })
    }

    /// Забирает поток решений proxy для передачи в UI.
    ///
    /// Поток забирается один раз: получатель у потока один, и владельцем
    /// становится задача-ретранслятор в UI.
    pub fn take_decision_stream(&self) -> Option<mpsc::Receiver<ProxyDecision>> {
        self.inner
            .decisions
            .lock()
            .ok()
            .and_then(|mut stream| stream.take())
    }

    /// Подписка на изменения состояния listeners.
    ///
    /// # Errors
    ///
    /// Возвращает [`IpcError`], если рантайм proxy недоступен.
    pub fn state_changes(&self) -> Result<watch::Receiver<u64>, IpcError> {
        Ok(self.proxy()?.state_changes())
    }

    /// Запускает listeners, включённые в конфигурации.
    ///
    /// Конфигурация сохраняется между запусками, поэтому включённые listeners
    /// должны возобновить работу вместе с приложением.
    pub fn start_enabled_listeners(&self) {
        let Ok(listeners) = self.listeners() else {
            return;
        };

        let Ok(configured) = listeners.list() else {
            return;
        };

        let Some(proxy) = self.inner.proxy.as_ref() else {
            return;
        };

        for listener in configured.into_iter().filter(|listener| listener.enabled) {
            egresskeeper_core::ListenerRuntimeState::request_start(proxy, &listener);
        }
    }

    /// Передаёт управление автозапуском после создания приложения.
    pub fn attach_autostart(&self, autostart: Arc<dyn Autostart>) {
        if let Ok(mut slot) = self.inner.autostart.lock() {
            *slot = Some(autostart);
        }
    }

    /// Настройки оболочки вместе с состоянием платформенных возможностей.
    ///
    /// # Errors
    ///
    /// Возвращает [`IpcError`], если настройки недоступны.
    pub fn shell_settings(&self) -> Result<ShellSettingsView, IpcError> {
        let settings = IpcError::from_result(self.settings()?.shell_settings())?;

        Ok(ShellSettingsView::build(
            settings,
            self.tray_available(),
            self.autostart_state(),
        ))
    }

    /// Меняет поведение при закрытии главного окна.
    ///
    /// # Errors
    ///
    /// Возвращает [`IpcError`], если записать настройку не удалось.
    pub fn set_close_behavior(
        &self,
        behavior: egresskeeper_core::CloseBehavior,
    ) -> Result<ShellSettingsView, IpcError> {
        IpcError::from_result(self.settings()?.set_close_behavior(behavior))?;

        self.shell_settings()
    }

    /// Включает или выключает запуск при входе в систему.
    ///
    /// # Errors
    ///
    /// Возвращает [`IpcError`] с кодом недоступной возможности, если автозапуск
    /// недоступен, или с ошибкой изменения.
    pub fn set_autostart(&self, enabled: bool) -> Result<ShellSettingsView, IpcError> {
        let autostart = self.autostart()?;
        autostart
            .set_enabled(enabled)
            .map_err(|error| IpcError::from_domain(&error))?;

        self.shell_settings()
    }

    /// Возвращает управление автозапуском.
    fn autostart(&self) -> Result<Arc<dyn Autostart>, IpcError> {
        self.inner
            .autostart
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .ok_or_else(|| {
                IpcError::from_domain(
                    &egresskeeper_core::EgressError::platform_feature_unavailable("autostart"),
                )
            })
    }

    /// Возвращает фактическое состояние автозапуска.
    fn autostart_state(&self) -> Option<Result<bool, egresskeeper_core::EgressError>> {
        self.autostart()
            .ok()
            .map(|autostart| autostart.is_enabled())
    }

    /// Сообщает, что значок в трее создан и доступен.
    pub fn set_tray_available(&self, available: bool) {
        self.inner
            .tray_available
            .store(available, Ordering::Relaxed);
    }

    /// Доступен ли значок в трее.
    #[must_use]
    pub fn tray_available(&self) -> bool {
        self.inner.tray_available.load(Ordering::Relaxed)
    }

    /// Состояние для значка трея.
    ///
    /// Собирается из конфигурации listeners и их фактического состояния, поэтому
    /// трей не может показывать не то, что видно в окне.
    ///
    /// # Errors
    ///
    /// Возвращает [`IpcError`], если конфигурация или рантайм недоступны.
    pub fn tray_status(&self) -> Result<crate::tray::TrayStatus, IpcError> {
        let configured = IpcError::from_result(self.listeners()?.list())?;
        let health = self.proxy()?.health();

        Ok(crate::tray::TrayStatus::from_listeners(
            &configured,
            &health,
        ))
    }

    /// Останавливает все listeners при завершении приложения.
    pub fn shutdown_proxy(&self) {
        if let Some(proxy) = &self.inner.proxy {
            proxy.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use egresskeeper_core::ErrorCode;

    use super::*;

    #[test]
    fn ready_state_exposes_runtime_overview_and_policy_service() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let state = AppState::initialize(directory.path(), "0.1.0");

        let overview = state.overview().expect("overview is available");
        assert_eq!(overview.app_version, "0.1.0");
        assert_eq!(
            overview.state_dir,
            directory.path().to_string_lossy(),
            "state directory must come from the composition root"
        );

        let service = state.policy().expect("policy storage is ready");
        assert_eq!(service.list_profiles().expect("profiles").len(), 1);
    }

    #[test]
    fn invalid_app_version_reports_validation_error() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let state = AppState::initialize(directory.path(), "   ");

        let error = state.overview().expect_err("backend is not ready");

        assert_eq!(error.code, ErrorCode::Validation.as_str());
        assert_eq!(
            error.details.map(|details| details.field),
            Some("app_version".to_owned())
        );
    }

    #[test]
    fn shell_stays_usable_when_storage_is_unavailable() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory
            .path()
            .join(egresskeeper_core::infrastructure::sqlite::DATABASE_FILE_NAME);
        std::fs::write(&database, b"this is not a database").expect("garbage database file");

        let state = AppState::initialize(directory.path(), "0.1.0");

        assert!(
            state.overview().is_ok(),
            "runtime info must stay available when only storage fails"
        );

        let error = state.policy().expect_err("storage is unavailable");
        assert_eq!(error.code, ErrorCode::StorageUnavailable.as_str());
        assert_eq!(error.details, None);
    }
}
