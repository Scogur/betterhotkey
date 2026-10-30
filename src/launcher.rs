use std::{env, os::windows::process::CommandExt, path::Path, process::Command, thread};
use crate::{config, log, overlay};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// Runs off the hook thread: Windows drops low-level hooks that take too long to return.
pub fn run(com: String) {
    thread::spawn(move || {
        let name = com.to_lowercase();
        let path = config::current().and_then(|cfg| cfg.search(&com).map(str::to_owned));
        let Some(path) = path else {
            overlay::flash(&format!("{name}: no match"));
            log::write(format!("{name}: no match"));
            return;
        };
        overlay::hide();
        log::write(format!("{name}: {path}"));
        let via_shell = !is_file(&path);
        match command_for(&path).spawn() {
            // cmd's exit code is the only sign that a command line failed.
            Ok(mut child) if via_shell && log::enabled() => {
                if let Ok(status) = child.wait()
                    && !status.success()
                {
                    log::write(format!("{name}: {status}"));
                }
            }
            Ok(_) => {}
            Err(err) => log::write(format!("{name}: could not start {path}: {err}")),
        }
    });
}

fn is_file(path: &str) -> bool {
    Path::new(path).is_file()
}

// An existing file is launched directly; anything else runs as a cmd command line.
pub fn command_for(path: &str) -> Command {
    if is_file(path) {
        return Command::new(path);
    }
    let shell = env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
    let mut cmd = Command::new(shell);
    cmd.raw_arg(format!("/S /C \"{}\"", fix_program_slashes(path)))
        .creation_flags(CREATE_NO_WINDOW);
    cmd
}

// cmd can't run a program written with forward slashes, so convert them in the program part only.
fn fix_program_slashes(line: &str) -> String {
    let line = line.trim_start();
    let end = match line.strip_prefix('"') {
        Some(rest) => rest.find('"').map_or(line.len(), |i| i + 2),
        None => line.find(char::is_whitespace).unwrap_or(line.len()),
    };
    let (program, args) = line.split_at(end);
    format!("{}{args}", program.replace('/', "\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdout_of(path: &str) -> String {
        let out = command_for(path).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn command_with_arguments_runs() {
        assert_eq!(stdout_of("echo hello world"), "hello world");
    }

    #[test]
    fn quoted_program_path_with_arguments_runs() {
        assert_eq!(stdout_of(r#""C:/Windows/System32/cmd.exe" /C echo "a b""#), "\"a b\"");
        assert_eq!(stdout_of("C:/Windows/System32/cmd.exe /C echo a/b"), "a/b");
    }

    #[test]
    fn existing_file_is_launched_directly() {
        assert_eq!(command_for("C:/Windows/System32/whoami.exe").get_program(), "C:/Windows/System32/whoami.exe");
    }
}
