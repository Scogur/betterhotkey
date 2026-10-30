use std::{
    fmt::Display,
    fs::{self, OpenOptions},
    io::Write,
    sync::Mutex,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use crate::config;

const LOG_NAME: &str = "betterhotkey.log";
const MAX_LOG_BYTES: u64 = 1024 * 1024;

static LOCK: Mutex<()> = Mutex::new(());

pub fn enabled() -> bool {
    config::current().is_some_and(|cfg| cfg.log)
}

// Appends a line to betterhotkey.log next to the exe, if `log = true` in cfg.toml.
pub fn write(msg: impl Display) {
    if !enabled() {
        return;
    }
    let Ok(_guard) = LOCK.lock() else { return; };
    let path = config::next_to_exe(LOG_NAME);
    // Keep one old log around instead of letting the file grow forever.
    if fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES) {
        let _ = fs::rename(&path, path.with_extension("log.old"));
    }
    let t = unsafe { GetLocalTime() };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            file,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}  {msg}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        );
    }
}
