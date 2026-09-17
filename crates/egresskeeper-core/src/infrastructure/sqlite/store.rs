//! Открытие, настройка и подготовка локальной базы данных.
//!
//! База живёт в каталоге состояния приложения. Режим WAL включён, чтобы чтения
//! не блокировались записью: этим воспользуется audit-писатель, когда появится
//! proxy runtime.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;

use crate::domain::error::EgressError;
use crate::domain::policy::action::Action;
use crate::domain::policy::entities::{Profile, ProfileId};
use crate::domain::time::now_unix_ms;
use crate::infrastructure::sqlite::migrations;

/// Имя файла базы данных в каталоге состояния.
pub const DATABASE_FILE_NAME: &str = "egresskeeper.sqlite3";

/// Имя профиля, создаваемого при первой инициализации.
const DEFAULT_PROFILE_NAME: &str = "По умолчанию";

/// Таймаут ожидания снятия блокировки базы.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Соединение с базой данных под мьютексом.
///
/// Одно соединение на процесс: запись идёт короткими транзакциями, а WAL даёт
/// параллельное чтение. Пул понадобится, если появится конкурентный писатель.
#[derive(Debug)]
pub(crate) struct SqliteStore {
    connection: Mutex<Connection>,
}

impl SqliteStore {
    /// Открывает базу в каталоге состояния, создавая каталог и файл при
    /// необходимости.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::StateDirUnavailable`], если каталог не удалось
    /// подготовить, и [`EgressError::Storage`], если база не открылась.
    pub(crate) fn open(state_dir: &Path) -> Result<Self, EgressError> {
        std::fs::create_dir_all(state_dir)
            .map_err(|source| EgressError::StateDirUnavailable { source })?;

        Self::open_path(state_dir.join(DATABASE_FILE_NAME))
    }

    /// Открывает базу в памяти.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Storage`], если база не открылась.
    pub(crate) fn open_in_memory() -> Result<Self, EgressError> {
        Self::open_path(PathBuf::from(":memory:"))
    }

    /// Возвращает версию схемы хранилища.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если версия недоступна.
    pub(crate) fn schema_version(&self) -> Result<i64, EgressError> {
        self.with_connection(|connection| {
            connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .map_err(|source| storage_error("read schema version", source))
        })
    }

    /// Выполняет операцию над соединением.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если соединение недоступно.
    pub(crate) fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, EgressError>,
    ) -> Result<T, EgressError> {
        let mut guard = self
            .connection
            .lock()
            .map_err(|_| EgressError::storage_message("database connection lock is poisoned"))?;

        operation(&mut guard)
    }

    /// Открывает базу по пути, настраивает соединение и применяет миграции.
    fn open_path(path: PathBuf) -> Result<Self, EgressError> {
        let connection =
            Connection::open(&path).map_err(|source| storage_error("open database", source))?;

        configure_connection(&connection)?;

        let store = Self {
            connection: Mutex::new(connection),
        };

        store.with_connection(migrations::apply)?;
        store.seed_default_profile()?;

        Ok(store)
    }

    /// Создаёт профиль по умолчанию, если профилей ещё нет.
    ///
    /// Профиль создаётся один раз: повторная инициализация не должна возвращать
    /// удалённые пользователем данные.
    fn seed_default_profile(&self) -> Result<(), EgressError> {
        self.with_connection(|connection| {
            let existing: i64 = connection
                .query_row("SELECT COUNT(*) FROM profiles", [], |row| row.get(0))
                .map_err(|source| storage_error("count profiles", source))?;

            if existing > 0 {
                return Ok(());
            }

            let now = now_unix_ms()?;

            connection
                .execute(
                    "INSERT INTO profiles (id, name, name_folded, default_action, created_at_unix_ms, updated_at_unix_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                    rusqlite::params![
                        ProfileId::new().as_str(),
                        DEFAULT_PROFILE_NAME,
                        Profile::fold_name(DEFAULT_PROFILE_NAME),
                        Action::Deny.as_str(),
                        now
                    ],
                )
                .map_err(|source| storage_error("seed default profile", source))?;

            tracing::info!(profile = DEFAULT_PROFILE_NAME, "default policy profile created");

            Ok(())
        })
    }
}

/// Настраивает соединение: WAL, внешние ключи и таймаут блокировки.
fn configure_connection(connection: &Connection) -> Result<(), EgressError> {
    // Для базы в памяти SQLite оставляет режим `memory`; ошибкой это не является.
    connection
        .query_row("PRAGMA journal_mode = WAL", [], |_row| Ok(()))
        .map_err(|source| storage_error("enable wal", source))?;

    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|source| storage_error("enable foreign keys", source))?;

    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|source| storage_error("set busy timeout", source))?;

    Ok(())
}

/// Оборачивает ошибку SQLite в ошибку хранилища.
pub(crate) fn storage_error(context: &str, source: rusqlite::Error) -> EgressError {
    EgressError::storage(std::io::Error::other(format!("{context}: {source}")))
}

/// Возвращает `true`, если ошибка вызвана нарушением уникальности.
pub(crate) fn is_unique_violation(source: &rusqlite::Error) -> bool {
    matches!(
        source,
        rusqlite::Error::SqliteFailure(error, _)
            if error.code == rusqlite::ErrorCode::ConstraintViolation
                && error.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    )
}
