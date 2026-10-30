use std::env;
use windows::{
    core::{HSTRING, PCWSTR, Result, w},
    Win32::System::Registry::*,
};
use crate::log;

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");

// Adds or removes this exe in the "run at sign-in" list of the current user.
pub fn sync(enabled: bool) {
    let result = env::current_exe().map_err(windows::core::Error::from).and_then(|exe| {
        // Named after the exe's path, so copies in different folders each get their own entry.
        let name = format!("betterhotkey ({})", exe.display());
        let command = format!("\"{}\"", exe.display());
        set_entry(RUN_KEY, &name, enabled.then_some(command.as_str()))
    });
    match result {
        Ok(Some(true)) => log::write("Autostart enabled"),
        Ok(Some(false)) => log::write("Autostart disabled"),
        Ok(None) => {}
        Err(err) => log::write(format!("Could not update autostart: {err}")),
    }
}

// Sets the value `name` under HKCU\`subkey` to `command`, or deletes it when `command` is None.
// Returns Some(enabled) when something changed.
fn set_entry(subkey: PCWSTR, name: &str, command: Option<&str>) -> Result<Option<bool>> {
    let name = HSTRING::from(name);
    let mut key = HKEY::default();
    unsafe {
        RegOpenKeyExW(HKEY_CURRENT_USER, subkey, None, KEY_QUERY_VALUE | KEY_SET_VALUE, &mut key).ok()?;
        let current = read_string(key, &name);
        let result = match command {
            Some(command) if current.as_deref() != Some(command) => {
                let data: Vec<u8> = command
                    .encode_utf16()
                    .chain([0])
                    .flat_map(u16::to_le_bytes)
                    .collect();
                RegSetValueExW(key, &name, None, REG_SZ, Some(&data)).ok().map(|_| Some(true))
            }
            None if current.is_some() => RegDeleteValueW(key, &name).ok().map(|_| Some(false)),
            _ => Ok(None),
        };
        let _ = RegCloseKey(key);
        result
    }
}

fn read_string(key: HKEY, name: &HSTRING) -> Option<String> {
    let mut buf = [0u16; 2048];
    let mut len = size_of_val(&buf) as u32;
    unsafe {
        RegGetValueW(key, PCWSTR::null(), name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len))
    }
    .ok()
    .ok()?;
    let chars = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const TEST_KEY: &str = r"HKCU\Software\betterhotkey-test";

    #[test]
    fn adds_updates_and_removes_entry() {
        let reg = |args: &[&str]| Command::new("reg").args(args).output().unwrap();
        reg(&["add", TEST_KEY, "/f"]);
        let subkey = w!(r"Software\betterhotkey-test");

        assert_eq!(set_entry(subkey, "bhk (C:/a b)", Some("\"C:/a b/x.exe\"")).unwrap(), Some(true));
        assert_eq!(set_entry(subkey, "bhk (C:/a b)", Some("\"C:/a b/x.exe\"")).unwrap(), None);
        let out = String::from_utf8_lossy(&reg(&["query", TEST_KEY, "/v", "bhk (C:/a b)"]).stdout).to_string();
        assert!(out.contains("\"C:/a b/x.exe\""), "{out}");
        assert_eq!(set_entry(subkey, "bhk (C:/a b)", None).unwrap(), Some(false));
        assert_eq!(set_entry(subkey, "bhk (C:/a b)", None).unwrap(), None);

        reg(&["delete", TEST_KEY, "/f"]);
    }
}
