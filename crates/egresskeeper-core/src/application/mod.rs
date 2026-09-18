//! Application-слой: use-cases и порты к infrastructure.
//!
//! Слой зависит только от `domain` и объявляет порты, которые реализует
//! `infrastructure`. Ввод-вывод здесь не выполняется напрямую.

pub mod listeners;
pub mod policy;
pub mod ports;
pub mod runtime_info;
pub mod settings;
