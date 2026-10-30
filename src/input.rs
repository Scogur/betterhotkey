// Keyboard input this program injects. It is tagged so the hook lets it through.
use std::{
    thread,
    time::{Duration, Instant},
};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

pub const INJECTED_TAG: usize = 0x6268_6B00;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyTap {
    pub vk: u16,
    pub scan: u16,
    pub extended: bool,
}

// Unassigned key. Tapping it while Win or Alt is held keeps their release from opening the
// Start menu or a menu bar, and it lets this program bring a window to the front.
pub const MASK_KEY: KeyTap = KeyTap { vk: 0xE8, scan: 0, extended: false };

const MODIFIERS: [VIRTUAL_KEY; 5] = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN];

fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                dwExtraInfo: INJECTED_TAG,
                ..Default::default()
            },
        },
    }
}

fn push_tap(inputs: &mut Vec<INPUT>, vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) {
    inputs.push(key_input(vk, scan, flags));
    inputs.push(key_input(vk, scan, flags | KEYEVENTF_KEYUP));
}

fn send_inputs(inputs: &[INPUT]) {
    unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
}

pub fn is_held(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

pub fn send(tap: KeyTap) {
    let flags = if tap.extended { KEYEVENTF_EXTENDEDKEY } else { KEYBD_EVENT_FLAGS(0) };
    let mut inputs = Vec::new();
    push_tap(&mut inputs, tap.vk, tap.scan, flags);
    send_inputs(&inputs);
}

// Types `text` into the focused window. Waits for Shift, Ctrl, Alt and Win to be let go first,
// since they would turn the text into shortcuts, and gives up if they are still held after 5 seconds.
pub fn type_text(text: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while MODIFIERS.iter().any(|&vk| is_held(vk)) {
        if Instant::now() > deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let mut inputs = Vec::new();
    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' => push_tap(&mut inputs, VK_RETURN.0, 0, KEYBD_EVENT_FLAGS(0)),
            '\t' => push_tap(&mut inputs, VK_TAB.0, 0, KEYBD_EVENT_FLAGS(0)),
            _ => {
                for unit in ch.encode_utf16(&mut [0; 2]) {
                    push_tap(&mut inputs, 0, *unit, KEYEVENTF_UNICODE);
                }
            }
        }
    }
    send_inputs(&inputs);
    true
}
