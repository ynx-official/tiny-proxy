fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icons/tiny-proxy.rc");
    println!("cargo:rerun-if-changed=assets/icons/tiny-proxy.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var_os("CARGO_FEATURE_DESKTOP").is_some()
    {
        embed_resource::compile_for(
            "assets/icons/tiny-proxy.rc",
            ["tiny-proxy"],
            embed_resource::NONE,
        )
        .manifest_required()
        .expect("Tiny Proxy's Windows icon resource could not be embedded");
    }
}
