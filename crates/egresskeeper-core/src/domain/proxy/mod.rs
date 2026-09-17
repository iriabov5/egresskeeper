//! Модель локального egress proxy.

pub mod listener;
pub mod outcome;

pub use listener::{Listener, ListenerDraft, ListenerId, ProxyPort};
pub use outcome::{PolicySnapshot, ProxyDecision, ProxyDecisionReason, decide, is_loopback_target};
