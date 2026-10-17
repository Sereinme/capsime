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
  <a href="#"><img alt="size" src="https://img.shields.io/badge/exe-~460%20KB-6272A4?style=flat-square"></a>
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
- 💬 **A hint on every switch** — a small 「中 / en」 card fades in at the insertion point / input field and fades out in ~0.7 s; when no caret is available it falls back to the input field's frame and then to the mouse cursor. It follows the system light/dark mode and can be turned off from the tray menu
- 🎨 **A tray icon that knows the room** — a geometric ring with a vermilion brush tail forming a "C"; follows the system light/dark mode and goes cool-gray while paused
- 🪶 **Light** — one ~460 KB exe; ~11 MB working set, 1.6 MB private bytes when idle
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
| Show switching hint | Pops the 「中 / en」 card near the caret / cursor (on by default) |
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
<summary><b>Where does the hint appear, and why is it sometimes next to the cursor?</b></summary>

It follows a four-step ladder, always trying to sit where you are actually typing:

1. **Win32 caret** (`GetGUIThreadInfo`) — provided by classic edit controls;
2. **UIA caret** — browsers, Electron and WinUI never create a Win32 caret, so capsime asks
   UI Automation's `TextPattern` instead (verified bit-for-bit against .NET's UIA readings);
3. **UIA field frame** — some apps (Chromium-based, Windows Terminal) expose the text field
   but no caret rectangle;
4. **Mouse position** — only when focus is not in a text field at all.

macOS can always anchor to the input field because its text system obliges every editable
view to report its insertion point; Windows has no such contract. The tier used is written to
the log (start with `CAPSIME_DEBUG=1`); to see which path your own apps take, run
`cargo run --example caret_probe`. Details and per-app measurements:
[docs/ime-switching.md](docs/ime-switching.md) (Chinese).
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
| Idle memory | ~10 MB working set, 1.6 MB private bytes |
| Idle CPU | 0 ms over a 15 s sample (0.0000% of one core) — event-driven, no polling |
| Threads / handles | 2 threads (main + switch worker) / ~140 handles |
| After the hint has been used | ~17 MB working set, 2.9 MB private, 10 threads, 203 handles — the resident cost of the UIA client (`uiautomationcore`), loaded only when actually needed; turn the hint off in the tray menu and it is never loaded |
| One switch | ~120-140 ms (includes the deliberate 100 ms wait and 14-25 ms of UIA anchoring; injection < 1 ms) |
| CPU per switch | ~1 ms (including the log write and the UIA query) |
| After 100 switches | working set / private bytes / handles / threads unchanged (no leaks) |
| Startup to tray ready | < 1 s |
| Release size | 467,968 bytes (~460 KB, LTO + strip, UIA client included) |

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
cargo test                  # 31 unit tests
cargo run --example icon_preview        # tray icon sheet: sizes × enabled/paused × themes
cargo run --example indicator_preview   # switch-hint card sheet: glyphs × themes × 100/150% DPI
cargo run --example caret_probe         # insertion-point probe: which anchoring path is used
cargo run --example ime_probe           # IME diagnostics: environment + per-path tests
```

```
src/
  main.rs       entry point (--version / --help / --selftest, tray dispatch, DPI awareness)
  app.rs        single instance, two message loops, lifecycle, theme & hint notifications
  hook.rs       WH_KEYBOARD_LL hook: intercepts a lone CapsLock only
  switcher.rs   the switching core: read state, inject switch key, fall back & verify
  tray.rs       tray icon and menu
  icon.rs       tray icon: geometric ring + vermilion brush tail "C", light/dark themes
  indicator.rs  switch hint: layered popup + hand-drawn card + four-tier anchoring
  uia.rs        UI Automation client: hand-written minimal COM interfaces, reads the
                insertion point of browsers / Electron / WinUI
  theme.rs      system light/dark mode detection
  autostart.rs  autostart via an HKCU Run entry
  log.rs        minimal file log (512 KB rotation)
  util.rs       small Win32 helpers
examples/
  icon_preview.rs       tray icon review sheet
  indicator_preview.rs  switch-hint card review sheet
  caret_probe.rs        insertion-point probe (Win32 / UIA / mouse, tier by tier)
  ime_probe.rs          IME switching diagnostics (also verifies in a real Notepad)
assets/icon/       icon design (SVG), previews, repo social-preview card and avatar
docs/              design and implementation notes
```

Both the icon and the hint card are drawn entirely in code (no image assets):
superellipse/rounded-rect tile with a gradient, edge highlight and a soft shadow,
supersampled 4×4 per pixel, rendered at the DPI of the monitor they land on. The card follows
Apple's published design system (no decorative gradients, a hairline and system-gray dark
surface); it is a layered window (`UpdateLayeredWindow`) giving it per-pixel transparency,
click-through and no focus stealing, and the glyph is rendered by GDI with system fonts.
Reading UI Automation uses hand-written minimal COM interfaces — like the tray icon,
**no new dependency was added**.

## Roadmap

- [ ] Configuration file: custom switch key, excluded applications
- [ ] Long-press CapsLock = native caps lock (short press toggles IME)
- [ ] Hold CapsLock for temporary English (release returns to Chinese)
- [ ] Hint card placement preference (above/below/left/right)

## License

MIT, see [LICENSE](LICENSE). Icon colours and materials take inspiration from the public
design systems collected in
[awesome-design-md](https://github.com/VoltAgent/awesome-design-md); no third-party brand
identity is claimed.