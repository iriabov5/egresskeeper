import { DecisionFeed } from './DecisionFeed';
import { ListenerPanel } from './ListenerPanel';
import { SetupHint } from './SetupHint';
import { useProxyConsole } from './useProxyConsole';

/**
 * Экран «Прокси»: состояние listeners, управление ими, живой поток решений и
 * подсказка по настройке инструментов.
 */
export function ProxyScreen() {
  const consoleModel = useProxyConsole();

  if (consoleModel.status === 'loading') {
    return (
      <section className="panel" aria-live="polite">
        <output className="panel__title">Загружаем состояние proxy…</output>
      </section>
    );
  }

  if (consoleModel.status === 'failed' || consoleModel.error !== null) {
    return (
      <section className="panel panel--error" role="alert">
        <h2 className="panel__title">Proxy недоступен</h2>
        <p className="panel__text">{consoleModel.error?.message}</p>
        <p className="panel__hint">
          Код ошибки: <code>{consoleModel.error?.code}</code>
        </p>
      </section>
    );
  }

  return (
    <div className="proxy">
      <ListenerPanel console={consoleModel} />
      <SetupHint listeners={consoleModel.listeners} />
      <DecisionFeed
        decisions={consoleModel.decisions}
        listeners={consoleModel.listeners}
        missedDecisions={consoleModel.missedDecisions}
      />
    </div>
  );
}
