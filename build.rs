//! Embeds the application icon and version info into the Windows executable
//! (Explorer, shortcuts, taskbar). No-op on other platforms.

fn main() {
    println!("cargo:rerun-if-changed=assets/icons/skrib.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/skrib.ico")
            .set("ProductName", "Skrib")
            .set("FileDescription", "Skrib image annotator")
            .set("LegalCopyright", "MIT OR Apache-2.0");
        res.compile().expect("failed to embed Windows resources");
    }
}
