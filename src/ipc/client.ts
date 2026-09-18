/**
 * Единственное место, где frontend обращается к Tauri API.
 *
 * Компоненты не импортируют `@tauri-apps/api` напрямую: поверхность IPC
 * перечислена явно, поэтому «случайный» вызов произвольной команды или события
 * невозможен, а `window.__TAURI__` в приложении отключён.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import { toIpcError } from './types';

/** Полный список команд, доступных frontend. */
export type KnownCommand =
  | 'get_runtime_overview'
  | 'proxy_status'
  | 'proxy_create_listener'
  | 'proxy_update_listener'
  | 'proxy_set_listener_enabled'
  | 'proxy_delete_listener'
  | 'settings_get'
  | 'settings_set_close_behavior'
  | 'settings_set_autostart'
  | 'policy_list_profiles'
  | 'policy_profile_detail'
  | 'policy_create_profile'
  | 'policy_rename_profile'
  | 'policy_set_default_action'
  | 'policy_delete_profile'
  | 'policy_add_rule'
  | 'policy_update_rule'
  | 'policy_delete_rule'
  | 'policy_reorder_rules'
  | 'policy_evaluate';

/**
 * Вызывает объявленную команду и нормализует ошибку.
 *
 * @throws {IpcError} типизированная ошибка с `code` и безопасным сообщением
 */
export async function invokeCommand<Result>(
  command: KnownCommand,
  args?: Record<string, unknown>,
): Promise<Result> {
  try {
    return await invoke<Result>(command, args);
  } catch (cause) {
    throw toIpcError(cause);
  }
}

/**
 * Подписывается на событие backend.
 *
 * @returns функция отписки, которую обязана вызвать очистка эффекта
 */
export function subscribeToEvent<Payload>(
  event: string,
  handler: (payload: unknown) => void,
): Promise<UnlistenFn> {
  return listen<Payload>(event, (message) => {
    handler(message.payload);
  });
}
