//! Domain-слой: модель предметной области и ошибки.
//!
//! Слой не выполняет ввод-вывод и не знает о способе доставки данных клиенту.

pub mod error;
pub mod policy;
pub mod proxy;
pub mod runtime;
pub mod time;
