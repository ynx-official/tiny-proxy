use super::{InstallationKind, parse_version};
use anyhow::Context as _;
#[cfg(windows)]
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};

#[cfg(windows)]
const INNO_UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{A1D8C90D-F9C8-44E6-9D82-671A8D41E7A4}_is1";
#[cfg(target_os = "macos")]
const MACOS_PKG_IDENTIFIER: &str = "dev.tiny-proxy.app";

pub(super) fn installation_kind() -> InstallationKind {
    #[cfg(windows)]
    {
        use winreg::{
            RegKey,
            enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
        };
        let directory = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf));
        if let Some(directory) = directory {
            let normalize = |path: &Path| {
                path.to_string_lossy()
                    .trim_end_matches(['/', '\\'])
                    .to_lowercase()
            };
            let installed = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
                .into_iter()
                .filter_map(|hive| RegKey::predef(hive).open_subkey(INNO_UNINSTALL_KEY).ok())
                .filter_map(|key| key.get_value::<String, _>("InstallLocation").ok())
                .any(|path| normalize(Path::new(&path)) == normalize(&directory));
            if installed {
                return InstallationKind::WindowsInstaller;
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let app = std::env::current_exe().ok().and_then(|path| {
            path.ancestors()
                .find(|path| path.extension().is_some_and(|ext| ext == "app"))
                .map(Path::to_path_buf)
        });
        if let Some(app) = app {
            // A receipt alone can be left behind by another copy of the app.
            let installed = app == Path::new("/Applications/TinyProxy.app")
                && std::process::Command::new("/usr/sbin/pkgutil")
                    .args(["--pkg-info-plist", MACOS_PKG_IDENTIFIER])
                    .output()
                    .is_ok_and(|output| output.status.success());
            return if installed {
                InstallationKind::MacInstaller
            } else {
                InstallationKind::MacApp
            };
        }
    }
    InstallationKind::Portable
}

#[cfg(windows)]
fn unique_temp_suffix() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub(super) fn install_and_restart(
    new_path: &Path,
    expected_version: &str,
    kind: InstallationKind,
) -> anyhow::Result<()> {
    let current = std::env::current_exe().context("无法获取当前应用路径")?;
    #[cfg(windows)]
    {
        match kind {
            InstallationKind::WindowsInstaller => {
                install_windows_setup(new_path, &current, expected_version)
            }
            InstallationKind::Portable => {
                let staged = stage_windows_portable_binary(new_path)?;
                install_windows_portable(&staged, &current, expected_version)
            }
            _ => anyhow::bail!("更新包与当前平台不匹配"),
        }
    }
    #[cfg(target_os = "macos")]
    {
        match kind {
            InstallationKind::MacInstaller => {
                install_macos_pkg(new_path, &current, expected_version)?
            }
            InstallationKind::MacApp | InstallationKind::Portable => {
                install_macos(new_path, &current, expected_version)?
            }
            _ => anyhow::bail!("更新包与当前平台不匹配"),
        }
        let target = current
            .ancestors()
            .find(|path| path.extension().is_some_and(|ext| ext == "app"))
            .unwrap_or(&current);
        // Wait for the old process (and its single-instance lock) to exit.
        let script = if target == current.as_path() {
            "while /bin/kill -0 \"$1\" 2>/dev/null; do /bin/sleep 0.2; done; exec \"$2\""
        } else {
            "while /bin/kill -0 \"$1\" 2>/dev/null; do /bin/sleep 0.2; done; exec /usr/bin/open -n \"$2\""
        };
        std::process::Command::new("/bin/sh")
            .args([
                "-c",
                script,
                "tiny-proxy-update",
                &std::process::id().to_string(),
            ])
            .arg(target)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("无法启动更新重启助手")?;
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (new_path, expected_version, kind, current);
        anyhow::bail!("该平台不支持更新安装")
    }
}

#[cfg(target_os = "macos")]
fn unique_update_sibling(target: &Path, suffix: &str) -> anyhow::Result<PathBuf> {
    let parent = target
        .parent()
        .context("installed executable has no parent directory")?;
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tiny-proxy");
    Ok(parent.join(format!(".{file_name}.{}.{}", uuid::Uuid::new_v4(), suffix)))
}

#[cfg(target_os = "macos")]
fn copy_exclusive_with_mode(source: &Path, destination: &Path, mode: u32) -> anyhow::Result<u64> {
    use std::os::unix::fs::PermissionsExt as _;

    let mut source_file = std::fs::File::open(source)
        .with_context(|| format!("failed to open update source {}", source.display()))?;
    let mut destination_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .with_context(|| {
            format!(
                "failed to create exclusive update file {}",
                destination.display()
            )
        })?;
    let copied_size = std::io::copy(&mut source_file, &mut destination_file)
        .with_context(|| format!("failed to copy update to {}", destination.display()))?;
    if copied_size == 0 {
        anyhow::bail!("downloaded update binary is empty");
    }
    destination_file
        .set_permissions(std::fs::Permissions::from_mode(mode))
        .context("failed to set executable permissions")?;
    destination_file
        .sync_all()
        .context("failed to flush staged update to disk")?;
    Ok(copied_size)
}

#[cfg(target_os = "macos")]
fn install_standalone(
    new_binary: &std::path::Path,
    current_exe: &std::path::Path,
) -> anyhow::Result<()> {
    // A unique create-new sibling prevents predictable-path symlink attacks and
    // lets multiple application instances stage independently. rename() over
    // an in-use Unix executable is safe because the old inode remains alive.
    let new_temp = unique_update_sibling(current_exe, "new")?;
    let install_result = (|| {
        copy_exclusive_with_mode(new_binary, &new_temp, 0o755)?;
        std::fs::rename(&new_temp, current_exe)
            .context("failed to rename new binary into place")?;
        Ok::<_, anyhow::Error>(())
    })();
    if install_result.is_err() {
        std::fs::remove_file(&new_temp).ok();
    }
    install_result?;

    tracing::info!("update installed to {}", current_exe.display());
    Ok(())
}

/// Recursively copy a directory tree. Unlike `std::fs::rename`, this works
/// across volumes and leaves the source intact, so a failure mid-copy does
/// not corrupt the running app bundle. Used when staging a new .app before
/// the atomic swap into place.
#[cfg(target_os = "macos")]
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_all(&path, &target)?;
        } else if ft.is_symlink() {
            let link_target = std::fs::read_link(&path)?;
            std::os::unix::fs::symlink(&link_target, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_macos(
    new_path: &Path,
    current_exe: &Path,
    expected_version: &str,
) -> anyhow::Result<()> {
    verify_macos_version(new_path, expected_version)?;
    let Some(app) = current_exe
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
    else {
        return install_standalone(&new_path.join("Contents/MacOS/tiny-proxy"), current_exe);
    };
    let staging = unique_update_sibling(app, "new")?;
    let backup = unique_update_sibling(app, "backup")?;
    std::fs::create_dir(&staging).context("failed to create exclusive staging directory")?;
    // Copy and verify before moving the live bundle; preserve its release signature.
    let prepared = (|| {
        copy_dir_all(new_path, &staging)?;
        let verified = std::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&staging)
            .output()?;
        anyhow::ensure!(
            verified.status.success(),
            "new app signature verification failed"
        );
        Ok::<_, anyhow::Error>(())
    })();
    if let Err(error) = prepared {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    std::fs::rename(app, &backup).context("failed to backup live app")?;
    if let Err(error) = std::fs::rename(&staging, app) {
        std::fs::rename(&backup, app)
            .context("install failed and old app could not be restored; backup retained")?;
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error).context("failed to install new app; old application restored");
    }
    let _ = std::fs::remove_dir_all(backup);
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_macos_pkg(
    pkg_path: &std::path::Path,
    _current_exe: &std::path::Path,
    expected_version: &str,
) -> anyhow::Result<()> {
    // The .pkg's install-location targets /Applications, so install into the
    // root domain and let the package lay the bundle down itself. installer
    // requires root; elevate via osascript, which shows the standard macOS
    // admin password prompt. The running process keeps its old inode alive,
    // and restart() relaunches the freshly installed bundle.
    let pkg = pkg_path.to_string_lossy().replace('\'', "'\\''");
    let shell_command = format!("/usr/sbin/installer -pkg '{pkg}' -target /");
    let apple_script = format!(
        "do shell script \"{}\" with administrator privileges",
        shell_command.replace('\\', "\\\\").replace('"', "\\\"")
    );

    let output = std::process::Command::new("osascript")
        .args(["-e", &apple_script])
        .output()
        .context("failed to invoke installer via osascript")?;
    if !output.status.success() {
        anyhow::bail!(
            "pkg installer failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    // Verify the installed bundle matches the expected version before
    // restarting. A silent installer failure (e.g. the old bundle was in use
    // and the new one could not replace it) would otherwise leave the app
    // running the previous version with no error surfaced. Compare as plain
    // strings: CFBundleShortVersionString is written verbatim from the plist
    // template, so it must equal the release version (with any leading 'v'
    // stripped) character-for-character.
    let installed_app = std::path::Path::new("/Applications/TinyProxy.app");
    verify_macos_version(installed_app, expected_version)
}

#[cfg(target_os = "macos")]
fn verify_macos_version(installed_app: &Path, expected_version: &str) -> anyhow::Result<()> {
    let installed_version = std::process::Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :TinyProxyReleaseVersion"])
        .arg(installed_app.join("Contents/Info.plist"))
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        });
    let expected = parse_version(expected_version)?.to_string();
    match installed_version.as_deref() {
        Some(v) if v == expected => Ok(()),
        Some(v) => anyhow::bail!("installed pkg version mismatch: expected {expected}, got {v}"),
        None => anyhow::bail!(
            "failed to read installed version from {} after pkg install",
            installed_app.display()
        ),
    }
}

#[cfg(target_os = "windows")]
fn stage_windows_portable_binary(new_binary: &Path) -> anyhow::Result<PathBuf> {
    let current_exe = std::env::current_exe().context("failed to get current executable path")?;
    stage_windows_portable_binary_at(new_binary, &current_exe, std::process::id())
}

#[cfg(target_os = "windows")]
fn stage_windows_portable_binary_at(
    new_binary: &Path,
    current_exe: &Path,
    process_id: u32,
) -> anyhow::Result<PathBuf> {
    let source_size = std::fs::metadata(new_binary)
        .with_context(|| format!("failed to inspect update binary {}", new_binary.display()))?
        .len();
    if source_size == 0 {
        anyhow::bail!("downloaded update binary is empty");
    }

    let install_dir = current_exe
        .parent()
        .context("current executable has no parent directory")?;
    let staged_binary = install_dir.join(format!(
        ".tiny-proxy-update-{process_id}-{}.exe",
        uuid::Uuid::new_v4()
    ));
    let mut source = std::fs::File::open(new_binary)?;
    let mut destination = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged_binary)
        .context("cannot stage update next to current executable")?;
    let copied_size = std::io::copy(&mut source, &mut destination)?;
    destination.sync_all()?;
    drop(destination);
    if copied_size != source_size {
        std::fs::remove_file(&staged_binary).ok();
        anyhow::bail!(
            "staged update size mismatch: expected {source_size} bytes, copied {copied_size} bytes"
        );
    }

    Ok(staged_binary)
}

#[cfg(target_os = "windows")]
fn install_windows_setup(
    installer: &std::path::Path,
    current_exe: &std::path::Path,
    expected_version: &str,
) -> anyhow::Result<()> {
    let expected_version = parse_version(expected_version)?.to_string();
    let installer_size = std::fs::metadata(installer)
        .with_context(|| format!("failed to inspect update installer {}", installer.display()))?
        .len();
    if installer_size == 0 {
        anyhow::bail!("downloaded update installer is empty");
    }

    let process_id = std::process::id();
    let suffix = unique_temp_suffix();
    let script_path = std::env::temp_dir().join(format!("tiny-proxy-setup-update-{suffix}.ps1"));
    let log_path = std::env::temp_dir().join(format!("tiny-proxy-update-{suffix}.log"));
    let plan = WindowsSetupUpdateScriptPlan {
        process_id,
        current_exe,
        installer,
        script_path: &script_path,
        log_path: &log_path,
        expected_version: &expected_version,
    };

    if let Err(error) =
        launch_windows_update_script(&script_path, build_windows_setup_update_script(&plan))
    {
        std::fs::remove_file(&script_path).ok();
        return Err(error);
    }

    tracing::info!(
        "setup update helper launched for process {process_id}; installer: {}",
        installer.display()
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_windows_portable(
    new_binary: &std::path::Path,
    current_exe: &std::path::Path,
    expected_version: &str,
) -> anyhow::Result<()> {
    let expected_version = parse_version(expected_version)?.to_string();
    let source_size = std::fs::metadata(new_binary)
        .with_context(|| format!("failed to inspect staged update {}", new_binary.display()))?
        .len();
    if source_size == 0 {
        anyhow::bail!("staged update binary is empty");
    }

    let process_id = std::process::id();
    let install_dir = current_exe
        .parent()
        .context("current executable has no parent directory")?;
    let staged_binary = new_binary.to_path_buf();
    if staged_binary.parent() != Some(install_dir) {
        anyhow::bail!(
            "staged update is not next to the installed executable: {}",
            staged_binary.display()
        );
    }
    let backup_binary = install_dir.join(format!(
        ".tiny-proxy-backup-{process_id}-{}.exe",
        uuid::Uuid::new_v4()
    ));
    let suffix = unique_temp_suffix();
    let script_path = std::env::temp_dir().join(format!("tiny-proxy-update-{suffix}.ps1"));
    let log_path = std::env::temp_dir().join(format!("tiny-proxy-update-{suffix}.log"));

    let script_plan = WindowsUpdateScriptPlan {
        process_id,
        current_exe,
        staged_binary: &staged_binary,
        backup_binary: &backup_binary,
        script_path: &script_path,
        log_path: &log_path,
        expected_size: source_size,
        expected_version: &expected_version,
    };
    if let Err(error) =
        launch_windows_update_script(&script_path, build_windows_update_script(&script_plan))
    {
        std::fs::remove_file(&staged_binary).ok();
        std::fs::remove_file(&script_path).ok();
        return Err(error);
    }

    tracing::info!(
        "update helper launched for process {process_id}; staged binary: {}",
        staged_binary.display()
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn launch_windows_update_script(script_path: &Path, script: String) -> anyhow::Result<()> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(script_path)
        .context("failed to create update PowerShell script")?;
    file.write_all(script.as_bytes())?;
    file.sync_all()?;
    std::process::Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .spawn()
        .context("failed to launch update PowerShell script")?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn powershell_path_literal(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

#[cfg(target_os = "windows")]
struct WindowsSetupUpdateScriptPlan<'a> {
    process_id: u32,
    current_exe: &'a Path,
    installer: &'a Path,
    script_path: &'a Path,
    log_path: &'a Path,
    expected_version: &'a str,
}

#[cfg(target_os = "windows")]
fn build_windows_setup_update_script(plan: &WindowsSetupUpdateScriptPlan<'_>) -> String {
    let current_exe = powershell_path_literal(plan.current_exe);
    let installer = powershell_path_literal(plan.installer);
    let script_path = powershell_path_literal(plan.script_path);
    let log_path = powershell_path_literal(plan.log_path);
    let expected_version = powershell_string_literal(plan.expected_version);
    let process_id = plan.process_id;

    format!(
        "$ErrorActionPreference = 'Stop'\r\n\
         $current = {current_exe}\r\n\
         $installer = {installer}\r\n\
         $script = {script_path}\r\n\
         $log = {log_path}\r\n\
         $expectedVersion = {expected_version}\r\n\
         function Write-UpdateLog([string]$message) {{\r\n\
             try {{\r\n\
                 Add-Content -LiteralPath $log -Value \"$(Get-Date -Format o) $message\" -Encoding UTF8\r\n\
             }} catch {{}}\r\n\
         }}\r\n\
         try {{\r\n\
             Write-UpdateLog 'waiting for application process {process_id} before setup update'\r\n\
             Wait-Process -Id {process_id} -ErrorAction SilentlyContinue\r\n\
             $arguments = @('/SP-', '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER', ('/DIR=\"' + (Split-Path -Parent $current) + '\"'))\r\n\
             $setup = Start-Process -FilePath $installer -ArgumentList $arguments -Wait -PassThru -WindowStyle Hidden\r\n\
             if ($setup.ExitCode -ne 0) {{\r\n\
                 throw \"setup exited with code $($setup.ExitCode)\"\r\n\
             }}\r\n\
             $installedVersion = (Get-Item -LiteralPath $current).VersionInfo.ProductVersion\r\n\
             if ($installedVersion -ne $expectedVersion) {{\r\n\
                 throw \"installed setup version mismatch: expected $expectedVersion, got $installedVersion\"\r\n\
             }}\r\n\
             Start-Process -FilePath $current -WorkingDirectory (Split-Path -Parent $current)\r\n\
             Write-UpdateLog 'setup update installed and application restarted successfully'\r\n\
             Remove-Item -LiteralPath $installer -Force -ErrorAction SilentlyContinue\r\n\
             Remove-Item -LiteralPath $script -Force -ErrorAction SilentlyContinue\r\n\
         }} catch {{\r\n\
             Write-UpdateLog \"setup update failed: $($_.Exception.Message)\"\r\n\
             if (Test-Path -LiteralPath $current) {{\r\n\
                 Start-Process -FilePath $current -WorkingDirectory (Split-Path -Parent $current)\r\n\
             }}\r\n\
             exit 1\r\n\
         }}\r\n"
    )
}

#[cfg(target_os = "windows")]
struct WindowsUpdateScriptPlan<'a> {
    process_id: u32,
    current_exe: &'a std::path::Path,
    staged_binary: &'a std::path::Path,
    backup_binary: &'a std::path::Path,
    script_path: &'a std::path::Path,
    log_path: &'a std::path::Path,
    expected_size: u64,
    expected_version: &'a str,
}

#[cfg(target_os = "windows")]
fn build_windows_update_script(plan: &WindowsUpdateScriptPlan<'_>) -> String {
    let current_exe = powershell_path_literal(plan.current_exe);
    let staged_binary = powershell_path_literal(plan.staged_binary);
    let backup_binary = powershell_path_literal(plan.backup_binary);
    let script_path = powershell_path_literal(plan.script_path);
    let log_path = powershell_path_literal(plan.log_path);
    let expected_version = powershell_string_literal(plan.expected_version);
    let process_id = plan.process_id;
    let expected_size = plan.expected_size;

    format!(
        "$ErrorActionPreference = 'Stop'\r\n\
         $current = {current_exe}\r\n\
         $staged = {staged_binary}\r\n\
         $backup = {backup_binary}\r\n\
         $script = {script_path}\r\n\
         $log = {log_path}\r\n\
         $expectedVersion = {expected_version}\r\n\
         function Write-UpdateLog([string]$message) {{\r\n\
             try {{\r\n\
                 Add-Content -LiteralPath $log -Value \"$(Get-Date -Format o) $message\" -Encoding UTF8\r\n\
             }} catch {{}}\r\n\
         }}\r\n\
         try {{\r\n\
             Write-UpdateLog 'waiting for application process {process_id} to exit'\r\n\
             Wait-Process -Id {process_id} -ErrorAction SilentlyContinue\r\n\
             Start-Sleep -Milliseconds 300\r\n\
             $stagedVersion = (Get-Item -LiteralPath $staged).VersionInfo.ProductVersion\r\n\
             if ($stagedVersion -ne $expectedVersion) {{\r\n\
                 throw \"staged update version mismatch: expected $expectedVersion, got $stagedVersion\"\r\n\
             }}\r\n\
             if (Test-Path -LiteralPath $backup) {{\r\n\
                 Remove-Item -LiteralPath $backup -Force\r\n\
             }}\r\n\
             $moved = $false\r\n\
             for ($attempt = 1; $attempt -le 20; $attempt++) {{\r\n\
                 try {{\r\n\
                     Move-Item -LiteralPath $current -Destination $backup -Force\r\n\
                     $moved = $true\r\n\
                     break\r\n\
                 }} catch {{\r\n\
                     if ($attempt -eq 20) {{ throw }}\r\n\
                     Start-Sleep -Milliseconds 250\r\n\
                 }}\r\n\
             }}\r\n\
             if (-not $moved) {{ throw 'failed to move installed executable to backup' }}\r\n\
             Move-Item -LiteralPath $staged -Destination $current -Force\r\n\
             $installedSize = (Get-Item -LiteralPath $current).Length\r\n\
             if ($installedSize -ne {expected_size}) {{\r\n\
                 throw \"installed update size mismatch: expected {expected_size}, got $installedSize\"\r\n\
             }}\r\n\
             Start-Process -FilePath $current -WorkingDirectory (Split-Path -Parent $current)\r\n\
             Remove-Item -LiteralPath $backup -Force\r\n\
             Write-UpdateLog 'update installed and application restarted successfully'\r\n\
             Remove-Item -LiteralPath $script -Force -ErrorAction SilentlyContinue\r\n\
         }} catch {{\r\n\
             Write-UpdateLog \"update failed: $($_.Exception.Message)\"\r\n\
             try {{\r\n\
                 if (Test-Path -LiteralPath $backup) {{\r\n\
                     if (Test-Path -LiteralPath $current) {{\r\n\
                         Remove-Item -LiteralPath $current -Force\r\n\
                     }}\r\n\
                     Move-Item -LiteralPath $backup -Destination $current -Force\r\n\
                 }}\r\n\
                 if (Test-Path -LiteralPath $current) {{\r\n\
                     Start-Process -FilePath $current -WorkingDirectory (Split-Path -Parent $current)\r\n\
                 }}\r\n\
             }} catch {{\r\n\
                 Write-UpdateLog \"rollback failed: $($_.Exception.Message)\"\r\n\
             }}\r\n\
             exit 1\r\n\
         }}\r\n"
    )
}

#[cfg(target_os = "windows")]
fn powershell_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(all(windows, test))]
mod tests {
    use super::*;

    fn assert_script_parses(script: &str) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("update.ps1");
        std::fs::write(&path, script).unwrap();
        let parse = format!(
            "$tokens=$null; $errors=$null; [System.Management.Automation.Language.Parser]::ParseFile({},[ref]$tokens,[ref]$errors) | Out-Null; if ($errors.Count -gt 0) {{$errors | ForEach-Object {{Write-Error $_.Message}}; exit 1}}",
            powershell_path_literal(&path)
        );
        assert!(
            std::process::Command::new("powershell.exe")
                .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
                .arg(parse)
                .creation_flags(0x08000000)
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn portable_helper_escapes_paths_waits_for_exit_and_keeps_rollback() {
        let plan = WindowsUpdateScriptPlan {
            process_id: 42,
            current_exe: Path::new(r"C:\Users\O'Brien\Tiny Proxy\tiny-proxy.exe"),
            staged_binary: Path::new(r"C:\Users\O'Brien\Tiny Proxy\.update.exe"),
            backup_binary: Path::new(r"C:\Users\O'Brien\Tiny Proxy\.backup.exe"),
            script_path: Path::new(r"C:\Temp\update.ps1"),
            log_path: Path::new(r"C:\Temp\update.log"),
            expected_size: 100,
            expected_version: "0.2.0",
        };
        let script = build_windows_update_script(&plan);
        assert!(script.contains("O''Brien"));
        assert!(script.contains("Wait-Process -Id 42"));
        assert!(script.contains("ProductVersion"));
        assert!(script.contains("Move-Item -LiteralPath $backup -Destination $current"));
        assert_script_parses(&script);
    }

    #[test]
    fn installer_helper_keeps_current_user_directory_and_verifies_version() {
        let plan = WindowsSetupUpdateScriptPlan {
            process_id: 42,
            current_exe: Path::new(r"C:\Users\O'Brien\Tiny Proxy\tiny-proxy.exe"),
            installer: Path::new(r"C:\Temp\setup.exe"),
            script_path: Path::new(r"C:\Temp\setup.ps1"),
            log_path: Path::new(r"C:\Temp\update.log"),
            expected_version: "0.2.0",
        };
        let script = build_windows_setup_update_script(&plan);
        assert!(script.contains("/CURRENTUSER"));
        assert!(script.contains("/DIR="));
        assert!(script.contains("-WindowStyle Hidden"));
        assert!(script.contains("ProductVersion"));
        assert_script_parses(&script);
    }

    #[test]
    fn portable_staging_preserves_running_binary_and_uses_unique_siblings() {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("tiny-proxy.exe");
        let source = temp.path().join("downloaded.exe");
        std::fs::write(&current, b"old executable").unwrap();
        std::fs::write(&source, b"new executable").unwrap();
        let first = stage_windows_portable_binary_at(&source, &current, 42).unwrap();
        let second = stage_windows_portable_binary_at(&source, &current, 42).unwrap();
        assert_ne!(first, second);
        assert_eq!(first.parent(), current.parent());
        assert_eq!(std::fs::read(current).unwrap(), b"old executable");
        assert_eq!(std::fs::read(first).unwrap(), b"new executable");
    }
}
