/**
 * Типы IPC-контракта: зеркало Rust DTO.
 *
 * Источник истины — Rust (`egresskeeper-core`). Общие fixtures в
 * `contracts/ipc/` проверяются тестами с обеих сторон, поэтому расхождение имён
 * и полей становится падающим тестом, а не ошибкой времени выполнения.
 *
 * Данные из IPC не доверяются: значения проходят проверку формы
 * (`parse*`) перед использованием в UI.
 */

/* ------------------------------------------------------------------ runtime */

/** Зеркало Rust-типа `RuntimeOverview`. */
export interface RuntimeOverview {
  app_version: string;
  core_version: string;
  os: string;
  arch: string;
  state_dir: string;
  started_at_unix_ms: number;
}

/** Поля контракта `RuntimeOverview` в порядке, зафиксированном fixture. */
export const RUNTIME_OVERVIEW_FIELDS = [
  'app_version',
  'core_version',
  'os',
  'arch',
  'state_dir',
  'started_at_unix_ms',
] as const satisfies readonly (keyof RuntimeOverview)[];

/* ------------------------------------------------------------------ ошибки */

/** Известные machine-readable коды ошибок backend. */
export type ErrorCode =
  | 'validation'
  | 'not_found'
  | 'storage_unavailable'
  | 'state_dir_unavailable'
  | 'internal';

/** Структурированные детали ошибки. */
export interface IpcErrorDetails {
  /** Имя поля контракта, не прошедшего валидацию. */
  field: string;
}

/** Зеркало Rust-типа `IpcError`. */
export interface IpcError {
  /**
   * Код ошибки. Известные коды перечислены в `ErrorCode`, но backend может
   * вернуть и новый: `(string & {})` сохраняет подсказки редактора и не сужает
   * тип до простого `string`.
   */
  code: ErrorCode | (string & {});
  message: string;
  details?: IpcErrorDetails;
}

/* ------------------------------------------------------------------ политика */

/** Действие политики. */
export type Action = 'allow' | 'deny';

/** Вид сопоставления host. */
export type HostKind = 'exact' | 'subdomains';

/** Ограничение порта. */
export type PortSpec =
  | { kind: 'any' }
  | { kind: 'exactly'; port: number }
  | { kind: 'range'; start: number; end: number };

/** Сопоставление host правила. */
export interface HostMatcher {
  kind: HostKind;
  value: string;
}

/** Профиль политики. */
export interface Profile {
  id: string;
  name: string;
  default_action: Action;
  created_at_unix_ms: number;
  updated_at_unix_ms: number;
}

/** Правило профиля. */
export interface Rule {
  id: string;
  profile_id: string;
  position: number;
  action: Action;
  host: HostMatcher;
  port: PortSpec;
}

/** Профиль вместе с количеством правил. */
export interface ProfileSummary {
  profile: Profile;
  rule_count: number;
}

/** Профиль вместе с его правилами. */
export interface ProfileDetail {
  profile: Profile;
  rules: Rule[];
}

/** Причина решения. */
export type DecisionReason =
  | { kind: 'matched_rule'; rule_id: string; position: number }
  | { kind: 'default_action'; profile_id: string; action: Action };

/** Решение по соединению. */
export interface Decision {
  action: Action;
  reason: DecisionReason;
}

/** Ввод правила: значения проверяет backend. */
export interface RuleInput {
  action: Action;
  host_kind: HostKind;
  host: string;
  port: PortSpec;
}

/* ------------------------------------------------------------------ proxy */

/** Состояние listener'а. */
export type ListenerStateView =
  | { state: 'stopped' }
  | { state: 'starting' }
  | { state: 'running' }
  | { state: 'failed'; code: string; message: string };

/** Конфигурация listener'а proxy. */
export interface Listener {
  id: string;
  port: number;
  profile_id: string;
  enabled: boolean;
  created_at_unix_ms: number;
  updated_at_unix_ms: number;
}

/** Listener вместе с фактическим состоянием. */
export interface ListenerView {
  listener: Listener;
  state: ListenerStateView;
  active_connections: number;
}

/** Состояние proxy целиком. */
export interface ProxyStatusView {
  listeners: ListenerView[];
  dropped_decisions: number;
}

/** Состояние одного listener'а в событии изменения состояния. */
export interface ListenerHealthView {
  listener_id: string;
  state: ListenerStateView;
  active_connections: number;
}

/** Состояние рантайма proxy в событии. */
export interface ProxyRuntimeView {
  listeners: ListenerHealthView[];
  dropped_decisions: number;
}

/** Причина решения proxy. */
export type ProxyDecisionReason =
  | { kind: 'policy'; decision: DecisionReason }
  | { kind: 'policy_unavailable' }
  | { kind: 'loopback_target' };

/** Решение proxy по соединению. */
export interface ProxyDecision {
  listener_id: string;
  host: string;
  port: number;
  action: Action;
  reason: ProxyDecisionReason;
  at_unix_ms: number;
}

/* ------------------------------------------------------- настройки оболочки */

/** Поведение приложения при закрытии главного окна. */
export type CloseBehavior = 'hide_to_tray' | 'quit';

/** Настройки оболочки вместе с состоянием платформенных возможностей. */
export interface ShellSettingsView {
  close_behavior: CloseBehavior;
  tray_available: boolean;
  autostart_supported: boolean;
  autostart_enabled: boolean;
}

/* ------------------------------------------------------------------ разбор */

/**
 * Приводит произвольное значение к `IpcError`.
 *
 * Tauri умеет возвращать ошибки вне нашего контракта (например, отказ при
 * вызове незарегистрированной команды), поэтому клиент нормализует любую ошибку:
 * UI всегда получает `code` и безопасное сообщение.
 */
export function toIpcError(cause: unknown): IpcError {
  if (typeof cause === 'object' && cause !== null) {
    const { code, message, details } = cause as {
      code?: unknown;
      message?: unknown;
      details?: unknown;
    };

    if (typeof code === 'string' && code.length > 0 && typeof message === 'string') {
      const normalized: IpcError = { code, message };
      const field = details === null || typeof details !== 'object'
        ? undefined
        : (details as { field?: unknown }).field;

      if (typeof field === 'string' && field.length > 0) {
        normalized.details = { field };
      }

      return normalized;
    }
  }

  return { code: 'internal', message: 'Внутренняя ошибка приложения.' };
}

/** Проверяет форму значения из IPC перед использованием в UI. */
export function parseRuntimeOverview(value: unknown): RuntimeOverview {
  const candidate = asObject(value, 'runtime overview');

  const startedAt = candidate.started_at_unix_ms;
  if (typeof startedAt !== 'number' || !Number.isFinite(startedAt)) {
    throw new TypeError('runtime overview: поле `started_at_unix_ms` должно быть числом');
  }

  return {
    app_version: asString(candidate.app_version, 'app_version', 'runtime overview'),
    core_version: asString(candidate.core_version, 'core_version', 'runtime overview'),
    os: asString(candidate.os, 'os', 'runtime overview'),
    arch: asString(candidate.arch, 'arch', 'runtime overview'),
    state_dir: asString(candidate.state_dir, 'state_dir', 'runtime overview'),
    started_at_unix_ms: startedAt,
  };
}

/** Разбирает ограничение порта. */
export function parsePortSpec(value: unknown): PortSpec {
  const candidate = asObject(value, 'port');

  switch (candidate.kind) {
    case 'any':
      return { kind: 'any' };
    case 'exactly':
      return { kind: 'exactly', port: asNumber(candidate.port, 'port', 'port') };
    case 'range':
      return {
        kind: 'range',
        start: asNumber(candidate.start, 'start', 'port'),
        end: asNumber(candidate.end, 'end', 'port'),
      };
    default:
      throw new TypeError('port: неизвестный вид ограничения порта');
  }
}

/** Разбирает сопоставление host. */
export function parseHostMatcher(value: unknown): HostMatcher {
  const candidate = asObject(value, 'host');

  if (candidate.kind !== 'exact' && candidate.kind !== 'subdomains') {
    throw new TypeError('host: неизвестный вид сопоставления');
  }

  return {
    kind: candidate.kind,
    value: asString(candidate.value, 'value', 'host'),
  };
}

/** Разбирает профиль. */
export function parseProfile(value: unknown): Profile {
  const candidate = asObject(value, 'profile');

  return {
    id: asString(candidate.id, 'id', 'profile'),
    name: asString(candidate.name, 'name', 'profile'),
    default_action: asAction(candidate.default_action, 'profile'),
    created_at_unix_ms: asNumber(candidate.created_at_unix_ms, 'created_at_unix_ms', 'profile'),
    updated_at_unix_ms: asNumber(candidate.updated_at_unix_ms, 'updated_at_unix_ms', 'profile'),
  };
}

/** Разбирает правило. */
export function parseRule(value: unknown): Rule {
  const candidate = asObject(value, 'rule');

  return {
    id: asString(candidate.id, 'id', 'rule'),
    profile_id: asString(candidate.profile_id, 'profile_id', 'rule'),
    position: asNumber(candidate.position, 'position', 'rule'),
    action: asAction(candidate.action, 'rule'),
    host: parseHostMatcher(candidate.host),
    port: parsePortSpec(candidate.port),
  };
}

/** Разбирает профиль с количеством правил. */
export function parseProfileSummary(value: unknown): ProfileSummary {
  const candidate = asObject(value, 'profile summary');

  return {
    profile: parseProfile(candidate.profile),
    rule_count: asNumber(candidate.rule_count, 'rule_count', 'profile summary'),
  };
}

/** Разбирает список профилей. */
export function parseProfileSummaries(value: unknown): ProfileSummary[] {
  return asArray(value, 'profiles').map(parseProfileSummary);
}

/** Разбирает профиль вместе с правилами. */
export function parseProfileDetail(value: unknown): ProfileDetail {
  const candidate = asObject(value, 'profile detail');

  return {
    profile: parseProfile(candidate.profile),
    rules: asArray(candidate.rules, 'rules').map(parseRule),
  };
}

/** Разбирает список правил. */
export function parseRules(value: unknown): Rule[] {
  return asArray(value, 'rules').map(parseRule);
}

/** Разбирает решение по соединению. */
export function parseDecision(value: unknown): Decision {
  const candidate = asObject(value, 'decision');

  return {
    action: asAction(candidate.action, 'decision'),
    reason: parseDecisionReason(candidate.reason),
  };
}

/** Разбирает настройки оболочки. */
export function parseShellSettings(value: unknown): ShellSettingsView {
  const candidate = asObject(value, 'shell settings');

  if (candidate.close_behavior !== 'hide_to_tray' && candidate.close_behavior !== 'quit') {
    throw new TypeError('shell settings: неизвестное поведение при закрытии окна');
  }

  for (const field of ['tray_available', 'autostart_supported', 'autostart_enabled'] as const) {
    if (typeof candidate[field] !== 'boolean') {
      throw new TypeError(`shell settings: поле \`${field}\` должно быть логическим`);
    }
  }

  return {
    close_behavior: candidate.close_behavior,
    tray_available: candidate.tray_available as boolean,
    autostart_supported: candidate.autostart_supported as boolean,
    autostart_enabled: candidate.autostart_enabled as boolean,
  };
}

/** Разбирает состояние listener'а. */
export function parseListenerState(value: unknown): ListenerStateView {
  const candidate = asObject(value, 'listener state');

  switch (candidate.state) {
    case 'stopped':
      return { state: 'stopped' };
    case 'starting':
      return { state: 'starting' };
    case 'running':
      return { state: 'running' };
    case 'failed':
      return {
        state: 'failed',
        code: asString(candidate.code, 'code', 'listener state'),
        message: asString(candidate.message, 'message', 'listener state'),
      };
    default:
      throw new TypeError('listener state: неизвестное состояние');
  }
}

/** Разбирает конфигурацию listener'а. */
export function parseListener(value: unknown): Listener {
  const candidate = asObject(value, 'listener');

  if (typeof candidate.enabled !== 'boolean') {
    throw new TypeError('listener: поле `enabled` должно быть логическим');
  }

  return {
    id: asString(candidate.id, 'id', 'listener'),
    port: asNumber(candidate.port, 'port', 'listener'),
    profile_id: asString(candidate.profile_id, 'profile_id', 'listener'),
    enabled: candidate.enabled,
    created_at_unix_ms: asNumber(candidate.created_at_unix_ms, 'created_at_unix_ms', 'listener'),
    updated_at_unix_ms: asNumber(candidate.updated_at_unix_ms, 'updated_at_unix_ms', 'listener'),
  };
}

/** Разбирает listener вместе с состоянием. */
export function parseListenerView(value: unknown): ListenerView {
  const candidate = asObject(value, 'listener view');

  return {
    listener: parseListener(candidate.listener),
    state: parseListenerState(candidate.state),
    active_connections: asNumber(
      candidate.active_connections,
      'active_connections',
      'listener view',
    ),
  };
}

/** Разбирает состояние proxy. */
export function parseProxyStatus(value: unknown): ProxyStatusView {
  const candidate = asObject(value, 'proxy status');

  return {
    listeners: asArray(candidate.listeners, 'listeners').map(parseListenerView),
    dropped_decisions: asNumber(candidate.dropped_decisions, 'dropped_decisions', 'proxy status'),
  };
}

/** Разбирает состояние рантайма из события. */
export function parseProxyRuntimeView(value: unknown): ProxyRuntimeView {
  const candidate = asObject(value, 'proxy runtime');

  return {
    listeners: asArray(candidate.listeners, 'listeners').map((entry) => {
      const listener = asObject(entry, 'proxy runtime listener');

      return {
        listener_id: asString(listener.listener_id, 'listener_id', 'proxy runtime'),
        state: parseListenerState(listener.state),
        active_connections: asNumber(
          listener.active_connections,
          'active_connections',
          'proxy runtime',
        ),
      };
    }),
    dropped_decisions: asNumber(
      candidate.dropped_decisions,
      'dropped_decisions',
      'proxy runtime',
    ),
  };
}

/** Разбирает решение proxy. */
export function parseProxyDecision(value: unknown): ProxyDecision {
  const candidate = asObject(value, 'proxy decision');
  const reason = asObject(candidate.reason, 'proxy decision');

  let parsedReason: ProxyDecisionReason;
  switch (reason.kind) {
    case 'policy':
      parsedReason = { kind: 'policy', decision: parseDecisionReason(reason.decision) };
      break;
    case 'policy_unavailable':
      parsedReason = { kind: 'policy_unavailable' };
      break;
    case 'loopback_target':
      parsedReason = { kind: 'loopback_target' };
      break;
    default:
      throw new TypeError('proxy decision: неизвестная причина');
  }

  return {
    listener_id: asString(candidate.listener_id, 'listener_id', 'proxy decision'),
    host: asString(candidate.host, 'host', 'proxy decision'),
    port: asNumber(candidate.port, 'port', 'proxy decision'),
    action: asAction(candidate.action, 'proxy decision'),
    reason: parsedReason,
    at_unix_ms: asNumber(candidate.at_unix_ms, 'at_unix_ms', 'proxy decision'),
  };
}

/** Разбирает причину доменного решения. */
function parseDecisionReason(value: unknown): DecisionReason {
  const reason = asObject(value, 'decision');

  switch (reason.kind) {
    case 'matched_rule':
      return {
        kind: 'matched_rule',
        rule_id: asString(reason.rule_id, 'rule_id', 'decision'),
        position: asNumber(reason.position, 'position', 'decision'),
      };
    case 'default_action':
      return {
        kind: 'default_action',
        profile_id: asString(reason.profile_id, 'profile_id', 'decision'),
        action: asAction(reason.action, 'decision'),
      };
    default:
      throw new TypeError('decision: неизвестная причина решения');
  }
}

/* ------------------------------------------------------------------ helpers */

function asObject(value: unknown, context: string): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new TypeError(`${context}: ожидался объект`);
  }

  return value as Record<string, unknown>;
}

function asArray(value: unknown, field: string): unknown[] {
  if (!Array.isArray(value)) {
    throw new TypeError(`ожидался массив в поле \`${field}\``);
  }

  return value;
}

function asString(value: unknown, field: string, context: string): string {
  if (typeof value !== 'string' || value.length === 0) {
    throw new TypeError(`${context}: поле \`${field}\` должно быть непустой строкой`);
  }

  return value;
}

function asNumber(value: unknown, field: string, context: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    throw new TypeError(`${context}: поле \`${field}\` должно быть числом`);
  }

  return value;
}

function asAction(value: unknown, context: string): Action {
  if (value === 'allow' || value === 'deny') {
    return value;
  }

  throw new TypeError(`${context}: неизвестное действие`);
}
