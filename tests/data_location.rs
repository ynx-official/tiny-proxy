use std::{ffi::OsStr, path::PathBuf};
use tiny_proxy::platform::data_dir::resolve;

#[test]
fn default_location_is_outside_virtualized_app_data() {
    let profile = OsStr::new("C:/Users/test-user");
    assert_eq!(
        resolve(None, Some(profile)).unwrap(),
        PathBuf::from(profile).join(".tiny-proxy")
    );
}

#[test]
fn explicit_location_still_wins() {
    let path = OsStr::new("D:/isolated-proxy");
    assert_eq!(resolve(Some(path), None).unwrap(), PathBuf::from(path));
}

#[test]
fn macos_home_uses_the_same_private_directory_name() {
    assert_eq!(
        resolve(None, Some(OsStr::new("/Users/test-user"))).unwrap(),
        PathBuf::from("/Users/test-user/.tiny-proxy")
    );
    assert!(resolve(None, None).is_err());
}

#[test]
fn finds_both_normal_and_packaged_legacy_locations() {
    let root = tempfile::tempdir().unwrap();
    let packaged = root
        .path()
        .join("Packages/example/LocalCache/Local/TinyProxy");
    std::fs::create_dir_all(&packaged).unwrap();
    std::fs::write(packaged.join("app.db"), []).unwrap();
    let found = tiny_proxy::platform::data_dir::discover_legacy(root.path()).unwrap();
    assert!(found.contains(&root.path().join("TinyProxy")));
    assert!(found.contains(&packaged));
}
