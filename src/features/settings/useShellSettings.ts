import { useCallback, useEffect, useState } from 'react';

import * as settingsApi from '../../ipc/settings';
import { toIpcError, type CloseBehavior, type IpcError, type ShellSettingsView } from '../../ipc/types';

/** Состояние загрузки экрана настроек. */
export type SettingsStatus = 'loading' | 'ready' | 'failed';

/** Модель экрана настроек. */
export interface ShellSettingsConsole {
  status: SettingsStatus;
  error: IpcError | null;
  settings: ShellSettingsView | null;
  setCloseBehavior: (value: CloseBehavior) => Promise<IpcError | null>;
  setAutostart: (enabled: boolean) => Promise<IpcError | null>;
}

/**
 * Управляет настройками приложения.
 *
 * Каждое изменение возвращает состояние с backend, поэтому отображаемые значения
 * всегда соответствуют факту: если система отклонила изменение автозапуска,
 * интерфейс покажет это, а не сохранённое намерение.
 */
export function useShellSettings(): ShellSettingsConsole {
  const [status, setStatus] = useState<SettingsStatus>('loading');
  const [error, setError] = useState<IpcError | null>(null);
  const [settings, setSettings] = useState<ShellSettingsView | null>(null);

  const load = useCallback(async () => {
    try {
      setSettings(await settingsApi.getSettings());
      setStatus('ready');
      setError(null);
    } catch (cause) {
      setStatus('failed');
      setError(toIpcError(cause));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const run = useCallback(
    async (operation: () => Promise<ShellSettingsView>) => {
      try {
        setSettings(await operation());
        return null;
      } catch (cause) {
        return toIpcError(cause);
      }
    },
    [],
  );

  const setCloseBehavior = useCallback(
    (value: CloseBehavior) => run(() => settingsApi.setCloseBehavior(value)),
    [run],
  );

  const setAutostart = useCallback(
    (enabled: boolean) => run(() => settingsApi.setAutostart(enabled)),
    [run],
  );

  return { status, error, settings, setCloseBehavior, setAutostart };
}
