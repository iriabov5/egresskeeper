import { useState } from 'react';

import type { Action, Decision, ProfileDetail, Rule } from '../../ipc/types';
import { RuleForm } from './RuleForm';
import type { PolicyEditor } from './usePolicyEditor';

interface RulePanelProps {
  readonly detail: ProfileDetail;
  readonly editor: PolicyEditor;
}

/** Панель правил выбранного профиля. */
export function RulePanel({ detail, editor }: RulePanelProps) {
  const [editingRuleId, setEditingRuleId] = useState<string | null>(null);
  const editingRule = detail.rules.find((rule) => rule.id === editingRuleId);

  return (
    <section className="panel" aria-label="Правила профиля">
      <header className="panel__header">
        <h2 className="panel__title">Правила профиля «{detail.profile.name}»</h2>
        <label className="panel__inline">
          <span className="form__label">Default action</span>
          <select
            value={detail.profile.default_action}
            onChange={(event) => {
              void editor.changeDefaultAction(event.target.value as Action);
            }}
          >
            <option value="deny">Запретить</option>
            <option value="allow">Разрешить</option>
          </select>
        </label>
      </header>

      {detail.rules.length === 0 ? (
        <output className="panel__hint">
          У профиля нет правил: к соединениям применяется default action профиля.
        </output>
      ) : (
        <ol className="rules">
          {detail.rules.map((rule, index) => (
            <li key={rule.id} className="rules__item">
              <span className="rules__position">{index + 1}</span>
              <span className={`badge badge--${rule.action}`}>{actionLabel(rule.action)}</span>
              <span className="rules__target">
                {rule.host.kind === 'subdomains' ? `*.${rule.host.value}` : rule.host.value}
              </span>
              <span className="rules__port">{portLabel(rule.port)}</span>
              <span className="rules__controls">
                <button
                  type="button"
                  className="button button--icon"
                  aria-label={`Поднять правило ${index + 1}`}
                  disabled={index === 0}
                  onClick={() => {
                    void editor.moveRule(rule.id, 'up');
                  }}
                >
                  ↑
                </button>
                <button
                  type="button"
                  className="button button--icon"
                  aria-label={`Опустить правило ${index + 1}`}
                  disabled={index === detail.rules.length - 1}
                  onClick={() => {
                    void editor.moveRule(rule.id, 'down');
                  }}
                >
                  ↓
                </button>
                <button
                  type="button"
                  className="button button--icon"
                  aria-label={`Изменить правило ${index + 1}`}
                  onClick={() => setEditingRuleId(rule.id)}
                >
                  ✎
                </button>
                <button
                  type="button"
                  className="button button--icon"
                  aria-label={`Удалить правило ${index + 1}`}
                  onClick={() => {
                    void editor.removeRule(rule.id);
                  }}
                >
                  ✕
                </button>
              </span>
            </li>
          ))}
        </ol>
      )}

      {editingRule === undefined ? (
        <RuleForm onSubmit={editor.addRule} />
      ) : (
        <RuleForm
          key={editingRule.id}
          rule={editingRule}
          onSubmit={async (input) => {
            const error = await editor.updateRule(editingRule.id, input);
            if (error === null) {
              setEditingRuleId(null);
            }
            return error;
          }}
          onCancel={() => setEditingRuleId(null)}
        />
      )}
    </section>
  );
}

/** Панель проверки соединения. */
export function EvaluationPanel({
  detail,
  editor,
}: {
  readonly detail: ProfileDetail;
  readonly editor: PolicyEditor;
}) {
  const evaluation = editor.evaluation;
  // Управляемые поля: значения формы не приходится доставать из FormData и
  // приводить к строке, где значение может оказаться файлом.
  const [host, setHost] = useState('');
  const [port, setPort] = useState('443');

  return (
    <section className="panel" aria-label="Проверка соединения">
      <h2 className="panel__title">Проверка соединения</h2>
      <form
        className="form__row"
        aria-label="Проверка соединения"
        onSubmit={(event) => {
          event.preventDefault();
          void editor.evaluateConnection(host, Number(port));
        }}
      >
        <div className="form__field form__field--wide">
          <label className="form__label" htmlFor="evaluate-host">
            Host
          </label>
          <input
            id="evaluate-host"
            name="host"
            type="text"
            placeholder="api.example.com"
            value={host}
            onChange={(event) => setHost(event.target.value)}
          />
        </div>
        <div className="form__field">
          <label className="form__label" htmlFor="evaluate-port">
            Порт
          </label>
          <input
            id="evaluate-port"
            name="port"
            type="number"
            min={1}
            max={65535}
            value={port}
            onChange={(event) => setPort(event.target.value)}
          />
        </div>
        <button type="submit" className="button">
          Проверить
        </button>
      </form>

      {evaluation.status === 'running' && (
        <output className="panel__hint">Проверяем…</output>
      )}

      {evaluation.status === 'done' && evaluation.decision !== undefined && (
        <output className="decision">
          <span className={`badge badge--${evaluation.decision.action}`}>
            {actionLabel(evaluation.decision.action)}
          </span>{' '}
          <span>{describeReason(evaluation.decision, detail.rules)}</span>
        </output>
      )}

      {evaluation.status === 'failed' && evaluation.error !== undefined && (
        <p className="form__error" role="alert">
          {evaluation.error.message} (код: {evaluation.error.code})
        </p>
      )}
    </section>
  );
}

/** Текст причины решения. */
function describeReason(decision: Decision, rules: readonly Rule[]): string {
  const { reason } = decision;

  if (reason.kind === 'default_action') {
    return `действует default action профиля (${actionLabel(reason.action)})`;
  }

  const rule = rules.find((candidate) => candidate.id === reason.rule_id);
  if (rule === undefined) {
    return `сработало правило №${reason.position + 1}`;
  }

  const target =
    rule.host.kind === 'subdomains' ? `*.${rule.host.value}` : rule.host.value;

  return `сработало правило №${rule.position + 1}: ${target}, ${portLabel(rule.port)}`;
}

/** Текст действия для показа пользователю. */
export function actionLabel(action: Action): string {
  return action === 'allow' ? 'Разрешено' : 'Запрещено';
}

/** Текст ограничения порта для показа пользователю. */
export function portLabel(port: Rule['port']): string {
  switch (port.kind) {
    case 'any':
      return 'любой порт';
    case 'exactly':
      return `порт ${port.port}`;
    case 'range':
      return `порты ${port.start}–${port.end}`;
  }
}
