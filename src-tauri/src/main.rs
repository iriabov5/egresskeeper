//! Точка входа desktop-приложения.

// В release-сборке на Windows не открываем консольное окно рядом с приложением.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    egresskeeper_app::run();
}
