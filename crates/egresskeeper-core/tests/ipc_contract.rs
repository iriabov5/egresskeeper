//! Проверка IPC-контракта против общего fixture.
//!
//! Fixture `contracts/ipc/runtime_overview.sample.json` — единый источник истины
//! для формы DTO. Тот же файл читают frontend-тесты, поэтому дрейф между Rust и
//! TypeScript превращается в падающий тест, а не в ошибку времени выполнения.

use egresskeeper_core::RuntimeOverview;

const FIXTURE: &str = include_str!("../../../contracts/ipc/runtime_overview.sample.json");

fn fixture() -> RuntimeOverview {
    serde_json::from_str(FIXTURE).expect("fixture matches the RuntimeOverview contract")
}

#[test]
fn fixture_deserializes_into_runtime_overview() {
    let overview = fixture();

    assert_eq!(overview.app_version, "0.1.0");
    assert_eq!(overview.state_dir, "/home/dev/.local/share/egresskeeper");
    assert_eq!(overview.started_at_unix_ms, 1_758_100_000_000);
}

#[test]
fn serialized_overview_matches_fixture_field_for_field() {
    let expected = fixture();
    let actual = RuntimeOverview::new(
        expected.app_version.clone(),
        expected.state_dir.clone(),
        expected.started_at_unix_ms,
    );

    let actual_json = serde_json::to_value(&actual).expect("overview is serializable");
    let expected_json: serde_json::Value =
        serde_json::from_str(FIXTURE).expect("fixture is valid JSON");

    assert_eq!(
        actual_json, expected_json,
        "DTO shape drifted from contracts/ipc/runtime_overview.sample.json"
    );
}

#[test]
fn fixture_contains_no_unexpected_contract_fields() {
    let value: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture is valid JSON");
    let object = value.as_object().expect("fixture is a JSON object");

    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();

    assert_eq!(
        keys,
        vec![
            "app_version",
            "arch",
            "core_version",
            "os",
            "started_at_unix_ms",
            "state_dir",
        ],
        "IPC contract fields changed; update the fixture and the frontend types together"
    );
}
