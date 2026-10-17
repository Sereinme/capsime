<div align="center">
  <img src="assets/icon/capsime-icon.png" width="132" alt="capsime" />
  <h1>capsime</h1>
  <p><b>One-key Chinese/English input switching on Windows, triggered by CapsLock</b><br/>
  <sub>Single exe · no dependencies · tray-resident · no network · no admin rights</sub></p>
  <p><a href="README.md">中文</a> · English</p>
  <a href="https://github.com/Sereinme/capsime/releases"><img alt="release" src="https://img.shields.io/github/v/release/Sereinme/capsime?style=flat-square&amp;sort=semver"></a>
  <a href="https://github.com/Sereinme/capsime/releases"><img alt="downloads" src="https://img.shields.io/github/downloads/Sereinme/capsime/total?style=flat-square"></a>
  <a href="https://github.com/Sereinme/capsime/blob/master/LICENSE"><img alt="license" src="https://img.shields.io/github/license/Sereinme/capsime?style=flat-square"></a>
  <a href="#"><img alt="platform" src="https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078D6?style=flat-square"></a>
  <a href="#"><img alt="rust" src="https://img.shields.io/badge/rust-1.85%2B-orange?style=flat-square"></a>
  <a href="#"><img alt="size" src="https://img.shields.io/badge/exe-~440%20KB-6272A4?style=flat-square"></a>
</div>

CapsLock is a dead key for most people. capsime turns it into the Chinese/English switch:
**tap CapsLock to toggle input mode, and the caps-lock state and LED stay untouched**
(both key-down and key-up are swallowed, so nothing reaches the system). Chords like
`Ctrl/Alt/Shift/Win + CapsLock` pass through untouched, auto-repeat never re-triggers,
and CapsLock is the only key intercepted.

Written in pure Rust. No network, no driver, no injection into other processes, and every
switch is logged. Diagnostic tools included.

## Features

- ⌨️ **CapsLock toggles Chinese/English** — only a lone CapsLock press is intercepted; chords stay native
- 🎨 **A tray icon that knows the room** — a geometric ring with a vermilion brush tail forming a "C"; follows the system light/dark mode and goes cool-gray while paused
- 🪶 **Light** — one ~440 KB exe; ~11 MB working set, 1.6 MB private bytes when idle
- 🍃 **Zero polling** — the hook callback only posts one message; idle CPU measured at 0 ms over 15 s
- 🔍 **Debuggable** — every switch logs method/state/verification/foreground app; ships `--selftest` and an `ime_probe` diagnostic
- 🧩 **No third parties** — no network, no telemetry, no admin rights, no injection into other processes
- 🖥️ **DPI-aware rendering** — the icon is drawn at the exact tray pixel size (100–200% scaling)

## Quick start

### Download

1. Grab `capsime.exe` from [Releases](https://github.com/Sereinme/capsime/releases) (Windows 10/11 x64)
2. Run it — a "C" icon appears in the tray
3. Tap **CapsLock** in any app to toggle Chinese/English

### Build from source

```powershell
git clone https://github.com/Sereinme/capsime.git
cd capsime
cargo build --release        # output: target\release\capsime.exe
```

Requires Rust 1.85+. Works with either the MSVC or GNU toolchain.

## Usage

**Tray menu**

| Item | What it does |
| --- | --- |
| Enable CapsLock switching | Pause/resume interception; while paused CapsLock behaves natively and the icon turns cool-gray |
| Start with Windows | Adds/removes an `HKCU\...\CurrentVersion\Run` entry (current user only) |
| View log | Opens `%LOCALAPPDATA%\capsime\capsime.log` (rotates at 512 KB) |
| Quit | Uninstalls the hook and exits |

**Command line**

```
capsime                 start (tray-resident)
capsime --selftest [s]  smoke test: install hook, deliver one switch, exit (default 3 s)
capsime --version       print version
capsime --help          print help
```

**Need native CapsLock for a moment?** Pause from the tray menu, or press
`Shift+CapsLock` (chords are not intercepted — note it also turns caps lock off, which is
native Windows behaviour).

## How it works

```
CapsLock pressed
   │
   ▼
Main thread: WH_KEYBOARD_LL hook — only PostMessage, returns immediately
   ▼
Worker thread: inject the IME's own switch key (Ctrl+Space) → read back → log
```

In one sentence: **injecting `Ctrl+Space` is exactly what your fingers would do**, and the
IME handles the key inside the *target application's own context*, so it works everywhere.
Why not call the IMM APIs directly? Because `ImmGetContext` always returns NULL for other
processes, and the state read back from the default IME window is simply **wrong** for
TSF-based IMEs (measured: the IME had switched to Chinese while that API kept reporting
English).

Full rationale, measurements and a reproducible probe: **[docs/ime-switching.md](docs/ime-switching.md)**
(Chinese).

## When *not* to use it

- Your IME does not bind Chinese/English switching to `Ctrl+Space` (change it in the IME
  settings first; that is the key capsime injects)
- Some games with kernel-level anti-cheat block global keyboard hooks — no effect in-game
  (the rest of the system is unaffected)
- Keys are not received on the UAC secure desktop, by design
- You actually want to keep CapsLock as caps lock — replacing it is the whole point
  (you can pause anytime)

## FAQ

<details>
<summary><b>CapsLock does nothing?</b></summary>

1. Check the log at `%LOCALAPPDATA%\capsime\capsime.log` — you should see a
   "已切换到中文/英文（方式：输入法切换键…）" line
2. Is the tray icon gray? Gray means paused — re-check "启用大写键切换" in the tray menu
3. Run `capsime --selftest`: printing `OK: 键盘钩子已安装` means the hook is fine;
   failures come with an explicit error code (usually security software)
4. Run `cargo run --example ime_probe -- --notepad` to see, by *typing*, which path works
   on your machine
</details>

<details>
<summary><b>What does "未核对" (unverified) in the log mean?</b></summary>

It means there was no independent read-back available to verify that particular switch —
modern apps (TSF IMEs, e.g. Notepad) don't expose their IME state. It is not a failure;
trust what you see on screen.
</details>

<details>
<summary><b>Is the switch instant? Does it slow down typing?</b></summary>

The key injection itself takes < 1 ms and takes effect immediately. A switch is processed
in ~110 ms, of which 100 ms is a deliberate wait (letting the system and IME settle before
checking whether the key was treated as an *input language* switch, and rolling back if so).
All of it happens on the worker thread and never blocks the hook or your typing.
</details>

<details>
<summary><b>How do I uninstall?</b></summary>

Quit from the tray menu and delete the exe. If you enabled autostart, clear it in the tray
menu first (or remove the `capsime` value under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`). Nothing else is written anywhere.
</details>

## Performance

Measured on Windows 11 x64, release build:

| Metric | Value |
| --- | --- |
| Idle memory | 10.9 MB working set, 1.6 MB private bytes |
| Idle CPU | 0 ms over a 15 s sample (0.0000% of one core) — event-driven, no polling |
| Threads / handles | 2 threads (main + switch worker) / 142 handles |
| One switch | 105–123 ms (includes the deliberate 100 ms wait; injection < 1 ms) |
| CPU per switch | ~0.8 ms (including the log write) |
| After 100 switches | working set / private bytes / handles / threads unchanged (no leaks) |
| Startup to tray ready | < 1 s |
| Release size | 450,048 bytes (~440 KB, LTO + strip) |

The hook callback only checks "is this CapsLock with no modifiers" and posts one message;
ordinary keystrokes cost two atomic reads and an integer compare, so global typing speed is
unaffected and the system's `LowLevelHooksTimeout` (300 ms by default) is never a concern.

## Compatibility & limitations

- Windows 10 / 11 (x64); ARM64 untested
- Switching injects `Ctrl+Space`, which Microsoft Pinyin, Sogou and QQ Pinyin support by
  default; if your IME has it unbound, the switch fails and the log says so
- A few apps are TSF-only and don't expose their state — the log then shows "未核对"

## Development

```powershell
cargo fmt
cargo clippy                # no warnings
cargo test                  # 21 unit tests
cargo run --example icon_preview        # tray icon sheet: sizes × enabled/paused × themes
cargo run --example ime_probe           # IME diagnostics: environment + per-path tests
```

```
src/
  main.rs       entry point (--version / --help / --selftest, tray dispatch)
  app.rs        single instance, two message loops, lifecycle, theme broadcasts
  hook.rs       WH_KEYBOARD_LL hook: intercepts a lone CapsLock only
  switcher.rs   the switching core: read state, inject switch key, fall back & verify
  tray.rs       tray icon and menu
  icon.rs       tray icon: geometric ring + vermilion brush tail "C", light/dark themes
  theme.rs      system light/dark mode detection
  autostart.rs  autostart via an HKCU Run entry
  log.rs        minimal file log (512 KB rotation)
  util.rs       small Win32 helpers
examples/
  icon_preview.rs  tray icon review sheet
  ime_probe.rs     IME switching diagnostics (also verifies in a real Notepad)
assets/icon/       icon design (SVG) and previews
docs/              design and implementation notes
```

The icon is drawn entirely in code (no image assets): superellipse tile with a vertical
gradient, edge highlight and a soft glyph shadow, supersampled 4×4 per pixel, rendered at
the exact tray size for the current DPI.

## Roadmap

- [ ] Show a brief "中 / 英" indicator **at the mouse position** when switching (like macOS)
- [ ] Configuration file: custom switch key, excluded applications
- [ ] Long-press CapsLock = native caps lock (short press toggles IME)
- [ ] Hold CapsLock for temporary English (release returns to Chinese)

## License

MIT, see [LICENSE](LICENSE). Icon colours and materials take inspiration from the public
design systems collected in
[awesome-design-md](https://github.com/VoltAgent/awesome-design-md); no third-party brand
identity is claimed.