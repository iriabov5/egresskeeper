import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import fixture from '../../../contracts/ipc/shell_settings.sample.json';
import { SettingsScreen } from './SettingsScreen';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

/** Настраивает ответы backend и запоминает вызовы команд. */
function respond(overrides: Record<string, unknown> = {}) {
  const handlers: Record<string, unknown> = {
    settings_get: fixture,
    settings_set_close_behavior: fixture,
    settings_set_autostart: fixture,
    ...overrides,
  };

  invokeMock.mockImplementation((command: string) => {
    const answer = handlers[command];

    if (answer === undefined) {
      return Promise.reject({ code: 'internal', message: `неожиданная команда: ${command}` });
    }

    // Ответ с полем `code` — структурная ошибка IPC, а не представление.
    const isError = typeof answer === 'object' && answer !== null && 'code' in answer;

    return isError || answer instanceof Error
      ? Promise.reject(answer)
      : Promise.resolve(answer);
  });
}

/** Находит переключатель по подписи. */
function option(label: string): HTMLInputElement {
  return screen.getByLabelText(label, { exact: false }) as HTMLInputElement;
}

describe('экран настроек', () => {
  it('показывает фактические настройки из backend', async () => {
    respond();
    render(<SettingsScreen />);

    expect(await screen.findByRole('radio', { name: /Скрывать в трей/ })).toBeChecked();
    expect(screen.getByRole('radio', { name: /Завершать приложение/ })).not.toBeChecked();
    expect(option('Запускать при входе в систему')).not.toBeChecked();
  });

  it('отправляет выбранное поведение при закрытии окна', async () => {
    respond({
      settings_set_close_behavior: { ...fixture, close_behavior: 'quit' },
    });
    render(<SettingsScreen />);

    await userEvent.click(await screen.findByRole('radio', { name: /Завершать приложение/ }));

    expect(invokeMock).toHaveBeenCalledWith('settings_set_close_behavior', {
      close_behavior: 'quit',
    });
    expect(option('Завершать приложение')).toBeChecked();
  });

  it('включает автозапуск и показывает подтверждённое состояние', async () => {
    respond({ settings_set_autostart: { ...fixture, autostart_enabled: true } });
    render(<SettingsScreen />);

    await userEvent.click(await screen.findByLabelText(/Запускать при входе в систему/));

    expect(invokeMock).toHaveBeenCalledWith('settings_set_autostart', { enabled: true });
    expect(option('Запускать при входе в систему')).toBeChecked();
  });

  it('блокирует недоступные возможности с пояснением', async () => {
    respond({
      settings_get: {
        ...fixture,
        close_behavior: 'quit',
        tray_available: false,
        autostart_supported: false,
      },
    });
    render(<SettingsScreen />);

    expect(await screen.findByRole('radio', { name: /Скрывать в трей/ })).toBeDisabled();
    expect(option('Запускать при входе в систему')).toBeDisabled();
    expect(screen.getByText(/Трей недоступен на этой платформе/)).toBeInTheDocument();
    expect(screen.getByText(/platform_feature_unavailable/)).toBeInTheDocument();
  });

  it('показывает ошибку изменения, не выдавая её за успех', async () => {
    respond({
      settings_set_autostart: { code: 'platform_feature_unavailable', message: 'Автозапуск недоступен.' },
    });
    render(<SettingsScreen />);

    await userEvent.click(await screen.findByLabelText(/Запускать при входе в систему/));

    const alert = await screen.findByRole('alert');
    expect(within(alert).getByText('Автозапуск недоступен.')).toBeInTheDocument();
    expect(option('Запускать при входе в систему')).not.toBeChecked();
  });

  it('показывает ошибку загрузки настроек', async () => {
    respond({ settings_get: { code: 'internal', message: 'Хранилище недоступно.' } });
    render(<SettingsScreen />);

    const alert = await screen.findByRole('alert');
    expect(within(alert).getByText('Хранилище недоступно.')).toBeInTheDocument();
  });
});
