import type {
  Action,
  DecisionReason,
  ListenerView,
  ProxyDecision,
  ProxyDecisionReason,
} from '../../ipc/types';
import { DECISION_FEED_LIMIT } from './useProxyConsole';

interface DecisionFeedProps {
  readonly decisions: ProxyDecision[];
  readonly listeners: ListenerView[];
  readonly missedDecisions: number;
}

/** Живой поток решений proxy. */
export function DecisionFeed({ decisions, listeners, missedDecisions }: DecisionFeedProps) {
  return (
    <section className="panel" aria-label="Поток решений">
      <header className="panel__header">
        <h2 className="panel__title">Поток решений</h2>
        <span className="panel__hint" data-testid="missed-decisions">
          пропущено: {missedDecisions}
        </span>
      </header>

      <p className="panel__hint">
        Поток решений не сохраняется: показываются последние {DECISION_FEED_LIMIT} решений,
        и он очищается при перезапуске приложения.
      </p>

      {decisions.length === 0 ? (
        <output className="panel__hint">
          Решений пока нет: они появятся, когда инструмент начнёт ходить через proxy.
        </output>
      ) : (
        <ul className="decisions">
          {decisions.map((decision, index) => (
            <li key={`${decision.at_unix_ms}-${decision.host}-${index}`} className="decisions__item">
              <span className="decisions__time">{formatTime(decision.at_unix_ms)}</span>
              <span className={`badge badge--${decision.action}`}>
                {decision.action === 'allow' ? 'разрешено' : 'запрещено'}
              </span>
              <span className="decisions__target">
                {decision.host}:{decision.port}
              </span>
              <span className="decisions__reason">{reasonText(decision.reason)}</span>
              <span className="decisions__listener">
                порт {listenerPort(decision.listener_id, listeners)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/** Текст причины решения. */
export function reasonText(reason: ProxyDecisionReason): string {
  switch (reason.kind) {
    case 'policy':
      return policyReasonText(reason.decision);
    case 'policy_unavailable':
      return 'политика недоступна';
    case 'loopback_target':
      return 'цель — сам proxy (петля)';
  }
}

/** Текст причины доменного решения. */
function policyReasonText(decision: DecisionReason): string {
  if (decision.kind === 'matched_rule') {
    return `правило №${decision.position + 1}`;
  }

  return `default action профиля (${actionText(decision.action)})`;
}

/** Действие словами. */
function actionText(action: Action): string {
  return action === 'allow' ? 'разрешить' : 'запретить';
}

/** Порт listener'а по его идентификатору. */
function listenerPort(listenerId: string, listeners: ListenerView[]): string {
  return String(
    listeners.find((listener) => listener.listener.id === listenerId)?.listener.port ?? '—',
  );
}

/** Форматирует время решения. */
function formatTime(atUnixMs: number): string {
  return new Intl.DateTimeFormat('ru-RU', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(atUnixMs));
}
