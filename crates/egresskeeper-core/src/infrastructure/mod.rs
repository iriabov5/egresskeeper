//! Infrastructure-слой: реализации портов application.
//!
//! Здесь находятся сеть, файловая система, хранилище и OS-интеграции. Слой
//! зависит от `application` и `domain`, но не наоборот.

pub mod fs_state_dir;
pub mod proxy;
pub mod sqlite;
