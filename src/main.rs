mod desktop;

use tiny_proxy::{
    app::Backend,
    platform::{data_dir::DataLocation, single_instance::InstanceGuard},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let _instance = InstanceGuard::acquire(&data.join("instance.lock"))
        .map_err(|_| "Tiny Proxy is already running, or its data directory cannot be locked")?;
    let mut backend = Backend::start_with_legacy(data.join("app.db"), location.legacy_directories)?;
    let events = backend
        .events
        .take()
        .ok_or("backend event receiver unavailable")?;
    desktop::run(backend.commands.clone(), events);
    backend.stop();
    Ok(())
}
