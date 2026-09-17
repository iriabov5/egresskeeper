import { ProfilePanel } from './ProfilePanel';
import { EvaluationPanel, RulePanel } from './RulePanel';
import { usePolicyEditor } from './usePolicyEditor';

/**
 * Экран управления политиками.
 *
 * Компонент только компонует панели и показывает состояния загрузки и ошибки:
 * данные, операции и их последствия живут в `usePolicyEditor`.
 */
export function PolicyScreen() {
  const editor = usePolicyEditor();

  if (editor.status === 'loading') {
    return (
      <section className="panel" aria-live="polite">
        <output className="panel__title">Загружаем политики…</output>
      </section>
    );
  }

  if (editor.status === 'failed' || editor.error !== null) {
    return (
      <section className="panel panel--error" role="alert">
        <h2 className="panel__title">Политики недоступны</h2>
        <p className="panel__text">{editor.error?.message}</p>
        <p className="panel__hint">
          Код ошибки: <code>{editor.error?.code}</code>
        </p>
      </section>
    );
  }

  return (
    <div className="policy">
      <ProfilePanel
        profiles={editor.profiles}
        selectedProfileId={editor.selectedProfileId}
        editor={editor}
      />

      {editor.detail === null ? (
        <section className="panel">
          <h2 className="panel__title">Профиль не выбран</h2>
          <output className="panel__hint">Выберите профиль, чтобы увидеть его правила.</output>
        </section>
      ) : (
        <div className="policy__rules">
          <RulePanel detail={editor.detail} editor={editor} />
          <EvaluationPanel detail={editor.detail} editor={editor} />
        </div>
      )}
    </div>
  );
}
