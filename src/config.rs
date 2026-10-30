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
# or a virtual key code like 0x5D. Use a list like ["Copilot", "Apps"] for several keys.
activation_key = "Apps"

# Tapping the activation key without typing anything works like the key normally does.
tap_passthrough = true
# Launch as soon as what you typed matches only one command, without waiting for release.
instant = false
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

# More things a command can do. Copy a line into the list above to use it.
#
# path: a program, or a command line (it runs through cmd, so `start` works):
#   {name = 'yt', path = 'start https://youtube.com'}
# A list of paths starts all of them:
#   {name = 'wrk', path = ['notepad.exe', 'start https://github.com']}
# text: types the text instead of starting anything. In double quotes, \n is Enter:
#   {name = 'mail', text = 'me@example.com'}
#   {name = 'sig', text = "Best regards,\nMe"}
# {clipboard} is replaced with the copied text; {clipboard:url} makes it safe in a web address:
#   {name = 'g', path = 'start https://www.google.com/search?q={clipboard:url}'}
# focus: if the program already has a window open, brings it to the front instead:
#   {name = 'txt', path = 'notepad.exe', focus = true}
# dir: the folder the command runs in:
#   {name = 'prj', path = 'start cmd', dir = 'C:/Projects'}
# admin: runs as administrator (Windows asks you first):
#   {name = 'adm', path = 'start cmd', admin = true}
# key: with several activation keys, the command works with only this one:
#   {name = 'c', path = 'calc.exe', key = 'Copilot'}
"#;

static CURRENT: RwLock<Option<Arc<Config>>> = RwLock::new(None);

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Comm {
    pub name: String,
    #[serde(default)]
    pub path: Paths,
    pub text: Option<String>,
    pub dir: Option<String>,
    #[serde(default)]
    pub focus: bool,
    #[serde(default)]
    pub admin: bool,
    pub key: Option<ActivationKey>,
}

impl Comm {
    pub fn uses_clipboard(&self) -> bool {
        self.path.0.iter().chain(&self.text).chain(&self.dir).any(|s| s.contains("{clipboard"))
    }

    fn problem(&self, activation_keys: &[u32]) -> Option<&'static str> {
        if self.path.0.is_empty() == self.text.is_none() {
            Some("needs either path or text")
        } else if self.text.is_some() && (self.dir.is_some() || self.focus || self.admin) {
            Some("can't use dir, focus or admin together with text")
        } else if self.admin && self.uses_clipboard() {
            Some("can't use {clipboard} together with admin")
        } else if self.key.is_some_and(|key| !activation_keys.contains(&key.0)) {
            Some("has a key that isn't in activation_key")
        } else {
            None
        }
    }
}

// One path or a list of them.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(from = "PathList")]
pub struct Paths(pub Vec<String>);

#[derive(Deserialize)]
#[serde(untagged)]
enum PathList {
    One(String),
    Many(Vec<String>),
}

impl From<PathList> for Paths {
    fn from(list: PathList) -> Self {
        match list {
            PathList::One(path) => Paths(vec![path]),
            PathList::Many(paths) => Paths(paths),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub commands: Vec<Comm>,
    pub activation_key: ActivationKeys,
    pub tap_passthrough: bool,
    pub instant: bool,
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
            activation_key: ActivationKeys(vec![0x5D]),
            tap_passthrough: true,
            instant: false,
            overlay: false,
            escape_cancels: true,
            watch_config: true,
            autostart: false,
            log: false,
        }
    }
}

impl Config {
    // Commands that work with the activation key `key`.
    fn commands_for(&self, key: u32) -> impl Iterator<Item = &Comm> {
        self.commands
            .iter()
            .filter(move |com| com.key.is_none_or(|k| k.0 == key))
    }

    pub fn search(&self, name: &str, key: u32) -> Option<&Comm> {
        self.commands_for(key).find(|com| com.name.eq_ignore_ascii_case(name))
    }

    // True when `typed` names a command and no other command's name starts with it.
    pub fn is_unique_match(&self, typed: &str, key: u32) -> bool {
        let mut exact = false;
        for com in self.commands_for(key) {
            if com.name.eq_ignore_ascii_case(typed) {
                exact = true;
            } else if com.name.get(..typed.len()).is_some_and(|start| start.eq_ignore_ascii_case(typed)) {
                return false;
            }
        }
        exact
    }
}

// A virtual key code, or keys::COPILOT. Written in cfg.toml as a number or a key name.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(try_from = "KeySpec")]
pub struct ActivationKey(pub u32);

// One key or a list of keys.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(try_from = "KeyList")]
pub struct ActivationKeys(pub Vec<u32>);

#[derive(Deserialize)]
#[serde(untagged)]
enum KeySpec {
    Code(u32),
    Name(String),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum KeyList {
    One(KeySpec),
    Many(Vec<KeySpec>),
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
                "unknown key {shown}; use a key name like \"Apps\", \"F23\" or \"Copilot\", \
                 or a virtual key code like 0x5D"
            )),
        }
    }
}

impl TryFrom<KeyList> for ActivationKeys {
    type Error = String;

    fn try_from(list: KeyList) -> Result<Self, String> {
        let specs = match list {
            KeyList::One(spec) => vec![spec],
            KeyList::Many(specs) => specs,
        };
        if specs.is_empty() {
            return Err("activation_key needs at least one key".to_string());
        }
        specs
            .into_iter()
            .map(|spec| ActivationKey::try_from(spec).map(|key| key.0))
            .collect::<Result<_, _>>()
            .map(ActivationKeys)
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
    hook::set_activation_keys(cfg.activation_key.0.clone());
    let autostart = cfg.autostart;
    if let Ok(mut current) = CURRENT.write() {
        *current = Some(Arc::new(cfg));
    }
    autostart::sync(autostart);
}

fn parse(text: &str, path: &Path) -> Result<Config, String> {
    let cfg: Config = toml::from_str(text).map_err(|err| format!("Invalid {}:\n{err}", path.display()))?;
    for com in &cfg.commands {
        if let Some(problem) = com.problem(&cfg.activation_key.0) {
            return Err(format!("Invalid {}:\ncommand '{}' {problem}", path.display(), com.name));
        }
    }
    Ok(cfg)
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

    const APPS: u32 = 0x5D;
    const F23: u32 = 0x86;

    fn parse_str(text: &str) -> Result<Config, String> {
        parse(text, Path::new(CFG_NAME))
    }

    fn path_of<'a>(cfg: &'a Config, name: &str, key: u32) -> Option<&'a str> {
        cfg.search(name, key).map(|com| com.path.0[0].as_str())
    }

    fn command_problem(command: &str) -> String {
        parse_str(&format!("commands = [{command}]")).unwrap_err()
    }

    #[test]
    fn commented_examples_in_default_cfg_are_valid() {
        let examples: Vec<&str> = DEFAULT_CFG
            .lines()
            .filter_map(|line| line.strip_prefix("#   {"))
            .collect();
        assert!(examples.len() >= 9);
        let commands = examples.iter().map(|e| format!("{{{e}")).collect::<Vec<_>>().join(",\n");
        let cfg = parse_str(&format!("activation_key = ['Apps', 'Copilot']\ncommands = [\n{commands}\n]")).unwrap();
        assert_eq!(cfg.commands.len(), examples.len());
    }

    #[test]
    fn commands_take_path_lists_text_and_options() {
        let cfg = parse_str(
            "commands = [\n\
             {name = 'w', path = ['a.exe', 'start b'], dir = 'C:/x', focus = true},\n\
             {name = 's', text = \"hi\\nthere\"},\n\
             {name = 'g', path = 'start x?q={clipboard:url}'},\n\
             {name = 'r', path = 'regedit.exe', admin = true},\n\
             ]",
        )
        .unwrap();
        let w = cfg.search("w", APPS).unwrap();
        assert_eq!(w.path, Paths(vec!["a.exe".into(), "start b".into()]));
        assert_eq!(w.dir.as_deref(), Some("C:/x"));
        assert!(w.focus && !w.admin && !w.uses_clipboard());
        assert_eq!(cfg.search("s", APPS).unwrap().text.as_deref(), Some("hi\nthere"));
        assert!(cfg.search("g", APPS).unwrap().uses_clipboard());
        assert!(cfg.search("r", APPS).unwrap().admin);
    }

    #[test]
    fn conflicting_command_fields_are_reported() {
        assert!(command_problem("{name = 'a'}").contains("command 'a' needs either path or text"));
        assert!(command_problem("{name = 'a', path = 'x', text = 'y'}").contains("needs either path or text"));
        assert!(command_problem("{name = 'a', path = []}").contains("needs either path or text"));
        assert!(command_problem("{name = 'a', text = 'y', focus = true}").contains("can't use dir, focus or admin"));
        assert!(command_problem("{name = 'a', path = 'x {clipboard}', admin = true}").contains("can't use {clipboard} together with admin"));
    }

    #[test]
    fn default_cfg_parses_and_matches_case_insensitively() {
        let cfg = parse_str(DEFAULT_CFG).unwrap();
        assert_eq!(cfg.activation_key, ActivationKeys(vec![APPS]));
        assert!(cfg.tap_passthrough && !cfg.instant && !cfg.overlay && cfg.escape_cancels);
        assert!(cfg.watch_config && !cfg.autostart && !cfg.log);
        assert_eq!(path_of(&cfg, "TXT", APPS), Some("notepad.exe"));
        assert_eq!(path_of(&cfg, "nope", APPS), None);
    }

    #[test]
    fn old_config_without_new_options_still_parses() {
        let cfg = parse_str("activation_key = 0x5D\ncommands = [{name = 'a', path = 'b'}]").unwrap();
        assert_eq!(cfg.activation_key, ActivationKeys(vec![APPS]));
        assert!(cfg.escape_cancels && cfg.watch_config && cfg.tap_passthrough);
    }

    #[test]
    fn activation_key_accepts_names_and_lists() {
        assert_eq!(parse_str("activation_key = 'Copilot'").unwrap().activation_key, ActivationKeys(vec![keys::COPILOT]));
        assert_eq!(parse_str("activation_key = \"F23\"").unwrap().activation_key, ActivationKeys(vec![F23]));
        assert_eq!(
            parse_str("activation_key = ['Copilot', 0x5D]").unwrap().activation_key,
            ActivationKeys(vec![keys::COPILOT, APPS])
        );
    }

    #[test]
    fn commands_can_be_limited_to_one_key() {
        let cfg = parse_str(
            "activation_key = ['Apps', 'F23']\n\
             commands = [{name = 'a', path = 'any'}, {name = 'b', path = 'f23 only', key = 'F23'}]",
        )
        .unwrap();
        assert_eq!(path_of(&cfg, "a", APPS), Some("any"));
        assert_eq!(path_of(&cfg, "a", F23), Some("any"));
        assert_eq!(path_of(&cfg, "b", APPS), None);
        assert_eq!(path_of(&cfg, "b", F23), Some("f23 only"));
    }

    #[test]
    fn unique_match_needs_no_longer_name() {
        let cfg = parse_str(
            "activation_key = ['Apps', 'F23']\n\
             commands = [{name = 'txt', path = 'x'}, {name = 'ab', path = 'x'}, {name = 'abc', path = 'x'},\n\
             {name = 'q', path = 'x'}, {name = 'qq', path = 'x', key = 'F23'}]",
        )
        .unwrap();
        assert!(cfg.is_unique_match("TXT", APPS));
        assert!(!cfg.is_unique_match("tx", APPS));
        assert!(!cfg.is_unique_match("ab", APPS));
        assert!(cfg.is_unique_match("abc", APPS));
        assert!(cfg.is_unique_match("q", APPS));
        assert!(!cfg.is_unique_match("q", F23));
    }

    #[test]
    fn bad_values_are_reported() {
        assert!(parse_str("activation_key = 'Nope'").unwrap_err().contains("unknown key \"Nope\""));
        assert!(parse_str("activation_key = 0x1FF").unwrap_err().contains("unknown key 0x1FF"));
        assert!(parse_str("activation_key = []").unwrap_err().contains("at least one key"));
        assert!(parse_str("overley = true").unwrap_err().contains("unknown field `overley`"));
        let err = parse_str("activation_key = 'Apps'\ncommands = [{name = 'a', path = 'b', key = 'F23'}]").unwrap_err();
        assert!(err.contains("command 'a' has a key that isn't in activation_key"), "{err}");
    }
}
