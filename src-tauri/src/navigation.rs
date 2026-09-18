//! Политика навигации главного окна.
//!
//! Webview не должен уходить на внешние origins: bundled UI — единственный
//! источник контента окна. Tauri применяет navigation handler на уровне webview,
//! поэтому окно создаётся в Rust (`setup`), а не автоматически из конфигурации.

use tauri::Url;

/// Разрешает ли приложение навигацию на указанный URL.
#[must_use]
pub fn is_allowed(url: &Url) -> bool {
    decide(url, tauri::is_dev())
}

/// Чистая функция политики: проверяется тестами без запуска приложения.
///
/// Во время разработки UI отдаёт Vite dev server на loopback-адресе, поэтому
/// HTTP разрешён только для loopback и только в dev-сборке.
fn decide(url: &Url, dev_server_allowed: bool) -> bool {
    match url.scheme() {
        // Внутренние схемы Tauri: bundled assets и IPC-канал.
        "tauri" | "asset" | "ipc" => true,
        "http" | "https" => dev_server_allowed && is_loopback(url),
        _ => false,
    }
}

/// Возвращает `true`, если хост указывает на loopback-адрес.
fn is_loopback(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(value: &str) -> Url {
        Url::parse(value).expect("test URL is valid")
    }

    #[test]
    fn bundled_and_ipc_schemes_are_always_allowed() {
        for candidate in ["tauri://localhost/index.html", "ipc://localhost/ping"] {
            assert!(
                decide(&url(candidate), false),
                "{candidate} must be allowed in production"
            );
            assert!(
                decide(&url(candidate), true),
                "{candidate} must be allowed in development"
            );
        }
    }

    #[test]
    fn dev_server_is_allowed_on_loopback_only_in_development() {
        let dev_url = url("http://localhost:1420/");

        assert!(
            decide(&dev_url, true),
            "dev server must work in development"
        );
        assert!(
            !decide(&dev_url, false),
            "dev server must be unreachable in production builds"
        );
        assert!(decide(&url("http://127.0.0.1:1420/"), true));
    }

    #[test]
    fn external_origins_are_never_allowed() {
        for candidate in [
            "https://example.com/",
            "https://localhost.evil.example/",
            "http://localhost.evil.example/",
            "http://192.168.0.10/admin",
            "file:///etc/passwd",
            "data:text/html,<script>alert(1)</script>",
        ] {
            assert!(
                !decide(&url(candidate), true),
                "{candidate} must never be reachable from the webview"
            );
        }
    }
}
