import { useState } from 'react';

import { PolicyScreen } from './features/policy/PolicyScreen';
import { ProxyScreen } from './features/proxy/ProxyScreen';
import { RuntimeStatusView } from './features/runtime/RuntimeStatusView';
import { SettingsScreen } from './features/settings/SettingsScreen';
import { useRuntimeOverview } from './features/runtime/useRuntimeOverview';

type Screen = 'status' | 'policies' | 'proxy' | 'settings';

/**
 * Корневой компонент shell.
 *
 * Держит состояние готовности backend (оно общее для экранов) и переключает
 * экраны. Бизнес-логики здесь нет, она живёт в Rust.
 */
export function App() {
  const { state, retry } = useRuntimeOverview();
  const [screen, setScreen] = useState<Screen>('status');

  return (
    <div className="app">
      <header className="app__header">
        <h1 className="app__title">EgressKeeper</h1>
        <p className="app__subtitle">
          Локальный контроль egress для AI-агентов, MCP-серверов и dev-инструментов
        </p>
        <nav className="tabs" aria-label="Экраны приложения">
          <button
            type="button"
            className={screen === 'status' ? 'tabs__item tabs__item--active' : 'tabs__item'}
            aria-current={screen === 'status' ? 'page' : undefined}
            onClick={() => setScreen('status')}
          >
            Состояние
          </button>
          <button
            type="button"
            className={screen === 'policies' ? 'tabs__item tabs__item--active' : 'tabs__item'}
            aria-current={screen === 'policies' ? 'page' : undefined}
            onClick={() => setScreen('policies')}
          >
            Политики
          </button>
          <button
            type="button"
            className={screen === 'proxy' ? 'tabs__item tabs__item--active' : 'tabs__item'}
            aria-current={screen === 'proxy' ? 'page' : undefined}
            onClick={() => setScreen('proxy')}
          >
            Прокси
          </button>
          <button
            type="button"
            className={screen === 'settings' ? 'tabs__item tabs__item--active' : 'tabs__item'}
            aria-current={screen === 'settings' ? 'page' : undefined}
            onClick={() => setScreen('settings')}
          >
            Настройки
          </button>
        </nav>
      </header>

      {screen === 'status' && <RuntimeStatusView state={state} onRetry={retry} />}
      {screen === 'policies' && <PolicyScreen />}
      {screen === 'proxy' && <ProxyScreen />}
      {screen === 'settings' && <SettingsScreen />}
    </div>
  );
}
