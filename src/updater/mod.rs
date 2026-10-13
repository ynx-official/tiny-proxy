//! Adapted from ashell's updater. Network, verified staging and installation
//! stay outside GPUI; replacing the application always requires a user action.
mod install;
mod release_source;
pub(crate) mod runtime;
pub use runtime::Snapshot;

use anyhow::Context as _;
use futures_util::StreamExt as _;
use semver::Version;
use sha2::{Digest as _, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RELEASES_URL: &str = "https://github.com/ynx-official/tiny-proxy/releases/latest";
const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CheckMode {
    #[default]
    Startup,
    Interval,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    pub mode: CheckMode,
    pub interval_hours: u32,
    pub notify: bool,
    pub last_checked_at: i64,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            mode: CheckMode::Startup,
            interval_hours: 24,
            notify: true,
            last_checked_at: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallationKind {
    WindowsInstaller,
    Portable,
    MacApp,
    MacInstaller,
}

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub version: String,
    pub notes: String,
}
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub download_url: String,
    pub size: u64,
    pub digest: String,
    pub installation_kind: InstallationKind,
}
#[derive(Debug, Clone)]
pub enum UpdateCheckResult {
    UpToDate(ReleaseInfo),
    UpdateAvailable(UpdateInfo),
}
#[derive(Debug, Clone, Default)]
pub enum UpdateStatus {
    #[default]
    NotChecked,
    Checking,
    UpToDate(ReleaseInfo),
    UpdateAvailable(UpdateInfo),
    Downloading(UpdateInfo, u64, u64),
    DownloadCancelled(UpdateInfo),
    DownloadFailed(UpdateInfo, String),
    ReadyToRestart(UpdateInfo, PathBuf),
    Installing,
    Error(String),
}
impl UpdateStatus {
    pub fn busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading(..) | Self::Installing
        )
    }
    pub fn available(&self) -> Option<&UpdateInfo> {
        match self {
            Self::UpdateAvailable(info)
            | Self::DownloadCancelled(info)
            | Self::DownloadFailed(info, _) => Some(info),
            _ => None,
        }
    }
    pub fn notes(&self) -> &str {
        match self {
            Self::UpToDate(info) => &info.notes,
            Self::UpdateAvailable(info)
            | Self::DownloadCancelled(info)
            | Self::DownloadFailed(info, _)
            | Self::Downloading(info, ..)
            | Self::ReadyToRestart(info, _) => &info.notes,
            _ => "",
        }
    }
}

pub fn automatic_update_delay(interval_hours: u32, last_checked_at: i64, now: i64) -> Duration {
    if last_checked_at <= 0 {
        return Duration::ZERO;
    }
    let interval = i64::from(interval_hours.clamp(1, 8760)) * 3600;
    Duration::from_secs(
        interval
            .saturating_sub(now.saturating_sub(last_checked_at).max(0))
            .max(0) as u64,
    )
}
fn parse_version(version: &str) -> anyhow::Result<Version> {
    Version::parse(version.trim().trim_start_matches(['v', 'V'])).context("无效更新版本")
}
pub fn is_newer_version(current: &str, latest: &str) -> anyhow::Result<bool> {
    Ok(parse_version(latest)? > parse_version(current)?)
}
fn platform_name() -> anyhow::Result<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok("windows-x86_64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok("macos-aarch64")
    } else {
        anyhow::bail!("在线更新仅支持 Windows x64 和 macOS arm64");
    }
}
pub fn runtime_environment_label() -> String {
    format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH)
}
pub fn installation_kind() -> InstallationKind {
    install::installation_kind()
}
fn client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(format!("tiny-proxy/{CURRENT_VERSION}"))
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .context("无法初始化更新连接")
}
fn select_release_asset<'a>(
    assets: &'a [release_source::ReleaseAssetMetadata],
    tag: &str,
    platform: &str,
    kind: InstallationKind,
) -> Option<&'a release_source::ReleaseAssetMetadata> {
    let suffix = match kind {
        InstallationKind::WindowsInstaller => "setup.exe",
        InstallationKind::MacInstaller => "setup.pkg",
        _ => "portable.zip",
    };
    let name = format!("tiny-proxy-{tag}-{platform}-{suffix}");
    assets.iter().find(|asset| asset.name == name)
}
pub async fn check_for_update() -> anyhow::Result<UpdateCheckResult> {
    let platform = platform_name()?;
    let release = release_source::fetch_latest_release(&client()?).await?;
    if !is_newer_version(CURRENT_VERSION, &release.version)? {
        return Ok(UpdateCheckResult::UpToDate(ReleaseInfo {
            version: parse_version(&release.version)?.to_string(),
            notes: release.notes,
        }));
    }
    let kind = installation_kind();
    let asset = select_release_asset(&release.assets, &release.version, platform, kind)
        .context("该版本缺少匹配平台和安装方式的更新包")?;
    Ok(UpdateCheckResult::UpdateAvailable(UpdateInfo {
        version: parse_version(&release.version)?.to_string(),
        notes: release.notes,
        download_url: asset.download_url.clone(),
        size: asset.size,
        digest: asset.digest.clone(),
        installation_kind: kind,
    }))
}
fn verify_download_digest(bytes: &[u8], digest: &str) -> anyhow::Result<()> {
    let expected = digest
        .strip_prefix("sha256:")
        .context("缺少 SHA-256 校验值")?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    anyhow::ensure!(
        expected.len() == 64 && actual.eq_ignore_ascii_case(expected),
        "更新包 SHA-256 校验失败"
    );
    Ok(())
}

/// An exclusive, unguessable directory avoids deleting another instance's
/// files or following a pre-created shared-temp-directory symlink.
fn prepare_update_dir(root: &Path, version: &str) -> anyhow::Result<PathBuf> {
    let path = root.join(format!(
        "tiny-proxy-update-{}-{}",
        parse_version(version)?,
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&path).context("无法创建更新暂存目录")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(path)
}

fn extract_zip(
    bytes: Vec<u8>,
    directory: &Path,
    kind: InstallationKind,
) -> anyhow::Result<PathBuf> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("无效 ZIP 更新包")?;
    anyhow::ensure!(archive.len() <= 10000, "更新包文件过多");
    let mut expanded = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let relative = entry.enclosed_name().context("更新包包含不安全路径")?;
        anyhow::ensure!(!relative.as_os_str().is_empty(), "更新包包含空路径");
        // Tiny Proxy has no bundled third-party frameworks or symlinks.
        anyhow::ensure!(
            entry
                .unix_mode()
                .is_none_or(|mode| mode & 0o170000 != 0o120000),
            "更新包包含符号链接"
        );
        let path = directory.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        std::fs::create_dir_all(path.parent().context("无效更新路径")?)?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let remaining = MAX_EXTRACTED.saturating_sub(expanded);
        let copied = std::io::copy(&mut (&mut entry).take(remaining + 1), &mut output)?;
        expanded += copied;
        anyhow::ensure!(expanded <= MAX_EXTRACTED, "更新包解压大小超过限制");
        output.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if let Some(mode) = entry.unix_mode() {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o777))?;
            }
        }
    }
    find_payload(directory, kind)
}

fn find_payload(directory: &Path, kind: InstallationKind) -> anyhow::Result<PathBuf> {
    if matches!(kind, InstallationKind::MacApp) || cfg!(target_os = "macos") {
        let app = directory.join("TinyProxy.app");
        anyhow::ensure!(
            app.join("Contents/MacOS/tiny-proxy").is_file()
                && app.join("Contents/Info.plist").is_file(),
            "更新包缺少 TinyProxy.app"
        );
        Ok(app)
    } else {
        let entries = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        anyhow::ensure!(
            entries.len() == 1 && entries[0].file_type()?.is_dir(),
            "便携更新包顶层结构错误"
        );
        let binary = entries[0].path().join("tiny-proxy.exe");
        anyhow::ensure!(binary.is_file(), "更新包缺少 tiny-proxy.exe");
        Ok(binary)
    }
}

pub async fn download_update<F>(
    info: &UpdateInfo,
    cancel: &CancellationToken,
    mut progress: F,
) -> anyhow::Result<PathBuf>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    anyhow::ensure!(info.size <= MAX_DOWNLOAD, "更新包大小超过限制");
    let response = tokio::select! { biased; _ = cancel.cancelled() => anyhow::bail!("下载已取消"), result = client()?.get(&info.download_url).send() => result?.error_for_status()? };
    let total = response.content_length().unwrap_or(info.size);
    anyhow::ensure!(total <= MAX_DOWNLOAD, "更新包大小超过限制");
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    loop {
        let chunk = tokio::select! { biased; _ = cancel.cancelled() => anyhow::bail!("下载已取消"), chunk = stream.next() => chunk };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk?;
        anyhow::ensure!(
            bytes.len() as u64 + chunk.len() as u64 <= MAX_DOWNLOAD,
            "更新下载超过大小限制"
        );
        bytes.extend_from_slice(&chunk);
        progress(bytes.len() as u64, total);
    }
    anyhow::ensure!(!cancel.is_cancelled(), "下载已取消");
    anyhow::ensure!(
        info.size == 0 || bytes.len() as u64 == info.size,
        "更新包大小校验失败"
    );
    verify_download_digest(&bytes, &info.digest)?;
    let kind = info.installation_kind;
    let version = info.version.clone();
    let cancel = cancel.clone();
    tokio::task::spawn_blocking(move || {
        let directory = prepare_update_dir(&std::env::temp_dir(), &version)?;
        let result = (|| {
            anyhow::ensure!(!cancel.is_cancelled(), "下载已取消");
            let path = match kind {
                InstallationKind::WindowsInstaller | InstallationKind::MacInstaller => {
                    let path = directory.join(if kind == InstallationKind::WindowsInstaller {
                        "setup.exe"
                    } else {
                        "setup.pkg"
                    });
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)?;
                    file.write_all(&bytes)?;
                    file.sync_all()?;
                    path
                }
                _ => extract_zip(bytes, &directory, kind)?,
            };
            anyhow::ensure!(!cancel.is_cancelled(), "下载已取消");
            Ok(path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&directory);
        }
        result
    })
    .await
    .context("更新解压任务失败")?
}

/// Prepare the OS helper and return; the caller closes the gateway, database
/// and desktop before this process exits and the helper launches the update.
pub(crate) fn install_and_restart(path: &Path, info: &UpdateInfo) -> anyhow::Result<()> {
    install::install_and_restart(path, &info.version, info.installation_kind)
}

#[cfg(test)]
mod tests;
