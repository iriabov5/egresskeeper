//! Значок приложения в системном трее.
//!
//! Трей — способ управлять proxy без окна: он показывает состояние и позволяет
//! открыть окно, запустить или остановить listeners и завершить приложение.
//! Решения о том, что показывать, вынесены в чистые функции: сам значок тестом не
//! покрыть, а содержимое подсказки и доступность пунктов меню — можно.

use egresskeeper_core::{Listener, ListenerHealth, ListenerState};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

/// Идентификатор пункта меню «Открыть».
const MENU_OPEN: &str = "tray.open";
/// Идентификатор пункта меню «Запустить listeners».
const MENU_START: &str = "tray.start_listeners";
/// Идентификатор пункта меню «Остановить listeners».
const MENU_STOP: &str = "tray.stop_listeners";
/// Идентификатор пункта меню «Выход».
const MENU_QUIT: &str = "tray.quit";

/// Идентификатор значка.
const TRAY_ID: &str = "main";

/// Действие, выбранное пользователем в трее.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Открыть главное окно.
    Open,
    /// Запустить listeners, включённые в конфигурации.
    StartListeners,
    /// Остановить все listeners.
    StopListeners,
    /// Завершить приложение.
    Quit,
}

/// Состояние, которое показывает трей.
///
/// Значение по умолчанию — «ничего не настроено»: оно используется, когда
/// настройки или listeners недоступны, чтобы трей остался работоспособным.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrayStatus {
    /// Сколько listeners принимает соединения.
    pub running_listeners: usize,
    /// Сколько listeners включено в конфигурации.
    pub enabled_listeners: usize,
}

impl TrayStatus {
    /// Собирает состояние из конфигурации и фактического состояния listeners.
    #[must_use]
    pub fn from_listeners(configured: &[Listener], health: &[ListenerHealth]) -> Self {
        Self {
            running_listeners: health
                .iter()
                .filter(|health| health.state == ListenerState::Running)
                .count(),
            enabled_listeners: configured
                .iter()
                .filter(|listener| listener.enabled)
                .count(),
        }
    }

    /// Подсказка значка.
    #[must_use]
    pub fn tooltip(&self) -> String {
        match (self.running_listeners, self.enabled_listeners) {
            (0, 0) => "EgressKeeper: proxy не слушает соединения".to_owned(),
            (0, enabled) => {
                format!("EgressKeeper: proxy остановлен, включено listeners: {enabled}")
            }
            (1, _) => "EgressKeeper: работает 1 listener".to_owned(),
            (running, _) => format!("EgressKeeper: работает listeners: {running}"),
        }
    }

    /// Доступен ли запуск listeners из трея.
    #[must_use]
    pub const fn can_start(&self) -> bool {
        self.enabled_listeners > 0 && self.running_listeners == 0
    }

    /// Доступна ли остановка listeners из трея.
    #[must_use]
    pub const fn can_stop(&self) -> bool {
        self.running_listeners > 0
    }
}

/// Пункты меню трея, созданные при запуске.
///
/// `Debug` не выводится: типы меню Tauri его не реализуют.
pub struct TrayHandles {
    start: MenuItem<Wry>,
    stop: MenuItem<Wry>,
    status: MenuItem<Wry>,
}

impl TrayHandles {
    /// Приводит пункты меню в соответствие состоянию.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку, если пункты меню больше недоступны.
    pub fn apply(&self, status: &TrayStatus) -> tauri::Result<()> {
        self.status.set_text(status.tooltip())?;
        self.start.set_enabled(status.can_start())?;
        self.stop.set_enabled(status.can_stop())?;

        Ok(())
    }

    /// Показывает окно и делает его активным.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку, если окно не найдено или не удалось его показать.
    pub fn show_window(app: &AppHandle) -> tauri::Result<()> {
        let Some(window) = app.get_webview_window(crate::MAIN_WINDOW_LABEL) else {
            return Err(tauri::Error::WindowNotFound);
        };

        window.show()?;
        window.unminimize()?;
        window.set_focus()?;

        Ok(())
    }
}

/// Создаёт значок трея.
///
/// # Errors
///
/// Возвращает ошибку, если платформа не поддерживает трей или значок создать не
/// удалось: вызывающая сторона обязана продолжить работу без трея.
pub fn create(
    app: &AppHandle,
    status: &TrayStatus,
    on_action: impl Fn(TrayAction) + Send + Sync + 'static,
) -> tauri::Result<TrayHandles> {
    let status_item = MenuItem::with_id(app, "tray.status", status.tooltip(), false, None::<&str>)?;
    let open = MenuItem::with_id(app, MENU_OPEN, "Открыть EgressKeeper", true, None::<&str>)?;
    let start = MenuItem::with_id(
        app,
        MENU_START,
        "Запустить listeners",
        status.can_start(),
        None::<&str>,
    )?;
    let stop = MenuItem::with_id(
        app,
        MENU_STOP,
        "Остановить listeners",
        status.can_stop(),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Выход", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[&status_item, &separator, &open, &start, &stop, &quit],
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(status.tooltip())
        .show_menu_on_left_click(true)
        .on_menu_event(move |_app, event| {
            let action = match event.id().as_ref() {
                MENU_OPEN => Some(TrayAction::Open),
                MENU_START => Some(TrayAction::StartListeners),
                MENU_STOP => Some(TrayAction::StopListeners),
                MENU_QUIT => Some(TrayAction::Quit),
                _ => None,
            };

            if let Some(action) = action {
                on_action(action);
            }
        });

    // Значок — монохромный и помеченный как template: macOS сама подбирает цвет
    // под тему оформления.
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder = builder.icon_as_template(true);
    builder.build(app)?;

    // Пункт «Открыть» не нужно обновлять: он всегда доступен.
    let _ = &open;

    Ok(TrayHandles {
        start,
        stop,
        status: status_item,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egresskeeper_core::{Action, ListenerFailure, ListenerId, ProfileId, ProxyPort};

    fn listener(enabled: bool) -> Listener {
        Listener {
            id: ListenerId::new(),
            port: ProxyPort::parse(8787).expect("port"),
            profile_id: ProfileId::new(),
            enabled,
            created_at_unix_ms: 0,
            updated_at_unix_ms: 0,
        }
    }

    fn health(state: ListenerState) -> ListenerHealth {
        ListenerHealth {
            listener_id: ListenerId::new(),
            state,
            active_connections: 0,
        }
    }

    #[test]
    fn nothing_enabled_reports_idle_proxy() {
        let status = TrayStatus::from_listeners(&[listener(false)], &[]);

        assert_eq!(status.running_listeners, 0);
        assert_eq!(status.enabled_listeners, 0);
        assert_eq!(
            status.tooltip(),
            "EgressKeeper: proxy не слушает соединения"
        );
        assert!(!status.can_start(), "запускать нечего");
        assert!(!status.can_stop(), "останавливать нечего");
    }

    #[test]
    fn enabled_but_stopped_listener_offers_start() {
        let status = TrayStatus::from_listeners(&[listener(true)], &[]);

        assert_eq!(
            status.tooltip(),
            "EgressKeeper: proxy остановлен, включено listeners: 1"
        );
        assert!(status.can_start());
        assert!(!status.can_stop());
    }

    #[test]
    fn running_listener_offers_stop() {
        let configured = [listener(true)];
        let status = TrayStatus::from_listeners(
            &configured,
            &[
                health(ListenerState::Running),
                health(ListenerState::Stopped),
            ],
        );

        assert_eq!(status.running_listeners, 1);
        assert_eq!(status.tooltip(), "EgressKeeper: работает 1 listener");
        assert!(!status.can_start(), "уже работает");
        assert!(status.can_stop());
    }

    #[test]
    fn several_running_listeners_are_counted() {
        let status = TrayStatus::from_listeners(
            &[listener(true), listener(true), listener(false)],
            &[
                health(ListenerState::Running),
                health(ListenerState::Running),
            ],
        );

        assert_eq!(status.running_listeners, 2);
        assert_eq!(status.enabled_listeners, 2);
        assert_eq!(status.tooltip(), "EgressKeeper: работает listeners: 2");
    }

    #[test]
    fn failed_listener_is_not_counted_as_running() {
        let status = TrayStatus::from_listeners(
            &[listener(true)],
            &[health(ListenerState::Failed(ListenerFailure {
                code: egresskeeper_core::ErrorCode::PortUnavailable,
                message: "Порт уже занят другой программой.".to_owned(),
            }))],
        );

        assert_eq!(status.running_listeners, 0);
        assert!(
            status.can_start(),
            "listener включён и должен пробовать снова"
        );
    }

    #[test]
    fn active_connections_do_not_affect_tray_state() {
        let status = TrayStatus::from_listeners(
            &[listener(true)],
            &[ListenerHealth {
                listener_id: ListenerId::new(),
                state: ListenerState::Running,
                active_connections: 7,
            }],
        );

        assert_eq!(status.running_listeners, 1);
        assert!(status.can_stop());
        let _ = Action::Allow;
    }
}
