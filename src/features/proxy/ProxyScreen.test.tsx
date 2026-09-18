import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { act } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import decisionFixture from '../../../contracts/ipc/proxy_decision.sample.json';
import statusFixture from '../../../contracts/ipc/proxy_status.sample.json';
import profileFixture from '../../../contracts/ipc/policy_profile.sample.json';
import { ProxyScreen } from './ProxyScreen';

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

type Handlers = Record<string, (args: Record<string, unknown>) => unknown>;
type EventHandler = (message: { payload: unknown }) => void;

/** Настраивает ответы backend по имени команды. */
function respond(handlers: Handlers) {
  invokeMock.mockImplementation((command: string, args: Record<string, unknown> = {}) => {
    const handler = handlers[command];

    if (handler === undefined) {
      return Promise.reject({ code: 'internal', message: `неожиданная команда: ${command}` });
    }

    try {
      return Promise.resolve(handler(args));
    } catch (error) {
      return Promise.reject(error);
    }
  });
}

const profiles = [{ profile: profileFixture, rule_count: 1 }];

/** Ответы happy path. */
function handlers(overrides: Handlers = {}): Handlers {
  return {
    proxy_status: () => statusFixture,
    policy_list_profiles: () => profiles,
    proxy_create_listener: () => statusFixture,
    proxy_set_listener_enabled: () => statusFixture,
    proxy_delete_listener: () => ({ ...statusFixture, listeners: [] }),
    ...overrides,
  };
}

/** Перехватывает обработчики событий. */
function captureEvents(): Map<string, EventHandler> {
  const handlersByEvent = new Map<string, EventHandler>();

  listenMock.mockImplementation((event: string, handler: EventHandler) => {
    handlersByEvent.set(event, handler);
    return Promise.resolve(() => {
      handlersByEvent.delete(event);
    });
  });

  return handlersByEvent;
}

/** Ждёт загрузки экрана. */
async function renderScreen(overrides: Handlers = {}) {
  const events = captureEvents();
  respond(handlers(overrides));
  render(<ProxyScreen />);
  await screen.findByRole('region', { name: 'Listeners' });

  return events;
}

describe('экран «Прокси»', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    listenMock.mockResolvedValue(() => {});
  });

  it('показывает listeners с состоянием и активными соединениями', async () => {
    await renderScreen();

    expect(screen.getByText('127.0.0.1:8787')).toBeInTheDocument();
    expect(screen.getByText('работает')).toBeInTheDocument();
    expect(screen.getByText('соединений: 2')).toBeInTheDocument();
    expect(screen.getByText('127.0.0.1:9999')).toBeInTheDocument();
  });

  it('показывает ошибку запуска listener с кодом', async () => {
    await renderScreen();

    const badge = screen.getByText('ошибка: port_unavailable');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveAttribute('title', 'Порт уже занят другой программой.');
  });

  it('показывает пустое состояние без listeners', async () => {
    await renderScreen({
      proxy_status: () => ({ listeners: [], dropped_decisions: 0 }),
    });

    expect(screen.getByText(/Proxy ничего не слушает/)).toBeInTheDocument();
    expect(screen.getByText(/Сначала добавьте и включите listener/)).toBeInTheDocument();
  });

  it('подсказывает переменные окружения с адресом работающего listener', async () => {
    await renderScreen();

    const hint = screen.getByRole('region', { name: 'Настройка инструмента' });

    expect(within(hint).getByText(/HTTPS_PROXY=http:\/\/127\.0\.0\.1:8787/)).toBeInTheDocument();
    expect(within(hint).getByText(/HTTP_PROXY=http:\/\/127\.0\.0\.1:8787/)).toBeInTheDocument();
  });

  it('создаёт listener с выбранным профилем', async () => {
    const user = userEvent.setup();
    await renderScreen();

    await user.type(screen.getByLabelText('Порт'), '9000');
    await user.click(screen.getByRole('button', { name: 'Добавить listener' }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('proxy_create_listener', {
        port: 9000,
        profile_id: profileFixture.id,
      }),
    );
  });

  it('показывает ошибку валидации порта рядом с полем', async () => {
    // Порт в допустимом диапазоне, но занят: проверяется именно ответ backend,
    // потому что заведомо неверный порт отклоняет нативная валидация поля.
    const user = userEvent.setup();
    await renderScreen({
      proxy_create_listener: () => {
        throw {
          code: 'validation',
          message: 'Запрос содержит недопустимые данные.',
          details: { field: 'port' },
        };
      },
    });

    await user.type(screen.getByLabelText('Порт'), '9000');
    await user.click(screen.getByRole('button', { name: 'Добавить listener' }));

    const form = screen.getByRole('form', { name: 'Новый listener' });
    const alert = await within(form).findByRole('alert');

    expect(alert).toHaveTextContent('Запрос содержит недопустимые данные.');
    expect(screen.getByLabelText('Порт')).toHaveValue(9000);
  });

  it('показывает ошибку хранилища при создании', async () => {
    const user = userEvent.setup();
    await renderScreen({
      proxy_create_listener: () => {
        throw {
          code: 'storage_unavailable',
          message: 'Не удалось обратиться к локальному хранилищу.',
        };
      },
    });

    await user.type(screen.getByLabelText('Порт'), '9000');
    await user.click(screen.getByRole('button', { name: 'Добавить listener' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('storage_unavailable');
  });

  it('включает и выключает listener', async () => {
    const user = userEvent.setup();
    await renderScreen();

    const enabledButtons = screen.getAllByRole('button', { name: 'Выключить' });
    await user.click(enabledButtons[0] as HTMLElement);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('proxy_set_listener_enabled', {
        listener_id: statusFixture.listeners[0]?.listener.id,
        enabled: false,
      }),
    );
  });

  it('удаляет listener', async () => {
    const user = userEvent.setup();
    await renderScreen();

    const removeButtons = screen.getAllByRole('button', { name: 'Удалить' });
    await user.click(removeButtons[1] as HTMLElement);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('proxy_delete_listener', {
        listener_id: statusFixture.listeners[1]?.listener.id,
      }),
    );
  });

  it('показывает решение из события с целью, действием и причиной', async () => {
    const events = await renderScreen();
    const decisionHandler = events.get('proxy://decision');

    expect(decisionHandler).toBeDefined();

    act(() => {
      decisionHandler?.({ payload: decisionFixture });
    });

    const feed = screen.getByRole('region', { name: 'Поток решений' });

    expect(await within(feed).findByText('api.example.com:443')).toBeInTheDocument();
    expect(within(feed).getByText('запрещено')).toBeInTheDocument();
    expect(within(feed).getByText('правило №2')).toBeInTheDocument();
    expect(within(feed).getByText('порт 8787')).toBeInTheDocument();
  });

  it('показывает причину «политика недоступна»', async () => {
    const events = await renderScreen();

    act(() => {
      events.get('proxy://decision')?.({
        payload: { ...decisionFixture, reason: { kind: 'policy_unavailable' } },
      });
    });

    const feed = screen.getByRole('region', { name: 'Поток решений' });
    expect(await within(feed).findByText('политика недоступна')).toBeInTheDocument();
  });

  it('обновляет состояние listener по событию рантайма', async () => {
    const events = await renderScreen();
    const runtimeHandler = events.get('proxy://runtime');
    const runningId = statusFixture.listeners[0]?.listener.id;

    expect(runtimeHandler).toBeDefined();

    act(() => {
      runtimeHandler?.({
        payload: {
          listeners: [
            {
              listener_id: runningId,
              state: {
                state: 'failed',
                code: 'port_unavailable',
                message: 'Порт уже занят другой программой.',
              },
              active_connections: 0,
            },
          ],
          dropped_decisions: 0,
        },
      });
    });

    await waitFor(() => expect(screen.getAllByText('ошибка: port_unavailable')).toHaveLength(2));
  });

  it('считает пропущенные решения из события рантайма', async () => {
    const events = await renderScreen();

    // В fixture статуса уже пропущено 3 решения: счётчик абсолютный.
    expect(screen.getByTestId('missed-decisions')).toHaveTextContent('пропущено: 3');

    act(() => {
      events.get('proxy://runtime')?.({ payload: { listeners: [], dropped_decisions: 5 } });
    });

    await waitFor(() =>
      expect(screen.getByTestId('missed-decisions')).toHaveTextContent('пропущено: 5'),
    );
  });

  it('показывает ошибку хранилища вместо экрана', async () => {
    captureEvents();
    respond({
      proxy_status: () => {
        throw {
          code: 'storage_unavailable',
          message: 'Не удалось обратиться к локальному хранилищу.',
        };
      },
      policy_list_profiles: () => profiles,
    });

    render(<ProxyScreen />);

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Не удалось обратиться к локальному хранилищу.');
    expect(alert).toHaveTextContent('storage_unavailable');
  });

  it('явно сообщает, что поток решений не сохраняется', async () => {
    await renderScreen();

    expect(screen.getByText(/Поток решений не сохраняется/)).toBeInTheDocument();
  });
});
