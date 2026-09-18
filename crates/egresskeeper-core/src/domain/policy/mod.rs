//! Модель политики: профили, правила, сопоставление и решения.
//!
//! Слой не выполняет ввод-вывод: он описывает семантику и валидацию, а хранение
//! реализуется в infrastructure через порт `PolicyRepository`.

pub mod action;
pub mod decision;
pub mod entities;
pub mod host;
pub mod port;

pub use action::Action;
pub use decision::{Decision, DecisionReason, EvaluationTarget, Policy};
pub use entities::{Profile, ProfileDraft, ProfileId, Rule, RuleDraft, RuleId};
pub use host::{HostKind, HostMatcher, NormalizedHost};
pub use port::PortSpec;
