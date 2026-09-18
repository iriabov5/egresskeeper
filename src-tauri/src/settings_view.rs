//! Представление настроек приложения для UI.
//!
//! Настройки живут в ядре, состояние автозапуска знает операционная система, а
//! доступность трея — обвязка приложения. UI должен видеть одно согласованное
//! представление, поэтому они объединяются здесь, чистым способом.

use egresskeeper_core::{CloseBehavior, EgressError, ShellSettings};
use serde::{Deserialize, Serialize};

/// Настройки оболочки вместе с состоянием платформенных возможностей.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellSettingsView {
    /// Поведение при закрытии главного окна.
    pub close_behavior: CloseBehavior,
    /// Доступен ли значок в трее.
    pub tray_available: bool,
    /// Поддерживается ли автозапуск в текущем окружении.
    pub autostart_supported: bool,
    /// Включён ли автозапуск фактически.
    pub autostart_enabled: bool,
}

impl ShellSettingsView {
    /// Собирает представление из настроек и состояния автозапуска.
    ///
    /// Недоступность автозапуска не является ошибкой отображения: интерфейс
    /// показывает это состояние, а остальные настройки остаются доступными.
    #[must_use]
    pub fn build(
        settings: ShellSettings,
        tray_available: bool,
        autostart: Option<Result<bool, EgressError>>,
    ) -> Self {
        let (supported, enabled) = match autostart {
            Some(Ok(enabled)) => (true, enabled),
            Some(Err(_)) | None => (false, false),
        };

        Self {
            close_behavior: settings.close_behavior,
            tray_available,
            autostart_supported: supported,
            autostart_enabled: enabled,
        }
    }

    /// Доступно ли скрытие окна: для этого нужен трей.
    #[must_use]
    pub const fn can_hide_window(&self) -> bool {
        self.tray_available
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> ShellSettings {
        ShellSettings {
            close_behavior: CloseBehavior::HideToTray,
        }
    }

    #[test]
    fn autostart_state_comes_from_the_platform() {
        let view = ShellSettingsView::build(settings(), true, Some(Ok(true)));

        assert!(view.autostart_supported);
        assert!(view.autostart_enabled);
        assert_eq!(view.close_behavior, CloseBehavior::HideToTray);
    }

    #[test]
    fn disabled_autostart_is_supported_but_disabled() {
        let view = ShellSettingsView::build(settings(), true, Some(Ok(false)));

        assert!(view.autostart_supported);
        assert!(!view.autostart_enabled);
    }

    #[test]
    fn unavailable_autostart_is_reported_as_unsupported() {
        let view = ShellSettingsView::build(
            settings(),
            true,
            Some(Err(EgressError::platform_feature_unavailable("autostart"))),
        );

        assert!(!view.autostart_supported);
        assert!(
            !view.autostart_enabled,
            "недоступный автозапуск не может быть включён"
        );
    }

    #[test]
    fn missing_autostart_port_is_unsupported() {
        let view = ShellSettingsView::build(settings(), false, None);

        assert!(!view.autostart_supported);
        assert!(!view.can_hide_window(), "без трея скрывать окно нельзя");
    }

    #[test]
    fn hiding_requires_tray() {
        assert!(ShellSettingsView::build(settings(), true, None).can_hide_window());
        assert!(!ShellSettingsView::build(settings(), false, None).can_hide_window());
    }
}
