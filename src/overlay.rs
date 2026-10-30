// A small click-through box near the bottom of the screen that shows what is being typed.
use std::{
    cell::Cell,
    sync::{Mutex, atomic::{AtomicIsize, Ordering}},
};
use windows::{
    core::{Result, w},
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};
use crate::config;

const WM_OVERLAY_SHOW: u32 = WM_APP + 1;
const WM_OVERLAY_HIDE: u32 = WM_APP + 2;
const HIDE_TIMER: usize = 1;
const FLASH_MS: u32 = 900;
const BACKGROUND: COLORREF = COLORREF(0x0020_2020);
const FOREGROUND: COLORREF = COLORREF(0x00F0_F0F0);

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static TEXT: Mutex<String> = Mutex::new(String::new());

thread_local! {
    // Font for the DPI it was made for; only used on the window's thread.
    static FONT: Cell<(u32, HFONT)> = Cell::new((0, HFONT::default()));
}

pub fn init() -> Result<()> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class_name = w!("betterhotkey_overlay");
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            class_name,
            w!(""),
            WS_POPUP,
            0, 0, 0, 0,
            None,
            None,
            Some(instance),
            None,
        )?;
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 235, LWA_ALPHA)?;
        // Rounded corners on Windows 11; ignored elsewhere.
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as _,
            size_of_val(&corner) as u32,
        );
        WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);
    }
    Ok(())
}

fn enabled() -> bool {
    config::current().is_some_and(|cfg| cfg.overlay)
}

fn post(msg: u32, wparam: usize) {
    let raw = WINDOW.load(Ordering::SeqCst);
    if raw != 0 {
        let _ = unsafe { PostMessageW(Some(HWND(raw as _)), msg, WPARAM(wparam), LPARAM(0)) };
    }
}

fn set_text(text: &str) {
    if let Ok(mut current) = TEXT.lock() {
        *current = text.to_string();
    }
}

// Shows `text` until hide() is called.
pub fn show(text: &str) {
    if enabled() {
        set_text(text);
        post(WM_OVERLAY_SHOW, 0);
    }
}

// Shows `text` briefly.
pub fn flash(text: &str) {
    if enabled() {
        set_text(text);
        post(WM_OVERLAY_SHOW, 1);
    } else {
        hide();
    }
}

pub fn hide() {
    post(WM_OVERLAY_HIDE, 0);
}

fn display_text() -> Vec<u16> {
    let text = TEXT.lock().map(|t| t.clone()).unwrap_or_default();
    let text = if text.is_empty() { "…".to_string() } else { text };
    text.encode_utf16().collect()
}

fn scale(px: i32, dpi: u32) -> i32 {
    px * dpi as i32 / 96
}

fn font_for(dpi: u32) -> HFONT {
    FONT.with(|cell| {
        let (cached_dpi, font) = cell.get();
        if cached_dpi == dpi {
            return font;
        }
        unsafe {
            if !font.is_invalid() {
                let _ = DeleteObject(font.into());
            }
            let font = CreateFontW(
                -scale(26, dpi), 0, 0, 0,
                FW_SEMIBOLD.0 as i32,
                0, 0, 0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                w!("Segoe UI"),
            );
            cell.set((dpi, font));
            font
        }
    })
}

// Sizes the window to the text and puts it at the bottom center of the monitor with the cursor.
unsafe fn place(hwnd: HWND) {
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dpi, mut _dpi_y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _dpi_y);

        let font = font_for(dpi);
        let hdc = GetDC(Some(hwnd));
        let old = SelectObject(hdc, font.into());
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, &display_text(), &mut size);
        SelectObject(hdc, old);
        ReleaseDC(Some(hwnd), hdc);

        let width = size.cx + 2 * scale(24, dpi);
        let height = size.cy + 2 * scale(10, dpi);
        let work = info.rcWork;
        let x = work.left + (work.right - work.left - width) / 2;
        let y = work.bottom - height - scale(96, dpi);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

unsafe fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        let brush = CreateSolidBrush(BACKGROUND);
        FillRect(hdc, &rect, brush);
        let _ = DeleteObject(brush.into());

        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, FOREGROUND);
        let old = SelectObject(hdc, FONT.with(|cell| cell.get().1).into());
        DrawTextW(hdc, &mut display_text(), &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        SelectObject(hdc, old);
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_OVERLAY_SHOW => {
                let _ = KillTimer(Some(hwnd), HIDE_TIMER);
                place(hwnd);
                if wparam.0 == 1 {
                    SetTimer(Some(hwnd), HIDE_TIMER, FLASH_MS, None);
                }
                LRESULT(0)
            }
            WM_OVERLAY_HIDE | WM_TIMER => {
                let _ = KillTimer(Some(hwnd), HIDE_TIMER);
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(0)
            }
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
