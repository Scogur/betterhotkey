use std::{
    ptr::read_unaligned,
    sync::{Mutex, atomic::{AtomicBool, AtomicU32, Ordering}},
};
use windows::{
    core::Result,
    Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleA,
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};
use crate::{config, keys, launcher, overlay};

// Unassigned key, tapped while Win or Alt is held so releasing them later doesn't open the
// Start menu or a menu bar (the hook swallows the keys Windows would otherwise see in between).
const MASK_KEY: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);
// Marks input this program injects, so the hook lets it through.
const INJECTED_TAG: usize = 0x6268_6B00;

static ACTIVATION_KEY: AtomicU32 = AtomicU32::new(0x5D);
static ACTIVE: AtomicBool = AtomicBool::new(false);
// Keys whose key-down was swallowed, so only their matching key-up gets swallowed too.
static SWALLOWED: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
static INPUTSTR: Mutex<String> = Mutex::new(String::new());

pub fn set_activation_key(vk: u32) {
    ACTIVATION_KEY.store(vk, Ordering::SeqCst);
}

pub fn install() -> Result<HHOOK> {
    unsafe {
        let h_module = GetModuleHandleA(None)?;
        SetWindowsHookExA(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), Some(HINSTANCE(h_module.0)), 0)
    }
}

fn input_str() -> String {
    INPUTSTR.lock().map(|s| s.to_lowercase()).unwrap_or_default()
}

fn push_input(ch: char) {
    if let Ok(mut s) = INPUTSTR.lock() {
        s.push(ch);
    }
}

fn take_input() -> String {
    INPUTSTR.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default()
}

fn is_held(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

fn is_activation(vk: u32, is_down: bool) -> bool {
    match ACTIVATION_KEY.load(Ordering::SeqCst) {
        // The Win and Shift parts of Win+Shift+F23 arrive first and are let through.
        keys::COPILOT => {
            vk == keys::VK_LAUNCH_APP1
                || (vk == keys::VK_F23
                    && (!is_down || ((is_held(VK_LWIN) || is_held(VK_RWIN)) && is_held(VK_SHIFT))))
        }
        key => vk == key,
    }
}

fn send_mask_key() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: MASK_KEY, dwFlags: flags, dwExtraInfo: INJECTED_TAG, ..Default::default() },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

fn begin() {
    take_input();
    if is_held(VK_LWIN) || is_held(VK_RWIN) || is_held(VK_MENU) {
        send_mask_key();
    }
    overlay::show("");
}

fn finish() {
    let com = take_input();
    if com.is_empty() {
        overlay::hide();
    } else {
        launcher::run(com);
    }
}

// Keys stay swallowed until the activation key is released, so letters typed afterwards start
// a new name instead of reaching Windows (as Win+Shift shortcuts, with the Copilot key).
fn cancel() {
    take_input();
    overlay::show("");
}

// Returns true when the key event should be swallowed.
fn handle_key(vk: u32, event_type: u32) -> bool {
    // The SYS variants are sent instead while Alt is held.
    let is_down = event_type == WM_KEYDOWN || event_type == WM_SYSKEYDOWN;
    let is_up = event_type == WM_KEYUP || event_type == WM_SYSKEYUP;
    let swallowed = &SWALLOWED[(vk & 0xFF) as usize];

    if is_activation(vk, is_down) {
        if is_down {
            swallowed.store(true, Ordering::SeqCst);
            // Auto-repeat sends more key-downs while the key is held; only the first one begins.
            if !ACTIVE.swap(true, Ordering::SeqCst) {
                begin();
            }
            return true;
        }
        if is_up && swallowed.swap(false, Ordering::SeqCst) {
            if ACTIVE.swap(false, Ordering::SeqCst) {
                finish();
            }
            return true;
        }
        return false;
    }

    if is_down && ACTIVE.load(Ordering::SeqCst) {
        swallowed.store(true, Ordering::SeqCst);
        let escape_cancels = config::current().is_none_or(|cfg| cfg.escape_cancels);
        if vk == VK_ESCAPE.0 as u32 && escape_cancels {
            cancel();
        } else if let Some(ch) = keys::to_char(vk) {
            push_input(ch);
            overlay::show(&input_str());
        }
        return true;
    }
    is_up && swallowed.swap(false, Ordering::SeqCst)
}

unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 {
        let hook_struct = unsafe { read_unaligned(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if hook_struct.dwExtraInfo != INJECTED_TAG && handle_key(hook_struct.vkCode, wparam.0 as u32) {
            return LRESULT(1);
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
