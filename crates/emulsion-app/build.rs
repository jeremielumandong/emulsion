fn main() {
    #[cfg(windows)]
    embed_windows_icon();
}

#[cfg(windows)]
fn embed_windows_icon() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    const ICON: &str = "../../assets/icons/emulsion.ico";
    println!("cargo:rerun-if-changed={ICON}");
    assert!(
        std::path::Path::new(ICON).is_file(),
        "{ICON} is missing; the Windows executable requires its application icon"
    );

    // GPUI uses icon resource ID 1 for the window and taskbar.
    winresource::WindowsResource::new()
        .set_icon(ICON)
        .set("ProductName", "Emulsion")
        .set("FileDescription", "Emulsion image editor")
        .compile()
        .expect("embedding the Windows app icon requires rc.exe from the Windows SDK or llvm-rc");
}
