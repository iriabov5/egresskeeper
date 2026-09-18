import type { ReactNode } from 'react';

import type { RuntimeState } from './useRuntimeOverview';

interface RuntimeStatusViewProps {
  readonly state: RuntimeState;
  readonly onRetry: () => void;
}

/**
 * Представление состояния shell.
 *
 * Компонент только отображает то, что пришло из backend: пути, версии и
 * платформу UI не вычисляет.
 */
export function RuntimeStatusView({ state, onRetry }: RuntimeStatusViewProps) {
  if (state.status === 'starting') {
    return (
      <section className="panel" aria-live="polite">
        <output className="panel__title">Инициализация backend…</output>
        <output className="panel__hint">
          Приложение запрашивает состояние у Rust-процесса.
        </output>
      </section>
    );
  }

  if (state.status === 'failed') {
    return (
      <section className="panel panel--error" role="alert">
        <h2 className="panel__title">Backend недоступен</h2>
        <p className="panel__text">{state.error.message}</p>
        <p className="panel__hint">
          Код ошибки: <code>{state.error.code}</code>
        </p>
        <button type="button" className="button" onClick={onRetry}>
          Повторить
        </button>
      </section>
    );
  }

  const { overview } = state;

  return (
    <section className="panel" aria-label="Состояние приложения">
      <h2 className="panel__title">Приложение готово</h2>
      <dl className="facts">
        <Fact label="Версия приложения" value={overview.app_version} />
        <Fact label="Версия ядра" value={overview.core_version} />
        <Fact label="Платформа" value={`${overview.os} / ${overview.arch}`} />
        <Fact label="Каталог состояния" value={overview.state_dir} />
        <Fact label="Запуск backend" value={formatStartedAt(overview.started_at_unix_ms)} />
      </dl>
    </section>
  );
}

interface FactProps {
  readonly label: string;
  readonly value: ReactNode;
}

function Fact({ label, value }: FactProps) {
  return (
    <div className="facts__row">
      <dt className="facts__label">{label}</dt>
      <dd className="facts__value">{value}</dd>
    </div>
  );
}

/** Форматирует время старта backend для показа пользователю. */
function formatStartedAt(startedAtUnixMs: number): string {
  return new Intl.DateTimeFormat('ru-RU', { dateStyle: 'short', timeStyle: 'medium' }).format(
    new Date(startedAtUnixMs),
  );
}
