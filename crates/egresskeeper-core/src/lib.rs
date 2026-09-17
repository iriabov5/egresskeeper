//! Ядро EgressKeeper.
//!
//! Крейт содержит domain, application и infrastructure и **не зависит** от Tauri,
//! webview и GUI toolkit: composition root (`src-tauri`) передаёт платформенные
//! данные (например, каталог состояния) как параметры, поэтому логика ядра
//! тестируется headless.
//!
//! Направление зависимостей внутри крейта:
//!
//! ```text
//! infrastructure ──▶ application ──▶ domain
//! ```
//!
//! `domain` не знает ни о `application`, ни об `infrastructure`.
//! `application` зависит только от `domain` и объявляет порты.
//! `infrastructure` реализует порты `application`.

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::listeners::{ListenerRuntimeState, ListenerService};
pub use application::policy::{PolicyService, ProfileDetail, ProfileSummary, RuleInput};
pub use application::ports::{
    ListenerRepository, PolicyRepository, PolicySource, SettingsRepository, StateDirectory,
};
pub use application::runtime_info::RuntimeInfoService;
pub use application::settings::{CloseBehavior, SettingsService, ShellSettings};
pub use domain::error::{EgressError, ErrorCode};
pub use domain::policy::action::Action;
pub use domain::policy::decision::{Decision, DecisionReason, EvaluationTarget, Policy};
pub use domain::policy::entities::{Profile, ProfileDraft, ProfileId, Rule, RuleDraft, RuleId};
pub use domain::policy::host::{HostKind, HostMatcher, NormalizedHost};
pub use domain::policy::port::PortSpec;
pub use domain::proxy::listener::{Listener, ListenerDraft, ListenerId, ProxyPort};
pub use domain::proxy::outcome::{
    PolicySnapshot, ProxyDecision, ProxyDecisionReason, decide, is_loopback_target,
};
pub use domain::runtime::RuntimeOverview;
pub use infrastructure::fs_state_dir::FsStateDirectory;
pub use infrastructure::proxy::{
    ListenerFailure, ListenerHealth, ListenerState, ProxyConfig, ProxyLimits, ProxyRuntime,
    RepositoryPolicySource,
};
pub use infrastructure::sqlite::SqliteRepository;
