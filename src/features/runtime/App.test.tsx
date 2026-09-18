import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { act } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import fixture from '../../../contracts/ipc/runtime_overview.sample.json';
import { App } from '../../App';

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

interface EventMessage {
  readonly payload: unknown;
}

type EventHandler = (message: EventMessage) => void;

/** Подписывает мок события и возвращает доступ к зарегистрированному обработчику. */
function captureReadyHandler(): () => EventHandler | undefined {
  let handler: EventHandler | undefined;

  listenMock.mockImplementation((_event: string, callback: EventHandler) => {
    handler = callback;
    return Promise.resolve(() => {});
  });

  return () => handler;
}

/** Возвращает значение строки списка фактов по подписи. */
function factValue(label: string): string {
  const term = screen.getByText(label);
  return term.parentElement?.querySelector('dd')?.textContent ?? '';
}

describe('shell readiness', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
  });

  it('показывает состояние starting, пока backend не ответил', () => {
    listenMock.mockResolvedValue(() => {});
    invokeMock.mockReturnValue(new Promise(() => {}));

    render(<App />);

    expect(screen.getByText('Инициализация backend…')).toBeInTheDocument();
    expect(screen.getByText(/запрашивает состояние у Rust-процесса/)).toBeInTheDocument();
  });

  it('переходит в ready по событию, даже если команда ещё не ответила', async () => {
    const readyHandler = captureReadyHandler();
    invokeMock.mockReturnValue(new Promise(() => {}));

    render(<App />);
    await waitFor(() => expect(readyHandler()).toBeDefined());

    act(() => {
      readyHandler()?.({ payload: fixture });
    });

    expect(await screen.findByText('Приложение готово')).toBeInTheDocument();
    expect(screen.getByText(fixture.state_dir)).toBeInTheDocument();
  });

  it('переходит в ready по ответу команды, если событие не пришло', async () => {
    listenMock.mockResolvedValue(() => {});
    invokeMock.mockResolvedValue(fixture);

    render(<App />);

    expect(await screen.findByText('Приложение готово')).toBeInTheDocument();
    expect(factValue('Версия приложения')).toBe(fixture.app_version);
    expect(factValue('Версия ядра')).toBe(fixture.core_version);
    expect(factValue('Платформа')).toBe(`${fixture.os} / ${fixture.arch}`);
    expect(factValue('Каталог состояния')).toBe(fixture.state_dir);
  });

  it('переходит в failed и показывает код ошибки', async () => {
    listenMock.mockResolvedValue(() => {});
    invokeMock.mockRejectedValue({
      code: 'state_dir_unavailable',
      message: 'Не удалось подготовить каталог состояния приложения.',
    });

    render(<App />);

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Не удалось подготовить каталог состояния приложения.');
    expect(alert).toHaveTextContent('state_dir_unavailable');
  });

  it('нормализует ошибку вне контракта в код internal', async () => {
    listenMock.mockResolvedValue(() => {});
    invokeMock.mockRejectedValue('invalid args for command `get_runtime_overview`');

    render(<App />);

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('internal');
    expect(alert).not.toHaveTextContent('invalid args');
  });

  it('повторный запрос переводит shell из failed в ready', async () => {
    const user = userEvent.setup();
    listenMock.mockResolvedValue(() => {});
    invokeMock.mockRejectedValueOnce({ code: 'internal', message: 'Внутренняя ошибка приложения.' });
    invokeMock.mockResolvedValueOnce(fixture);

    render(<App />);
    await screen.findByRole('alert');

    await user.click(screen.getByRole('button', { name: 'Повторить' }));

    expect(await screen.findByText('Приложение готово')).toBeInTheDocument();
  });

  it('переключает экраны и сохраняет состояние backend', async () => {
    const user = userEvent.setup();
    listenMock.mockResolvedValue(() => {});
    invokeMock.mockImplementation((command: string) => {
      if (command === 'get_runtime_overview') {
        return Promise.resolve(fixture);
      }
      if (command === 'policy_list_profiles') {
        return Promise.resolve([]);
      }
      return Promise.reject({ code: 'internal', message: `неожиданная команда: ${command}` });
    });

    render(<App />);
    expect(await screen.findByText('Приложение готово')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Политики' }));
    expect(await screen.findByText('Профиль не выбран')).toBeInTheDocument();
    expect(screen.queryByText('Приложение готово')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Состояние' }));
    expect(await screen.findByText('Приложение готово')).toBeInTheDocument();
    expect(
      invokeMock.mock.calls.filter(([command]) => command === 'get_runtime_overview'),
    ).toHaveLength(1);
  });

  it('открывает экран настроек из навигации', async () => {
    const user = userEvent.setup();
    listenMock.mockResolvedValue(() => {});
    invokeMock.mockImplementation((command: string) => {
      if (command === 'get_runtime_overview') {
        return Promise.resolve(fixture);
      }
      if (command === 'settings_get') {
        return Promise.resolve({
          close_behavior: 'hide_to_tray',
          tray_available: true,
          autostart_supported: false,
          autostart_enabled: false,
        });
      }
      return Promise.reject({ code: 'internal', message: `неожиданная команда: ${command}` });
    });

    render(<App />);
    await screen.findByText('Приложение готово');

    await user.click(screen.getByRole('button', { name: 'Настройки' }));

    expect(await screen.findByRole('radio', { name: /Скрывать в трей/ })).toBeChecked();
    expect(screen.getByLabelText(/Запускать при входе в систему/)).toBeDisabled();
  });

  it('отписывается от события при размонтировании', async () => {
    const unlisten = vi.fn();
    listenMock.mockResolvedValue(unlisten);
    invokeMock.mockResolvedValue(fixture);

    const { unmount } = render(<App />);
    expect(await screen.findByText('Приложение готово')).toBeInTheDocument();

    unmount();

    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
