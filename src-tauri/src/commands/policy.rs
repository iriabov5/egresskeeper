//! Команды управления политикой.
//!
//! Все команды работают с хранилищем, поэтому выполняются в блокирующем пуле.
//! Входные данные не доверяются: валидация выполняется доменом, а команда только
//! передаёт типизированный ввод и маппит ошибку в контракт IPC.

use egresskeeper_core::{
    Action, Decision, Profile, ProfileDetail, ProfileSummary, Rule, RuleInput,
};

use crate::commands::run_blocking;
use crate::ipc::IpcError;
use crate::state::AppState;

/// Возвращает профили с количеством правил.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой хранилища, если оно недоступно.
#[tauri::command(rename_all = "snake_case")]
pub async fn policy_list_profiles(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProfileSummary>, IpcError> {
    let state = state.inner().clone();

    run_blocking("policy_list_profiles", move || {
        IpcError::from_result(state.policy()?.list_profiles())
    })
    .await
}

/// Возвращает профиль вместе с его правилами.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой валидации идентификатора, отсутствия профиля
/// или недоступности хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn policy_profile_detail(
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> Result<ProfileDetail, IpcError> {
    let state = state.inner().clone();

    run_blocking("policy_profile_detail", move || {
        IpcError::from_result(state.policy()?.profile_detail(&profile_id))
    })
    .await
}

/// Создаёт профиль с запрещающим default action.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой валидации имени или недоступности хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn policy_create_profile(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<Profile, IpcError> {
    let state = state.inner().clone();

    run_blocking("policy_create_profile", move || {
        IpcError::from_result(state.policy()?.create_profile(&name))
    })
    .await
}

/// Переименовывает профиль.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой валидации, отсутствия профиля или
/// недоступности хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn policy_rename_profile(
    state: tauri::State<'_, AppState>,
    profile_id: String,
    name: String,
) -> Result<Profile, IpcError> {
    let state = state.inner().clone();

    run_blocking("policy_rename_profile", move || {
        IpcError::from_result(state.policy()?.rename_profile(&profile_id, &name))
    })
    .await
}

/// Меняет default action профиля.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой отсутствия профиля или недоступности
/// хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn policy_set_default_action(
    state: tauri::State<'_, AppState>,
    profile_id: String,
    action: Action,
) -> Result<Profile, IpcError> {
    let state = state.inner().clone();

    run_blocking("policy_set_default_action", move || {
        IpcError::from_result(state.policy()?.set_default_action(&profile_id, action))
    })
    .await
}

/// Удаляет профиль вместе с его правилами.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой валидации (последний профиль), отсутствия
/// профиля или недоступности хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn policy_delete_profile(
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> Result<(), IpcError> {
    let state = state.inner().clone();

    run_blocking("policy_delete_profile", move || {
        IpcError::from_result(state.policy()?.delete_profile(&profile_id))
    })
    .await
}

/// Добавляет правило в конец профиля.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой валидации ввода, отсутствия профиля или
/// недоступности хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn policy_add_rule(
    state: tauri::State<'_, AppState>,
    profile_id: String,
    input: RuleInput,
) -> Result<Rule, IpcError> {
    let state = state.inner().clone();

    run_blocking("policy_add_rule", move || {
        IpcError::from_result(state.policy()?.add_rule(&profile_id, input))
    })
    .await
}

/// Изменяет правило, сохраняя его позицию.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой валидации ввода, отсутствия правила или
/// недоступности хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn policy_update_rule(
    state: tauri::State<'_, AppState>,
    rule_id: String,
    input: RuleInput,
) -> Result<Rule, IpcError> {
    let state = state.inner().clone();

    run_blocking("policy_update_rule", move || {
        IpcError::from_result(state.policy()?.update_rule(&rule_id, input))
    })
    .await
}

/// Удаляет правило.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой отсутствия правила или недоступности
/// хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn policy_delete_rule(
    state: tauri::State<'_, AppState>,
    rule_id: String,
) -> Result<(), IpcError> {
    let state = state.inner().clone();

    run_blocking("policy_delete_rule", move || {
        IpcError::from_result(state.policy()?.delete_rule(&rule_id))
    })
    .await
}

/// Заменяет порядок правил профиля.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой валидации списка правил, отсутствия профиля
/// или недоступности хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn policy_reorder_rules(
    state: tauri::State<'_, AppState>,
    profile_id: String,
    rule_ids: Vec<String>,
) -> Result<Vec<Rule>, IpcError> {
    let state = state.inner().clone();

    run_blocking("policy_reorder_rules", move || {
        IpcError::from_result(state.policy()?.reorder_rules(&profile_id, &rule_ids))
    })
    .await
}

/// Оценивает соединение по политике профиля.
///
/// Операция только читает данные: проверка решения не изменяет политику.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой валидации host или port, отсутствия профиля
/// или недоступности хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn policy_evaluate(
    state: tauri::State<'_, AppState>,
    profile_id: String,
    host: String,
    port: u16,
) -> Result<Decision, IpcError> {
    let state = state.inner().clone();

    run_blocking("policy_evaluate", move || {
        IpcError::from_result(state.policy()?.evaluate(&profile_id, &host, port))
    })
    .await
}
