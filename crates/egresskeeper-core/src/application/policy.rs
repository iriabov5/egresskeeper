//! Use-cases управления политикой.
//!
//! Сервис отвечает за валидацию ввода, проверку инвариантов, которые требуют
//! знания нескольких сущностей (уникальность имени, защита последнего профиля,
//! полнота перестановки при изменении порядка), и за сборку политики для оценки.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::application::ports::PolicyRepository;
use crate::domain::error::EgressError;
use crate::domain::policy::action::Action;
use crate::domain::policy::decision::{Decision, EvaluationTarget, Policy};
use crate::domain::policy::entities::{Profile, ProfileDraft, ProfileId, Rule, RuleDraft, RuleId};
use crate::domain::policy::host::{HostKind, HostMatcher};
use crate::domain::policy::port::PortSpec;

/// Профиль вместе с количеством правил.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileSummary {
    /// Профиль.
    pub profile: Profile,
    /// Количество правил профиля.
    pub rule_count: u32,
}

/// Профиль вместе с его правилами.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileDetail {
    /// Профиль.
    pub profile: Profile,
    /// Правила в порядке применения.
    pub rules: Vec<Rule>,
}

/// Ввод правила, пришедший из UI.
///
/// Значения проверяются при преобразовании в [`RuleDraft`], поэтому сам тип
/// может содержать некорректные данные.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RuleInput {
    /// Действие правила.
    pub action: Action,
    /// Вид сопоставления host.
    pub host_kind: HostKind,
    /// Значение host.
    pub host: String,
    /// Ограничение порта.
    pub port: PortSpec,
}

impl RuleInput {
    /// Проверяет ввод и преобразует его в данные правила.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] с полем `host` или `port`.
    pub fn into_draft(self) -> Result<RuleDraft, EgressError> {
        let host = HostMatcher::from_parts(self.host_kind, &self.host)?;
        let port = match self.port {
            PortSpec::Any => PortSpec::any(),
            PortSpec::Exactly { port } => PortSpec::exactly(port)?,
            PortSpec::Range { start, end } => PortSpec::range(start, end)?,
        };

        Ok(RuleDraft {
            action: self.action,
            host,
            port,
        })
    }
}

/// Use-cases управления профилями и правилами.
#[derive(Debug)]
pub struct PolicyService<R: PolicyRepository> {
    repository: R,
}

impl<R: PolicyRepository> PolicyService<R> {
    /// Создаёт сервис поверх реализации порта.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }

    /// Профили с количеством правил.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    pub fn list_profiles(&self) -> Result<Vec<ProfileSummary>, EgressError> {
        self.repository
            .list_profiles()?
            .into_iter()
            .map(|profile| {
                let rule_count = self.repository.count_rules(&profile.id)?;
                Ok(ProfileSummary {
                    profile,
                    rule_count,
                })
            })
            .collect()
    }

    /// Профиль вместе с правилами.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] при некорректном идентификаторе и
    /// [`EgressError::NotFound`], если профиля нет.
    pub fn profile_detail(&self, profile_id: &str) -> Result<ProfileDetail, EgressError> {
        let id = ProfileId::parse(profile_id)?;
        let profile = self.require_profile(&id)?;
        let rules = self.repository.list_rules(&id)?;

        Ok(ProfileDetail { profile, rules })
    }

    /// Создаёт профиль с запрещающим default action.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`], если имя пустое или уже занято.
    pub fn create_profile(&self, name: &str) -> Result<Profile, EgressError> {
        let name = Profile::validate_name(name)?;
        self.require_free_name(&name, None)?;

        self.repository.create_profile(&ProfileDraft {
            name,
            default_action: Action::Deny,
        })
    }

    /// Переименовывает профиль.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`], если имя пустое или занято другим
    /// профилем, и [`EgressError::NotFound`], если профиля нет.
    pub fn rename_profile(&self, profile_id: &str, name: &str) -> Result<Profile, EgressError> {
        let id = ProfileId::parse(profile_id)?;
        let name = Profile::validate_name(name)?;
        self.require_free_name(&name, Some(&id))?;

        self.repository.rename_profile(&id, &name)
    }

    /// Меняет default action профиля.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если профиля нет.
    pub fn set_default_action(
        &self,
        profile_id: &str,
        action: Action,
    ) -> Result<Profile, EgressError> {
        let id = ProfileId::parse(profile_id)?;

        self.repository.set_default_action(&id, action)
    }

    /// Удаляет профиль вместе с правилами.
    ///
    /// Последний профиль удалить нельзя: система не должна оставаться без единой
    /// политики.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] при попытке удалить последний
    /// профиль и [`EgressError::NotFound`], если профиля нет.
    pub fn delete_profile(&self, profile_id: &str) -> Result<(), EgressError> {
        let id = ProfileId::parse(profile_id)?;
        self.require_profile(&id)?;

        if self.repository.count_profiles()? <= 1 {
            return Err(EgressError::validation(
                "profile_id",
                "the last profile cannot be deleted",
            ));
        }

        self.repository.delete_profile(&id)
    }

    /// Добавляет правило в конец профиля.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] при некорректном вводе и
    /// [`EgressError::NotFound`], если профиля нет.
    pub fn add_rule(&self, profile_id: &str, input: RuleInput) -> Result<Rule, EgressError> {
        let id = ProfileId::parse(profile_id)?;
        self.require_profile(&id)?;

        self.repository.add_rule(&id, &input.into_draft()?)
    }

    /// Изменяет правило, сохраняя его позицию.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] при некорректном вводе и
    /// [`EgressError::NotFound`], если правила нет.
    pub fn update_rule(&self, rule_id: &str, input: RuleInput) -> Result<Rule, EgressError> {
        let id = RuleId::parse(rule_id)?;

        self.repository.update_rule(&id, &input.into_draft()?)
    }

    /// Удаляет правило.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если правила нет.
    pub fn delete_rule(&self, rule_id: &str) -> Result<(), EgressError> {
        self.repository.delete_rule(&RuleId::parse(rule_id)?)
    }

    /// Меняет порядок правил профиля.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`], если список не является полной
    /// перестановкой правил профиля без повторов, и [`EgressError::NotFound`],
    /// если профиля нет.
    pub fn reorder_rules(
        &self,
        profile_id: &str,
        rule_ids: &[String],
    ) -> Result<Vec<Rule>, EgressError> {
        let id = ProfileId::parse(profile_id)?;
        let current = self.repository.list_rules(&id)?;

        let mut ordered = Vec::with_capacity(rule_ids.len());
        for raw in rule_ids {
            ordered.push(RuleId::parse(raw)?);
        }

        let current_ids: HashSet<&str> = current.iter().map(|rule| rule.id.as_str()).collect();
        let ordered_ids: HashSet<&str> = ordered.iter().map(RuleId::as_str).collect();

        if ordered_ids.len() != ordered.len() {
            return Err(EgressError::validation(
                "rule_ids",
                "must not contain duplicates",
            ));
        }

        if ordered_ids != current_ids {
            return Err(EgressError::validation(
                "rule_ids",
                "must list exactly the rules of the profile",
            ));
        }

        self.repository.reorder_rules(&id, &ordered)
    }

    /// Оценивает соединение по политике профиля.
    ///
    /// Операция только читает данные: оценка не изменяет профили и правила.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] при некорректном host или port и
    /// [`EgressError::NotFound`], если профиля нет.
    pub fn evaluate(
        &self,
        profile_id: &str,
        host: &str,
        port: u16,
    ) -> Result<Decision, EgressError> {
        let target = EvaluationTarget::parse(host, port)?;
        let id = ProfileId::parse(profile_id)?;
        let profile = self.require_profile(&id)?;
        let rules = self.repository.list_rules(&id)?;

        Ok(Policy::new(&profile, &rules).evaluate(&target))
    }

    /// Возвращает профиль или ошибку отсутствия.
    fn require_profile(&self, id: &ProfileId) -> Result<Profile, EgressError> {
        self.repository
            .find_profile(id)?
            .ok_or_else(|| EgressError::not_found("profile"))
    }

    /// Проверяет, что имя профиля свободно.
    ///
    /// При переименовании профиль с тем же именем допустим: `except` исключает
    /// сам изменяемый профиль.
    fn require_free_name(&self, name: &str, except: Option<&ProfileId>) -> Result<(), EgressError> {
        if let Some(existing) = self.repository.find_profile_by_name(name)?
            && except != Some(&existing.id)
        {
            return Err(EgressError::validation("name", "must be unique"));
        }

        Ok(())
    }
}
