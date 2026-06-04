#![windows_subsystem = "windows"]

use std::{fs, io::{ErrorKind, Write}, process::Command, ptr::read_unaligned, sync::{Mutex, OnceLock, atomic::{AtomicBool, Ordering, AtomicU32}}};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleA,
        UI::{
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
};
use serde::Deserialize;
// const CHANGED_KEY: u32 = 0x5D;
const CFG_PATH:&str = "cfg.toml";

static CHANGED_KEY: AtomicU32 = AtomicU32::new(0x5d);

static IS_CHANGED_KEY_DOWN: AtomicBool = AtomicBool::new(false);
static INPUTSTR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static CFG: OnceLock<Mutex<Option<Config>>> = OnceLock::new();

fn get_global_input_str() -> Option<String> {
    let mutex = INPUTSTR.get()?;
    let guard = mutex.lock().ok()?;
    guard.clone()
}

fn add_to_global_input_str(ch: char) {
    let mtx = INPUTSTR.get_or_init(|| Mutex::new(Some(String::new())));
    if let Ok(mut guard) = mtx.lock() {
        if let Some(ref mut s) = *guard {
            s.push(ch);
        }
        else {
            *guard = Some(ch.to_string());
        }
    }
}

fn clear_global_input_str() {
    if let Some(mtx) = INPUTSTR.get() {
        if let Ok(mut guard) = mtx.lock() {
            *guard = Some(String::new());
        }
    }
}

fn update_cfg(){
    let cfg = parse_cfg();
    CHANGED_KEY.store(cfg.activation_key, Ordering::SeqCst);
    let mtx = CFG.get_or_init(|| Mutex::new(Some(cfg.clone())));
    if let Ok(mut guard) = mtx.lock() {
        *guard = Some(cfg);
    }
}

fn get_cfg() -> Option<Config>{
    if let Some(mtx) = CFG.get() {
        if let Ok(mut guard) = mtx.lock() {
            return guard.clone();
        }
        else {panic!("get_cfg mtx.lock error");}
    }
    else {panic!("get_cfg error");}
}

fn call_app() {
    let stropt = get_global_input_str();
    let mut com = String::new();
    match stropt {
        Some(val) => {if(!val.is_empty()){com = val} else {return;}},
        None => {return;}
    }
    println!("calling {}", com);
    let Some(path) = find_app_path(com) else {return;};
    let _ = Command::new(path).spawn();
}

fn find_app_path(com:String) -> Option<String>{
    let cfg = get_cfg().unwrap();
    cfg.search(com)
}


unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && code == HC_ACTION as i32 {
        let hook_struct = read_unaligned(lparam.0 as *const KBDLLHOOKSTRUCT);
        let event_type = wparam.0 as u32;
        let mut inputstr = String::new();
        if hook_struct.vkCode == CHANGED_KEY.load(Ordering::SeqCst) {
            if event_type == WM_KEYDOWN {
                IS_CHANGED_KEY_DOWN.store(true, Ordering::SeqCst);
            }
            if event_type == WM_KEYUP {
                IS_CHANGED_KEY_DOWN.store(false, Ordering::SeqCst);
                call_app();
                clear_global_input_str();
            }
            
            return LRESULT(1);
        }

        if IS_CHANGED_KEY_DOWN.load(Ordering::SeqCst){
            if event_type == WM_KEYDOWN || event_type == WM_SYSKEYDOWN {
                let vk = hook_struct.vkCode;

                if (0x41..=0x5A).contains(&vk) {
                    let letter = vk as u8 as char;
                    println!("{}", letter);
                    add_to_global_input_str(letter);
                }
                else {
                    println!("{}", vk);
                }

            }
            return LRESULT(1);
        }
    }
    
    CallNextHookEx(None, code, wparam, lparam)
}


#[derive(Deserialize, Debug, Clone)]
struct Comm{
    name: String,
    path:String,
}

#[derive(Deserialize, Debug, Clone)]
struct Config{
    commands: Vec<Comm>,
    activation_key: u32,
}

impl Config {
    fn search(&self, key: String) -> Option<String> {
        for com in &self.commands {
            if com.name.to_uppercase() == key.to_uppercase() {
                return Some(com.path.clone());
            }
        }
        None
    }
}

fn parse_cfg() -> Config {
    let s = match fs::read_to_string(CFG_PATH) {
        Ok(val) => {val},
        Err(err) => match err.kind(){
            ErrorKind::NotFound => {
                let mut file = fs::File::create(CFG_PATH).unwrap();
                file.write_all((
                    r#"
                activation_key = 0x5D

                commands = [
                    {name = 'clc', path = 'calc.exe'},
                    {name = 'txt', path = 'notepad.exe'},
                ]
                    "#).as_bytes()).unwrap();
                fs::read_to_string(CFG_PATH).unwrap()
            },
            ErrorKind::PermissionDenied => {panic!("PermissionDenied")},
            _ => {panic!("parce_cfg error")},
        },
    };
    toml::from_str(&s).unwrap()
}

fn main() -> Result<()> {
    unsafe {
        let h_module = GetModuleHandleA(None)?;
        let h_instance = Some(HINSTANCE(h_module.0));
        
        let hook = SetWindowsHookExA(
            WH_KEYBOARD_LL,
            Some(low_level_keyboard_proc),
            h_instance,
            0,
        )?;

        update_cfg();

        println!("Hook activated.");
        let mut msg = MSG::default();
        while GetMessageA(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
        UnhookWindowsHookEx(hook)?;
    }
    Ok(())
}