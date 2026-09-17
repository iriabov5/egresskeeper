//! Проверки [`FsStateDirectory`] на реальной файловой системе.
//!
//! Тесты используют временные каталоги, поэтому не зависят от окружения и не
//! трогают данные пользователя.

use std::fs;

use egresskeeper_core::{EgressError, ErrorCode, FsStateDirectory, StateDirectory};

#[test]
fn ensure_creates_missing_directory_tree() {
    let root = tempfile::tempdir().expect("temporary directory");
    let target = root.path().join("nested").join("state");

    let resolved = FsStateDirectory::new(&target)
        .ensure()
        .expect("directory is created");

    assert_eq!(resolved, target);
    assert!(target.is_dir(), "directory must exist after ensure");
}

#[test]
fn ensure_is_idempotent_for_existing_directory() {
    let root = tempfile::tempdir().expect("temporary directory");
    let target = root.path().join("state");
    let directory = FsStateDirectory::new(&target);

    let first = directory.ensure().expect("first call succeeds");
    let second = directory.ensure().expect("second call succeeds");

    assert_eq!(first, target);
    assert_eq!(second, target);
}

#[test]
fn ensure_fails_when_path_is_a_file() {
    let root = tempfile::tempdir().expect("temporary directory");
    let file_path = root.path().join("not-a-directory");
    fs::write(&file_path, b"occupied").expect("file is created");

    let error = FsStateDirectory::new(&file_path)
        .ensure()
        .expect_err("file path must be rejected");

    assert_eq!(error.code(), ErrorCode::StateDirUnavailable);
}

#[test]
fn state_dir_error_keeps_cause_for_logs_but_not_for_ipc() {
    let root = tempfile::tempdir().expect("temporary directory");
    let file_path = root.path().join("not-a-directory");
    fs::write(&file_path, b"occupied").expect("file is created");

    let error = FsStateDirectory::new(&file_path)
        .ensure()
        .expect_err("file path must be rejected");

    let leaked_path = file_path.to_string_lossy();
    assert!(
        !error.public_message().contains(leaked_path.as_ref()),
        "public message must not contain the absolute path"
    );
    assert_ne!(
        error.to_string(),
        error.public_message(),
        "internal display must differ from the message shown to the user"
    );
    assert!(
        std::error::Error::source(&error).is_some(),
        "internal error must keep the underlying cause for logs"
    );
}

#[test]
fn validation_error_for_empty_path_is_not_state_dir_failure() {
    let error = FsStateDirectory::new("").ensure().expect_err("empty path");

    assert!(matches!(error, EgressError::Validation { .. }));
    assert_eq!(
        error.public_message(),
        "Запрос содержит недопустимые данные."
    );
}
