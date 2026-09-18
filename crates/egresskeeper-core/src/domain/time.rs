//! Работа со временем.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::error::EgressError;

/// Текущее время в миллисекундах Unix epoch.
///
/// Вынесено отдельно, чтобы отметки времени создавались в одном месте и не
/// зависели от системных часов, вызванных из разных слоёв.
///
/// # Errors
///
/// Возвращает [`EgressError::Internal`], если системные часы установлены раньше
/// начала Unix epoch.
pub fn now_unix_ms() -> Result<i64, EgressError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|source| EgressError::Internal {
            source: Box::new(source),
        })?;

    Ok(i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_close_to_itself() {
        let first = now_unix_ms().expect("clock is available");
        let second = now_unix_ms().expect("clock is available");

        assert!(second >= first);
        assert!(first > 1_600_000_000_000, "clock must be after 2020");
    }
}
