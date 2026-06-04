# betterhotkey

**betterhotkey** is a lightweight program that helps you make use of otherwise useless keys on your keyboard.

Ever wondered why modern keyboards have a **Copilot** key that almost nobody uses? Now you can turn it into something useful.

For example, hold the activation key, type `txt`, and `notepad.exe` launches instantly. Simple and effective.

## Installation

To get started, place the following files in the same directory:

* `betterhotkey.exe`
* `cfg.toml`

> `cfg.toml` is created automatically the first time you run `betterhotkey.exe` if none is found.

## Changing the Activation Key

1. Find the virtual key code for the key you want to use [here](https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes).

2. Open `cfg.toml`.

3. Change the value of `activation_key` to the desired virtual key code.

## Adding New Hotkeys

Add entries to the hotkeys list using the following format:

```toml
{name = 'clc', path = 'calc.exe'}
```

### Fields

* `name` — The key sequence you type while holding the activation key. Matching is case-insensitive.
* `path` — The executable to launch or the command to execute.

Examples:

```toml
{name = 'clc', path = 'calc.exe'}
{name = 'txt', path = 'notepad.exe'}
{name = 'git', path = 'git status'}
```

### Notes

* If specifying a file path, avoid using backslashes (`\`) or use `\\` instead.

## Applying Changes

Changes to `cfg.toml` are loaded only when **betterhotkey** starts.

After modifying the configuration, restart the program:

1. Open **Task Manager**.
2. End the `betterhotkey.exe` process.
3. Launch `betterhotkey.exe` again.

That's it — your new hotkeys are ready to use.
