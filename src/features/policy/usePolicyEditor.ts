import { useCallback, useEffect, useState } from 'react';

import * as policyApi from '../../ipc/policy';
import { toIpcError, type Action, type Decision, type IpcError, type ProfileDetail, type ProfileSummary, type RuleInput } from '../../ipc/types';

/** Состояние загрузки экрана политик. */
export type PolicyStatus = 'loading' | 'ready' | 'failed';

/** Состояние проверки соединения. */
export interface EvaluationState {
  status: 'idle' | 'running' | 'done' | 'failed';
  decision?: Decision;
  error?: IpcError;
}

/** Модель экрана политик: данные, выбранный профиль и операции. */
export interface PolicyEditor {
  status: PolicyStatus;
  error: IpcError | null;
  profiles: ProfileSummary[];
  selectedProfileId: string | null;
  detail: ProfileDetail | null;
  evaluation: EvaluationState;
  selectProfile: (profileId: string) => void;
  createProfile: (name: string) => Promise<IpcError | null>;
  renameProfile: (name: string) => Promise<IpcError | null>;
  changeDefaultAction: (action: Action) => Promise<IpcError | null>;
  removeProfile: () => Promise<IpcError | null>;
  addRule: (input: RuleInput) => Promise<IpcError | null>;
  updateRule: (ruleId: string, input: RuleInput) => Promise<IpcError | null>;
  removeRule: (ruleId: string) => Promise<IpcError | null>;
  moveRule: (ruleId: string, direction: 'up' | 'down') => Promise<IpcError | null>;
  evaluateConnection: (host: string, port: number) => Promise<void>;
}

/**
 * Управляет данными экрана политик.
 *
 * После каждой мутации данные перезапрашиваются у backend: источник истины —
 * хранилище (порядок правил, нормализованные host, позиции), поэтому UI не
 * повторяет нормализацию и не может разойтись с ним.
 */
export function usePolicyEditor(): PolicyEditor {
  const [status, setStatus] = useState<PolicyStatus>('loading');
  const [error, setError] = useState<IpcError | null>(null);
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [selectedProfileId, setSelectedProfileId] = useState<string | null>(null);
  const [detail, setDetail] = useState<ProfileDetail | null>(null);
  const [evaluation, setEvaluation] = useState<EvaluationState>({ status: 'idle' });

  const load = useCallback(async (preferredProfileId: string | null) => {
    try {
      const summaries = await policyApi.listProfiles();
      const selected =
        preferredProfileId !== null &&
        summaries.some((summary) => summary.profile.id === preferredProfileId)
          ? preferredProfileId
          : (summaries[0]?.profile.id ?? null);

      setProfiles(summaries);
      setSelectedProfileId(selected);
      setDetail(selected === null ? null : await policyApi.profileDetail(selected));
      setStatus('ready');
      setError(null);
    } catch (cause) {
      setStatus('failed');
      setError(toIpcError(cause));
    }
  }, []);

  useEffect(() => {
    void load(null);
  }, [load]);

  const selectProfile = useCallback(
    (profileId: string) => {
      setSelectedProfileId(profileId);
      setEvaluation({ status: 'idle' });
      void load(profileId);
    },
    [load],
  );

  const createProfile = useCallback(
    async (name: string) => {
      try {
        const profile = await policyApi.createProfile(name);
        await load(profile.id);
        return null;
      } catch (cause) {
        return toIpcError(cause);
      }
    },
    [load],
  );

  const renameProfile = useCallback(
    async (name: string) => {
      if (selectedProfileId === null) {
        return null;
      }

      try {
        await policyApi.renameProfile(selectedProfileId, name);
        await load(selectedProfileId);
        return null;
      } catch (cause) {
        return toIpcError(cause);
      }
    },
    [load, selectedProfileId],
  );

  const changeDefaultAction = useCallback(
    async (action: Action) => {
      if (selectedProfileId === null) {
        return null;
      }

      try {
        await policyApi.setDefaultAction(selectedProfileId, action);
        await load(selectedProfileId);
        return null;
      } catch (cause) {
        return toIpcError(cause);
      }
    },
    [load, selectedProfileId],
  );

  const removeProfile = useCallback(async () => {
    if (selectedProfileId === null) {
      return null;
    }

    try {
      await policyApi.deleteProfile(selectedProfileId);
      await load(null);
      return null;
    } catch (cause) {
      return toIpcError(cause);
    }
  }, [load, selectedProfileId]);

  const addRule = useCallback(
    async (input: RuleInput) => {
      if (selectedProfileId === null) {
        return null;
      }

      try {
        await policyApi.addRule(selectedProfileId, input);
        await load(selectedProfileId);
        return null;
      } catch (cause) {
        return toIpcError(cause);
      }
    },
    [load, selectedProfileId],
  );

  const updateRule = useCallback(
    async (ruleId: string, input: RuleInput) => {
      if (selectedProfileId === null) {
        return null;
      }

      try {
        await policyApi.updateRule(ruleId, input);
        await load(selectedProfileId);
        return null;
      } catch (cause) {
        return toIpcError(cause);
      }
    },
    [load, selectedProfileId],
  );

  const removeRule = useCallback(
    async (ruleId: string) => {
      if (selectedProfileId === null) {
        return null;
      }

      try {
        await policyApi.deleteRule(ruleId);
        await load(selectedProfileId);
        return null;
      } catch (cause) {
        return toIpcError(cause);
      }
    },
    [load, selectedProfileId],
  );

  const moveRule = useCallback(
    async (ruleId: string, direction: 'up' | 'down') => {
      if (detail === null) {
        return null;
      }

      const order = detail.rules.map((rule) => rule.id);
      const index = order.indexOf(ruleId);
      const target = direction === 'up' ? index - 1 : index + 1;

      if (index < 0 || target < 0 || target >= order.length) {
        return null;
      }

      const moved = order[index];
      const swapped = order[target];
      if (moved === undefined || swapped === undefined) {
        return null;
      }
      order[index] = swapped;
      order[target] = moved;

      try {
        await policyApi.reorderRules(detail.profile.id, order);
        await load(detail.profile.id);
        return null;
      } catch (cause) {
        return toIpcError(cause);
      }
    },
    [detail, load],
  );

  const evaluateConnection = useCallback(
    async (host: string, port: number) => {
      if (selectedProfileId === null) {
        return;
      }

      setEvaluation({ status: 'running' });

      try {
        const decision = await policyApi.evaluate(selectedProfileId, host, port);
        setEvaluation({ status: 'done', decision });
      } catch (cause) {
        setEvaluation({ status: 'failed', error: toIpcError(cause) });
      }
    },
    [selectedProfileId],
  );

  return {
    status,
    error,
    profiles,
    selectedProfileId,
    detail,
    evaluation,
    selectProfile,
    createProfile,
    renameProfile,
    changeDefaultAction,
    removeProfile,
    addRule,
    updateRule,
    removeRule,
    moveRule,
    evaluateConnection,
  };
}
