/**
 * Настройки приложения: узкий API поверх IPC.
 *
 * Изменение настроек возвращает свежее состояние, поэтому UI показывает факт, а
 * не своё предположение о результате.
 */

import { invokeCommand } from './client';
import { parseShellSettings, type CloseBehavior, type ShellSettingsView } from './types';

/** Возвращает настройки приложения и состояние платформенных возможностей. */
export async function getSettings(): Promise<ShellSettingsView> {
  return parseShellSettings(await invokeCommand<unknown>('settings_get'));
}

/** Меняет поведение при закрытии главного окна. */
export async function setCloseBehavior(closeBehavior: CloseBehavior): Promise<ShellSettingsView> {
  return parseShellSettings(
    await invokeCommand<unknown>('settings_set_close_behavior', {
      close_behavior: closeBehavior,
    }),
  );
}

/** Включает или выключает запуск при входе в систему. */
export async function setAutostart(enabled: boolean): Promise<ShellSettingsView> {
  return parseShellSettings(
    await invokeCommand<unknown>('settings_set_autostart', { enabled }),
  );
}
