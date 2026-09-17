//! Use-cases конфигурации listeners proxy.
//!
//! Сервис отвечает за валидацию порта, проверку существования профиля и за
//! правило «работающий listener нельзя переконфигурировать»: изменение порта или
//! профиля применяется только к остановленному listener'у, иначе состояние
//! приложения перестало бы соответствовать конфигурации.
//!
//! Переходы состояния выполняются через порт [`ListenerRuntimeState`]: сервис
//! владеет правилами, а рантайм proxy — фактом работы. Порт намеренно
//! синхронный: запуск и остановка listener'а — это сигнал, а не ожидание
//! завершения, поэтому приложение не блокируется на сетевых операциях.

use crate::application::ports::ListenerRepository;
use crate::domain::error::EgressError;
use crate::domain::policy::entities::ProfileId;
use crate::domain::proxy::listener::{Listener, ListenerDraft, ListenerId, ProxyPort};

/// Состояние работы listeners, известное рантайму proxy.
pub trait ListenerRuntimeState: std::fmt::Debug + Send + Sync {
    /// Возвращает `true`, если listener принимает соединения или запускается.
    fn is_active(&self, id: &ListenerId) -> bool;

    /// Просит рантайм запустить listener с его конфигурацией.
    ///
    /// Listener передаётся целиком, чтобы рантайму не требовался доступ к
    /// хранилищу: он получает только то, что нужно для приёма соединений.
    /// Идемпотентно и не блокирует.
    fn request_start(&self, listener: &Listener);

    /// Просит рантайм остановить listener. Идемпотентно и не блокирует.
    fn request_stop(&self, id: &ListenerId);
}

/// Use-cases конфигурации listeners.
#[derive(Debug)]
pub struct ListenerService<R, S> {
    repository: R,
    runtime: S,
}

impl<R: ListenerRepository, S: ListenerRuntimeState> ListenerService<R, S> {
    /// Создаёт сервис поверх реализации порта и рантайма.
    pub const fn new(repository: R, runtime: S) -> Self {
        Self {
            repository,
            runtime,
        }
    }

    /// Все listeners в порядке создания.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если данные недоступны.
    pub fn list(&self) -> Result<Vec<Listener>, EgressError> {
        self.repository.list_listeners()
    }

    /// Создаёт listener.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] при некорректном или занятом порте
    /// и [`EgressError::NotFound`], если профиль не существует.
    pub fn create(&self, port: u16, profile_id: &str) -> Result<Listener, EgressError> {
        let draft = self.draft(port, profile_id)?;

        self.repository.create_listener(&draft)
    }

    /// Изменяет конфигурацию listener'а.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если listener'а или профиля нет, и
    /// [`EgressError::Validation`] при некорректном порте или попытке изменить
    /// работающий listener.
    pub fn update(
        &self,
        listener_id: &str,
        port: u16,
        profile_id: &str,
    ) -> Result<Listener, EgressError> {
        let id = ListenerId::parse(listener_id)?;

        // Существование проверяется до правила о состоянии: пользователь должен
        // получить «listener не найден», а не «listener работает».
        self.repository
            .find_listener(&id)?
            .ok_or_else(|| EgressError::not_found("listener"))?;

        if self.runtime.is_active(&id) {
            return Err(EgressError::validation(
                "listener_id",
                "listener must be stopped before reconfiguration",
            ));
        }

        let draft = self.draft(port, profile_id)?;

        self.repository.update_listener(&id, &draft)
    }

    /// Включает или выключает listener.
    ///
    /// Признак включения — это намерение пользователя; фактическое состояние
    /// приходит из рантайма и может отличаться, например при занятом порте.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если listener'а нет.
    pub fn set_enabled(&self, listener_id: &str, enabled: bool) -> Result<Listener, EgressError> {
        let id = ListenerId::parse(listener_id)?;
        let listener = self.repository.set_listener_enabled(&id, enabled)?;

        if enabled {
            self.runtime.request_start(&listener);
        } else {
            self.runtime.request_stop(&id);
        }

        Ok(listener)
    }

    /// Удаляет listener, предварительно останавливая его.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::NotFound`], если listener'а нет.
    pub fn delete(&self, listener_id: &str) -> Result<(), EgressError> {
        let id = ListenerId::parse(listener_id)?;

        self.repository
            .find_listener(&id)?
            .ok_or_else(|| EgressError::not_found("listener"))?;

        // Сначала сигнал остановки, затем удаление конфигурации: иначе рантайм
        // остался бы с listener'ом, которого больше нет в хранилище.
        self.runtime.request_stop(&id);

        self.repository.delete_listener(&id)
    }

    /// Проверяет порт и профиль и собирает данные listener'а.
    fn draft(&self, port: u16, profile_id: &str) -> Result<ListenerDraft, EgressError> {
        let profile_id = ProfileId::parse(profile_id)?;

        if !self.repository.profile_exists(&profile_id)? {
            return Err(EgressError::not_found("profile"));
        }

        Ok(ListenerDraft {
            port: ProxyPort::parse(port)?,
            profile_id,
        })
    }
}
