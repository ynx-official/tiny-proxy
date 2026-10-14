pub mod data_dir;
pub mod protection;
pub mod single_instance;
#[cfg(all(windows, feature = "desktop"))]
pub mod tray;
