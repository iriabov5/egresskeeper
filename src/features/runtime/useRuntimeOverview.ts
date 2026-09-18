import type { UnlistenFn } from '@tauri-apps/api/event';
import { useCallback, useEffect, useState } from 'react';

import { getRuntimeOverview, onRuntimeReady } from '../../ipc/runtime';
import { toIpcError, type IpcError, type RuntimeOverview } from '../../ipc/types';

/** Состояние shell: backend инициализируется, готов или не смог стартовать. */
export type RuntimeState =
  | { readonly status: 'starting' }
  | { readonly status: 'ready'; readonly overview: RuntimeOverview }
  | { readonly status: 'failed'; readonly error: IpcError };

/** Результат хука: состояние и попытка повторного запроса. */
export interface RuntimeOverviewHandle {
  readonly state: RuntimeState;
  readonly retry: () => void;
}

/**
 * Приводит shell к согласованному состоянию независимо от гонки старта.
 *
 * Запускаются два пути одновременно: подписка на событие готовности backend и
 * запрос runtime-информации командой. Событие может прийти раньше подписки UI —
 * тогда состояние придёт ответом команды; если событие пришло после подписки,
 * UI переходит в `ready` без дополнительного запроса. Ошибка команды не
 * затирает уже полученное состояние `ready`.
 */
export function useRuntimeOverview(): RuntimeOverviewHandle {
  const [state, setState] = useState<RuntimeState>({ status: 'starting' });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | undefined;

    onRuntimeReady((overview) => {
      if (active) {
        setState({ status: 'ready', overview });
      }
    })
      .then((release) => {
        if (active) {
          unlisten = release;
        } else {
          release();
        }
      })
      .catch(() => {
        // Подписка на событие недоступна: актуальное состояние придёт командой.
      });

    getRuntimeOverview()
      .then((overview) => {
        if (active) {
          setState({ status: 'ready', overview });
        }
      })
      .catch((cause: unknown) => {
        if (!active) {
          return;
        }
        const error = toIpcError(cause);
        setState((current) =>
          current.status === 'ready' ? current : { status: 'failed', error },
        );
      });

    return () => {
      active = false;
      unlisten?.();
    };
  }, [attempt]);

  const retry = useCallback(() => {
    setState({ status: 'starting' });
    setAttempt((current) => current + 1);
  }, []);

  return { state, retry };
}
