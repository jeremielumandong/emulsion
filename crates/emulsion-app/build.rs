fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        build_linux_player();
    }
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

// Build a tiny dynamically linked adapter; GTK/WebKit/GStreamer stay system
// dependencies and are never pulled into the application executable/AppImage.
fn build_linux_player() {
    use std::{path::PathBuf, process::Command};
    let source = "../emulsion-ui/src/web_player/linux_helper.c";
    println!("cargo:rerun-if-changed={source}");
    println!("cargo:rerun-if-env-changed=EMULSION_WEB_PLAYER_HELPER");
    let destination =
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("emulsion-web-player");
    if let Some(prebuilt) = std::env::var_os("EMULSION_WEB_PLAYER_HELPER") {
        std::fs::copy(&prebuilt, destination).expect("copying the target Linux player adapter");
        println!(
            "cargo:rerun-if-changed={}",
            PathBuf::from(prebuilt).display()
        );
        return;
    }
    if std::env::var("HOST").ok() != std::env::var("TARGET").ok() {
        println!(
            "cargo:warning=Cross compiling Linux video requires EMULSION_WEB_PLAYER_HELPER for the target architecture"
        );
        std::fs::write(destination, []).unwrap();
        return;
    }
    let flags = Command::new("pkg-config")
        .args([
            "--cflags",
            "--libs",
            "webkit2gtk-4.1",
            "gtk+-3.0",
            "gstreamer-1.0",
        ])
        .output();
    let Ok(flags) = flags else {
        panic!("pkg-config is required to build the Linux video adapter");
    };
    if !flags.status.success() {
        println!(
            "cargo:warning=Linux video adapter unavailable: install WebKitGTK 4.1, GTK3 and GStreamer development headers"
        );
        std::fs::write(destination, []).unwrap();
        return;
    }
    let status = Command::new("cc")
        .args([
            "-std=c11", "-O2", "-s", "-Wall", "-Wextra", "-Werror", source, "-o",
        ])
        .arg(destination)
        .args(String::from_utf8(flags.stdout).unwrap().split_whitespace())
        .status()
        .expect("C compiler for Linux video adapter");
    assert!(status.success(), "compiling the Linux video adapter failed");
}
