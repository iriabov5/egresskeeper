import { describe, expect, it } from 'vitest';

import fixture from '../../contracts/ipc/runtime_overview.sample.json';
import {
  RUNTIME_OVERVIEW_FIELDS,
  parseRuntimeOverview,
  toIpcError,
  type RuntimeOverview,
} from './types';

describe('IPC contract fixture', () => {
  it('разбирается в RuntimeOverview', () => {
    const overview = parseRuntimeOverview(fixture);

    expect(overview.app_version).toBe('0.1.0');
    expect(overview.core_version).toBe('0.1.0');
    expect(overview.state_dir).toBe('/home/dev/.local/share/egresskeeper');
    expect(overview.started_at_unix_ms).toBe(1_758_100_000_000);
  });

  it('содержит ровно те поля, что объявлены в контракте', () => {
    const fixtureFields = Object.keys(fixture).sort();
    const contractFields = [...RUNTIME_OVERVIEW_FIELDS].sort();

    expect(fixtureFields).toEqual(contractFields);
  });

  it('покрывает все обязательные поля типа', () => {
    const required: Record<keyof RuntimeOverview, true> = {
      app_version: true,
      core_version: true,
      os: true,
      arch: true,
      state_dir: true,
      started_at_unix_ms: true,
    };

    expect(contractFieldsFrom(required).sort()).toEqual(Object.keys(fixture).sort());
  });
});

describe('parseRuntimeOverview', () => {
  it('отклоняет значение, которое не является объектом', () => {
    expect(() => parseRuntimeOverview('not-an-object')).toThrow(/ожидался объект/);
  });

  it('отклоняет отсутствующее поле', () => {
    const withoutStateDir: Record<string, unknown> = { ...fixture };
    delete withoutStateDir.state_dir;

    expect(() => parseRuntimeOverview(withoutStateDir)).toThrow(/state_dir/);
  });

  it('отклоняет пустую строку', () => {
    expect(() => parseRuntimeOverview({ ...fixture, app_version: '' })).toThrow(/app_version/);
  });

  it('отклоняет нечисловое время старта', () => {
    expect(() => parseRuntimeOverview({ ...fixture, started_at_unix_ms: 'soon' })).toThrow(
      /started_at_unix_ms/,
    );
  });
});

describe('toIpcError', () => {
  it('пропускает типизированную ошибку backend без изменений', () => {
    const error = toIpcError({
      code: 'state_dir_unavailable',
      message: 'Не удалось подготовить каталог состояния приложения.',
    });

    expect(error).toEqual({
      code: 'state_dir_unavailable',
      message: 'Не удалось подготовить каталог состояния приложения.',
    });
  });

  it('нормализует ошибку Tauri вне контракта', () => {
    const error = toIpcError('invalid args for command `get_runtime_overview`');

    expect(error.code).toBe('internal');
    expect(error.message).toBe('Внутренняя ошибка приложения.');
  });

  it('нормализует объект без кода', () => {
    expect(toIpcError({ message: 'boom' }).code).toBe('internal');
  });
});

/** Возвращает список ключей контракта, стабильный к порядку объявления. */
function contractFieldsFrom(required: Record<keyof RuntimeOverview, true>): string[] {
  return Object.keys(required);
}
