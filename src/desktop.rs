//! Linux desktop integration: `markly --install-desktop-entry` registers
//! a `.desktop` launcher and hicolor icons for the current user, so Markly
//! appears in the application grid/menu and its running window gets the
//! right icon (GNOME and KDE match the Wayland app id `markly` to
//! `markly.desktop`). `--uninstall-desktop-entry` removes them again.
//!
//! Files written (under `$XDG_DATA_HOME`, default `~/.local/share`):
//! `applications/markly.desktop`, `icons/hicolor/<N>x<N>/apps/markly.png`,
//! `icons/hicolor/scalable/apps/markly.svg`.

use crate::icon;
use std::io;
use std::path::{Path, PathBuf};

pub const APP_ID: &str = "markly";
const PNG_SIZES: [u32; 8] = [16, 24, 32, 48, 64, 128, 256, 512];

fn data_home() -> io::Result<PathBuf> {
    if let Some(p) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from)
        && p.is_absolute()
    {
        return Ok(p);
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local/share"))
        .ok_or_else(|| io::Error::other("HOME is not set"))
}

/// Quotes an `Exec=` argument per the Desktop Entry specification.
fn exec_quote(path: &str) -> String {
    let needs = path
        .chars()
        .any(|c| c.is_whitespace() || "\"'\\><~|&;$*?#()`".contains(c));
    // `%` starts a field code and must always be doubled.
    if !needs {
        return path.replace('%', "%%");
    }
    let mut out = String::from("\"");
    for c in path.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out.replace('%', "%%")
}

pub fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]
Type=Application
Name=Markly
GenericName=Image Annotator
Comment=Paste an image, annotate it, copy the result
Exec={} %f
Icon={APP_ID}
Terminal=false
Categories=Graphics;2DGraphics;
Keywords=annotate;annotation;screenshot;arrow;markup;pixelate;
MimeType=image/png;image/jpeg;image/webp;
StartupNotify=true
StartupWMClass={APP_ID}
",
        exec_quote(&exe.to_string_lossy())
    )
}

fn icon_paths(base: &Path) -> Vec<(PathBuf, Option<u32>)> {
    let hicolor = base.join("icons/hicolor");
    let mut v: Vec<_> = PNG_SIZES
        .iter()
        .map(|&s| (hicolor.join(format!("{s}x{s}/apps/{APP_ID}.png")), Some(s)))
        .collect();
    v.push((hicolor.join(format!("scalable/apps/{APP_ID}.svg")), None));
    v
}

/// Installs the launcher for the binary at `exe`. Returns the paths written.
pub fn install(exe: &Path) -> io::Result<Vec<PathBuf>> {
    install_into(&data_home()?, exe)
}

pub fn install_into(base: &Path, exe: &Path) -> io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for (path, size) in icon_paths(base) {
        let bytes = match size {
            Some(s) => {
                crate::store::encode_png(&icon::render(s, icon::MARGIN_DEFAULT)).map_err(io::Error::other)?
            }
            None => icon::svg(icon::MARGIN_DEFAULT).into_bytes(),
        };
        crate::store::atomic_write(&path, &bytes)?;
        written.push(path);
    }
    let entry = base.join(format!("applications/{APP_ID}.desktop"));
    crate::store::atomic_write(&entry, desktop_entry(exe).as_bytes())?;
    written.push(entry);
    refresh_caches(base);
    Ok(written)
}

pub fn uninstall() -> io::Result<Vec<PathBuf>> {
    uninstall_from(&data_home()?)
}

pub fn uninstall_from(base: &Path) -> io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    let mut paths: Vec<PathBuf> = icon_paths(base).into_iter().map(|(p, _)| p).collect();
    paths.push(base.join(format!("applications/{APP_ID}.desktop")));
    for p in paths {
        match std::fs::remove_file(&p) {
            Ok(()) => removed.push(p),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    refresh_caches(base);
    Ok(removed)
}

/// Best effort: desktops usually notice changes by themselves; these tools
/// just make it immediate where they are installed.
fn refresh_caches(base: &Path) {
    let run = |cmd: &str, args: &[&std::ffi::OsStr]| {
        let _ = std::process::Command::new(cmd)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    };
    let apps = base.join("applications");
    let hicolor = base.join("icons/hicolor");
    run("update-desktop-database", &[apps.as_os_str()]);
    run(
        "gtk-update-icon-cache",
        &["-q".as_ref(), "-t".as_ref(), "-f".as_ref(), hicolor.as_os_str()],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_quoting() {
        assert_eq!(exec_quote("/usr/bin/markly"), "/usr/bin/markly");
        assert_eq!(exec_quote("/home/a b/markly"), "\"/home/a b/markly\"");
        assert_eq!(exec_quote("/x/$HOME\"q/m"), "\"/x/\\$HOME\\\"q/m\"");
        assert_eq!(exec_quote("/x y/100%/m"), "\"/x y/100%%/m\"");
        assert_eq!(exec_quote("/x/100%/m"), "/x/100%%/m");
    }

    #[test]
    fn install_and_uninstall_roundtrip() {
        let base = std::env::temp_dir().join(format!("markly-desktop-{}", crate::model::new_document_id()));
        let written = install_into(&base, Path::new("/opt/My Apps/markly")).unwrap();
        assert_eq!(written.len(), PNG_SIZES.len() + 2);
        let entry = std::fs::read_to_string(base.join("applications/markly.desktop")).unwrap();
        assert!(entry.contains("Exec=\"/opt/My Apps/markly\" %f"));
        assert!(entry.contains("StartupWMClass=markly"));
        let png = image::open(base.join("icons/hicolor/48x48/apps/markly.png")).unwrap();
        assert_eq!(png.width(), 48);
        assert!(base.join("icons/hicolor/scalable/apps/markly.svg").is_file());
        assert_eq!(uninstall_from(&base).unwrap().len(), written.len());
        assert!(!base.join("applications/markly.desktop").exists());
        std::fs::remove_dir_all(base).unwrap();
    }
}
