# betterhotkey

**betterhotkey** is a lightweight program that helps you make use of otherwise useless keys on your keyboard.

Ever wondered why modern keyboards have a **Copilot** key that almost nobody uses? Now you can turn it into something useful.

For example, hold the activation key, type `txt`, and `notepad.exe` launches instantly. Simple and effective.

## Installation

To get started, place the following files in the same directory:

* `betterhotkey.exe`
* `cfg.toml`

> `cfg.toml` is created automatically next to `betterhotkey.exe` the first time you run it if none is found. If `cfg.toml` can't be read or contains an error, a message box explains what went wrong.

betterhotkey runs invisibly in the background. To start it every time you sign in, set `autostart = true` (see [Options](#options)).

## Changing the Activation Key

Open `cfg.toml` and set `activation_key` to a key name or a [virtual key code](https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes):

```toml
activation_key = "Copilot"   # the Copilot key
activation_key = "Apps"      # the Menu key (the default)
activation_key = "F23"
activation_key = 0x5D        # a virtual key code also works
```

Key names include `Copilot`, `Apps`/`Menu`, `CapsLock`, `ScrollLock`, `Pause`, `PrintScreen`, `Insert`, `Home`, `End`, `PageUp`, `PageDown`, `F1`–`F24`, `LWin`/`RWin`, `LCtrl`/`RCtrl`, `LAlt`/`RAlt`, `LShift`/`RShift`, `LaunchApp1`, `LaunchApp2` and the media keys (`VolumeUp`, `MediaPlayPause`, …). Case, spaces, `_` and `-` don't matter.

### The Copilot key

Most keyboards send the Copilot key as Win+Shift+F23, and some send it as `LaunchApp1`. `activation_key = "Copilot"` handles both, and keeps the Start menu from opening when you let go of the key.

## Adding New Hotkeys

Add entries to the `commands` list using the following format:

```toml
{name = 'clc', path = 'calc.exe'}
```

### Fields

* `name` — The key sequence you type while holding the activation key. Letters (A–Z) and digits (0–9, including the number pad) are recorded. Matching is case-insensitive.
* `path` — The executable to launch or the command to execute. A path to an existing file is launched directly; anything else runs as a `cmd` command line, so it can take arguments.

Examples:

```toml
{name = 'clc', path = 'calc.exe'}
{name = 'txt', path = 'notepad.exe'}
{name = 'cde', path = 'C:/Program Files/Microsoft VS Code/Code.exe'}
{name = 'prj', path = '"C:/Program Files/Microsoft VS Code/Code.exe" C:/Projects/app'}
{name = 'git', path = 'start cmd /k git status'}
{name = 'yt', path = 'start https://youtube.com'}
```

### Notes

* Commands run without a window, so use `start cmd /k ...` (as above) to see the output of a console program.
* If the program path contains spaces and is followed by arguments, wrap the path in double quotes.
* Both `/` and `\` work in paths. In single-quoted strings (`'...'`) a backslash needs no escaping; in double-quoted strings write it as `\\`.

## Options

All options are optional and go at the top of `cfg.toml`, before `commands`:

| Option | Default | What it does |
| --- | --- | --- |
| `overlay` | `false` | Shows what you type in a small box at the bottom of the screen, and "no match" when nothing matches. |
| `escape_cancels` | `true` | Pressing Escape while holding the activation key clears what you typed, so releasing the key launches nothing. You can also type a new name before releasing. |
| `watch_config` | `true` | Applies changes to `cfg.toml` as soon as you save it. |
| `autostart` | `false` | Starts betterhotkey when you sign in to Windows. Setting it back to `false` removes it again. |
| `log` | `false` | Writes launches, "no match" and errors to `betterhotkey.log` next to `betterhotkey.exe`. Only the names you type while holding the activation key are logged. |

## Applying Changes

With `watch_config = true` (the default), changes to `cfg.toml` apply as soon as you save the file. If the file has an error, a message box tells you and the previous settings stay in use.

With `watch_config = false`, restart the program instead:

1. Open **Task Manager**.
2. End the `betterhotkey.exe` process.
3. Launch `betterhotkey.exe` again.

That's it — your new hotkeys are ready to use.

## Releasing

Releases are built by GitHub Actions when a version tag is pushed. Set `version` in `Cargo.toml`, commit, then tag that version:

```bash
git tag v0.2.0
git push origin v0.2.0
```

The workflow checks that the tag matches `Cargo.toml`, runs the tests, and publishes `betterhotkey.exe` as a release.
