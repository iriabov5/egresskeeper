//! Локальное хранилище на SQLite.

mod listeners;
mod migrations;
mod repository;
mod settings;
mod store;

pub use migrations::latest_version as latest_schema_version;
pub use repository::SqliteRepository;
pub use store::DATABASE_FILE_NAME;
