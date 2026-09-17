import { useState, type SubmitEvent } from 'react';

import type { IpcError, ListenerView, ProfileSummary } from '../../ipc/types';
import type { ProxyConsole } from './useProxyConsole';

interface ListenerPanelProps {
  readonly console: ProxyConsole;
}

/** Панель listeners: список, состояние и управление. */
export function ListenerPanel({ console: consoleModel }: ListenerPanelProps) {
  const [port, setPort] = useState('');
  const [profileId, setProfileId] = useState('');
  const [createError, setCreateError] = useState<IpcError | null>(null);
  const [actionError, setActionError] = useState<IpcError | null>(null);

  const selectedProfile = profileId === '' ? (consoleModel.profiles[0]?.profile.id ?? '') : profileId;

  async function handleCreate(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();

    const error = await consoleModel.createListener(Number(port), selectedProfile);

    if (error === null) {
      setPort('');
    }

    setCreateError(error);
  }

  const portError =
    createError?.details?.field === 'port' ? createError.message : null;

  return (
    <section className="panel" aria-label="Listeners">
      <h2 className="panel__title">Listeners</h2>

      {consoleModel.listeners.length === 0 ? (
        <output className="panel__hint">
          Proxy ничего не слушает: добавьте listener, чтобы инструменты могли
          направлять трафик через EgressKeeper.
        </output>
      ) : (
        <ul className="listeners">
          {consoleModel.listeners.map((listener) => (
            <ListenerRow
              key={listener.listener.id}
              view={listener}
              profiles={consoleModel.profiles}
              onToggle={(enabled) => {
                void consoleModel.setEnabled(listener.listener.id, enabled).then(setActionError);
              }}
              onRemove={() => {
                void consoleModel.removeListener(listener.listener.id).then(setActionError);
              }}
            />
          ))}
        </ul>
      )}

      {actionError !== null && (
        <p className="form__error" role="alert">
          {actionError.message} (код: {actionError.code})
        </p>
      )}

      <form className="form" onSubmit={handleCreate} aria-label="Новый listener">
        <div className="form__row">
          <div className="form__field">
            <label className="form__label" htmlFor="listener-port">
              Порт
            </label>
            <input
              id="listener-port"
              type="number"
              min={1024}
              max={65535}
              value={port}
              aria-invalid={portError !== null}
              onChange={(event) => setPort(event.target.value)}
            />
            {portError !== null && (
              <span className="form__error" role="alert">
                {portError}
              </span>
            )}
          </div>

          <div className="form__field form__field--wide">
            <label className="form__label" htmlFor="listener-profile">
              Профиль политики
            </label>
            <select
              id="listener-profile"
              value={selectedProfile}
              onChange={(event) => setProfileId(event.target.value)}
            >
              {consoleModel.profiles.map((summary) => (
                <option key={summary.profile.id} value={summary.profile.id}>
                  {summary.profile.name}
                </option>
              ))}
            </select>
          </div>
        </div>

        {createError !== null && portError === null && (
          <p className="form__error" role="alert">
            {createError.message} (код: {createError.code})
          </p>
        )}

        <button type="submit" className="button">
          Добавить listener
        </button>
      </form>
    </section>
  );
}

interface ListenerRowProps {
  readonly view: ListenerView;
  readonly profiles: ProfileSummary[];
  readonly onToggle: (enabled: boolean) => void;
  readonly onRemove: () => void;
}

/** Строка списка listeners. */
function ListenerRow({ view, profiles, onToggle, onRemove }: ListenerRowProps) {
  const profileName =
    profiles.find((summary) => summary.profile.id === view.listener.profile_id)?.profile.name ??
    view.listener.profile_id;

  return (
    <li className="listeners__item">
      <div className="listeners__main">
        <span className="listeners__port">127.0.0.1:{view.listener.port}</span>
        <span className="listeners__profile">профиль: {profileName}</span>
        <StateBadge state={view.state} />
        <span className="listeners__connections">соединений: {view.active_connections}</span>
      </div>

      <div className="listeners__controls">
        <button type="button" className="button" onClick={() => onToggle(!view.listener.enabled)}>
          {view.listener.enabled ? 'Выключить' : 'Включить'}
        </button>
        <button type="button" className="button button--danger" onClick={onRemove}>
          Удалить
        </button>
      </div>
    </li>
  );
}

/** Значок фактического состояния listener'а. */
function StateBadge({ state }: { readonly state: ListenerView['state'] }) {
  switch (state.state) {
    case 'running':
      return <span className="badge badge--allow">работает</span>;
    case 'starting':
      return <span className="badge">запускается</span>;
    case 'stopped':
      return <span className="badge">остановлен</span>;
    case 'failed':
      return (
        <span className="badge badge--deny" title={state.message}>
          ошибка: {state.code}
        </span>
      );
  }
}
