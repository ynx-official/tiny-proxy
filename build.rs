fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icons/tiny-proxy.rc");
    println!("cargo:rerun-if-changed=assets/icons/tiny-proxy.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var_os("CARGO_FEATURE_DESKTOP").is_some()
    {
        // The updater verifies ProductVersion before replacing a Windows EXE.
        // Both numeric resources and visible strings come from Cargo's version.
        let version = std::env::var("CARGO_PKG_VERSION").expect("Cargo package version missing");
        let numbers = version
            .split(['-', '+'])
            .next()
            .expect("version core missing");
        let numbers = numbers.replace('.', ",");
        let mut resource =
            std::fs::read_to_string("assets/icons/tiny-proxy.rc").expect("icon resource missing");
        resource.push_str(&format!(
            r#"
1 VERSIONINFO
FILEVERSION {numbers},0
PRODUCTVERSION {numbers},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "Tiny Proxy\0"
      VALUE "FileVersion", "{version}\0"
      VALUE "ProductName", "Tiny Proxy\0"
      VALUE "ProductVersion", "{version}\0"
      VALUE "OriginalFilename", "tiny-proxy.exe\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#
        ));
        let resource_path = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").expect("Cargo output directory missing"),
        )
        .join("tiny-proxy.rc");
        std::fs::write(&resource_path, resource)
            .expect("Windows version resource could not be written");
        embed_resource::compile_for(resource_path, ["tiny-proxy"], embed_resource::NONE)
            .manifest_required()
            .expect("Tiny Proxy's Windows icon resource could not be embedded");
    }
}
