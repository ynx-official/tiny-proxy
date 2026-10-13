#![cfg(feature = "desktop")]

#[test]
fn version_command_exits_before_opening_a_window_or_database() {
    let temp = tempfile::tempdir().unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_tiny-proxy"))
        .arg("--version")
        .env("TINY_PROXY_DATA_DIR", temp.path())
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap().trim(),
        format!("Tiny Proxy {}", env!("CARGO_PKG_VERSION"))
    );
    assert!(result.stderr.is_empty());
    assert!(!temp.path().join("app.db").exists());
}
