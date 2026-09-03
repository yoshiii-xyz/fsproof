use std::{fs, path::Path, process::Command, thread, time::Duration};

use tempfile::tempdir;

fn write_and_verify(path: &Path, contents: &[u8]) {
    fs::write(path, contents).expect("write fixture");
    assert_eq!(fs::read(path).expect("read fixture"), contents);
}

#[test]
fn inspect_json_reports_snapshot_and_filesystem() {
    let directory = tempdir().expect("tempdir");
    write_and_verify(&directory.path().join("file.txt"), b"fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fsproof"))
        .args([
            "inspect",
            directory.path().to_str().expect("path"),
            "--format",
            "json",
        ])
        .output()
        .expect("inspect command");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["tool"], "fsproof");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["snapshot"]["entries"], 2);
    assert!(value["filesystem"]["kind"].as_str().is_some());
}

#[test]
fn verify_poll_emits_a_valid_report() {
    let directory = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_fsproof"))
        .args([
            "verify",
            directory.path().to_str().expect("path"),
            "--duration",
            "40ms",
            "--backend",
            "poll",
            "--poll-interval",
            "5ms",
            "--format",
            "json",
        ])
        .output()
        .expect("verify command");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["tool"], "fsproof");
    assert_eq!(value["status"], "ok");
    assert_eq!(value["backend"]["selected"], "poll");
    assert!(
        value["guarantee"]
            .as_str()
            .expect("guarantee")
            .contains("best-effort")
    );
}

#[test]
fn watch_jsonl_starts_with_ready_record() {
    let directory = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_fsproof"))
        .args([
            "watch",
            directory.path().to_str().expect("path"),
            "--duration",
            "30ms",
            "--backend",
            "poll",
            "--poll-interval",
            "5ms",
            "--format",
            "jsonl",
        ])
        .output()
        .expect("watch command");
    assert_eq!(output.status.code(), Some(0));
    let lines = String::from_utf8(output.stdout).expect("utf8");
    let first: serde_json::Value = lines
        .lines()
        .next()
        .expect("ready line")
        .parse()
        .expect("ready json");
    assert_eq!(first["record_type"], "ready");
    assert_eq!(first["backend"]["selected"], "poll");
}

#[test]
fn watch_jsonl_emits_a_reconciled_event() {
    let directory = tempdir().expect("tempdir");
    let path = directory.path().to_path_buf();
    let writer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        write_and_verify(&path.join("created.txt"), b"created");
    });
    let output = Command::new(env!("CARGO_BIN_EXE_fsproof"))
        .args([
            "watch",
            directory.path().to_str().expect("path"),
            "--duration",
            "180ms",
            "--backend",
            "poll",
            "--poll-interval",
            "5ms",
            "--format",
            "jsonl",
        ])
        .output()
        .expect("watch command");
    writer.join().expect("writer");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = String::from_utf8(output.stdout).expect("utf8");
    let values = lines
        .lines()
        .map(|line| line.parse::<serde_json::Value>().expect("json line"))
        .collect::<Vec<_>>();
    assert!(values.iter().any(|value| {
        value["record_type"] == "event"
            && value["kind"] == "Created"
            && value["path"] == "created.txt"
    }));
}

#[test]
fn invalid_duration_and_snapshot_limit_are_errors() {
    let directory = tempdir().expect("tempdir");
    write_and_verify(&directory.path().join("file.txt"), b"fixture");
    let invalid = Command::new(env!("CARGO_BIN_EXE_fsproof"))
        .args([
            "verify",
            directory.path().to_str().expect("path"),
            "--duration",
            "10h",
        ])
        .output()
        .expect("invalid duration command");
    assert_eq!(invalid.status.code(), Some(3));
    assert!(!invalid.stderr.is_empty());

    let limited = Command::new(env!("CARGO_BIN_EXE_fsproof"))
        .args([
            "inspect",
            directory.path().to_str().expect("path"),
            "--max-entries",
            "1",
        ])
        .output()
        .expect("limited inspect command");
    assert_eq!(limited.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&limited.stderr).contains("max entries"));
}
