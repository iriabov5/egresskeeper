//! Проверка IPC-контракта настроек против общего fixture.
//!
//! Тот же файл читает frontend, поэтому расхождение имён полей или значений
//! становится падающим тестом, а не ошибкой времени выполнения в UI.

use egresskeeper_app::ShellSettingsView;
use egresskeeper_core::CloseBehavior;

const SETTINGS_FIXTURE: &str = include_str!("../../contracts/ipc/shell_settings.sample.json");

#[test]
fn settings_fixture_deserializes_and_round_trips() {
    let settings: ShellSettingsView =
        serde_json::from_str(SETTINGS_FIXTURE).expect("fixture matches the contract");

    assert_eq!(settings.close_behavior, CloseBehavior::HideToTray);
    assert!(settings.tray_available);
    assert!(settings.autostart_supported);
    assert!(!settings.autostart_enabled);

    assert_eq!(
        serde_json::to_value(&settings).expect("serialize"),
        serde_json::from_str::<serde_json::Value>(SETTINGS_FIXTURE).expect("fixture json")
    );
}

#[test]
fn close_behavior_serializes_as_snake_case() {
    let quit = serde_json::to_value(CloseBehavior::Quit).expect("serialize");

    assert_eq!(quit, serde_json::Value::String("quit".to_owned()));
    assert_eq!(
        serde_json::to_value(CloseBehavior::HideToTray).expect("serialize"),
        serde_json::Value::String("hide_to_tray".to_owned())
    );
}
