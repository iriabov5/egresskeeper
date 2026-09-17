//! Порты, которые application объявляет для infrastructure.

use std::fmt::Debug;
use std::path::PathBuf;

use crate::domain::error::EgressError;
use crate::domain::policy::action::Action;
use crate::domain::policy::entities::{Profile, ProfileDraft, ProfileId, Rule, RuleDraft, RuleId};
use crate::domain::proxy::listener::{Listener, ListenerDraft, ListenerId};
use crate::domain::proxy::outcome::PolicySnapshot;

/// Порт доступа к каталогу состояния приложения.
///
/// Каталог резолвится вне ядра (composition root передаёт платформенно-корректный
/// путь), а ядро отвечает только за его подготовку и проверку. Это держит ядро
/// свободным от Tauri и позволяет тестировать поведение на временных каталогах.
pub trait StateDirectory: Debug + Send + Sync {
    /// Гарантирует, что каталог состояния существует, и возвращает его путь.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::StateDirUnavailable`], если каталог не удалось
    /// подготовить, и [`EgressError::Validation`], если путь не задан.
    fn ensure(&self) -> Result<PathBuf, EgressError>;
}

/// Порт хранения политик.
///
/// Идентификаторы и отметки времени сущностей создаёт реализация порта: она
/// владеет идентичностью и монотонностью времени, как это делает база данных.
/// Валидация ввода выполняется до вызова порта, поэтому методы принимают уже
/// проверенные [`ProfileDraft`] и [`RuleDraft`].
///
/// Методы, изменяющие существующую сущность, возвращают
/// [`EgressError::NotFound`], если сущности нет.
pub trait PolicyRepository: Debug + Send + Sync {
    /// Все профили в порядке создания.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    fn list_profiles(&self) -> Result<Vec<Profile>, EgressError>;

    /// Находит профиль по идентификатору.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    fn find_profile(&self, id: &ProfileId) -> Result<Option<Profile>, EgressError>;

    /// Находит профиль по имени без учёта регистра.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    fn find_profile_by_name(&self, name: &str) -> Result<Option<Profile>, EgressError>;

    /// Количество профилей.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    fn count_profiles(&self) -> Result<u64, EgressError>;

    /// Создаёт профиль.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] при нарушении уникальности имени.
    fn create_profile(&self, draft: &ProfileDraft) -> Result<Profile, EgressError>;

    /// Переименовывает профиль.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`] или [`EgressError::Validation`].
    fn rename_profile(&self, id: &ProfileId, name: &str) -> Result<Profile, EgressError>;

    /// Меняет default action профиля.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если профиля нет.
    fn set_default_action(&self, id: &ProfileId, action: Action) -> Result<Profile, EgressError>;

    /// Удаляет профиль вместе с его правилами.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если профиля нет.
    fn delete_profile(&self, id: &ProfileId) -> Result<(), EgressError>;

    /// Правила профиля в порядке применения.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    fn list_rules(&self, profile_id: &ProfileId) -> Result<Vec<Rule>, EgressError>;

    /// Количество правил профиля.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    fn count_rules(&self, profile_id: &ProfileId) -> Result<u32, EgressError>;

    /// Добавляет правило в конец профиля.
    ///
    /// Позиция назначается реализацией порта, поэтому операция атомарна.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если профиля нет.
    fn add_rule(&self, profile_id: &ProfileId, draft: &RuleDraft) -> Result<Rule, EgressError>;

    /// Изменяет правило, сохраняя его позицию.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если правила нет.
    fn update_rule(&self, id: &RuleId, draft: &RuleDraft) -> Result<Rule, EgressError>;

    /// Удаляет правило.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если правила нет.
    fn delete_rule(&self, id: &RuleId) -> Result<(), EgressError>;

    /// Заменяет порядок правил профиля.
    ///
    /// При ошибке порядок остаётся прежним.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`], если список не является полной
    /// перестановкой правил профиля, и [`EgressError::NotFound`], если профиля нет.
    fn reorder_rules(
        &self,
        profile_id: &ProfileId,
        ordered: &[RuleId],
    ) -> Result<Vec<Rule>, EgressError>;
}

/// Порт хранения конфигурации listeners proxy.
///
/// Идентификаторы и отметки времени создаёт реализация порта, как и в
/// [`PolicyRepository`]. Проверка существования профиля вынесена сюда, потому что
/// она нужна именно этому сценарию: listener нельзя привязать к несуществующему
/// профилю.
pub trait ListenerRepository: Debug + Send + Sync {
    /// Все listeners в порядке создания.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    fn list_listeners(&self) -> Result<Vec<Listener>, EgressError>;

    /// Находит listener по идентификатору.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    fn find_listener(&self, id: &ListenerId) -> Result<Option<Listener>, EgressError>;

    /// Возвращает `true`, если профиль существует.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    fn profile_exists(&self, id: &ProfileId) -> Result<bool, EgressError>;

    /// Создаёт listener.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] при занятом порте.
    fn create_listener(&self, draft: &ListenerDraft) -> Result<Listener, EgressError>;

    /// Изменяет конфигурацию listener'а.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`] или [`EgressError::Validation`].
    fn update_listener(
        &self,
        id: &ListenerId,
        draft: &ListenerDraft,
    ) -> Result<Listener, EgressError>;

    /// Меняет признак включения listener'а.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если listener'а нет.
    fn set_listener_enabled(&self, id: &ListenerId, enabled: bool)
    -> Result<Listener, EgressError>;

    /// Удаляет listener.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если listener'а нет.
    fn delete_listener(&self, id: &ListenerId) -> Result<(), EgressError>;
}

/// Порт хранения настроек приложения.
///
/// Настройки — пары «ключ — значение»: отсутствие ключа означает значение по
/// умолчанию, которое знает сервис настроек, а не хранилище.
pub trait SettingsRepository: Debug + Send + Sync {
    /// Возвращает сохранённое значение настройки.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    fn get_setting(&self, key: &str) -> Result<Option<String>, EgressError>;

    /// Сохраняет значение настройки, перезаписывая прежнее.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если запись не удалась.
    fn set_setting(&self, key: &str, value: &str) -> Result<(), EgressError>;
}

/// Источник политики для рантайма proxy.
///
/// Рантайм читает снимок политики на каждое соединение, поэтому реализация порта
/// должна быть потокобезопасной и не выполнять сетевых операций. Отсутствие
/// профиля и ошибка чтения одинаково означают, что применять нечего: proxy
/// запрещает соединение (fail-closed).
pub trait PolicySource: Debug + Send + Sync {
    /// Возвращает снимок политики профиля.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если политику не удалось прочитать.
    fn snapshot(&self, profile_id: &ProfileId) -> Result<Option<PolicySnapshot>, EgressError>;
}
