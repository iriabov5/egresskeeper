/**
 * Управление политикой: узкий API поверх IPC.
 *
 * Аргументы команд объявлены в `snake_case` — так же, как поля DTO
 * (`src-tauri` использует `rename_all = "snake_case"`), чтобы в контракте была
 * одна нотация.
 */

import { invokeCommand } from './client';
import {
  parseDecision,
  parseProfile,
  parseProfileDetail,
  parseProfileSummaries,
  parseRule,
  parseRules,
  type Action,
  type Decision,
  type Profile,
  type ProfileDetail,
  type ProfileSummary,
  type Rule,
  type RuleInput,
} from './types';

/** Профили с количеством правил. */
export async function listProfiles(): Promise<ProfileSummary[]> {
  return parseProfileSummaries(await invokeCommand<unknown>('policy_list_profiles'));
}

/** Профиль вместе с его правилами. */
export async function profileDetail(profileId: string): Promise<ProfileDetail> {
  return parseProfileDetail(
    await invokeCommand<unknown>('policy_profile_detail', { profile_id: profileId }),
  );
}

/** Создаёт профиль с запрещающим default action. */
export async function createProfile(name: string): Promise<Profile> {
  return parseProfile(await invokeCommand<unknown>('policy_create_profile', { name }));
}

/** Переименовывает профиль. */
export async function renameProfile(profileId: string, name: string): Promise<Profile> {
  return parseProfile(
    await invokeCommand<unknown>('policy_rename_profile', { profile_id: profileId, name }),
  );
}

/** Меняет default action профиля. */
export async function setDefaultAction(profileId: string, action: Action): Promise<Profile> {
  return parseProfile(
    await invokeCommand<unknown>('policy_set_default_action', {
      profile_id: profileId,
      action,
    }),
  );
}

/** Удаляет профиль вместе с его правилами. */
export async function deleteProfile(profileId: string): Promise<void> {
  await invokeCommand<unknown>('policy_delete_profile', { profile_id: profileId });
}

/** Добавляет правило в конец профиля. */
export async function addRule(profileId: string, input: RuleInput): Promise<Rule> {
  return parseRule(
    await invokeCommand<unknown>('policy_add_rule', { profile_id: profileId, input }),
  );
}

/** Изменяет правило, сохраняя его позицию. */
export async function updateRule(ruleId: string, input: RuleInput): Promise<Rule> {
  return parseRule(await invokeCommand<unknown>('policy_update_rule', { rule_id: ruleId, input }));
}

/** Удаляет правило. */
export async function deleteRule(ruleId: string): Promise<void> {
  await invokeCommand<unknown>('policy_delete_rule', { rule_id: ruleId });
}

/** Заменяет порядок правил профиля. */
export async function reorderRules(profileId: string, ruleIds: string[]): Promise<Rule[]> {
  return parseRules(
    await invokeCommand<unknown>('policy_reorder_rules', {
      profile_id: profileId,
      rule_ids: ruleIds,
    }),
  );
}

/** Проверяет, будет ли соединение разрешено политикой профиля. */
export async function evaluate(
  profileId: string,
  host: string,
  port: number,
): Promise<Decision> {
  return parseDecision(
    await invokeCommand<unknown>('policy_evaluate', {
      profile_id: profileId,
      host,
      port,
    }),
  );
}
