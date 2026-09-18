// Каждый интеграционный тест подключает этот модуль целиком, поэтому часть
// вспомогательных реализаций в отдельном бинарнике может не использоваться.
#![allow(dead_code)]

//! Общая тестовая инфраструктура интеграционных тестов.
//!
//! Содержит реализацию порта `PolicyRepository` в памяти. Она используется как
//! эталон поведения порта: SQLite-реализация обязана давать те же наблюдаемые
//! результаты.

use std::sync::Mutex;

use egresskeeper_core::{
    Action, EgressError, Listener, ListenerDraft, ListenerId, ListenerRepository, PolicyRepository,
    Profile, ProfileDraft, ProfileId, Rule, RuleDraft, RuleId,
};

/// Хранилище политик в памяти.
#[derive(Debug, Default)]
pub struct InMemoryPolicyRepository {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    profiles: Vec<Profile>,
    rules: Vec<Rule>,
    clock: i64,
}

impl InMemoryPolicyRepository {
    /// Создаёт пустое хранилище.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Возвращает следующую отметку времени, увеличивая внутренние часы.
    fn tick(&self, state: &mut State) -> i64 {
        state.clock += 1;
        state.clock
    }
}

impl PolicyRepository for InMemoryPolicyRepository {
    fn list_profiles(&self) -> Result<Vec<Profile>, EgressError> {
        let state = self.lock()?;
        Ok(state.profiles.clone())
    }

    fn find_profile(&self, id: &ProfileId) -> Result<Option<Profile>, EgressError> {
        let state = self.lock()?;
        Ok(state
            .profiles
            .iter()
            .find(|profile| &profile.id == id)
            .cloned())
    }

    fn find_profile_by_name(&self, name: &str) -> Result<Option<Profile>, EgressError> {
        let state = self.lock()?;
        Ok(state
            .profiles
            .iter()
            .find(|profile| Profile::fold_name(&profile.name) == Profile::fold_name(name))
            .cloned())
    }

    fn count_profiles(&self) -> Result<u64, EgressError> {
        let state = self.lock()?;
        Ok(state.profiles.len() as u64)
    }

    fn create_profile(&self, draft: &ProfileDraft) -> Result<Profile, EgressError> {
        let mut state = self.lock()?;

        if state
            .profiles
            .iter()
            .any(|profile| Profile::fold_name(&profile.name) == Profile::fold_name(&draft.name))
        {
            return Err(EgressError::validation("name", "must be unique"));
        }

        let now = self.tick(&mut state);
        let profile = Profile {
            id: ProfileId::new(),
            name: draft.name.clone(),
            default_action: draft.default_action,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        state.profiles.push(profile.clone());

        Ok(profile)
    }

    fn rename_profile(&self, id: &ProfileId, name: &str) -> Result<Profile, EgressError> {
        let mut state = self.lock()?;

        if state.profiles.iter().any(|profile| {
            &profile.id != id && Profile::fold_name(&profile.name) == Profile::fold_name(name)
        }) {
            return Err(EgressError::validation("name", "must be unique"));
        }

        let now = self.tick(&mut state);
        let profile = state
            .profiles
            .iter_mut()
            .find(|profile| &profile.id == id)
            .ok_or_else(|| EgressError::not_found("profile"))?;

        profile.name = name.to_owned();
        profile.updated_at_unix_ms = now;

        Ok(profile.clone())
    }

    fn set_default_action(&self, id: &ProfileId, action: Action) -> Result<Profile, EgressError> {
        let mut state = self.lock()?;
        let now = self.tick(&mut state);
        let profile = state
            .profiles
            .iter_mut()
            .find(|profile| &profile.id == id)
            .ok_or_else(|| EgressError::not_found("profile"))?;

        profile.default_action = action;
        profile.updated_at_unix_ms = now;

        Ok(profile.clone())
    }

    fn delete_profile(&self, id: &ProfileId) -> Result<(), EgressError> {
        let mut state = self.lock()?;

        let before = state.profiles.len();
        state.profiles.retain(|profile| &profile.id != id);

        if state.profiles.len() == before {
            return Err(EgressError::not_found("profile"));
        }

        state.rules.retain(|rule| &rule.profile_id != id);

        Ok(())
    }

    fn list_rules(&self, profile_id: &ProfileId) -> Result<Vec<Rule>, EgressError> {
        let state = self.lock()?;
        let mut rules: Vec<Rule> = state
            .rules
            .iter()
            .filter(|rule| &rule.profile_id == profile_id)
            .cloned()
            .collect();
        rules.sort_by_key(|rule| rule.position);

        Ok(rules)
    }

    fn count_rules(&self, profile_id: &ProfileId) -> Result<u32, EgressError> {
        let state = self.lock()?;
        Ok(state
            .rules
            .iter()
            .filter(|rule| &rule.profile_id == profile_id)
            .count() as u32)
    }

    fn add_rule(&self, profile_id: &ProfileId, draft: &RuleDraft) -> Result<Rule, EgressError> {
        let mut state = self.lock()?;

        if !state
            .profiles
            .iter()
            .any(|profile| &profile.id == profile_id)
        {
            return Err(EgressError::not_found("profile"));
        }

        let next = state
            .rules
            .iter()
            .filter(|rule| &rule.profile_id == profile_id)
            .map(|rule| rule.position + 1)
            .max()
            .unwrap_or(0);

        let rule = Rule {
            id: RuleId::new(),
            profile_id: profile_id.clone(),
            position: next,
            action: draft.action,
            host: draft.host.clone(),
            port: draft.port,
        };
        state.rules.push(rule.clone());

        Ok(rule)
    }

    fn update_rule(&self, id: &RuleId, draft: &RuleDraft) -> Result<Rule, EgressError> {
        let mut state = self.lock()?;
        let rule = state
            .rules
            .iter_mut()
            .find(|rule| &rule.id == id)
            .ok_or_else(|| EgressError::not_found("rule"))?;

        rule.action = draft.action;
        rule.host = draft.host.clone();
        rule.port = draft.port;

        Ok(rule.clone())
    }

    fn delete_rule(&self, id: &RuleId) -> Result<(), EgressError> {
        let mut state = self.lock()?;
        let before = state.rules.len();
        state.rules.retain(|rule| &rule.id != id);

        if state.rules.len() == before {
            return Err(EgressError::not_found("rule"));
        }

        Ok(())
    }

    fn reorder_rules(
        &self,
        profile_id: &ProfileId,
        ordered: &[RuleId],
    ) -> Result<Vec<Rule>, EgressError> {
        let mut state = self.lock()?;

        let current: Vec<RuleId> = state
            .rules
            .iter()
            .filter(|rule| &rule.profile_id == profile_id)
            .map(|rule| rule.id.clone())
            .collect();

        if current.len() != ordered.len()
            || ordered.iter().any(|id| !current.contains(id))
            || ordered
                .iter()
                .enumerate()
                .any(|(index, id)| ordered[index + 1..].contains(id))
        {
            return Err(EgressError::validation(
                "rule_ids",
                "must list exactly the rules of the profile",
            ));
        }

        for (index, rule_id) in ordered.iter().enumerate() {
            if let Some(rule) = state.rules.iter_mut().find(|rule| &rule.id == rule_id) {
                rule.position = index as u32;
            }
        }

        let mut rules: Vec<Rule> = state
            .rules
            .iter()
            .filter(|rule| &rule.profile_id == profile_id)
            .cloned()
            .collect();
        rules.sort_by_key(|rule| rule.position);

        Ok(rules)
    }
}

impl InMemoryPolicyRepository {
    /// Берёт блокировку состояния.
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, EgressError> {
        self.state
            .lock()
            .map_err(|_| EgressError::storage_message("in-memory repository lock is poisoned"))
    }
}

/// Хранилище listeners в памяти.
#[derive(Debug, Default)]
pub struct InMemoryListenerRepository {
    state: Mutex<ListenerState>,
}

#[derive(Debug, Default)]
struct ListenerState {
    listeners: Vec<Listener>,
    profiles: Vec<ProfileId>,
    clock: i64,
}

impl InMemoryListenerRepository {
    /// Создаёт хранилище с известными профилями.
    #[must_use]
    pub fn with_profiles(profiles: Vec<ProfileId>) -> Self {
        Self {
            state: Mutex::new(ListenerState {
                profiles,
                ..ListenerState::default()
            }),
        }
    }

    /// Возвращает текущее состояние listeners.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку при отравленном мьютексе.
    pub fn listeners(&self) -> Result<Vec<Listener>, EgressError> {
        Ok(self.lock()?.listeners.clone())
    }

    /// Берёт блокировку состояния.
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ListenerState>, EgressError> {
        self.state
            .lock()
            .map_err(|_| EgressError::storage_message("in-memory listener lock is poisoned"))
    }

    /// Увеличивает внутренние часы.
    fn tick(state: &mut ListenerState) -> i64 {
        state.clock += 1;
        state.clock
    }
}

impl ListenerRepository for InMemoryListenerRepository {
    fn list_listeners(&self) -> Result<Vec<Listener>, EgressError> {
        Ok(self.lock()?.listeners.clone())
    }

    fn find_listener(&self, id: &ListenerId) -> Result<Option<Listener>, EgressError> {
        Ok(self
            .lock()?
            .listeners
            .iter()
            .find(|listener| &listener.id == id)
            .cloned())
    }

    fn profile_exists(&self, id: &ProfileId) -> Result<bool, EgressError> {
        Ok(self.lock()?.profiles.contains(id))
    }

    fn create_listener(&self, draft: &ListenerDraft) -> Result<Listener, EgressError> {
        let mut state = self.lock()?;

        if state
            .listeners
            .iter()
            .any(|listener| listener.port == draft.port)
        {
            return Err(EgressError::validation(
                "port",
                "is already used by another listener",
            ));
        }

        let now = Self::tick(&mut state);
        let listener = Listener {
            id: ListenerId::new(),
            port: draft.port,
            profile_id: draft.profile_id.clone(),
            enabled: false,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        state.listeners.push(listener.clone());

        Ok(listener)
    }

    fn update_listener(
        &self,
        id: &ListenerId,
        draft: &ListenerDraft,
    ) -> Result<Listener, EgressError> {
        let mut state = self.lock()?;

        if state
            .listeners
            .iter()
            .any(|listener| &listener.id != id && listener.port == draft.port)
        {
            return Err(EgressError::validation(
                "port",
                "is already used by another listener",
            ));
        }

        let now = Self::tick(&mut state);
        let listener = state
            .listeners
            .iter_mut()
            .find(|listener| &listener.id == id)
            .ok_or_else(|| EgressError::not_found("listener"))?;

        listener.port = draft.port;
        listener.profile_id = draft.profile_id.clone();
        listener.updated_at_unix_ms = now;

        Ok(listener.clone())
    }

    fn set_listener_enabled(
        &self,
        id: &ListenerId,
        enabled: bool,
    ) -> Result<Listener, EgressError> {
        let mut state = self.lock()?;
        let now = Self::tick(&mut state);
        let listener = state
            .listeners
            .iter_mut()
            .find(|listener| &listener.id == id)
            .ok_or_else(|| EgressError::not_found("listener"))?;

        listener.enabled = enabled;
        listener.updated_at_unix_ms = now;

        Ok(listener.clone())
    }

    fn delete_listener(&self, id: &ListenerId) -> Result<(), EgressError> {
        let mut state = self.lock()?;
        let before = state.listeners.len();
        state.listeners.retain(|listener| &listener.id != id);

        if state.listeners.len() == before {
            return Err(EgressError::not_found("listener"));
        }

        Ok(())
    }
}
