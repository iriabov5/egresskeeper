//! Сопоставление host.
//!
//! Это самая чувствительная часть продукта: ошибка здесь превращается в
//! незаметную дыру в контроле egress. Поэтому модель намеренно узкая — два явных
//! вида сопоставления и никаких «умных» неявных расширений:
//!
//! - [`HostMatcher::Exact`] совпадает только с указанным host;
//! - [`HostMatcher::Subdomains`] совпадает с поддоменами любой глубины и
//!   **не** совпадает с самим доменом.
//!
//! Совпадение поддоменов проверяется по границе метки, поэтому `evil-example.com`
//! и `example.com.evil.test` не совпадают ни с `example.com`, ни с
//! `*.example.com`.

use serde::{Deserialize, Serialize};

use crate::domain::error::EgressError;

/// Максимальная длина hostname.
const MAX_HOST_LENGTH: usize = 253;
/// Максимальная длина одной метки (label).
const MAX_LABEL_LENGTH: usize = 63;

/// Host, прошедший нормализацию.
///
/// Нормализация: обрезка внешних пробелов, приведение к нижнему регистру,
/// отбрасывание завершающей точки, проверка ASCII и корректности меток.
/// Существование этого типа означает, что host уже проверен.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NormalizedHost(String);

impl NormalizedHost {
    /// Разбирает и нормализует host.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] с полем `host`, если значение
    /// пустое, содержит не-ASCII символы, содержит пустые метки, недопустимые
    /// символы или числовой top-level label.
    pub fn parse(input: &str) -> Result<Self, EgressError> {
        normalize_host(input).map(Self)
    }

    /// Нормализованное значение.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Возвращает `true`, если host является IP-литералом (IPv4 или IPv6 в скобках).
    #[must_use]
    pub fn is_ip_literal(&self) -> bool {
        is_ipv4_literal(&self.0) || self.0.starts_with('[')
    }
}

impl std::fmt::Display for NormalizedHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Вид сопоставления host, который выбирает пользователь.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    /// Точное совпадение.
    Exact,
    /// Совпадение с поддоменами.
    Subdomains,
}

/// Правило сопоставления host.
///
/// Значения внутри вариантов уже нормализованы: сконструировать matcher можно
/// только через [`HostMatcher::exact`], [`HostMatcher::subdomains`] или
/// [`HostMatcher::from_parts`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum HostMatcher {
    /// Точное совпадение с host.
    Exact(String),
    /// Совпадение с поддоменами домена любой глубины; сам домен не совпадает.
    Subdomains(String),
}

impl HostMatcher {
    /// Создаёт matcher точного совпадения.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку валидации, если host некорректен.
    pub fn exact(input: &str) -> Result<Self, EgressError> {
        Ok(Self::Exact(NormalizedHost::parse(input)?.0))
    }

    /// Создаёт matcher поддоменов.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку валидации, если домен некорректен или является
    /// IP-литералом: у IP-адреса нет поддоменов.
    pub fn subdomains(input: &str) -> Result<Self, EgressError> {
        let domain = NormalizedHost::parse(input)?;
        if domain.is_ip_literal() {
            return Err(EgressError::validation(
                "host",
                "subdomain matcher cannot be an ip address",
            ));
        }
        Ok(Self::Subdomains(domain.0))
    }

    /// Создаёт matcher по виду и значению из запроса пользователя.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку валидации, если значение не подходит выбранному виду.
    pub fn from_parts(kind: HostKind, value: &str) -> Result<Self, EgressError> {
        match kind {
            HostKind::Exact => Self::exact(value),
            HostKind::Subdomains => Self::subdomains(value),
        }
    }

    /// Вид сопоставления.
    #[must_use]
    pub const fn kind(&self) -> HostKind {
        match self {
            Self::Exact(_) => HostKind::Exact,
            Self::Subdomains(_) => HostKind::Subdomains,
        }
    }

    /// Нормализованное значение matcher.
    #[must_use]
    pub fn value(&self) -> &str {
        match self {
            Self::Exact(value) | Self::Subdomains(value) => value,
        }
    }

    /// Проверяет совпадение с уже нормализованным host.
    ///
    /// Для поддоменов совпадение проверяется по границе метки, поэтому
    /// `evil-example.com` не совпадает с `*.example.com`.
    #[must_use]
    pub fn matches(&self, host: &NormalizedHost) -> bool {
        match self {
            Self::Exact(expected) => host.as_str() == expected,
            Self::Subdomains(domain) => is_subdomain_of(host.as_str(), domain),
        }
    }

    /// Описание matcher для показа пользователю.
    #[must_use]
    pub fn display_value(&self) -> String {
        match self {
            Self::Exact(value) => value.clone(),
            Self::Subdomains(domain) => format!("*.{domain}"),
        }
    }
}

/// Проверяет, что `host` является поддоменом `domain` по границе метки.
fn is_subdomain_of(host: &str, domain: &str) -> bool {
    let (Some(boundary), true) = (
        host.len().checked_sub(domain.len() + 1),
        host.len() > domain.len(),
    ) else {
        return false;
    };

    host.ends_with(domain) && host.as_bytes().get(boundary) == Some(&b'.')
}

/// Нормализует host.
///
/// Разбито на шаги, потому что проверка формы, приведение к нижнему регистру и
/// валидация меток — разные задачи, и вместе они дают нечитаемую функцию.
fn normalize_host(input: &str) -> Result<String, EgressError> {
    let trimmed = input.trim();

    if trimmed.is_empty() {
        return Err(empty_host_error());
    }

    if !trimmed.is_ascii() {
        return Err(EgressError::validation(
            "host",
            "must contain ascii characters only; international domains are not supported yet",
        ));
    }

    if let Some(rest) = trimmed.strip_prefix('[') {
        return normalize_ipv6_literal(rest);
    }

    if trimmed.contains(':') {
        return Err(EgressError::validation(
            "host",
            "must not contain a port; pass the port separately",
        ));
    }

    let lowered = normalize_domain(trimmed)?;

    if is_ipv4_literal(&lowered) {
        return Ok(lowered);
    }

    validate_labels(&lowered)?;

    Ok(lowered)
}

/// Отбрасывает завершающую точку и приводит домен к нижнему регистру.
fn normalize_domain(input: &str) -> Result<String, EgressError> {
    let without_trailing_dot = input.strip_suffix('.').unwrap_or(input);

    if without_trailing_dot.is_empty() {
        return Err(empty_host_error());
    }

    let lowered = without_trailing_dot.to_ascii_lowercase();

    if lowered.len() > MAX_HOST_LENGTH {
        return Err(EgressError::validation("host", "is too long"));
    }

    Ok(lowered)
}

/// Проверяет метки домена и top-level label.
fn validate_labels(host: &str) -> Result<(), EgressError> {
    for label in host.split('.') {
        validate_label(label)?;
    }

    if let Some(top_level) = host.rsplit('.').next()
        && top_level.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(EgressError::validation(
            "host",
            "top-level label must not be numeric",
        ));
    }

    Ok(())
}

/// Проверяет одну метку домена.
fn validate_label(label: &str) -> Result<(), EgressError> {
    if label.is_empty() {
        return Err(EgressError::validation(
            "host",
            "must not contain empty labels",
        ));
    }

    if label.len() > MAX_LABEL_LENGTH {
        return Err(EgressError::validation("host", "label is too long"));
    }

    if label.starts_with('-') || label.ends_with('-') {
        return Err(EgressError::validation(
            "host",
            "label must not start or end with a hyphen",
        ));
    }

    if !label
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(EgressError::validation(
            "host",
            "contains unsupported characters",
        ));
    }

    Ok(())
}

/// Ошибка пустого host.
const fn empty_host_error() -> EgressError {
    EgressError::validation("host", "must not be empty")
}

/// Нормализует IPv6-литерал, записанный в квадратных скобках.
fn normalize_ipv6_literal(rest: &str) -> Result<String, EgressError> {
    let Some(inner) = rest.strip_suffix(']') else {
        return Err(EgressError::validation(
            "host",
            "ipv6 literal must be enclosed in brackets",
        ));
    };

    let lowered = inner.to_ascii_lowercase();

    if lowered.is_empty() || !lowered.contains(':') {
        return Err(EgressError::validation("host", "ipv6 literal is invalid"));
    }

    if !lowered
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() || byte == b':' || byte == b'.')
    {
        return Err(EgressError::validation("host", "ipv6 literal is invalid"));
    }

    Ok(format!("[{lowered}]"))
}

/// Возвращает `true`, если значение является IPv4-литералом.
fn is_ipv4_literal(value: &str) -> bool {
    let mut parts = 0_usize;

    for part in value.split('.') {
        if part.is_empty() || part.parse::<u8>().is_err() {
            return false;
        }
        parts += 1;
    }

    parts == 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::ErrorCode;

    fn host(value: &str) -> NormalizedHost {
        NormalizedHost::parse(value).expect("host is valid")
    }

    fn rejection(value: &str) -> EgressError {
        NormalizedHost::parse(value).expect_err("host must be rejected")
    }

    #[test]
    fn normalization_lowercases_and_trims() {
        assert_eq!(host("  API.Example.COM  ").as_str(), "api.example.com");
    }

    #[test]
    fn normalization_drops_single_trailing_dot() {
        assert_eq!(host("api.example.com.").as_str(), "api.example.com");
    }

    #[test]
    fn normalization_rejects_empty_input() {
        for value in ["", "   ", "."] {
            let error = rejection(value);
            assert_eq!(error.code(), ErrorCode::Validation);
            assert_eq!(error.invalid_field(), Some("host"));
        }
    }

    #[test]
    fn normalization_rejects_non_ascii_hosts() {
        let error = rejection("пример.рф");

        assert_eq!(error.code(), ErrorCode::Validation);

        assert_eq!(error.invalid_field(), Some("host"));
    }

    #[test]
    fn normalization_rejects_structural_problems() {
        for value in [
            "a..example.com",
            "-api.example.com",
            "api-.example.com",
            "api.*.com",
        ] {
            let error = rejection(value);
            assert_eq!(error.code(), ErrorCode::Validation, "value: {value}");
        }
    }

    #[test]
    fn normalization_rejects_label_and_host_length_overflow() {
        let long_label = "a".repeat(MAX_LABEL_LENGTH + 1);
        assert!(NormalizedHost::parse(&long_label).is_err());

        let long_host = [
            "a".repeat(60),
            "b".repeat(60),
            "c".repeat(60),
            "d".repeat(60),
            "e".repeat(60),
        ]
        .join(".");
        assert!(long_host.len() > MAX_HOST_LENGTH);
        assert!(NormalizedHost::parse(&long_host).is_err());
    }

    #[test]
    fn normalization_rejects_numeric_top_level_label() {
        assert!(NormalizedHost::parse("example.123").is_err());
        assert!(NormalizedHost::parse("999.1.1.1").is_err());
    }

    #[test]
    fn normalization_accepts_ipv4_literal() {
        let parsed = host("192.168.0.10");

        assert_eq!(parsed.as_str(), "192.168.0.10");
        assert!(parsed.is_ip_literal());
    }

    #[test]
    fn normalization_accepts_bracketed_ipv6_literal() {
        let parsed = host("[::1]");

        assert_eq!(parsed.as_str(), "[::1]");
        assert!(parsed.is_ip_literal());
    }

    #[test]
    fn normalization_rejects_malformed_ipv6_and_ports() {
        for value in ["[::1", "[]", "[zzzz::1]", "example.com:443"] {
            assert!(
                NormalizedHost::parse(value).is_err(),
                "value must be rejected: {value}"
            );
        }
    }

    #[test]
    fn single_label_hosts_are_allowed() {
        assert_eq!(host("localhost").as_str(), "localhost");
        assert_eq!(host("LocalHost").as_str(), "localhost");
    }

    #[test]
    fn exact_matcher_matches_only_the_same_host() {
        let matcher = HostMatcher::exact("api.example.com").expect("matcher");

        assert!(matcher.matches(&host("api.example.com")));
        assert!(matcher.matches(&host("API.EXAMPLE.COM.")));
        assert!(!matcher.matches(&host("other.example.com")));
        assert!(!matcher.matches(&host("example.com")));
        assert!(!matcher.matches(&host("api.example.com.evil.test")));
    }

    #[test]
    fn subdomains_matcher_matches_subdomains_of_any_depth() {
        let matcher = HostMatcher::subdomains("example.com").expect("matcher");

        assert!(matcher.matches(&host("api.example.com")));
        assert!(matcher.matches(&host("a.b.example.com")));
        assert!(matcher.matches(&host("API.Example.com")));
    }

    #[test]
    fn subdomains_matcher_does_not_match_the_apex_domain() {
        let matcher = HostMatcher::subdomains("example.com").expect("matcher");

        assert!(!matcher.matches(&host("example.com")));
    }

    #[test]
    fn subdomains_matcher_respects_label_boundary() {
        let matcher = HostMatcher::subdomains("example.com").expect("matcher");

        assert!(!matcher.matches(&host("evil-example.com")));
        assert!(!matcher.matches(&host("example.com.evil.test")));
        assert!(!matcher.matches(&host("notexample.com")));
    }

    #[test]
    fn subdomains_matcher_rejects_ip_literals() {
        for value in ["192.168.0.10", "[::1]"] {
            let error = HostMatcher::subdomains(value).expect_err("ip literal");
            assert_eq!(error.code(), ErrorCode::Validation);
            assert_eq!(error.invalid_field(), Some("host"));
        }
    }

    #[test]
    fn from_parts_builds_matching_kind() {
        let exact = HostMatcher::from_parts(HostKind::Exact, "api.example.com").expect("exact");
        let subdomains =
            HostMatcher::from_parts(HostKind::Subdomains, "example.com").expect("subdomains");

        assert_eq!(exact.kind(), HostKind::Exact);
        assert_eq!(exact.value(), "api.example.com");
        assert_eq!(subdomains.kind(), HostKind::Subdomains);
        assert_eq!(subdomains.display_value(), "*.example.com");
    }

    #[test]
    fn matcher_serializes_with_kind_and_value() {
        let matcher = HostMatcher::subdomains("example.com").expect("matcher");

        assert_eq!(
            serde_json::to_value(&matcher).expect("serialize"),
            serde_json::json!({ "kind": "subdomains", "value": "example.com" })
        );
    }
}
