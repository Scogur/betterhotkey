// Key names accepted in cfg.toml, mapped to Windows virtual key codes.

// Not a real virtual key code: stands for the Copilot key, which most keyboards send
// as Win+Shift+F23 and some as VK_LAUNCH_APP1.
pub const COPILOT: u32 = 0x1_0000;
pub const VK_F23: u32 = 0x86;
pub const VK_LAUNCH_APP1: u32 = 0xB6;

const NAMED: &[(&str, u32)] = &[
    ("copilot", COPILOT),
    ("apps", 0x5D),
    ("menu", 0x5D),
    ("contextmenu", 0x5D),
    ("capslock", 0x14),
    ("numlock", 0x90),
    ("scrolllock", 0x91),
    ("pause", 0x13),
    ("printscreen", 0x2C),
    ("insert", 0x2D),
    ("delete", 0x2E),
    ("home", 0x24),
    ("end", 0x23),
    ("pageup", 0x21),
    ("pagedown", 0x22),
    ("left", 0x25),
    ("up", 0x26),
    ("right", 0x27),
    ("down", 0x28),
    ("tab", 0x09),
    ("space", 0x20),
    ("enter", 0x0D),
    ("backspace", 0x08),
    ("lwin", 0x5B),
    ("rwin", 0x5C),
    ("lshift", 0xA0),
    ("rshift", 0xA1),
    ("lctrl", 0xA2),
    ("rctrl", 0xA3),
    ("lalt", 0xA4),
    ("ralt", 0xA5),
    ("browserback", 0xA6),
    ("browserforward", 0xA7),
    ("browserhome", 0xAC),
    ("volumemute", 0xAD),
    ("volumedown", 0xAE),
    ("volumeup", 0xAF),
    ("medianext", 0xB0),
    ("mediaprev", 0xB1),
    ("mediastop", 0xB2),
    ("mediaplaypause", 0xB3),
    ("launchmail", 0xB4),
    ("launchmedia", 0xB5),
    ("launchapp1", 0xB6),
    ("launchapp2", 0xB7),
];

// Accepts names like "Apps", "F23", "Page Up", "R_Ctrl" (case, spaces, `_` and `-` don't matter)
// and hex codes like "0x5D".
pub fn parse(name: &str) -> Option<u32> {
    let name: String = name
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .collect::<String>()
        .to_ascii_lowercase();
    if let Some(hex) = name.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).ok();
    }
    if let Some(&(_, vk)) = NAMED.iter().find(|(n, _)| *n == name) {
        return Some(vk);
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u32>().ok())
        && (1..=24).contains(&n)
    {
        return Some(0x6F + n);
    }
    match name.as_bytes() {
        [c @ (b'a'..=b'z' | b'0'..=b'9')] => Some(c.to_ascii_uppercase() as u32),
        _ => None,
    }
}

// The character a key adds to the typed command name, if any.
pub fn to_char(vk: u32) -> Option<char> {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => Some(vk as u8 as char),
        0x60..=0x69 => Some((b'0' + (vk - 0x60) as u8) as char),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_names() {
        assert_eq!(parse("Apps"), Some(0x5D));
        assert_eq!(parse("copilot"), Some(COPILOT));
        assert_eq!(parse("F23"), Some(0x86));
        assert_eq!(parse("f1"), Some(0x70));
        assert_eq!(parse("Page Up"), Some(0x21));
        assert_eq!(parse("R_Ctrl"), Some(0xA3));
        assert_eq!(parse("0x5d"), Some(0x5D));
        assert_eq!(parse("f"), Some(0x46));
        assert_eq!(parse("7"), Some(0x37));
        assert_eq!(parse("F25"), None);
        assert_eq!(parse("nope"), None);
    }

    #[test]
    fn maps_letters_and_digits_to_chars() {
        assert_eq!(to_char(0x41), Some('A'));
        assert_eq!(to_char(0x35), Some('5'));
        assert_eq!(to_char(0x65), Some('5'));
        assert_eq!(to_char(0x20), None);
    }
}
