import { useState, type SubmitEvent } from 'react';

import type { Action, IpcError, ProfileSummary } from '../../ipc/types';
import type { PolicyEditor } from './usePolicyEditor';

interface ProfilePanelProps {
  readonly profiles: ProfileSummary[];
  readonly selectedProfileId: string | null;
  readonly editor: PolicyEditor;
}

/** Панель профилей: список, создание, переименование и удаление. */
export function ProfilePanel({ profiles, selectedProfileId, editor }: ProfilePanelProps) {
  const selected = profiles.find((summary) => summary.profile.id === selectedProfileId);
  const [newName, setNewName] = useState('');
  const [renameValue, setRenameValue] = useState('');
  const [createError, setCreateError] = useState<IpcError | null>(null);
  const [renameError, setRenameError] = useState<IpcError | null>(null);
  const [deleteError, setDeleteError] = useState<IpcError | null>(null);

  async function handleCreate(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    const error = await editor.createProfile(newName);

    if (error === null) {
      setNewName('');
    }

    setCreateError(error);
  }

  async function handleRename(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    const error = await editor.renameProfile(renameValue);

    if (error === null) {
      setRenameValue('');
    }

    setRenameError(error);
  }

  return (
    <section className="panel" aria-label="Профили">
      <h2 className="panel__title">Профили</h2>

      <ul className="profiles">
        {profiles.map((summary) => (
          <li key={summary.profile.id}>
            <button
              type="button"
              className={
                summary.profile.id === selectedProfileId
                  ? 'profiles__item profiles__item--active'
                  : 'profiles__item'
              }
              aria-pressed={summary.profile.id === selectedProfileId}
              onClick={() => editor.selectProfile(summary.profile.id)}
            >
              <span className="profiles__name">{summary.profile.name}</span>
              <span className="profiles__meta">
                правил: {summary.rule_count} · default: {defaultActionLabel(summary.profile.default_action)}
              </span>
            </button>
          </li>
        ))}
      </ul>

      <form className="form" onSubmit={handleCreate} aria-label="Новый профиль">
        <label className="form__field form__field--wide">
          <span className="form__label">Имя нового профиля</span>
          <input
            name="name"
            type="text"
            value={newName}
            onChange={(event) => setNewName(event.target.value)}
          />
        </label>
        {createError !== null && (
          <p className="form__error" role="alert">
            {createError.message} (код: {createError.code})
          </p>
        )}
        <button type="submit" className="button">
          Создать профиль
        </button>
      </form>

      {selected !== undefined && (
        <div className="panel__footer">
          <form className="form" onSubmit={handleRename} aria-label="Переименование профиля">
            <label className="form__field form__field--wide">
              <span className="form__label">Переименовать «{selected.profile.name}»</span>
              <input
                name="rename"
                type="text"
                value={renameValue}
                placeholder={selected.profile.name}
                onChange={(event) => setRenameValue(event.target.value)}
              />
            </label>
            {renameError !== null && (
              <p className="form__error" role="alert">
                {renameError.message} (код: {renameError.code})
              </p>
            )}
            <button type="submit" className="button" disabled={renameValue.trim() === ''}>
              Переименовать
            </button>
          </form>

          <div className="panel__actions">
            {deleteError !== null && (
              <p className="form__error" role="alert">
                {deleteError.message} (код: {deleteError.code})
              </p>
            )}
            <button
              type="button"
              className="button button--danger"
              onClick={() => {
                void editor.removeProfile().then(setDeleteError);
              }}
            >
              Удалить профиль
            </button>
          </div>
        </div>
      )}
    </section>
  );
}

/** Текст default action для списка профилей. */
function defaultActionLabel(action: Action): string {
  return action === 'allow' ? 'разрешить' : 'запретить';
}
