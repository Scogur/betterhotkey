// Finding an open window of a program and bringing it to the front.
use std::path::Path;
use windows::{
    core::{BOOL, PWSTR},
    Win32::{
        Foundation::*,
        Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        System::Threading::*,
        UI::WindowsAndMessaging::*,
    },
};
use crate::input;

// Brings the frontmost window of the running program named `exe` (like "code.exe") to the front.
// Returns false when that program has no window open.
pub fn focus(exe: &str) -> bool {
    let Some(hwnd) = find(exe) else { return false; };
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // Windows only lets the program that sent the last input change the foreground window.
        input::send(input::MASK_KEY);
        let _ = SetForegroundWindow(hwnd);
    }
    true
}

struct Search {
    exe: String,
    found: Option<HWND>,
}

fn find(exe: &str) -> Option<HWND> {
    let mut search = Search { exe: exe.to_ascii_lowercase(), found: None };
    // Windows are listed from front to back, so the first match is the most recently used one.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
    search.found
}

unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = unsafe { &mut *(lparam.0 as *mut Search) };
    if is_app_window(hwnd) && exe_name(hwnd).is_some_and(|name| name == search.exe) {
        search.found = Some(hwnd);
        return false.into();
    }
    true.into()
}

// A window that shows up in Alt+Tab, roughly.
fn is_app_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || GetWindow(hwnd, GW_OWNER).is_ok() {
            return false;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        // Windows on other virtual desktops and suspended Store apps are cloaked.
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as _, size_of::<u32>() as u32);
        cloaked == 0
    }
}

fn exe_name(hwnd: HWND) -> Option<String> {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let result = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) };
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    Path::new(&path).file_name().map(|name| name.to_string_lossy().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Needs a desktop session, so it doesn't run on CI: cargo test -- --ignored
    #[test]
    #[ignore]
    fn finds_explorer_window() {
        assert!(find("explorer.exe").is_some());
        assert!(find("no-such-program.exe").is_none());
    }
}
