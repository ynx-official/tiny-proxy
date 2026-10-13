use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

pub struct DataLocation {
    pub directory: PathBuf,
    pub legacy_directories: Vec<PathBuf>,
}

impl DataLocation {
    pub fn from_environment() -> io::Result<Self> {
        let explicit = std::env::var_os("TINY_PROXY_DATA_DIR");
        let profile = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
        let local = std::env::var_os("LOCALAPPDATA");
        let directory = resolve(explicit.as_deref(), profile.as_deref())?;
        let legacy_directories = if explicit.is_none() && !directory.join("app.db").try_exists()? {
            local
                .as_deref()
                .map(Path::new)
                .map(discover_legacy)
                .transpose()?
                .unwrap_or_default()
        } else {
            vec![]
        };
        Ok(Self {
            directory,
            legacy_directories,
        })
    }
}

pub fn resolve(explicit: Option<&OsStr>, profile: Option<&OsStr>) -> io::Result<PathBuf> {
    if let Some(path) = explicit {
        if path.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TINY_PROXY_DATA_DIR is empty",
            ));
        }
        return Ok(PathBuf::from(path));
    }
    // AppData can resolve to an MSIX launcher's private LocalCache. This location
    // gives PowerShell, Explorer and packaged launchers the same database.
    Ok(PathBuf::from(profile.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "user home directory is unavailable",
        )
    })?)
    .join(".tiny-proxy"))
}

pub fn discover_legacy(local: &Path) -> io::Result<Vec<PathBuf>> {
    let mut directories = vec![local.join("TinyProxy")];
    let packages = match std::fs::read_dir(local.join("Packages")) {
        Ok(packages) => packages,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(directories),
        Err(error) => return Err(error),
    };
    for package in packages {
        let path = package?.path().join("LocalCache/Local/TinyProxy");
        if path.join("app.db").try_exists()? {
            directories.push(path);
        }
    }
    Ok(directories)
}
