/**
 * Runtime-информация приложения: узкий API поверх IPC.
 *
 * Значения не вычисляются на стороне UI — они приходят из Rust, который
 * разрешает платформенные пути и владеет состоянием.
 */

import type { UnlistenFn } from '@tauri-apps/api/event';

import { invokeCommand, subscribeToEvent } from './client';
import { parseRuntimeOverview, type RuntimeOverview } from './types';

/** Событие готовности backend; совпадает с константой в `src-tauri/src/lib.rs`. */
export const RUNTIME_READY_EVENT = 'runtime://ready';

/** Запрашивает runtime-информацию у backend. */
export async function getRuntimeOverview(): Promise<RuntimeOverview> {
  const payload = await invokeCommand<unknown>('get_runtime_overview');
  return parseRuntimeOverview(payload);
}

/** Подписывается на событие готовности backend. */
export function onRuntimeReady(handler: (overview: RuntimeOverview) => void): Promise<UnlistenFn> {
  return subscribeToEvent(RUNTIME_READY_EVENT, (payload) => {
    handler(parseRuntimeOverview(payload));
  });
}
