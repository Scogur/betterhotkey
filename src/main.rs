#![windows_subsystem = "windows"]

mod autostart;
mod config;
mod hook;
mod keys;
mod launcher;
mod log;
mod overlay;

use windows::{
    core::{HSTRING, Result, w},
    Win32::UI::{HiDpi::*, WindowsAndMessaging::*},
};

// There is no console, so errors are shown in a message box.
pub fn error_box(msg: &str) {
    unsafe {
        MessageBoxW(None, &HSTRING::from(msg), w!("betterhotkey"), MB_OK | MB_ICONERROR);
    }
}

fn fail(msg: &str) -> ! {
    error_box(msg);
    std::process::exit(1);
}

fn main() -> Result<()> {
    // Lets the overlay render sharply on scaled displays.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    let path = config::path();
    let cfg = config::load(&path).unwrap_or_else(|err| fail(&err));
    let watch = cfg.watch_config;
    config::apply(cfg);
    log::write(format!("Started betterhotkey {} (pid {})", env!("CARGO_PKG_VERSION"), std::process::id()));

    if let Err(err) = overlay::init() {
        log::write(format!("Could not create overlay: {err}"));
    }
    let hook = hook::install()
        .unwrap_or_else(|err| fail(&format!("Could not install keyboard hook:\n{err}")));
    if watch {
        config::watch(path);
    }

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(hook)?;
    }
    Ok(())
}
