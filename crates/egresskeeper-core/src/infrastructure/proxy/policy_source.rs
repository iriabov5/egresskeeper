//! Источник политики для рантайма proxy.

use crate::application::ports::PolicySource;
use crate::domain::error::EgressError;
use crate::domain::policy::entities::ProfileId;
use crate::domain::proxy::outcome::PolicySnapshot;
use crate::infrastructure::sqlite::SqliteRepository;

/// Источник политики поверх локального хранилища.
///
/// Профиль и его правила читаются в рамках одного соединения, поэтому снимок не
/// может смешать старую версию профиля с новой версией правил.
#[derive(Debug)]
pub struct RepositoryPolicySource {
    repository: SqliteRepository,
}

impl RepositoryPolicySource {
    /// Создаёт источник поверх хранилища.
    ///
    /// Хранилище клонируется дешёво и использует то же соединение, поэтому
    /// источник политики видит те же данные, что и сервисы приложения.
    #[must_use]
    pub const fn new(repository: SqliteRepository) -> Self {
        Self { repository }
    }
}

impl PolicySource for RepositoryPolicySource {
    fn snapshot(&self, profile_id: &ProfileId) -> Result<Option<PolicySnapshot>, EgressError> {
        self.repository.with_connection(|connection| {
            let Some(profile) = SqliteRepository::read_profile(connection, profile_id)? else {
                return Ok(None);
            };

            let rules = SqliteRepository::read_rules_with(connection, profile_id)?;

            Ok(Some(PolicySnapshot::Loaded { profile, rules }))
        })
    }
}
