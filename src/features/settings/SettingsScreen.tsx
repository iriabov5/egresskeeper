import { useId, useState } from 'react';

import type { CloseBehavior, IpcError } from '../../ipc/types';
import { useShellSettings } from './useShellSettings';

/**
 * Экран настроек приложения.
 *
 * Показывает только те возможности, которые подтверждены backend: если трей или
 * автозапуск недоступны на платформе, соответствующий элемент управления
 * блокируется с пояснением, а не молча сохраняет настройку, которая ни на что не
 * влияет.
 */
export function SettingsScreen() {
  const { status, error, settings, setCloseBehavior, setAutostart } = useShellSettings();
  const [actionError, setActionError] = useState<IpcError | null>(null);
  const fieldId = useId();

  if (status === 'loading') {
    return (
      <section className="panel">
        <output className="panel__title">Загружаем настройки…</output>
      </section>
    );
  }

  if (status === 'failed' || settings === null) {
    return (
      <section className="panel panel--error" role="alert">
        <h2 className="panel__title">Настройки недоступны</h2>
        <p className="panel__text">{error?.message ?? 'Не удалось получить настройки.'}</p>
        <p className="panel__hint">Код ошибки: {error?.code ?? 'неизвестно'}.</p>
      </section>
    );
  }

  const onCloseBehavior = (value: CloseBehavior) => {
    void setCloseBehavior(value).then(setActionError);
  };

  const onAutostart = (enabled: boolean) => {
    void setAutostart(enabled).then(setActionError);
  };

  return (
    <div className="settings">
      {actionError !== null && (
        <section className="panel panel--error" role="alert">
          <h2 className="panel__title">Не удалось изменить настройку</h2>
          <p className="panel__text">{actionError.message}</p>
          <p className="panel__hint">Код ошибки: {actionError.code}.</p>
        </section>
      )}

      <section className="panel" aria-label="Окно">
        <h2 className="panel__title">Окно</h2>
        <p className="panel__text">Что делать при закрытии главного окна.</p>
        <div className="form__field">
          <div className="form__label" id={`${fieldId}-close`}>
            Поведение при закрытии
          </div>
          <div role="radiogroup" aria-labelledby={`${fieldId}-close`}>
            <label className="form__option" htmlFor={`${fieldId}-hide`}>
              <input
                id={`${fieldId}-hide`}
                type="radio"
                name={`${fieldId}-close-behavior`}
                value="hide_to_tray"
                checked={settings.close_behavior === 'hide_to_tray'}
                disabled={!settings.tray_available}
                onChange={() => onCloseBehavior('hide_to_tray')}
              />
              <span>Скрывать в трей — proxy продолжает работать</span>
            </label>
            <label className="form__option" htmlFor={`${fieldId}-quit`}>
              <input
                id={`${fieldId}-quit`}
                type="radio"
                name={`${fieldId}-close-behavior`}
                value="quit"
                checked={settings.close_behavior === 'quit'}
                onChange={() => onCloseBehavior('quit')}
              />
              <span>Завершать приложение</span>
            </label>
          </div>
        </div>
        <p className="panel__hint">
          {settings.tray_available
            ? 'Иконка в трее управляет listeners и открывает окно.'
            : 'Трей недоступен на этой платформе, поэтому окно всегда завершает приложение.'}
        </p>
      </section>

      <section className="panel" aria-label="Система">
        <h2 className="panel__title">Система</h2>
        <div className="form__field">
          <label className="form__option" htmlFor={`${fieldId}-autostart`}>
            <input
              id={`${fieldId}-autostart`}
              type="checkbox"
              checked={settings.autostart_enabled}
              disabled={!settings.autostart_supported}
              onChange={(event) => onAutostart(event.target.checked)}
            />
            <span>Запускать при входе в систему</span>
          </label>
        </div>
        <p className="panel__hint">
          {settings.autostart_supported
            ? 'Автозапуск регистрируется в системе для установленного приложения.'
            : 'Автозапуск недоступен на этой платформе (код: platform_feature_unavailable).'}
        </p>
      </section>
    </div>
  );
}
