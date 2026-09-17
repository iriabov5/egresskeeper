//! Composition root: сборка приложения, инициализация backend и регистрация IPC.
//!
//! Модуль отвечает только за wiring: резолв платформенных путей, инициализацию
//! ядра, управление состоянием, создание главного окна, отправку события
//! готовности и регистрацию команд. Правила поведения живут в
//! `egresskeeper-core`.

mod autostart;
mod commands;
mod ipc;
mod lifecycle;
mod navigation;
pub mod proxy_view;
mod settings_view;
mod state;
mod tray;

use std::sync::Arc;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub use commands::{
    get_runtime_overview, policy_add_rule, policy_create_profile, policy_delete_profile,
    policy_delete_rule, policy_evaluate, policy_list_profiles, policy_profile_detail,
    policy_rename_profile, policy_reorder_rules, policy_set_default_action, policy_update_rule,
    proxy_create_listener, proxy_delete_listener, proxy_set_listener_enabled, proxy_status,
    proxy_update_listener, settings_get, settings_set_autostart, settings_set_close_behavior,
};
pub use ipc::{IpcError, IpcErrorDetails};
pub use proxy_view::{ListenerStateView, ListenerView, ProxyRuntimeView, ProxyStatusView};

/// Собирает состояние proxy для тестов и внутренних задач.
///
/// Обёртка нужна, чтобы интеграционные тесты проверяли то же представление,
/// которое отдают команды.
#[doc(hidden)]
#[must_use]
pub fn proxy_view_for_tests(
    listeners: Vec<egresskeeper_core::Listener>,
    runtime: &egresskeeper_core::ProxyRuntime,
) -> ProxyStatusView {
    proxy_view::build_status(listeners, runtime)
}
pub use settings_view::ShellSettingsView;
pub use state::AppState;

/// Событие с решением proxy.
///
/// Событие ускоряет отображение: состояние всегда можно перечитать командой
/// `proxy_status`, поэтому потеря события не приводит к неверному экрану.
pub const PROXY_DECISION_EVENT: &str = "proxy://decision";

/// Событие изменения состояния listeners.
pub const PROXY_RUNTIME_EVENT: &str = "proxy://runtime";

/// Событие готовности backend.
///
/// Событие может прийти раньше, чем UI успеет подписаться, поэтому UI обязан
/// также запрашивать состояние командой — оба пути ведут к одному результату.
pub const RUNTIME_READY_EVENT: &str = "runtime://ready";

/// Метка главного окна; на неё же нацелена capability-политика.
const MAIN_WINDOW_LABEL: &str = "main";

/// Запускает desktop-приложение.
///
/// Завершает процесс с ненулевым кодом, если окно приложения не удалось поднять:
/// молчаливый выход без диагностики неприемлем для desktop-продукта.
pub fn run() {
    init_tracing();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            // Состояние кладётся в managed state до создания окна: иначе команда,
            // вызванная сразу после загрузки UI, может не найти состояние.
            let state_dir = app.path().app_data_dir()?;
            let state = AppState::initialize(&state_dir, app.package_info().version.to_string());
            let ready_payload = state.overview().ok();

            if ready_payload.is_none() {
                tracing::error!("backend initialization failed; UI will receive a typed error");
            }

            app.manage(state);
            create_main_window(app.handle())?;

            let state = app.state::<AppState>().inner().clone();
            spawn_decision_forwarder(app.handle(), &state);
            spawn_runtime_forwarder(app.handle(), &state);
            state.attach_autostart(std::sync::Arc::new(autostart::SystemAutostart::new(
                app.handle().clone(),
            )));
            spawn_tray(app.handle(), &state);
            state.start_enabled_listeners();

            if let Some(overview) = ready_payload {
                app.emit(RUNTIME_READY_EVENT, overview)?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                return;
            };

            let state = window.app_handle().state::<AppState>().inner().clone();

            // Скрыть окно можно только тогда, когда его есть чем вернуть, то есть
            // когда значок в трее создан.
            match lifecycle::close_action(state.close_behavior(), state.tray_available()) {
                lifecycle::CloseAction::Hide => {
                    api.prevent_close();

                    if let Err(error) = window.hide() {
                        tracing::warn!(error = %error, "failed to hide the main window");
                    }
                }
                lifecycle::CloseAction::Quit => {
                    tracing::info!("application is shutting down on window close");
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::runtime::get_runtime_overview,
            commands::policy::policy_list_profiles,
            commands::policy::policy_profile_detail,
            commands::policy::policy_create_profile,
            commands::policy::policy_rename_profile,
            commands::policy::policy_set_default_action,
            commands::policy::policy_delete_profile,
            commands::policy::policy_add_rule,
            commands::policy::policy_update_rule,
            commands::policy::policy_delete_rule,
            commands::policy::policy_reorder_rules,
            commands::policy::policy_evaluate,
            commands::proxy::proxy_status,
            commands::proxy::proxy_create_listener,
            commands::proxy::proxy_update_listener,
            commands::proxy::proxy_set_listener_enabled,
            commands::proxy::proxy_delete_listener,
            commands::settings::settings_get,
            commands::settings::settings_set_close_behavior,
            commands::settings::settings_set_autostart,
        ])
        .build(tauri::generate_context!());

    match app {
        Ok(app) => app.run(|app_handle, event| {
            // Завершение приложения обязано остановить listeners: иначе proxy
            // продолжил бы принимать соединения до конца процесса.
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) && let Some(state) = app_handle.try_state::<AppState>()
            {
                state.shutdown_proxy();
            }
        }),
        Err(error) => {
            tracing::error!(error = %error, "desktop shell terminated with an error");
            std::process::exit(1);
        }
    }
}

/// Создаёт значок трея и поддерживает его состояние.
///
/// Недоступность трея не является ошибкой запуска: приложение продолжает
/// работать, а окно в этом случае нельзя скрыть — иначе вернуть его будет нечем.
fn spawn_tray(app: &tauri::AppHandle, state: &AppState) {
    let status = state.tray_status().unwrap_or_default();
    let app_handle = app.clone();
    let state_for_actions = state.clone();

    let on_action = move |action: tray::TrayAction| match action {
        tray::TrayAction::Open => {
            if let Err(error) = tray::TrayHandles::show_window(&app_handle) {
                tracing::warn!(error = %error, "failed to show the main window");
            }
        }
        tray::TrayAction::StartListeners => state_for_actions.start_enabled_listeners(),
        tray::TrayAction::StopListeners => state_for_actions.shutdown_proxy(),
        tray::TrayAction::Quit => app_handle.exit(0),
    };

    let handles = match tray::create(app, &status, on_action) {
        Ok(handles) => Arc::new(handles),
        Err(error) => {
            tracing::warn!(
                error = %error,
                "tray is unavailable; the window will not be hidden on close"
            );

            return;
        }
    };

    state.set_tray_available(true);

    let Ok(mut changes) = state.state_changes() else {
        return;
    };

    let state_for_updates = state.clone();

    tauri::async_runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            let state = state_for_updates.clone();
            let status =
                match tauri::async_runtime::spawn_blocking(move || state.tray_status()).await {
                    Ok(Ok(status)) => status,
                    Ok(Err(error)) => {
                        tracing::warn!(code = error.code, "tray state is unavailable");
                        tray::TrayStatus::default()
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "tray state task failed");
                        tray::TrayStatus::default()
                    }
                };

            if let Err(error) = handles.apply(&status) {
                tracing::warn!(error = %error, "failed to update the tray");
            }
        }
    });
}

/// Передаёт решения proxy в UI.
///
/// Поток решений ограничен по ёмкости, поэтому медленный UI не тормозит
/// обработку соединений: пропущенные решения учитываются счётчиком.
fn spawn_decision_forwarder(app: &tauri::AppHandle, state: &AppState) {
    let Some(mut decisions) = state.take_decision_stream() else {
        tracing::warn!("decision stream is unavailable; ui will not receive proxy decisions");
        return;
    };

    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        while let Some(decision) = decisions.recv().await {
            if let Err(error) = app.emit(PROXY_DECISION_EVENT, &decision) {
                tracing::debug!(error = %error, "failed to emit proxy decision");
            }
        }
    });
}

/// Передаёт в UI изменения состояния listeners.
fn spawn_runtime_forwarder(app: &tauri::AppHandle, state: &AppState) {
    let Ok(mut changes) = state.state_changes() else {
        return;
    };
    let Ok(proxy) = state.proxy().cloned() else {
        return;
    };

    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            let view = proxy_view::build_runtime_view(&proxy);

            if let Err(error) = app.emit(PROXY_RUNTIME_EVENT, view) {
                tracing::debug!(error = %error, "failed to emit proxy runtime state");
            }
        }
    });
}

/// Создаёт главное окно с явной политикой навигации.
///
/// Параметры окна задаются здесь, а не в `tauri.conf.json`: navigation handler
/// привязан к созданию webview, и политика навигации должна применяться к окну
/// гарантированно, а не зависеть от поведения webview по умолчанию.
fn create_main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::default())
        .title("EgressKeeper")
        .inner_size(1100.0, 720.0)
        .min_inner_size(900.0, 600.0)
        .resizable(true)
        .on_navigation(navigation::is_allowed)
        .build()?;

    Ok(())
}

/// Инициализирует логирование.
///
/// Вывод идёт в stdout: файловые appender'ы, ротация и просмотр логов в UI —
/// тема отдельного change, когда появится proxy и станет понятен объём событий.
fn init_tracing() {
    let max_level = if cfg!(debug_assertions) {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    let _ = tracing_subscriber::fmt()
        .with_max_level(max_level)
        .try_init();
}
