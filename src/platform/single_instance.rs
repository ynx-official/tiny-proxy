use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
};

pub struct InstanceGuard {
    _file: File,
}
impl InstanceGuard {
    pub fn acquire(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        fs2::FileExt::try_lock_exclusive(&file)?;
        Ok(Self { _file: file })
    }
}
