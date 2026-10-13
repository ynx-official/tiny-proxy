use super::*;

#[test]
fn asset_selection_matches_platform_version_and_installation_exactly() {
    let asset = |name: &str| release_source::ReleaseAssetMetadata {
        name: name.into(),
        download_url: String::new(),
        size: 1,
        digest: String::new(),
    };
    let assets = vec![
        asset("tiny-proxy-v0.2.0-windows-x86_64-setup.exe"),
        asset("tiny-proxy-v0.2.0-windows-x86_64-portable.zip"),
        asset("tiny-proxy-v0.2.0-macos-aarch64-setup.pkg"),
        asset("tiny-proxy-v0.2.0-macos-aarch64-portable.zip"),
    ];
    assert!(
        select_release_asset(
            &assets,
            "v0.2.0",
            "windows-x86_64",
            InstallationKind::Portable
        )
        .unwrap()
        .name
        .ends_with("portable.zip")
    );
    assert!(
        select_release_asset(
            &assets,
            "v0.2.0",
            "windows-x86_64",
            InstallationKind::WindowsInstaller
        )
        .unwrap()
        .name
        .ends_with("setup.exe")
    );
    assert!(
        select_release_asset(
            &assets,
            "v0.2.0",
            "macos-aarch64",
            InstallationKind::MacInstaller
        )
        .unwrap()
        .name
        .ends_with("setup.pkg")
    );
    assert!(
        select_release_asset(&assets, "v0.2.0", "macos-aarch64", InstallationKind::MacApp)
            .unwrap()
            .name
            .ends_with("portable.zip")
    );
    assert!(
        select_release_asset(
            &assets,
            "v0.1.0",
            "windows-x86_64",
            InstallationKind::Portable
        )
        .is_none()
    );
    assert!(
        select_release_asset(&assets, "v0.2.0", "macos-x86_64", InstallationKind::MacApp).is_none()
    );
}

#[test]
fn digest_mismatch_is_rejected_and_valid_digest_is_accepted() {
    let digest = format!("sha256:{:x}", Sha256::digest(b"payload"));
    verify_download_digest(b"payload", &digest).unwrap();
    assert!(verify_download_digest(b"tampered", &digest).is_err());
    assert!(verify_download_digest(b"payload", "sha256:bad").is_err());
}

#[test]
fn each_download_gets_a_fresh_staging_directory_without_deleting_others() {
    let root = tempfile::tempdir().unwrap();
    let first = prepare_update_dir(root.path(), "v0.2.0").unwrap();
    std::fs::write(first.join("sentinel"), b"keep").unwrap();
    let second = prepare_update_dir(root.path(), "v0.2.0").unwrap();
    assert_ne!(first, second);
    assert!(first.join("sentinel").exists());
    assert!(prepare_update_dir(root.path(), "../../unsafe").is_err());
}

#[test]
fn zip_traversal_is_rejected_before_writing_outside_staging() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("staging");
    std::fs::create_dir(&directory).unwrap();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("../sentinel", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"unsafe").unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert!(extract_zip(bytes, &directory, InstallationKind::Portable).is_err());
    assert!(!root.path().join("sentinel").exists());
}

#[tokio::test]
async fn pre_cancelled_download_does_not_create_update_artifacts() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let info = UpdateInfo {
        version: "0.2.0".into(),
        notes: String::new(),
        download_url: "https://example.invalid/payload.zip".into(),
        size: 10,
        digest: String::new(),
        installation_kind: InstallationKind::Portable,
    };
    let error = download_update(&info, &cancel, |_, _| {})
        .await
        .unwrap_err();
    assert!(error.to_string().contains("取消"));
}
