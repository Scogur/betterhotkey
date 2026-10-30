use std::{
    env,
    ffi::OsString,
    io,
    os::windows::process::CommandExt,
    path::Path,
    process::Command,
    thread,
};
use windows::{
    core::{HSTRING, PCWSTR, w},
    Win32::{
        System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx},
        UI::{Shell::*, WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL}},
    },
};
use crate::{clipboard, config::Comm, config, input, log, overlay, window};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// Runs off the hook thread: Windows drops low-level hooks that take too long to return.
pub fn run(com: String, key: u32) {
    thread::spawn(move || {
        let name = com.to_lowercase();
        let found = config::current().and_then(|cfg| cfg.search(&com, key).cloned());
        let Some(command) = found else {
            overlay::flash(&format!("{name}: no match"));
            log::write(format!("{name}: no match"));
            return;
        };
        overlay::hide();
        let clip = if command.uses_clipboard() { clipboard::text() } else { String::new() };

        if let Some(text) = &command.text {
            log::write(format!("{name}: typing text"));
            if !input::type_text(&fill(text, &clip)) {
                log::write(format!("{name}: didn't type the text because Shift, Ctrl, Alt or Win stayed held"));
            }
            return;
        }
        let dir = command.dir.as_deref().map(|dir| fill(dir, &clip));
        for entry in &command.path.0 {
            launch(&name, entry, &command, dir.as_deref(), &clip);
        }
    });
}

fn launch(name: &str, entry: &str, command: &Comm, dir: Option<&str>, clip: &str) {
    if command.focus
        && let Some(exe) = program_name(entry)
        && window::focus(&exe)
    {
        log::write(format!("{name}: focused {exe}"));
        return;
    }
    log::write(format!("{name}: {entry}"));
    let result = if command.admin {
        run_as_admin(entry, dir).map_err(io::Error::from)
    } else {
        spawn(name, entry, dir, clip)
    };
    if let Err(err) = result {
        log::write(format!("{name}: could not start {entry}: {err}"));
    }
}

fn spawn(name: &str, entry: &str, dir: Option<&str>, clip: &str) -> io::Result<()> {
    let mut cmd = command_for(entry, clip);
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn()?;
    // cmd's exit code is the only sign that a command line failed.
    if !is_file(entry) && log::enabled() {
        let name = name.to_string();
        thread::spawn(move || {
            if let Ok(status) = child.wait()
                && !status.success()
            {
                log::write(format!("{name}: {status}"));
            }
        });
    }
    Ok(())
}

// Puts the copied text into a value that isn't run through cmd, like typed text or a folder.
fn fill(template: &str, clip: &str) -> String {
    template
        .replace("{clipboard:url}", &clipboard::url_encode(clip))
        .replace("{clipboard}", clip)
}

fn is_file(path: &str) -> bool {
    Path::new(path).is_file()
}

fn shell() -> OsString {
    env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into())
}

// An existing file is launched directly; anything else runs as a cmd command line.
pub fn command_for(path: &str, clip: &str) -> Command {
    if is_file(path) {
        return Command::new(path);
    }
    let mut cmd = Command::new(shell());
    let mut line = fix_program_slashes(path);
    let mut switches = "";
    // The copied text goes in through variables that cmd only expands after it has parsed the
    // line (/V:ON), so characters like & or | in it can't run other commands.
    if line.contains("{clipboard") {
        cmd.env("BHK_CLIPBOARD", clip).env("BHK_CLIPBOARD_URL", clipboard::url_encode(clip));
        line = line
            .replace("{clipboard:url}", "!BHK_CLIPBOARD_URL!")
            .replace("{clipboard}", "!BHK_CLIPBOARD!");
        switches = "/V:ON ";
    }
    cmd.raw_arg(format!("{switches}/S /C \"{line}\"")).creation_flags(CREATE_NO_WINDOW);
    cmd
}

fn run_as_admin(entry: &str, dir: Option<&str>) -> windows::core::Result<()> {
    // ShellExecuteEx may use COM, which has to be set up on this thread first.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let (file, params, show) = if is_file(entry) {
        (HSTRING::from(entry), HSTRING::new(), SW_SHOWNORMAL)
    } else {
        // An elevated cmd starts in System32, so change folders inside the command line.
        let line = fix_program_slashes(entry);
        let line = match dir {
            Some(dir) => format!("cd /d \"{dir}\" && {line}"),
            None => line,
        };
        (HSTRING::from(shell().as_os_str()), HSTRING::from(format!("/S /C \"{line}\"")), SW_HIDE)
    };
    let dir = dir.map(HSTRING::from);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        lpDirectory: dir.as_ref().map_or(PCWSTR::null(), |dir| PCWSTR(dir.as_ptr())),
        nShow: show.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }
}

// Splits a command line into the program and the rest, keeping quotes around the program.
fn split_program(line: &str) -> (&str, &str) {
    let line = line.trim_start();
    let end = match line.strip_prefix('"') {
        Some(rest) => rest.find('"').map_or(line.len(), |i| i + 2),
        None => line.find(char::is_whitespace).unwrap_or(line.len()),
    };
    line.split_at(end)
}

// cmd can't run a program written with forward slashes, so convert them in the program part only.
fn fix_program_slashes(line: &str) -> String {
    let (program, args) = split_program(line);
    format!("{}{args}", program.replace('/', "\\"))
}

// The file name of the program a path starts, like "code.exe", used to find its windows.
fn program_name(entry: &str) -> Option<String> {
    let program = if is_file(entry) { entry } else { split_program(entry).0.trim_matches('"') };
    let name = Path::new(program).file_name()?.to_string_lossy().to_ascii_lowercase();
    Some(if Path::new(&name).extension().is_some() { name } else { format!("{name}.exe") })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdout_of(path: &str, clip: &str) -> String {
        let out = command_for(path, clip).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn command_with_arguments_runs() {
        assert_eq!(stdout_of("echo hello world", ""), "hello world");
    }

    #[test]
    fn quoted_program_path_with_arguments_runs() {
        assert_eq!(stdout_of(r#""C:/Windows/System32/cmd.exe" /C echo "a b""#, ""), "\"a b\"");
        assert_eq!(stdout_of("C:/Windows/System32/cmd.exe /C echo a/b", ""), "a/b");
    }

    #[test]
    fn existing_file_is_launched_directly() {
        assert_eq!(command_for("C:/Windows/System32/whoami.exe", "").get_program(), "C:/Windows/System32/whoami.exe");
    }

    #[test]
    fn copied_text_cant_run_commands() {
        let clip = r#"a & echo INJECTED | "q" %PATH% ^x"#;
        assert_eq!(stdout_of("echo [{clipboard}]", clip), format!("[{clip}]"));
        assert_eq!(stdout_of("echo {clipboard:url}", "a b&c"), "a%20b%26c");
    }

    #[test]
    fn fill_puts_copied_text_in_plain_values() {
        assert_eq!(fill("q={clipboard:url} raw={clipboard}", "a b"), "q=a%20b raw=a b");
    }

    #[test]
    fn program_name_finds_the_exe() {
        assert_eq!(program_name("C:/Windows/System32/notepad.exe").as_deref(), Some("notepad.exe"));
        assert_eq!(program_name(r#""C:/Program Files/App/App.exe" --x"#).as_deref(), Some("app.exe"));
        assert_eq!(program_name("notepad").as_deref(), Some("notepad.exe"));
        assert_eq!(program_name("Code.exe .").as_deref(), Some("code.exe"));
    }
}
