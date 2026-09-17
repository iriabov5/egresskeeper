import type { UnlistenFn } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef, useState } from 'react';

import * as policyApi from '../../ipc/policy';
import * as proxyApi from '../../ipc/proxy';
import {
  toIpcError,
  type IpcError,
  type ListenerView,
  type ProfileSummary,
  type ProxyDecision,
  type ProxyRuntimeView,
  type ProxyStatusView,
} from '../../ipc/types';

/** Сколько решений удерживается в ленте. */
export const DECISION_FEED_LIMIT = 200;

/**
 * Добавляет решение в начало ленты, удерживая её длину в пределе.
 *
 * Вынесено из обработчика события: вложение колбэков внутри обработки состояния
 * делает поток данных нечитаемым.
 */
function prependDecision(feed: ProxyDecision[], decision: ProxyDecision): ProxyDecision[] {
  return [decision, ...feed].slice(0, DECISION_FEED_LIMIT);
}

/**
 * Объединяет фактическое состояние из рантайма с уже загруженной конфигурацией.
 *
 * Конфигурация приходит командами, состояние — событием, поэтому сопоставляем их
 * по идентификатору listener'а.
 */
function mergeRuntimeState(
  listeners: ListenerView[],
  view: ProxyRuntimeView,
): ListenerView[] {
  return listeners.map((listener) => {
    const health = view.listeners.find(
      (candidate) => candidate.listener_id === listener.listener.id,
    );

    return health === undefined
      ? listener
      : {
          ...listener,
          state: health.state,
          active_connections: health.active_connections,
        };
  });
}

/** Состояние загрузки экрана proxy. */
export type ProxyStatus = 'loading' | 'ready' | 'failed';

/** Модель экрана «Прокси». */
export interface ProxyConsole {
  status: ProxyStatus;
  error: IpcError | null;
  listeners: ListenerView[];
  profiles: ProfileSummary[];
  decisions: ProxyDecision[];
  /**
   * Решения, не попавшие в ленту: пропущенные backend'ом (абсолютный счётчик) и
   * вытесненные из ленты самой UI.
   */
  missedDecisions: number;
  createListener: (port: number, profileId: string) => Promise<IpcError | null>;
  setEnabled: (listenerId: string, enabled: boolean) => Promise<IpcError | null>;
  removeListener: (listenerId: string) => Promise<IpcError | null>;
}

/**
 * Управляет данными экрана «Прокси».
 *
 * Состояние читается командой, события используются как ускоритель: решения
 * приходят потоком, изменения состояния — отдельным событием, а конфигурация
 * listeners — ответами команд. Лента ограничена по длине, поэтому число
 * пропущенных решений складывается из пропущенных backend'ом и вытесненных из
 * ленты.
 */
export function useProxyConsole(): ProxyConsole {
  const [status, setStatus] = useState<ProxyStatus>('loading');
  const [error, setError] = useState<IpcError | null>(null);
  const [listeners, setListeners] = useState<ListenerView[]>([]);
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [decisions, setDecisions] = useState<ProxyDecision[]>([]);
  const [backendDropped, setBackendDropped] = useState(0);
  const [feedEvicted, setFeedEvicted] = useState(0);
  const received = useRef(0);

  const applyStatus = useCallback((next: ProxyStatusView) => {
    setListeners(next.listeners);
    setBackendDropped(next.dropped_decisions);
    setStatus('ready');
    setError(null);
  }, []);

  const load = useCallback(async () => {
    try {
      const [proxyStatus, profileList] = await Promise.all([
        proxyApi.proxyStatus(),
        policyApi.listProfiles(),
      ]);

      setProfiles(profileList);
      applyStatus(proxyStatus);
    } catch (cause) {
      setStatus('failed');
      setError(toIpcError(cause));
    }
  }, [applyStatus]);

  useEffect(() => {
    void load();
  }, [load]);

  const handleDecision = useCallback((decision: ProxyDecision) => {
    received.current += 1;
    setDecisions((current) => prependDecision(current, decision));
    // Вытесненные из ленты решения тоже пропущены пользователем.
    setFeedEvicted(Math.max(0, received.current - DECISION_FEED_LIMIT));
  }, []);

  const handleRuntime = useCallback((view: ProxyRuntimeView) => {
    setListeners((current) => mergeRuntimeState(current, view));
    // Счётчик backend'а абсолютный: он заменяет прежнее значение, а не
    // накапливается с ним.
    setBackendDropped(view.dropped_decisions);
  }, []);

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    let active = true;

    const registerFailure = () => {
      // События недоступны: состояние всё равно читается командой.
    };

    const register = (release: UnlistenFn) => {
      if (active) {
        unlisteners.push(release);
      } else {
        release();
      }
    };

    void proxyApi.onProxyDecision(handleDecision).then(register).catch(registerFailure);
    void proxyApi.onProxyRuntime(handleRuntime).then(register).catch(registerFailure);

    return () => {
      active = false;

      for (const release of unlisteners) {
        release();
      }
    };
  }, [handleDecision, handleRuntime]);

  const run = useCallback(
    async (operation: () => Promise<ProxyStatusView>) => {
      try {
        applyStatus(await operation());
        return null;
      } catch (cause) {
        return toIpcError(cause);
      }
    },
    [applyStatus],
  );

  const createListener = useCallback(
    (port: number, profileId: string) => run(() => proxyApi.createListener(port, profileId)),
    [run],
  );

  const setEnabled = useCallback(
    (listenerId: string, enabled: boolean) =>
      run(() => proxyApi.setListenerEnabled(listenerId, enabled)),
    [run],
  );

  const removeListener = useCallback(
    (listenerId: string) => run(() => proxyApi.deleteListener(listenerId)),
    [run],
  );

  return {
    status,
    error,
    listeners,
    profiles,
    decisions,
    missedDecisions: backendDropped + feedEvicted,
    createListener,
    setEnabled,
    removeListener,
  };
}
