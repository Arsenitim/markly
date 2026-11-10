// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use skrib::app::{PROCESS_START, SkribApp, icons};
use skrib::store::Store;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

const USAGE: &str = "\
Skrib — paste, annotate, copy.

Usage: skrib [IMAGE]

Options:
  -h, --help                   Show this help
  -V, --version                Show version
  --install-desktop-entry      Linux: add Skrib with its icon to the app launcher
  --uninstall-desktop-entry    Linux: remove the launcher entry and icons

Environment:
  SKRIB_DATA_DIR  Override the data directory
  SKRIB_TRACE     Print startup/shutdown timings to stderr";

fn main() -> ExitCode {
    let _ = PROCESS_START.set(Instant::now());
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();

    match args.first().and_then(|a| a.to_str()) {
        Some(flag @ ("--install-desktop-entry" | "--uninstall-desktop-entry")) => {
            return desktop_entry(flag == "--install-desktop-entry");
        }
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("skrib {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    let file = args.iter().map(PathBuf::from).find(|p| p.is_file());
    if file.is_none() && !args.is_empty() {
        eprintln!("skrib: ignoring arguments (no readable image file given)");
    }

    let store = match Store::open_default() {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("skrib: storage unavailable: {e}");
            None
        }
    };
    let settings = store.as_ref().map(Store::load_settings).unwrap_or_default();
    let size = settings.window_size.unwrap_or([1100.0, 760.0]);
    let size = [size[0].clamp(480.0, 8000.0), size[1].clamp(360.0, 8000.0)];

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Skrib")
            .with_app_id("skrib")
            .with_inner_size(size)
            .with_min_inner_size([480.0, 360.0])
            .with_drag_and_drop(true)
            .with_icon(icons::app_icon()),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    let result = eframe::run_native(
        "Skrib",
        options,
        Box::new(move |cc| Ok(Box::new(SkribApp::new(cc, store, settings, file)))),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("skrib: {e}");
            ExitCode::FAILURE
        }
    }
}

fn desktop_entry(install: bool) -> ExitCode {
    if !cfg!(all(unix, not(target_os = "macos"))) {
        eprintln!("skrib: desktop entries are a Linux feature");
        return ExitCode::FAILURE;
    }
    let result = if install {
        std::env::current_exe()
            .and_then(|exe| std::fs::canonicalize(&exe).or(Ok(exe)))
            .and_then(|exe| skrib::desktop::install(&exe))
    } else {
        skrib::desktop::uninstall()
    };
    match result {
        Ok(paths) => {
            let verb = if install { "Installed" } else { "Removed" };
            for p in &paths {
                println!("{verb} {}", p.display());
            }
            if install {
                println!("Skrib now appears in your application launcher.");
                println!("If you move this binary, run --install-desktop-entry again.");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("skrib: {e}");
            ExitCode::FAILURE
        }
    }
}
