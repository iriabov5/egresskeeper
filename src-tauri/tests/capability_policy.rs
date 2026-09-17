//! Аудит capability-политики приложения.
//!
//! Проверка живёт в тестах, а не в ревью-чеклисте, потому что расширение прав
//! происходит незаметно: достаточно добавить плагин или одну строку в
//! capabilities-файл. Тест падает, если появляется запрещённое разрешение,
//! неявный default-набор или wildcard-грант.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Префиксы разрешений, которые frontend не должен получать.
const FORBIDDEN_PERMISSION_PREFIXES: [&str; 6] =
    ["fs:", "shell:", "process:", "http:", "updater:", "os:"];

/// Возвращает пути всех capability-файлов приложения.
fn capability_files() -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");

    let mut files: Vec<PathBuf> = fs::read_dir(&directory)
        .expect("capabilities directory exists")
        .map(|entry| entry.expect("directory entry is readable").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();

    assert!(
        !files.is_empty(),
        "at least one capability file is required"
    );

    files
}

/// Извлекает разрешения из capability-файла.
fn permissions_of(file: &Path) -> Vec<String> {
    let raw = fs::read_to_string(file).expect("capability file is readable");
    let document: Value = serde_json::from_str(&raw).expect("capability file is valid JSON");

    document
        .get("permissions")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{}: `permissions` array is required", file.display()))
        .iter()
        .map(|permission| match permission {
            Value::String(identifier) => identifier.clone(),
            Value::Object(object) => object
                .get("identifier")
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    panic!("{}: object permission needs `identifier`", file.display())
                })
                .to_owned(),
            _ => panic!("{}: unsupported permission entry", file.display()),
        })
        .collect()
}

#[test]
fn capabilities_do_not_grant_filesystem_shell_or_os_access() {
    for file in capability_files() {
        for permission in permissions_of(&file) {
            for forbidden in FORBIDDEN_PERMISSION_PREFIXES {
                assert!(
                    !permission.starts_with(forbidden),
                    "{}: permission `{permission}` grants access that the security model forbids",
                    file.display()
                );
            }
        }
    }
}

#[test]
fn capabilities_grant_no_wildcard_permissions() {
    for file in capability_files() {
        for permission in permissions_of(&file) {
            assert!(
                !permission.contains('*'),
                "{}: permission `{permission}` uses a wildcard grant",
                file.display()
            );
        }
    }
}

#[test]
fn capabilities_list_every_permission_explicitly() {
    for file in capability_files() {
        for permission in permissions_of(&file) {
            assert!(
                !permission.ends_with(":default"),
                "{}: permission `{permission}` pulls in a default permission set instead of explicit grants",
                file.display()
            );
        }
    }
}

#[test]
fn capability_files_target_the_main_window_only() {
    for file in capability_files() {
        let raw = fs::read_to_string(&file).expect("capability file is readable");
        let document: Value = serde_json::from_str(&raw).expect("capability file is valid JSON");

        let identifier = document
            .get("identifier")
            .and_then(Value::as_str)
            .expect("capability identifier is present");
        assert!(
            !identifier.is_empty(),
            "{}: identifier is empty",
            file.display()
        );

        let windows: Vec<&str> = document
            .get("windows")
            .and_then(Value::as_array)
            .expect("capability windows are present")
            .iter()
            .filter_map(Value::as_str)
            .collect();

        assert_eq!(
            windows,
            vec!["main"],
            "{}: capability must target the main window only",
            file.display()
        );
    }
}
