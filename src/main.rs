#![windows_subsystem = "windows"]

use std::{
    env, fs,
    io::ErrorKind,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    ptr::read_unaligned,
    sync::{Mutex, OnceLock, atomic::{AtomicBool, AtomicU32, Ordering}},
    thread,
};
use windows::{
    core::{HSTRING, Result, w},
    Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleA,
        UI::WindowsAndMessaging::*,
    },
};
use serde::Deserialize;

const CFG_NAME: &str = "cfg.toml";
const DEFAULT_CFG: &str = r#"activation_key = 0x5D

commands = [
    {name = 'clc', path = 'calc.exe'},
    {name = 'txt', path = 'notepad.exe'},
]
"#;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static CHANGED_KEY: AtomicU32 = AtomicU32::new(0x5d);

static IS_CHANGED_KEY_DOWN: AtomicBool = AtomicBool::new(false);
// Keys whose key-down was swallowed, so only their matching key-up gets swallowed too.
static SWALLOWED: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
static INPUTSTR: Mutex<String> = Mutex::new(String::new());
static CFG: OnceLock<Config> = OnceLock::new();

fn add_to_global_input_str(ch: char) {
    if let Ok(mut s) = INPUTSTR.lock() {
        s.push(ch);
    }
}

fn take_global_input_str() -> String {
    INPUTSTR.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default()
}

// Runs off the hook thread: Windows drops low-level hooks that take too long to return.
fn call_app(com: String) {
    if com.is_empty() {
        return;
    }
    thread::spawn(move || {
        let Some(path) = CFG.get().and_then(|cfg| cfg.search(&com)) else { return; };
        let _ = command_for(path).spawn();
    });
}

// An existing file is launched directly; anything else runs as a cmd command line.
fn command_for(path: &str) -> Command {
    if Path::new(path).is_file() {
        return Command::new(path);
    }
    let shell = env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
    let mut cmd = Command::new(shell);
    cmd.raw_arg(format!("/S /C \"{}\"", fix_program_slashes(path)))
        .creation_flags(CREATE_NO_WINDOW);
    cmd
}

// cmd can't run a program written with forward slashes, so convert them in the program part only.
fn fix_program_slashes(line: &str) -> String {
    let line = line.trim_start();
    let end = match line.strip_prefix('"') {
        Some(rest) => rest.find('"').map_or(line.len(), |i| i + 2),
        None => line.find(char::is_whitespace).unwrap_or(line.len()),
    };
    let (program, args) = line.split_at(end);
    format!("{}{args}", program.replace('/', "\\"))
}

unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 {
        let hook_struct = unsafe { read_unaligned(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let event_type = wparam.0 as u32;
        // The SYS variants are sent instead while Alt is held.
        let is_down = event_type == WM_KEYDOWN || event_type == WM_SYSKEYDOWN;
        let is_up = event_type == WM_KEYUP || event_type == WM_SYSKEYUP;
        let vk = hook_struct.vkCode;

        if vk == CHANGED_KEY.load(Ordering::SeqCst) {
            if is_down {
                IS_CHANGED_KEY_DOWN.store(true, Ordering::SeqCst);
            }
            if is_up {
                IS_CHANGED_KEY_DOWN.store(false, Ordering::SeqCst);
                call_app(take_global_input_str());
            }
            return LRESULT(1);
        }

        let swallowed = &SWALLOWED[(vk & 0xFF) as usize];
        if is_down && IS_CHANGED_KEY_DOWN.load(Ordering::SeqCst) {
            swallowed.store(true, Ordering::SeqCst);
            if (0x41..=0x5A).contains(&vk) {
                add_to_global_input_str(vk as u8 as char);
            }
            return LRESULT(1);
        }
        if is_up && swallowed.swap(false, Ordering::SeqCst) {
            return LRESULT(1);
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}


#[derive(Deserialize, Debug, Clone)]
struct Comm{
    name: String,
    path:String,
}

#[derive(Deserialize, Debug, Clone)]
struct Config{
    commands: Vec<Comm>,
    activation_key: u32,
}

impl Config {
    fn search(&self, key: &str) -> Option<&str> {
        self.commands
            .iter()
            .find(|com| com.name.eq_ignore_ascii_case(key))
            .map(|com| com.path.as_str())
    }
}

// The config lives next to the exe, so it is found no matter where the program is started from.
fn cfg_path() -> PathBuf {
    env::current_exe()
        .map(|exe| exe.with_file_name(CFG_NAME))
        .unwrap_or_else(|_| PathBuf::from(CFG_NAME))
}

fn parse_cfg(path: &Path) -> std::result::Result<Config, String> {
    let s = match fs::read_to_string(path) {
        Ok(val) => val,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            fs::write(path, DEFAULT_CFG)
                .map_err(|err| format!("Could not create {}:\n{err}", path.display()))?;
            DEFAULT_CFG.to_string()
        }
        Err(err) => return Err(format!("Could not read {}:\n{err}", path.display())),
    };
    toml::from_str(&s).map_err(|err| format!("Invalid {}:\n{err}", path.display()))
}

// There is no console, so errors are shown in a message box.
fn fail(msg: &str) -> ! {
    unsafe {
        MessageBoxW(None, &HSTRING::from(msg), w!("betterhotkey"), MB_OK | MB_ICONERROR);
    }
    std::process::exit(1);
}

fn main() -> Result<()> {
    let cfg = parse_cfg(&cfg_path()).unwrap_or_else(|err| fail(&err));
    CHANGED_KEY.store(cfg.activation_key, Ordering::SeqCst);
    let _ = CFG.set(cfg);

    unsafe {
        let h_module = GetModuleHandleA(None)?;
        let h_instance = Some(HINSTANCE(h_module.0));

        let hook = SetWindowsHookExA(
            WH_KEYBOARD_LL,
            Some(low_level_keyboard_proc),
            h_instance,
            0,
        )
        .unwrap_or_else(|err| fail(&format!("Could not install keyboard hook:\n{err}")));

        let mut msg = MSG::default();
        while GetMessageA(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
        UnhookWindowsHookEx(hook)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdout_of(path: &str) -> String {
        let out = command_for(path).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn default_cfg_parses_and_matches_case_insensitively() {
        let cfg: Config = toml::from_str(DEFAULT_CFG).unwrap();
        assert_eq!(cfg.activation_key, 0x5D);
        assert_eq!(cfg.search("TXT"), Some("notepad.exe"));
        assert_eq!(cfg.search("nope"), None);
    }

    #[test]
    fn command_with_arguments_runs() {
        assert_eq!(stdout_of("echo hello world"), "hello world");
    }

    #[test]
    fn quoted_program_path_with_arguments_runs() {
        assert_eq!(stdout_of(r#""C:/Windows/System32/cmd.exe" /C echo "a b""#), "\"a b\"");
        assert_eq!(stdout_of("C:/Windows/System32/cmd.exe /C echo a/b"), "a/b");
    }

    #[test]
    fn existing_file_is_launched_directly() {
        assert_eq!(command_for("C:/Windows/System32/whoami.exe").get_program(), "C:/Windows/System32/whoami.exe");
    }
}
