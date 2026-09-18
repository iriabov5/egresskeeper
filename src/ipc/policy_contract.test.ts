import { describe, expect, it } from 'vitest';

import decisionFixture from '../../contracts/ipc/policy_decision.sample.json';
import detailFixture from '../../contracts/ipc/policy_profile_detail.sample.json';
import profileFixture from '../../contracts/ipc/policy_profile.sample.json';
import ruleFixture from '../../contracts/ipc/policy_rule.sample.json';
import {
  parseDecision,
  parseProfile,
  parseProfileDetail,
  parseProfileSummaries,
  parseRule,
  toIpcError,
} from './types';

describe('контракт политик', () => {
  it('разбирает профиль', () => {
    const profile = parseProfile(profileFixture);

    expect(profile.name).toBe('Работа');
    expect(profile.default_action).toBe('deny');
  });

  it('разбирает правило', () => {
    const rule = parseRule(ruleFixture);

    expect(rule.position).toBe(0);
    expect(rule.action).toBe('allow');
    expect(rule.host).toEqual({ kind: 'subdomains', value: 'example.com' });
    expect(rule.port).toEqual({ kind: 'exactly', port: 443 });
  });

  it('разбирает профиль с правилами и сохраняет порядок', () => {
    const detail = parseProfileDetail(detailFixture);

    expect(detail.rules.map((rule) => rule.position)).toEqual([0, 1]);
    expect(detail.rules[1]?.port).toEqual({ kind: 'any' });
  });

  it('разбирает решение со сработавшим правилом', () => {
    const decision = parseDecision(decisionFixture);

    expect(decision.action).toBe('allow');
    expect(decision.reason).toEqual({
      kind: 'matched_rule',
      rule_id: '9c858901-8a57-4791-81fe-4c455b099bc9',
      position: 0,
    });
  });

  it('разбирает список профилей с количеством правил', () => {
    const summaries = parseProfileSummaries([{ profile: profileFixture, rule_count: 2 }]);

    expect(summaries).toHaveLength(1);
    expect(summaries[0]?.rule_count).toBe(2);
  });

  it('отклоняет правило с неизвестным видом ограничения порта', () => {
    expect(() => parseRule({ ...ruleFixture, port: { kind: 'even' } })).toThrow(/порт/);
  });

  it('отклоняет правило с неизвестным действием', () => {
    expect(() => parseRule({ ...ruleFixture, action: 'maybe' })).toThrow(/действие/);
  });

  it('отклоняет решение с неизвестной причиной', () => {
    expect(() =>
      parseDecision({ action: 'deny', reason: { kind: 'coin_flip' } }),
    ).toThrow(/причина/);
  });

  it('отклоняет значение, которое не является объектом', () => {
    expect(() => parseProfile('Работа')).toThrow(/ожидался объект/);
    expect(() => parseProfileSummaries({})).toThrow(/массив/);
  });
});

describe('нормализация ошибок IPC', () => {
  it('сохраняет код, сообщение и поле валидации', () => {
    const error = toIpcError({
      code: 'validation',
      message: 'Запрос содержит недопустимые данные.',
      details: { field: 'host' },
    });

    expect(error.code).toBe('validation');
    expect(error.details).toEqual({ field: 'host' });
  });

  it('отбрасывает детали без корректного поля', () => {
    const error = toIpcError({ code: 'not_found', message: 'нет', details: { field: 42 } });

    expect(error.details).toBeUndefined();
  });

  it('нормализует ошибку вне контракта', () => {
    const error = toIpcError('invalid args for command `policy_add_rule`');

    expect(error.code).toBe('internal');
    expect(error.message).toBe('Внутренняя ошибка приложения.');
  });
});
