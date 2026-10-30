# betterhotkey

**betterhotkey** is a lightweight program that helps you make use of otherwise useless keys on your keyboard.

Ever wondered why modern keyboards have a **Copilot** key that almost nobody uses? Now you can turn it into something useful.

For example, hold the activation key, type `txt`, and `notepad.exe` launches instantly. Simple and effective.

## Installation

To get started, place the following files in the same directory:

* `betterhotkey.exe`
* `cfg.toml`

> `cfg.toml` is created automatically next to `betterhotkey.exe` the first time you run it if none is found. If `cfg.toml` can't be read or contains an error, a message box explains what went wrong.

## Changing the Activation Key

1. Find the virtual key code for the key you want to use [here](https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes).

2. Open `cfg.toml`.

3. Change the value of `activation_key` to the desired virtual key code.

## Adding New Hotkeys

Add entries to the `commands` list using the following format:

```toml
{name = 'clc', path = 'calc.exe'}
```

### Fields

* `name` — The key sequence you type while holding the activation key. Only letters (A–Z) are recorded. Matching is case-insensitive.
* `path` — The executable to launch or the command to execute. A path to an existing file is launched directly; anything else runs as a `cmd` command line, so it can take arguments.

Examples:

```toml
{name = 'clc', path = 'calc.exe'}
{name = 'txt', path = 'notepad.exe'}
{name = 'cde', path = 'C:/Program Files/Microsoft VS Code/Code.exe'}
{name = 'prj', path = '"C:/Program Files/Microsoft VS Code/Code.exe" C:/Projects/app'}
{name = 'git', path = 'start cmd /k git status'}
```

### Notes

* Commands run without a window, so use `start cmd /k ...` (as above) to see the output of a console program.
* If the program path contains spaces and is followed by arguments, wrap the path in double quotes.
* Both `/` and `\` work in paths. In single-quoted strings (`'...'`) a backslash needs no escaping; in double-quoted strings write it as `\\`.

## Applying Changes

Changes to `cfg.toml` are loaded only when **betterhotkey** starts.

After modifying the configuration, restart the program:

1. Open **Task Manager**.
2. End the `betterhotkey.exe` process.
3. Launch `betterhotkey.exe` again.

That's it — your new hotkeys are ready to use.
