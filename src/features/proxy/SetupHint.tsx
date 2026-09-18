import type { ListenerView } from '../../ipc/types';

interface SetupHintProps {
  readonly listeners: ListenerView[];
}

/** Подсказка, как направить инструмент на proxy. */
export function SetupHint({ listeners }: SetupHintProps) {
  const active = listeners.find((listener) => listener.state.state === 'running') ?? listeners[0];

  if (active === undefined) {
    return (
      <section className="panel" aria-label="Настройка инструмента">
        <h2 className="panel__title">Как направить инструмент на proxy</h2>
        <p className="panel__hint">
          Сначала добавьте и включите listener: после этого здесь появится адрес proxy
          и переменные окружения.
        </p>
      </section>
    );
  }

  const address = `http://127.0.0.1:${active.listener.port}`;

  return (
    <section className="panel" aria-label="Настройка инструмента">
      <h2 className="panel__title">Как направить инструмент на proxy</h2>
      <p className="panel__hint">
        Настройте инструмент (AI-агент, MCP-сервер, CLI) на использование proxy{' '}
        {active.state.state === 'running' ? '' : '(listener ещё не работает) '}
        и перезапустите его:
      </p>
      <pre className="code-block">
        {`export HTTPS_PROXY=${address}\nexport HTTP_PROXY=${address}\nexport NO_PROXY=localhost,127.0.0.1`}
      </pre>
      <p className="panel__hint">
        Windows PowerShell:
      </p>
      <pre className="code-block">
        {`$env:HTTPS_PROXY="${address}"\n$env:HTTP_PROXY="${address}"`}
      </pre>
    </section>
  );
}
