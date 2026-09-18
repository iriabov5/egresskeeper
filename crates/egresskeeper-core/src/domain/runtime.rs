//! Модель runtime-информации приложения.
//!
//! DTO является частью IPC-контракта: имена полей и их типы фиксируются общим
//! fixture `contracts/ipc/runtime_overview.sample.json`, который проверяют тесты
//! и на стороне Rust, и на стороне frontend.

use serde::{Deserialize, Serialize};

/// Runtime-информация, которую shell показывает пользователю.
///
/// Все значения определяет backend: frontend не вычисляет пути и не определяет
/// платформу самостоятельно.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeOverview {
    /// Версия desktop-приложения.
    pub app_version: String,
    /// Версия ядра.
    pub core_version: String,
    /// Операционная система, для которой собрано приложение.
    pub os: String,
    /// Архитектура процессора, для которой собрано приложение.
    pub arch: String,
    /// Абсолютный путь каталога состояния приложения.
    pub state_dir: String,
    /// Момент старта backend в миллисекундах Unix epoch.
    pub started_at_unix_ms: i64,
}

impl RuntimeOverview {
    /// Создаёт runtime-информацию.
    #[must_use]
    pub fn new(
        app_version: impl Into<String>,
        state_dir: impl Into<String>,
        started_at_unix_ms: i64,
    ) -> Self {
        Self {
            app_version: app_version.into(),
            core_version: env!("CARGO_PKG_VERSION").to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            state_dir: state_dir.into(),
            started_at_unix_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_reports_core_version_and_build_target() {
        let overview = RuntimeOverview::new("1.2.3", "/state", 1_700_000_000_000);

        assert_eq!(overview.app_version, "1.2.3");
        assert_eq!(overview.core_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(overview.os, std::env::consts::OS);
        assert_eq!(overview.arch, std::env::consts::ARCH);
        assert_eq!(overview.state_dir, "/state");
        assert_eq!(overview.started_at_unix_ms, 1_700_000_000_000);
    }

    #[test]
    fn overview_round_trips_through_json() {
        let overview = RuntimeOverview::new("1.2.3", "/state", 1_700_000_000_000);

        let json = serde_json::to_string(&overview).expect("serialize");
        let restored: RuntimeOverview = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored, overview);
    }
}
