use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};
use serde::Deserialize;
use windows::{
    core::HSTRING,
    Win32::{
        Foundation::WAIT_OBJECT_0,
        Storage::FileSystem::*,
        System::Threading::{INFINITE, WaitForSingleObject},
    },
};
use crate::{autostart, hook, keys, log};

const CFG_NAME: &str = "cfg.toml";
pub const DEFAULT_CFG: &str = r#"# Key to hold while typing a command name: a key name like "Apps", "F23" or "Copilot",
# or a virtual key code like 0x5D.
activation_key = "Apps"

# Show what you type in a small box on screen.
overlay = false
# Pressing Escape while holding the activation key clears what you typed.
escape_cancels = true
# Apply changes to this file as soon as it is saved.
watch_config = true
# Start betterhotkey when you sign in to Windows.
autostart = false
# Write launches and errors to betterhotkey.log next to betterhotkey.exe.
log = false

commands = [
    {name = 'clc', path = 'calc.exe'},
    {name = 'txt', path = 'notepad.exe'},
]
"#;

static CURRENT: RwLock<Option<Arc<Config>>> = RwLock::new(None);

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Comm {
    pub name: String,
    pub path: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub commands: Vec<Comm>,
    pub activation_key: ActivationKey,
    pub overlay: bool,
    pub escape_cancels: bool,
    pub watch_config: bool,
    pub autostart: bool,
    pub log: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            commands: Vec::new(),
            activation_key: ActivationKey(0x5D),
            overlay: false,
            escape_cancels: true,
            watch_config: true,
            autostart: false,
            log: false,
        }
    }
}

impl Config {
    pub fn search(&self, key: &str) -> Option<&str> {
        self.commands
            .iter()
            .find(|com| com.name.eq_ignore_ascii_case(key))
            .map(|com| com.path.as_str())
    }
}

// A virtual key code, or keys::COPILOT. Written in cfg.toml as a number or a key name.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(try_from = "KeySpec")]
pub struct ActivationKey(pub u32);

#[derive(Deserialize)]
#[serde(untagged)]
enum KeySpec {
    Code(u32),
    Name(String),
}

impl TryFrom<KeySpec> for ActivationKey {
    type Error = String;

    fn try_from(spec: KeySpec) -> Result<Self, String> {
        let (vk, shown) = match spec {
            KeySpec::Code(vk) => (Some(vk), format!("{vk:#X}")),
            KeySpec::Name(name) => (keys::parse(&name), format!("{name:?}")),
        };
        match vk {
            Some(vk) if (0x01..=0xFE).contains(&vk) || vk == keys::COPILOT => Ok(ActivationKey(vk)),
            _ => Err(format!(
                "unknown activation_key {shown}; use a key name like \"Apps\", \"F23\" or \"Copilot\", \
                 or a virtual key code like 0x5D"
            )),
        }
    }
}

pub fn next_to_exe(name: &str) -> PathBuf {
    env::current_exe()
        .map(|exe| exe.with_file_name(name))
        .unwrap_or_else(|_| PathBuf::from(name))
}

// The config lives next to the exe, so it is found no matter where the program is started from.
pub fn path() -> PathBuf {
    next_to_exe(CFG_NAME)
}

pub fn current() -> Option<Arc<Config>> {
    CURRENT.read().ok()?.clone()
}

pub fn apply(cfg: Config) {
    hook::set_activation_key(cfg.activation_key.0);
    let autostart = cfg.autostart;
    if let Ok(mut current) = CURRENT.write() {
        *current = Some(Arc::new(cfg));
    }
    autostart::sync(autostart);
}

fn parse(text: &str, path: &Path) -> Result<Config, String> {
    toml::from_str(text).map_err(|err| format!("Invalid {}:\n{err}", path.display()))
}

// Reads the config, creating the default one if there is none yet.
pub fn load(path: &Path) -> Result<Config, String> {
    let text = match fs::read_to_string(path) {
        Ok(val) => val,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            fs::write(path, DEFAULT_CFG)
                .map_err(|err| format!("Could not create {}:\n{err}", path.display()))?;
            DEFAULT_CFG.to_string()
        }
        Err(err) => return Err(format!("Could not read {}:\n{err}", path.display())),
    };
    parse(&text, path)
}

// Reloads the config whenever it changes. The thread sleeps until Windows reports a change
// in the exe's folder, so this costs nothing while the file is left alone.
pub fn watch(path: PathBuf) {
    thread::spawn(move || {
        let Some(dir) = path.parent() else { return; };
        let filter = FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME;
        let handle = match unsafe { FindFirstChangeNotificationW(&HSTRING::from(dir), false, filter) } {
            Ok(handle) => handle,
            Err(err) => {
                log::write(format!("Could not watch {}: {err}", dir.display()));
                return;
            }
        };
        let mut last = fs::read_to_string(&path).ok();
        loop {
            if unsafe { WaitForSingleObject(handle, INFINITE) } != WAIT_OBJECT_0 {
                break;
            }
            // Editors often save in several steps, so give them a moment to finish.
            thread::sleep(Duration::from_millis(200));
            if unsafe { FindNextChangeNotification(handle) }.is_err() {
                break;
            }
            let Ok(text) = fs::read_to_string(&path) else { continue; };
            if last.as_deref() == Some(text.as_str()) {
                continue;
            }
            last = Some(text.clone());
            match parse(&text, &path) {
                Ok(cfg) => {
                    let keep_watching = cfg.watch_config;
                    apply(cfg);
                    log::write("Config reloaded");
                    if !keep_watching {
                        log::write("Stopped watching the config");
                        break;
                    }
                }
                Err(err) => {
                    log::write(&err);
                    crate::error_box(&err);
                }
            }
        }
        let _ = unsafe { FindCloseChangeNotification(handle) };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(text: &str) -> Result<Config, String> {
        parse(text, Path::new(CFG_NAME))
    }

    #[test]
    fn default_cfg_parses_and_matches_case_insensitively() {
        let cfg = parse_str(DEFAULT_CFG).unwrap();
        assert_eq!(cfg.activation_key, ActivationKey(0x5D));
        assert!(!cfg.overlay && cfg.escape_cancels && cfg.watch_config && !cfg.autostart && !cfg.log);
        assert_eq!(cfg.search("TXT"), Some("notepad.exe"));
        assert_eq!(cfg.search("nope"), None);
    }

    #[test]
    fn old_config_without_new_options_still_parses() {
        let cfg = parse_str("activation_key = 0x5D\ncommands = [{name = 'a', path = 'b'}]").unwrap();
        assert_eq!(cfg.activation_key, ActivationKey(0x5D));
        assert!(cfg.escape_cancels && cfg.watch_config);
    }

    #[test]
    fn activation_key_accepts_names() {
        assert_eq!(parse_str("activation_key = 'Copilot'").unwrap().activation_key, ActivationKey(keys::COPILOT));
        assert_eq!(parse_str("activation_key = \"F23\"").unwrap().activation_key, ActivationKey(0x86));
    }

    #[test]
    fn bad_values_are_reported() {
        assert!(parse_str("activation_key = 'Nope'").unwrap_err().contains("unknown activation_key \"Nope\""));
        assert!(parse_str("activation_key = 0x1FF").unwrap_err().contains("unknown activation_key 0x1FF"));
        assert!(parse_str("overley = true").unwrap_err().contains("unknown field `overley`"));
    }
}
