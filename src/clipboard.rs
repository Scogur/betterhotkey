use std::{thread, time::Duration};
use windows::Win32::{
    Foundation::HGLOBAL,
    System::{DataExchange::*, Memory::*},
};

const CF_UNICODETEXT: u32 = 13;

// The copied text without surrounding spaces and line breaks, or "" when nothing text-like is copied.
pub fn text() -> String {
    // Another program may have the clipboard open for a moment.
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            let text = unsafe { read() };
            let _ = unsafe { CloseClipboard() };
            return text.trim().to_string();
        }
        thread::sleep(Duration::from_millis(20));
    }
    String::new()
}

unsafe fn read() -> String {
    unsafe {
        let Ok(handle) = GetClipboardData(CF_UNICODETEXT) else { return String::new(); };
        let mem = HGLOBAL(handle.0);
        let ptr = GlobalLock(mem) as *const u16;
        if ptr.is_null() {
            return String::new();
        }
        let units = std::slice::from_raw_parts(ptr, GlobalSize(mem) / 2);
        let len = units.iter().position(|&unit| unit == 0).unwrap_or(units.len());
        let text = String::from_utf16_lossy(&units[..len]);
        let _ = GlobalUnlock(mem);
        text
    }
}

// Percent-encodes everything but letters, digits and -_.~, so the text is safe in a web address.
pub fn url_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encode_escapes_everything_but_safe_characters() {
        assert_eq!(url_encode("rust lang"), "rust%20lang");
        assert_eq!(url_encode("a&b=c?d/e"), "a%26b%3Dc%3Fd%2Fe");
        assert_eq!(url_encode("Az09-_.~"), "Az09-_.~");
        assert_eq!(url_encode("я"), "%D1%8F");
    }
}
