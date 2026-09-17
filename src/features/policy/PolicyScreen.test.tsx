import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import decisionFixture from '../../../contracts/ipc/policy_decision.sample.json';
import detailFixture from '../../../contracts/ipc/policy_profile_detail.sample.json';
import profileFixture from '../../../contracts/ipc/policy_profile.sample.json';
import ruleFixture from '../../../contracts/ipc/policy_rule.sample.json';
import { PolicyScreen } from './PolicyScreen';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

type Handlers = Record<string, (args: Record<string, unknown>) => unknown>;

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

/** Возвращает правило fixture по индексу, проверяя его наличие. */
function ruleAt(index: number) {
  const rule = detailFixture.rules[index];

  if (rule === undefined) {
    throw new Error(`fixture не содержит правила с индексом ${index}`);
  }

  return rule;
}

const secondProfile = {
  ...profileFixture,
  id: '1b4e28ba-2fa1-11d2-883f-0016d3cca427',
  name: 'Личное',
};

/** Ответы happy path. */
function handlers(overrides: Handlers = {}): Handlers {
  return {
    policy_list_profiles: () => [
      { profile: profileFixture, rule_count: detailFixture.rules.length },
    ],
    policy_profile_detail: () => detailFixture,
    policy_add_rule: () => ruleFixture,
    policy_update_rule: () => ruleFixture,
    policy_delete_rule: () => null,
    policy_reorder_rules: () => detailFixture.rules,
    policy_set_default_action: () => ({ ...profileFixture, default_action: 'allow' }),
    policy_create_profile: () => ({ ...profileFixture, id: secondProfile.id, name: 'Личное' }),
    policy_rename_profile: () => profileFixture,
    policy_delete_profile: () => null,
    policy_evaluate: () => decisionFixture,
    ...overrides,
  };
}

/** Находит форму по доступному имени. */
function form(name: string): HTMLElement {
  return screen.getByRole('form', { name });
}

/** Ждёт загрузки экрана политик. */
async function renderScreen(overrides: Handlers = {}) {
  respond(handlers(overrides));
  render(<PolicyScreen />);
  await screen.findByText(`Правила профиля «${profileFixture.name}»`);
}

describe('экран политик', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('показывает правила выбранного профиля в порядке применения', async () => {
    await renderScreen();

    expect(screen.getByText('api.example.com')).toBeInTheDocument();
    expect(screen.getByText('*.tracker.example.com')).toBeInTheDocument();
    expect(screen.getByText('порт 443')).toBeInTheDocument();
    expect(screen.getByText('любой порт')).toBeInTheDocument();
    expect(screen.getByText('правил: 2 · default: запретить')).toBeInTheDocument();
  });

  it('показывает пустое состояние, когда у профиля нет правил', async () => {
    await renderScreen({
      policy_profile_detail: () => ({ profile: profileFixture, rules: [] }),
    });

    expect(
      screen.getByText(/У профиля нет правил: к соединениям применяется default action/),
    ).toBeInTheDocument();
  });

  it('добавляет правило и перезапрашивает данные', async () => {
    const user = userEvent.setup();
    await renderScreen();

    const ruleForm = form('Новое правило');
    await user.type(within(ruleForm).getByLabelText('Host'), 'api.example.com');
    await user.click(within(ruleForm).getByRole('button', { name: 'Добавить правило' }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('policy_add_rule', {
        profile_id: profileFixture.id,
        input: {
          action: 'allow',
          host_kind: 'exact',
          host: 'api.example.com',
          port: { kind: 'any' },
        },
      }),
    );
    await waitFor(() => expect(within(ruleForm).getByLabelText('Host')).toHaveValue(''));
  });

  it('показывает ошибку валидации рядом с полем и сохраняет ввод', async () => {
    const user = userEvent.setup();
    await renderScreen({
      policy_add_rule: () => {
        throw {
          code: 'validation',
          message: 'Запрос содержит недопустимые данные.',
          details: { field: 'host' },
        };
      },
    });

    const ruleForm = form('Новое правило');
    await user.type(within(ruleForm).getByLabelText('Host'), 'плохой хост');
    await user.click(within(ruleForm).getByRole('button', { name: 'Добавить правило' }));

    const alert = await within(ruleForm).findByRole('alert');
    expect(alert).toHaveTextContent('Запрос содержит недопустимые данные.');
    expect(within(ruleForm).getByLabelText('Host')).toHaveValue('плохой хост');
  });

  it('меняет порядок правил', async () => {
    const user = userEvent.setup();
    await renderScreen();

    await user.click(screen.getByRole('button', { name: 'Опустить правило 1' }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('policy_reorder_rules', {
        profile_id: profileFixture.id,
        rule_ids: [ruleAt(1).id, ruleAt(0).id],
      }),
    );
  });

  it('не даёт поднять первое правило выше первого места', async () => {
    const user = userEvent.setup();
    await renderScreen();

    expect(screen.getByRole('button', { name: 'Поднять правило 1' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Опустить правило 2' }));
    expect(screen.getByRole('button', { name: 'Опустить правило 2' })).toBeDisabled();
  });

  it('удаляет правило', async () => {
    const user = userEvent.setup();
    await renderScreen();

    await user.click(screen.getByRole('button', { name: 'Удалить правило 2' }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('policy_delete_rule', {
        rule_id: ruleAt(1).id,
      }),
    );
  });

  it('меняет default action профиля', async () => {
    const user = userEvent.setup();
    await renderScreen();

    await user.selectOptions(screen.getByLabelText('Default action'), 'allow');

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('policy_set_default_action', {
        profile_id: profileFixture.id,
        action: 'allow',
      }),
    );
  });

  it('проверяет соединение и показывает сработавшее правило', async () => {
    const user = userEvent.setup();
    await renderScreen();

    const evaluationForm = form('Проверка соединения');
    await user.type(within(evaluationForm).getByLabelText('Host'), 'api.example.com');
    await user.click(within(evaluationForm).getByRole('button', { name: 'Проверить' }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('policy_evaluate', {
        profile_id: profileFixture.id,
        host: 'api.example.com',
        port: 443,
      }),
    );
    expect(await screen.findByText(/сработало правило №1/)).toBeInTheDocument();
  });

  it('показывает причину default action, когда правило не совпало', async () => {
    const user = userEvent.setup();
    await renderScreen({
      policy_evaluate: () => ({
        action: 'deny',
        reason: { kind: 'default_action', profile_id: profileFixture.id, action: 'deny' },
      }),
    });

    const evaluationForm = form('Проверка соединения');
    await user.type(within(evaluationForm).getByLabelText('Host'), 'other.example.com');
    await user.click(within(evaluationForm).getByRole('button', { name: 'Проверить' }));

    expect(await screen.findByText(/действует default action профиля/)).toBeInTheDocument();
  });

  it('показывает ошибку проверки соединения с кодом', async () => {
    const user = userEvent.setup();
    await renderScreen({
      policy_evaluate: () => {
        throw {
          code: 'validation',
          message: 'Запрос содержит недопустимые данные.',
          details: { field: 'host' },
        };
      },
    });

    const evaluationForm = form('Проверка соединения');
    await user.type(within(evaluationForm).getByLabelText('Host'), 'плохой хост');
    await user.click(within(evaluationForm).getByRole('button', { name: 'Проверить' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('validation');
  });

  it('переключает профиль и загружает его правила', async () => {
    const user = userEvent.setup();
    await renderScreen({
      policy_list_profiles: () => [
        { profile: profileFixture, rule_count: 2 },
        { profile: secondProfile, rule_count: 0 },
      ],
      policy_profile_detail: (args) =>
        args.profile_id === secondProfile.id
          ? { profile: secondProfile, rules: [] }
          : detailFixture,
    });

    await user.click(screen.getByRole('button', { name: /Личное/ }));

    expect(await screen.findByText('Правила профиля «Личное»')).toBeInTheDocument();
    expect(screen.getByText(/У профиля нет правил/)).toBeInTheDocument();
  });

  it('создаёт профиль', async () => {
    const user = userEvent.setup();
    await renderScreen();

    await user.type(screen.getByLabelText('Имя нового профиля'), 'Личное');
    await user.click(screen.getByRole('button', { name: 'Создать профиль' }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('policy_create_profile', { name: 'Личное' }),
    );
  });

  it('показывает ошибку при удалении последнего профиля', async () => {
    const user = userEvent.setup();
    await renderScreen({
      policy_delete_profile: () => {
        throw {
          code: 'validation',
          message: 'Запрос содержит недопустимые данные.',
          details: { field: 'profile_id' },
        };
      },
    });

    await user.click(screen.getByRole('button', { name: 'Удалить профиль' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Запрос содержит недопустимые данные.');
    expect(screen.getByText('api.example.com')).toBeInTheDocument();
  });

  it('изменяет существующее правило, подставляя его значения в форму', async () => {
    const user = userEvent.setup();
    await renderScreen();

    await user.click(screen.getByRole('button', { name: 'Изменить правило 2' }));

    const editForm = form('Изменение правила');
    expect(within(editForm).getByLabelText('Host')).toHaveValue('tracker.example.com');

    await user.clear(within(editForm).getByLabelText('Host'));
    await user.type(within(editForm).getByLabelText('Host'), 'ads.example.net');
    await user.click(within(editForm).getByRole('button', { name: 'Сохранить правило' }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('policy_update_rule', {
        rule_id: ruleAt(1).id,
        input: {
          action: 'deny',
          host_kind: 'subdomains',
          host: 'ads.example.net',
          port: { kind: 'any' },
        },
      }),
    );
    expect(await screen.findByRole('form', { name: 'Новое правило' })).toBeInTheDocument();
  });

  it('возвращается к созданию правила по кнопке отмены', async () => {
    const user = userEvent.setup();
    await renderScreen();

    await user.click(screen.getByRole('button', { name: 'Изменить правило 1' }));
    await user.click(within(form('Изменение правила')).getByRole('button', { name: 'Отмена' }));

    expect(screen.getByRole('form', { name: 'Новое правило' })).toBeInTheDocument();
  });

  it('переименовывает профиль', async () => {
    const user = userEvent.setup();
    await renderScreen();

    await user.type(screen.getByLabelText(/Переименовать/), 'Работа 2');
    await user.click(screen.getByRole('button', { name: 'Переименовать' }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('policy_rename_profile', {
        profile_id: profileFixture.id,
        name: 'Работа 2',
      }),
    );
  });

  it('предлагает выбрать профиль, когда их нет', async () => {
    respond({ policy_list_profiles: () => [] });

    render(<PolicyScreen />);

    expect(await screen.findByText('Профиль не выбран')).toBeInTheDocument();
    expect(screen.getByText(/Выберите профиль/)).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith('policy_evaluate', expect.anything());
  });

  it('показывает ошибку хранилища при сохранении и не меняет список', async () => {
    const user = userEvent.setup();
    await renderScreen({
      policy_add_rule: () => {
        throw {
          code: 'storage_unavailable',
          message: 'Не удалось обратиться к локальному хранилищу.',
        };
      },
    });

    const ruleForm = form('Новое правило');
    await user.type(within(ruleForm).getByLabelText('Host'), 'api.example.com');
    await user.click(within(ruleForm).getByRole('button', { name: 'Добавить правило' }));

    const alert = await within(ruleForm).findByRole('alert');
    expect(alert).toHaveTextContent('storage_unavailable');
    expect(alert).toHaveTextContent('Не удалось обратиться к локальному хранилищу.');
    expect(screen.getByText('api.example.com')).toBeInTheDocument();
    expect(screen.getByText('*.tracker.example.com')).toBeInTheDocument();
  });

  it('показывает ошибку хранилища вместо экрана политик', async () => {
    respond({
      policy_list_profiles: () => {
        throw {
          code: 'storage_unavailable',
          message: 'Не удалось обратиться к локальному хранилищу.',
        };
      },
    });

    render(<PolicyScreen />);

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Не удалось обратиться к локальному хранилищу.');
    expect(alert).toHaveTextContent('storage_unavailable');
  });
});
