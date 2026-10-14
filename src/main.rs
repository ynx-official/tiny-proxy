#![cfg_attr(windows, windows_subsystem = "windows")]

mod desktop;

use tiny_proxy::{
    app::Backend,
    platform::{data_dir::DataLocation, single_instance::InstanceGuard},
};

fn main() {
    if let Err(error) = start() {
        #[cfg(windows)]
        tiny_proxy::platform::tray::show_error(&error.to_string());
        #[cfg(not(windows))]
        eprintln!("Tiny Proxy could not start: {error}");
        std::process::exit(1);
    }
}

fn start() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "--version")
    {
        println!("Tiny Proxy {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let location = DataLocation::from_environment()?;
    let data = location.directory;
    std::fs::create_dir_all(&data)?;
    eprintln!("startup database={}", data.join("app.db").display());
    let _instance = match InstanceGuard::acquire(&data.join("instance.lock")) {
        Ok(instance) => instance,
        Err(error) => {
            #[cfg(windows)]
            if tiny_proxy::platform::tray::restore_existing_instance(&data) {
                return Ok(());
            }
            return Err(format!(
                "无法启动 Tiny Proxy：应用已在运行，或数据目录无法锁定（{error}）"
            )
            .into());
        }
    };
    let mut backend = Backend::start_with_legacy(data.join("app.db"), location.legacy_directories)?;
    let events = backend
        .events
        .take()
        .ok_or("backend event receiver unavailable")?;
    desktop::run(backend.commands.clone(), events, data);
    backend.stop();
    Ok(())
}
