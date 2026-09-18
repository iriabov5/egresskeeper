import { describe, expect, it } from 'vitest';

import fixture from '../../contracts/ipc/shell_settings.sample.json';
import { parseShellSettings } from './types';

describe('контракт настроек оболочки', () => {
  it('разбирает fixture, который читает Rust', () => {
    expect(parseShellSettings(fixture)).toEqual(fixture);
  });

  it('отклоняет неизвестное поведение при закрытии', () => {
    expect(() => parseShellSettings({ ...fixture, close_behavior: 'minimize' })).toThrow(
      TypeError,
    );
  });

  it('отклоняет нелогические признаки доступности', () => {
    expect(() => parseShellSettings({ ...fixture, tray_available: 'yes' })).toThrow(TypeError);
  });
});
