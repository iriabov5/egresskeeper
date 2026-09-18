import { useId, useState, type SubmitEvent } from 'react';

import type { Action, HostKind, IpcError, PortSpec, Rule, RuleInput } from '../../ipc/types';

interface RuleFormProps {
  /** Правило в режиме редактирования; без него форма создаёт новое правило. */
  readonly rule?: Rule;
  readonly onSubmit: (input: RuleInput) => Promise<IpcError | null>;
  readonly onCancel?: () => void;
}

type PortKind = PortSpec['kind'];

/** Текст ошибки валидации для конкретного поля формы. */
function fieldErrorText(error: IpcError | null, fields: readonly string[]): string | null {
  const field = error?.details?.field;

  return field !== undefined && fields.includes(field) ? error?.message ?? null : null;
}

/**
 * Форма добавления правила.
 *
 * Ошибки валидации backend приходят с именем поля контракта и показываются рядом
 * с соответствующим полем; введённые значения при ошибке не теряются.
 */
export function RuleForm({ rule, onSubmit, onCancel }: RuleFormProps) {
  const [action, setAction] = useState<Action>(rule?.action ?? 'allow');
  const [hostKind, setHostKind] = useState<HostKind>(rule?.host.kind ?? 'exact');
  const [host, setHost] = useState(rule?.host.value ?? '');
  const [portKind, setPortKind] = useState<PortKind>(rule?.port.kind ?? 'any');
  const [port, setPort] = useState(
    rule?.port.kind === 'exactly' ? String(rule.port.port) : '443',
  );
  const [rangeStart, setRangeStart] = useState(
    rule?.port.kind === 'range' ? String(rule.port.start) : '8000',
  );
  const [rangeEnd, setRangeEnd] = useState(
    rule?.port.kind === 'range' ? String(rule.port.end) : '8100',
  );
  const [error, setError] = useState<IpcError | null>(null);
  const fieldId = useId();
  const hostError = fieldErrorText(error, ['host']);
  const portError = fieldErrorText(error, ['port']);
  const rangeStartError = fieldErrorText(error, ['port_start']);
  const rangeEndError = fieldErrorText(error, ['port_end']);

  async function handleSubmit(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();

    const result = await onSubmit({
      action,
      host_kind: hostKind,
      host,
      port: buildPortSpec(portKind, port, rangeStart, rangeEnd),
    });

    if (result === null) {
      if (rule === undefined) {
        setHost('');
      }
      setError(null);
      return;
    }

    setError(result);
  }

  return (
    <form
      className="form"
      onSubmit={handleSubmit}
      aria-label={rule === undefined ? 'Новое правило' : `Изменение правила`}
    >
      <div className="form__row">
        <div className="form__field">
          <label className="form__label" htmlFor={`${fieldId}-action`}>
            Действие
          </label>
          <select
            id={`${fieldId}-action`}
            value={action}
            onChange={(event) => setAction(event.target.value as Action)}
          >
            <option value="allow">Разрешить</option>
            <option value="deny">Запретить</option>
          </select>
        </div>

        <div className="form__field">
          <label className="form__label" htmlFor={`${fieldId}-kind`}>
            Сопоставление
          </label>
          <select
            id={`${fieldId}-kind`}
            value={hostKind}
            onChange={(event) => setHostKind(event.target.value as HostKind)}
          >
            <option value="exact">Точный host</option>
            <option value="subdomains">Поддомены (*.домен)</option>
          </select>
        </div>

        <div className="form__field form__field--wide">
          <label className="form__label" htmlFor={`${fieldId}-host`}>
            Host
          </label>
          <input
            id={`${fieldId}-host`}
            name="host"
            type="text"
            value={host}
            placeholder={hostKind === 'exact' ? 'api.example.com' : 'example.com'}
            aria-invalid={hostError !== null}
            onChange={(event) => setHost(event.target.value)}
          />
          {hostError !== null && (
            <span className="form__error" role="alert">
              {hostError}
            </span>
          )}
        </div>
      </div>

      <div className="form__row">
        <div className="form__field">
          <label className="form__label" htmlFor={`${fieldId}-port-kind`}>
            Порт
          </label>
          <select
            id={`${fieldId}-port-kind`}
            value={portKind}
            onChange={(event) => setPortKind(event.target.value as PortKind)}
          >
            <option value="any">Любой</option>
            <option value="exactly">Один порт</option>
            <option value="range">Диапазон</option>
          </select>
        </div>

        {portKind === 'exactly' && (
          <div className="form__field">
            <label className="form__label" htmlFor={`${fieldId}-port`}>
              Номер порта
            </label>
            <input
              id={`${fieldId}-port`}
              type="number"
              min={1}
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
        )}

        {portKind === 'range' && (
          <>
            <div className="form__field">
              <label className="form__label" htmlFor={`${fieldId}-range-start`}>
                С
              </label>
              <input
                id={`${fieldId}-range-start`}
                type="number"
                min={1}
                max={65535}
                value={rangeStart}
                aria-invalid={rangeStartError !== null}
                onChange={(event) => setRangeStart(event.target.value)}
              />
              {rangeStartError !== null && (
                <span className="form__error" role="alert">
                  {rangeStartError}
                </span>
              )}
            </div>

            <div className="form__field">
              <label className="form__label" htmlFor={`${fieldId}-range-end`}>
                По
              </label>
              <input
                id={`${fieldId}-range-end`}
                type="number"
                min={1}
                max={65535}
                value={rangeEnd}
                aria-invalid={rangeEndError !== null}
                onChange={(event) => setRangeEnd(event.target.value)}
              />
              {rangeEndError !== null && (
                <span className="form__error" role="alert">
                  {rangeEndError}
                </span>
              )}
            </div>
          </>
        )}
      </div>

      {error !== null && error.details === undefined && (
        <p className="form__error" role="alert">
          {error.message} (код: {error.code})
        </p>
      )}

      <div className="form__row">
        <button type="submit" className="button">
          {rule === undefined ? 'Добавить правило' : 'Сохранить правило'}
        </button>
        {rule !== undefined && onCancel !== undefined && (
          <button type="button" className="button" onClick={onCancel}>
            Отмена
          </button>
        )}
      </div>
    </form>
  );
}

/** Собирает ограничение порта из значений формы. */
function buildPortSpec(
  kind: PortKind,
  port: string,
  rangeStart: string,
  rangeEnd: string,
): PortSpec {
  switch (kind) {
    case 'exactly':
      return { kind: 'exactly', port: Number(port) };
    case 'range':
      return { kind: 'range', start: Number(rangeStart), end: Number(rangeEnd) };
    default:
      return { kind: 'any' };
  }
}
