use std::{
    ptr::read_unaligned,
    sync::{Mutex, RwLock, atomic::{AtomicBool, AtomicU32, Ordering}},
};
use windows::{
    core::Result,
    Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleA,
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};
use crate::{
    config,
    input::{self, INJECTED_TAG, KeyTap, MASK_KEY, is_held},
    keys, launcher, overlay,
};

static ACTIVATION_KEYS: RwLock<Vec<u32>> = RwLock::new(Vec::new());
// The configured activation key being held (a vk or keys::COPILOT), or 0 when none is.
static ACTIVE_KEY: AtomicU32 = AtomicU32::new(0);
// Whether another key went down while the activation key was held, so its release isn't a tap.
static OTHER_KEY_PRESSED: AtomicBool = AtomicBool::new(false);
// The key-down that began the current session, sent again when the key is only tapped.
static TAP: Mutex<Option<KeyTap>> = Mutex::new(None);
// Keys whose key-down was swallowed, so only their matching key-up gets swallowed too.
static SWALLOWED: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
static INPUTSTR: Mutex<String> = Mutex::new(String::new());

// What the hook does besides swallowing or passing on the key.
#[derive(Debug, PartialEq)]
enum Effect {
    Send(KeyTap),
    Launch(String, u32),
}

pub fn set_activation_keys(keys: Vec<u32>) {
    if let Ok(mut current) = ACTIVATION_KEYS.write() {
        *current = keys;
    }
    // The held key may not be an activation key any more, so end the session instead of getting stuck in it.
    ACTIVE_KEY.store(0, Ordering::SeqCst);
    take_input();
}

pub fn install() -> Result<HHOOK> {
    unsafe {
        let h_module = GetModuleHandleA(None)?;
        SetWindowsHookExA(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), Some(HINSTANCE(h_module.0)), 0)
    }
}

fn push_input(ch: char) -> String {
    INPUTSTR
        .lock()
        .map(|mut s| {
            s.push(ch);
            s.clone()
        })
        .unwrap_or_default()
}

fn take_input() -> String {
    INPUTSTR.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default()
}

fn matches_key(key: u32, vk: u32, is_down: bool) -> bool {
    match key {
        // The Win and Shift parts of Win+Shift+F23 arrive first and are let through.
        keys::COPILOT => {
            vk == keys::VK_LAUNCH_APP1
                || (vk == keys::VK_F23
                    && (!is_down || ((is_held(VK_LWIN) || is_held(VK_RWIN)) && is_held(VK_SHIFT))))
        }
        key => vk == key,
    }
}

// The configured activation key this event belongs to, if any.
fn activation_for(vk: u32, is_down: bool) -> Option<u32> {
    let keys = ACTIVATION_KEYS.read().ok()?;
    keys.iter().copied().find(|&key| matches_key(key, vk, is_down))
}

fn begin(event: &KBDLLHOOKSTRUCT) -> Option<Effect> {
    take_input();
    OTHER_KEY_PRESSED.store(false, Ordering::SeqCst);
    let tap = KeyTap {
        vk: event.vkCode as u16,
        scan: event.scanCode as u16,
        extended: event.flags.0 & LLKHF_EXTENDED.0 != 0,
    };
    if let Ok(mut current) = TAP.lock() {
        *current = Some(tap);
    }
    overlay::show("");
    (is_held(VK_LWIN) || is_held(VK_RWIN) || is_held(VK_MENU)).then_some(Effect::Send(MASK_KEY))
}

fn finish(key: u32) -> Option<Effect> {
    let com = take_input();
    if !OTHER_KEY_PRESSED.load(Ordering::SeqCst) {
        overlay::hide();
        let passthrough = config::current().is_none_or(|cfg| cfg.tap_passthrough);
        let tap = TAP.lock().ok().and_then(|tap| *tap);
        return tap.filter(|_| passthrough).map(Effect::Send);
    }
    if com.is_empty() {
        overlay::hide();
        return None;
    }
    Some(Effect::Launch(com, key))
}

// Keys stay swallowed until the activation key is released, so letters typed afterwards start
// a new name instead of reaching Windows (as Win+Shift shortcuts, with the Copilot key).
fn cancel() {
    take_input();
    overlay::show("");
}

// Decides whether to swallow the key event, and what else to do.
fn handle_key(event: &KBDLLHOOKSTRUCT, event_type: u32) -> (bool, Option<Effect>) {
    let vk = event.vkCode;
    // The SYS variants are sent instead while Alt is held.
    let is_down = event_type == WM_KEYDOWN || event_type == WM_SYSKEYDOWN;
    let is_up = event_type == WM_KEYUP || event_type == WM_SYSKEYUP;
    let swallowed = &SWALLOWED[(vk & 0xFF) as usize];
    let active = ACTIVE_KEY.load(Ordering::SeqCst);

    if let Some(key) = activation_for(vk, is_down) {
        if is_down {
            swallowed.store(true, Ordering::SeqCst);
            // Auto-repeat sends more key-downs while the key is held; only the first one begins.
            if active == 0 {
                ACTIVE_KEY.store(key, Ordering::SeqCst);
                return (true, begin(event));
            }
            if active != key {
                OTHER_KEY_PRESSED.store(true, Ordering::SeqCst);
            }
            return (true, None);
        }
        if is_up && swallowed.swap(false, Ordering::SeqCst) {
            if active == key {
                ACTIVE_KEY.store(0, Ordering::SeqCst);
                return (true, finish(key));
            }
            return (true, None);
        }
        return (false, None);
    }

    if is_down && active != 0 {
        swallowed.store(true, Ordering::SeqCst);
        OTHER_KEY_PRESSED.store(true, Ordering::SeqCst);
        let cfg = config::current();
        if vk == VK_ESCAPE.0 as u32 && cfg.as_ref().is_none_or(|cfg| cfg.escape_cancels) {
            cancel();
        } else if let Some(ch) = keys::to_char(vk) {
            let typed = push_input(ch);
            if cfg.is_some_and(|cfg| cfg.instant && cfg.is_unique_match(&typed, active)) {
                take_input();
                return (true, Some(Effect::Launch(typed, active)));
            }
            overlay::show(&typed.to_lowercase());
        }
        return (true, None);
    }
    (is_up && swallowed.swap(false, Ordering::SeqCst), None)
}

unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 {
        let event = unsafe { read_unaligned(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if event.dwExtraInfo != INJECTED_TAG {
            let (swallow, effect) = handle_key(&event, wparam.0 as u32);
            match effect {
                Some(Effect::Send(tap)) => input::send(tap),
                Some(Effect::Launch(com, key)) => launcher::run(com, key),
                None => {}
            }
            if swallow {
                return LRESULT(1);
            }
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    const APPS: u32 = 0x5D;
    const F23: u32 = 0x86;
    const SHIFT: u32 = 0xA0;
    const ESC: u32 = 0x1B;

    fn down(key: char) -> (bool, Option<Effect>) {
        down_vk(key as u32)
    }

    fn down_vk(vk: u32) -> (bool, Option<Effect>) {
        handle_key(&KBDLLHOOKSTRUCT { vkCode: vk, ..Default::default() }, WM_KEYDOWN)
    }

    fn up_vk(vk: u32) -> (bool, Option<Effect>) {
        handle_key(&KBDLLHOOKSTRUCT { vkCode: vk, ..Default::default() }, WM_KEYUP)
    }

    fn type_name(name: &str) {
        for ch in name.chars() {
            assert_eq!(down(ch), (true, None));
            assert_eq!(up_vk(ch as u32), (true, None));
        }
    }

    fn use_config(text: &str) {
        let cfg: Config = toml::from_str(text).unwrap();
        config::apply(cfg);
    }

    const SWALLOW: (bool, Option<Effect>) = (true, None);
    const PASS: (bool, Option<Effect>) = (false, None);

    fn launch(name: &str, key: u32) -> (bool, Option<Effect>) {
        (true, Some(Effect::Launch(name.to_string(), key)))
    }

    fn tap(vk: u16) -> (bool, Option<Effect>) {
        (true, Some(Effect::Send(KeyTap { vk, scan: 0, extended: false })))
    }

    // One test, because the hook keeps its state in globals.
    #[test]
    fn hook_decisions() {
        use_config(
            "activation_key = ['Apps', 'F23']\n\
             commands = [{name = 'txt', path = 'x'}, {name = 'ab', path = 'x'}, {name = 'abc', path = 'x'}]",
        );

        // Typing a name launches it on release.
        assert_eq!(down_vk(APPS), SWALLOW);
        type_name("TXT");
        assert_eq!(up_vk(APPS), launch("TXT", APPS));

        // A tap sends the key on, and auto-repeat doesn't start a new session.
        assert_eq!(down_vk(APPS), SWALLOW);
        assert_eq!(down_vk(APPS), SWALLOW);
        assert_eq!(up_vk(APPS), tap(APPS as u16));

        // The same works with Alt held, which sends the SYS variants.
        let event = KBDLLHOOKSTRUCT { vkCode: APPS, ..Default::default() };
        assert_eq!(handle_key(&event, WM_SYSKEYDOWN), SWALLOW);
        assert_eq!(handle_key(&event, WM_SYSKEYUP), tap(APPS as u16));

        // Escape clears the name, and releasing is then neither a launch nor a tap.
        assert_eq!(down_vk(APPS), SWALLOW);
        type_name("TX");
        assert_eq!(down_vk(ESC), SWALLOW);
        assert_eq!(up_vk(ESC), SWALLOW);
        assert_eq!(up_vk(APPS), SWALLOW);

        // A key held before the activation key keeps working.
        assert_eq!(down_vk(SHIFT), PASS);
        assert_eq!(down_vk(APPS), SWALLOW);
        assert_eq!(up_vk(SHIFT), PASS);
        assert_eq!(up_vk(APPS), tap(APPS as u16));

        // The second activation key launches with its own key, and pressing both is not a tap.
        assert_eq!(down_vk(F23), SWALLOW);
        type_name("AB");
        assert_eq!(up_vk(F23), launch("AB", F23));
        assert_eq!(down_vk(APPS), SWALLOW);
        assert_eq!(down_vk(F23), SWALLOW);
        assert_eq!(up_vk(F23), SWALLOW);
        assert_eq!(up_vk(APPS), SWALLOW);

        // Keys pass through again once nothing is held.
        assert_eq!(down('T'), PASS);
        assert_eq!(up_vk('T' as u32), PASS);

        // Instant launch fires on the key that makes the name unique, then waits for release.
        use_config(
            "activation_key = 'Apps'\ninstant = true\ntap_passthrough = false\n\
             commands = [{name = 'txt', path = 'x'}, {name = 'ab', path = 'x'}, {name = 'abc', path = 'x'}]",
        );
        assert_eq!(down_vk(APPS), SWALLOW);
        type_name("TX");
        assert_eq!(down('T'), launch("TXT", APPS));
        assert_eq!(up_vk('T' as u32), SWALLOW);
        type_name("AB");
        assert_eq!(down('C'), launch("ABC", APPS));
        assert_eq!(up_vk('C' as u32), SWALLOW);
        assert_eq!(up_vk(APPS), SWALLOW);

        // With tap_passthrough off, a tap does nothing.
        assert_eq!(down_vk(APPS), SWALLOW);
        assert_eq!(up_vk(APPS), SWALLOW);
    }
}
