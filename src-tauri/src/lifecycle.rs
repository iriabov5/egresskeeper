//! Жизненный цикл главного окна.
//!
//! Само правило вынесено в чистую функцию: обработчик события окна тестом не
//! покрыть, а решение — можно. Это единственная часть жизненного цикла, которую
//! проверяют автотесты.

use egresskeeper_core::CloseBehavior;

/// Что делать при закрытии главного окна.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Скрыть окно, оставив приложение и proxy работающими.
    Hide,
    /// Завершить приложение вместе с listeners.
    Quit,
}

/// Определяет действие при закрытии главного окна.
///
/// Скрыть окно можно только тогда, когда окно есть чем вернуть, то есть когда
/// значок в трее доступен. Иначе окно скрывалось бы безвозвратно, и приложение
/// превращалось бы в невидимый процесс — пользователь не смог бы ни увидеть
/// состояние, ни завершить работу.
#[must_use]
pub const fn close_action(behavior: CloseBehavior, tray_available: bool) -> CloseAction {
    match (behavior, tray_available) {
        (CloseBehavior::HideToTray, true) => CloseAction::Hide,
        (CloseBehavior::HideToTray, false) | (CloseBehavior::Quit, _) => CloseAction::Quit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hiding_requires_a_way_back() {
        assert_eq!(
            close_action(CloseBehavior::HideToTray, true),
            CloseAction::Hide
        );
        assert_eq!(
            close_action(CloseBehavior::HideToTray, false),
            CloseAction::Quit,
            "без трея окно нельзя скрыть: вернуть его будет нечем"
        );
    }

    #[test]
    fn explicit_quit_always_quits() {
        for tray_available in [true, false] {
            assert_eq!(
                close_action(CloseBehavior::Quit, tray_available),
                CloseAction::Quit
            );
        }
    }

    #[test]
    fn default_behavior_hides_when_tray_is_available() {
        assert_eq!(
            close_action(CloseBehavior::default(), true),
            CloseAction::Hide,
            "proxy задуман как постоянно работающий"
        );
    }
}
