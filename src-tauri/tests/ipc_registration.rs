//! Проверка, что каждая команда, которую вызывает frontend, зарегистрирована в
//! backend.
//!
//! Это единственный вид рассинхронизации, который не ловят другие тесты: тесты
//! команд работают с `AppState` напрямую и не проходят через регистрацию Tauri.
//! Если команда забыта в `generate_handler!`, UI получает от Tauri ошибку «команда
//! не найдена», нормализует её как `internal` и показывает пользователю
//! «Внутренняя ошибка приложения» — именно так и произошло с командами proxy.
//!
//! Тест читает исходники, потому что регистрация выполняется макросом и недоступна
//! во время выполнения.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Извлекает имена команд из блока `generate_handler![…]`.
fn registered_commands(source: &str) -> BTreeSet<String> {
    let start = source
        .find("generate_handler![")
        .expect("lib.rs должен содержать generate_handler");
    let rest = &source[start..];
    let end = rest.find("])").expect("блок generate_handler закрывается");
    let block = &rest[..end];

    // Каждая строка блока — путь вида `commands::<модуль>::<команда>,`;
    // именем команды является последний сегмент.
    block
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim().trim_end_matches(',');

            // Строки без пути — это сам макрос и обрамление блока.
            if !trimmed.contains("::") {
                return None;
            }

            trimmed.rsplit("::").next().map(str::to_owned)
        })
        .collect()
}

/// Извлекает имена команд из объединения `KnownCommand` во frontend.
fn frontend_commands(source: &str) -> BTreeSet<String> {
    let start = source
        .find("export type KnownCommand =")
        .expect("client.ts должен объявлять KnownCommand");
    let rest = &source[start..];
    let end = rest
        .find(';')
        .expect("объединение KnownCommand закрывается");
    let block = &rest[..end];

    block
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .filter(|name| !name.is_empty())
        .collect()
}

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);

    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn every_frontend_command_is_registered_in_the_backend() {
    let registered = registered_commands(&read("src/lib.rs"));
    let used = frontend_commands(&read("../src/ipc/client.ts"));

    assert!(
        !used.is_empty(),
        "список команд frontend не должен быть пустым"
    );

    let missing: Vec<&String> = used.difference(&registered).collect();

    assert!(
        missing.is_empty(),
        "frontend вызывает команды, которых нет в generate_handler!: {missing:?}"
    );
}

#[test]
fn no_command_is_registered_without_a_frontend_caller() {
    let registered = registered_commands(&read("src/lib.rs"));
    let used = frontend_commands(&read("../src/ipc/client.ts"));

    let unused: Vec<&String> = registered.difference(&used).collect();

    assert!(
        unused.is_empty(),
        "команды зарегистрированы, но не вызываются из UI: {unused:?}. \
         Не держите IPC-поверхность шире необходимого"
    );
}

#[test]
fn registration_parser_sees_both_lists() {
    let registered = registered_commands(&read("src/lib.rs"));

    assert!(
        registered.len() >= 5,
        "парсер не нашёл команды: проверьте формат generate_handler!"
    );
    assert!(registered.contains("get_runtime_overview"));
}
