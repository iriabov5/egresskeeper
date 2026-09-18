/**
 * Управление proxy: узкий API поверх IPC.
 *
 * Состояние читается командой, а события лишь ускоряют отображение: потеря
 * события не приводит к неверному экрану, потому что статус можно перечитать.
 */

import type { UnlistenFn } from '@tauri-apps/api/event';

import { invokeCommand, subscribeToEvent } from './client';
import {
  parseProxyDecision,
  parseProxyRuntimeView,
  parseProxyStatus,
  type ProxyDecision,
  type ProxyRuntimeView,
  type ProxyStatusView,
} from './types';

/** Событие с решением proxy. */
export const PROXY_DECISION_EVENT = 'proxy://decision';

/** Событие изменения состояния listeners. */
export const PROXY_RUNTIME_EVENT = 'proxy://runtime';

/** Возвращает состояние proxy. */
export async function proxyStatus(): Promise<ProxyStatusView> {
  return parseProxyStatus(await invokeCommand<unknown>('proxy_status'));
}

/** Создаёт listener. */
export async function createListener(port: number, profileId: string): Promise<ProxyStatusView> {
  return parseProxyStatus(
    await invokeCommand<unknown>('proxy_create_listener', {
      port,
      profile_id: profileId,
    }),
  );
}

/** Изменяет конфигурацию listener'а. */
export async function updateListener(
  listenerId: string,
  port: number,
  profileId: string,
): Promise<ProxyStatusView> {
  return parseProxyStatus(
    await invokeCommand<unknown>('proxy_update_listener', {
      listener_id: listenerId,
      port,
      profile_id: profileId,
    }),
  );
}

/** Включает или выключает listener. */
export async function setListenerEnabled(
  listenerId: string,
  enabled: boolean,
): Promise<ProxyStatusView> {
  return parseProxyStatus(
    await invokeCommand<unknown>('proxy_set_listener_enabled', {
      listener_id: listenerId,
      enabled,
    }),
  );
}

/** Удаляет listener. */
export async function deleteListener(listenerId: string): Promise<ProxyStatusView> {
  return parseProxyStatus(
    await invokeCommand<unknown>('proxy_delete_listener', { listener_id: listenerId }),
  );
}

/** Подписывается на решения proxy. */
export function onProxyDecision(handler: (decision: ProxyDecision) => void): Promise<UnlistenFn> {
  return subscribeToEvent(PROXY_DECISION_EVENT, (payload) => {
    handler(parseProxyDecision(payload));
  });
}

/** Подписывается на изменения состояния listeners. */
export function onProxyRuntime(handler: (view: ProxyRuntimeView) => void): Promise<UnlistenFn> {
  return subscribeToEvent(PROXY_RUNTIME_EVENT, (payload) => {
    handler(parseProxyRuntimeView(payload));
  });
}
