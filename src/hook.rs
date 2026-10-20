//! 全局低级键盘钩子（WH_KEYBOARD_LL）。
//!
//! 设计要点：
//! - 只拦截"单独按下的 CapsLock"；带修饰键（Ctrl/Alt/Shift/Win）时一律放行，
//!   保留系统原生行为（如 Shift+CapsLock）。
//! - 吞掉 CapsLock 的按下与抬起后，系统不会翻转大写锁状态，指示灯也不会亮。
//! - 把 `alt_as_ctrl` 里列出的 `Alt+字母` 翻译成 `Ctrl+字母`（macOS 风格）：
//!   只翻译列出的字母、只认"单按 Alt"的组合；`Ctrl+C` 等一律原样放行——
//!   终端里用 `Ctrl+C` 终止命令完全不受影响。
//! - 钩子回调只做最轻量的判断并投递消息（PostMessage），真正的 IME 调用由工作
//!   线程执行，避免拖慢回调触发系统的 LowLevelHooksTimeout 保护而被静默移除。
//!   **唯一例外**是 Alt 重映射的 `SendInput`：它必须与按键同步（晚一步就会和抬起
//!   乱序），本身也是非阻塞的，而且只对配置里列出的字母执行。

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, AtomicU16, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_C,
    VK_CAPITAL, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_V,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetClassNameW, GetForegroundWindow, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::{config, util};

/// 钩子线程 -> 工作线程：请求执行一次"切换中英文"。
pub const WM_SWITCH: u32 = WM_APP + 1;

/// 是否拦截 CapsLock（托盘"启用/暂停"）。
static ENABLED: AtomicBool = AtomicBool::new(true);
static HOOK: AtomicIsize = AtomicIsize::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);
/// 0 = 空闲；1 = 按下已吞掉（等待抬起）；2 = 按下时已放行。
static CAPS_STATE: AtomicU8 = AtomicU8::new(0);

/// `Alt+字母 → Ctrl+字母` 的翻译表（位图，bit 0 = A … bit 25 = Z）。
static ALT_REMAP: AtomicU32 = AtomicU32::new(0);
/// 是否正按着 Alt（左/右都算）。
static ALT_DOWN: AtomicBool = AtomicBool::new(false);
/// 正在翻译的键（0 = 无）：抬起时据此吞掉物理事件并补发 Ctrl 抬起。
static ALT_PENDING: AtomicU16 = AtomicU16::new(0);
/// 当前按着的是哪个 Alt（VK_LMENU / VK_RMENU）：恢复应用侧状态时要用同一个。
static ALT_VK: AtomicU16 = AtomicU16::new(0);
/// 翻译期间已经替应用"松开"过 Alt、尚未恢复（应用侧 Alt = up）。
static ALT_RELEASED: AtomicBool = AtomicBool::new(false);
/// 这一轮 Alt 里翻译过按键 → 松开 Alt 前轻敲一次 Ctrl，遮蔽"点 Alt 打开菜单栏"。
static ALT_MASK: AtomicBool = AtomicBool::new(false);

/// 原生 Ctrl+C / Ctrl+V 的处理方式（[`config::CtrlCopy`] 的编码）。
static CTRL_COPY: AtomicU8 = AtomicU8::new(config::CtrlCopy::Gui.code());
/// 正被吞掉的 Ctrl 组合键（0 = 无）：抬起时一起吞掉。
static CTRL_SWALLOW: AtomicU16 = AtomicU16::new(0);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// 设置 `Alt+字母 → Ctrl+字母` 的翻译表（配置文件 `alt_as_ctrl`）。
pub fn set_alt_as_ctrl(mask: u32) {
    ALT_REMAP.store(mask, Ordering::SeqCst);
}

/// 设置原生 `Ctrl+C` / `Ctrl+V` 的处理方式（配置文件 `ctrl_copy_paste`）。
pub fn set_ctrl_copy(mode: config::CtrlCopy) {
    CTRL_COPY.store(mode.code(), Ordering::SeqCst);
}

/// 设置接收切换请求的工作线程窗口。
pub fn set_target(worker_hwnd: HWND) {
    TARGET.store(worker_hwnd as isize, Ordering::SeqCst);
}

pub fn install() -> Result<(), u32> {
    let hmod = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), hmod, 0) };
    if hook.is_null() {
        return Err(util::last_error());
    }
    HOOK.store(hook as isize, Ordering::SeqCst);
    Ok(())
}

pub fn uninstall() {
    let hook = HOOK.swap(0, Ordering::SeqCst);
    if hook != 0 {
        unsafe { UnhookWindowsHookEx(hook as HHOOK) };
    }
}

fn modifier_down() -> bool {
    [
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
    ]
    .iter()
    .any(|&vk| unsafe { GetAsyncKeyState(vk as i32) } < 0)
}

/// Alt 之外的修饰键是否也按着（AltGr = Ctrl+Alt 这类组合要放行）。
fn other_modifiers_down() -> bool {
    [
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LWIN,
        VK_RWIN,
    ]
    .iter()
    .any(|&vk| unsafe { GetAsyncKeyState(vk as i32) } < 0)
}

/// 只按着 Ctrl（没有 Alt/Shift/Win）：原生 Ctrl+C / Ctrl+V 的判断条件。
fn ctrl_only_down() -> bool {
    let ctrl = [VK_CONTROL, VK_LCONTROL, VK_RCONTROL]
        .iter()
        .any(|&vk| unsafe { GetAsyncKeyState(vk as i32) } < 0);
    let others = [VK_LSHIFT, VK_RSHIFT, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|&vk| unsafe { GetAsyncKeyState(vk as i32) } < 0);
    ctrl && !others
}

/// 控制台/终端窗口的类名：这些地方 Ctrl+C 是"终止命令"，不能吞。
const CONSOLE_CLASSES: [&str; 5] = [
    "ConsoleWindowClass",            // conhost（cmd / PowerShell 经典窗口）
    "CASCADIA_HOSTING_WINDOW_CLASS", // Windows Terminal
    "VirtualConsole",                // ConEmu
    "mintty",                        // Git Bash / Cygwin
    "PuTTY",
];

fn is_console_class(class: &str) -> bool {
    CONSOLE_CLASSES
        .iter()
        .any(|known| class.eq_ignore_ascii_case(known))
}

fn is_console_window() -> bool {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return false;
    }
    let mut buf = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 0 {
        return false;
    }
    is_console_class(&String::from_utf16_lossy(&buf[..len as usize]))
}

/// 字母键 → 翻译表里的位；非字母返回 0（永远不会命中）。
fn letter_bit(vk: u16) -> u32 {
    if (0x41..=0x5A).contains(&vk) {
        1 << (vk - 0x41)
    } else {
        0
    }
}

/// 我们自己注入的按键都带这个标记：钩子永远跳过它们（避免自激）。
const INJECT_TAG: usize = 0xCA51_0000;

fn send_key(vk: u16, up: bool) {
    let mut input: INPUT = unsafe { std::mem::zeroed() };
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: vk,
        wScan: 0,
        dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
        time: 0,
        dwExtraInfo: INJECT_TAG,
    };
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
}

/// 轻敲一次 Ctrl：让系统知道"Alt 期间还按过别的键"，松 Alt 时就不会激活菜单栏。
fn tap_ctrl() {
    send_key(VK_CONTROL, false);
    send_key(VK_CONTROL, true);
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code == HC_ACTION as i32 && lparam != 0 {
            let k = &*(lparam as *const KBDLLHOOKSTRUCT);
            // 只认物理按键；我们自己的注入带 INJECT_TAG，同样跳过（避免翻译动作自激）
            let injected = (k.flags & LLKHF_INJECTED) != 0 || k.dwExtraInfo == INJECT_TAG;
            let vk = k.vkCode as u16;
            let msg = wparam as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;

            if !injected {
                // Alt 的按下/抬起本身必须放行（Alt+Tab 等系统组合要用），只做记账
                if vk == VK_LMENU || vk == VK_RMENU {
                    if down {
                        ALT_DOWN.store(true, Ordering::SeqCst);
                        ALT_VK.store(vk, Ordering::SeqCst);
                    } else {
                        ALT_DOWN.store(false, Ordering::SeqCst);
                        // 翻译时已经替应用"松开"过 Alt 且没恢复：这次抬起对它不可见
                        if ALT_RELEASED.swap(false, Ordering::SeqCst) {
                            return 1;
                        }
                        if ALT_MASK.swap(false, Ordering::SeqCst) {
                            tap_ctrl();
                        }
                    }
                    return CallNextHookEx(
                        HOOK.load(Ordering::SeqCst) as HHOOK,
                        code,
                        wparam,
                        lparam,
                    );
                }

                // Alt+字母 → Ctrl+字母：只翻译配置里列出的字母
                if ENABLED.load(Ordering::SeqCst)
                    && ALT_REMAP.load(Ordering::SeqCst) & letter_bit(vk) != 0
                {
                    let pending = ALT_PENDING.load(Ordering::SeqCst);
                    if down {
                        if pending == vk {
                            // 长按的自动重复：Ctrl 还按着，只补发字母按下
                            send_key(vk, false);
                            return 1;
                        }
                        if pending == 0
                            && ALT_DOWN.load(Ordering::SeqCst)
                            && !other_modifiers_down()
                        {
                            ALT_PENDING.store(vk, Ordering::SeqCst);
                            ALT_MASK.store(true, Ordering::SeqCst);
                            // 光注入 Ctrl+C 是不够的：物理 Alt 还按着，应用会看到
                            // Ctrl+Alt+C。所以先替应用把 Alt 松开（中间先补一次 Ctrl，
                            // 免得这次 Alt 抬起被当成"点 Alt 开菜单"），发完再恢复。
                            let alt_vk = ALT_VK.load(Ordering::SeqCst);
                            send_key(VK_CONTROL, false);
                            send_key(alt_vk, true);
                            send_key(vk, false);
                            ALT_RELEASED.store(true, Ordering::SeqCst);
                            return 1;
                        }
                    } else if ALT_PENDING.swap(0, Ordering::SeqCst) == vk {
                        send_key(vk, true);
                        send_key(VK_CONTROL, true);
                        if ALT_DOWN.load(Ordering::SeqCst) {
                            // 用户还按着 Alt：把应用看到的 Alt 状态恢复
                            send_key(ALT_VK.load(Ordering::SeqCst), false);
                            ALT_RELEASED.store(false, Ordering::SeqCst);
                        }
                        return 1;
                    }
                }

                // 原生 Ctrl+C / Ctrl+V：按配置决定是否吞掉；终端窗口保留 Ctrl+C 终止命令
                if ENABLED.load(Ordering::SeqCst) && (vk == VK_C || vk == VK_V) {
                    if down {
                        if ctrl_only_down() {
                            let mode =
                                config::CtrlCopy::from_code(CTRL_COPY.load(Ordering::SeqCst));
                            let swallow = match mode {
                                config::CtrlCopy::Keep => false,
                                config::CtrlCopy::Off => true,
                                config::CtrlCopy::Gui => !is_console_window(),
                            };
                            if swallow {
                                CTRL_SWALLOW.store(vk, Ordering::SeqCst);
                                return 1;
                            }
                        }
                    } else if CTRL_SWALLOW.swap(0, Ordering::SeqCst) == vk {
                        // 按下被吞过：抬起也吞掉，对应用保持不可见
                        return 1;
                    }
                }
            }

            // 只处理物理 CapsLock 键，放行程序注入的按键（LLKHF_INJECTED）
            if vk == VK_CAPITAL && !injected {
                match msg {
                    WM_KEYDOWN | WM_SYSKEYDOWN => {
                        if CAPS_STATE.load(Ordering::SeqCst) == 1 {
                            return 1; // 长按产生的重复按下：吞掉，但不重复触发
                        }
                        if ENABLED.load(Ordering::SeqCst) && !modifier_down() {
                            CAPS_STATE.store(1, Ordering::SeqCst);
                            let target = TARGET.load(Ordering::SeqCst);
                            if target != 0 {
                                PostMessageW(target as HWND, WM_SWITCH, 0, 0);
                            }
                            return 1; // 吞掉：不触达系统，大写锁状态不变
                        }
                        CAPS_STATE.store(2, Ordering::SeqCst);
                    }
                    // 抬起时无论如何都要复位状态（swap 在 guard 中执行）；
                    // 若这次按下被吞掉过，连带抬起也吞掉，对系统保持不可见。
                    WM_KEYUP | WM_SYSKEYUP if CAPS_STATE.swap(0, Ordering::SeqCst) == 1 => {
                        return 1;
                    }
                    _ => {}
                }
            }
        }
        CallNextHookEx(HOOK.load(Ordering::SeqCst) as HHOOK, code, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_classes_are_recognized() {
        assert!(is_console_class("CASCADIA_HOSTING_WINDOW_CLASS"));
        assert!(is_console_class("consolewindowclass"), "大小写不敏感");
        assert!(is_console_class("mintty"));
        assert!(!is_console_class("Chrome_WidgetWin_1"));
        assert!(!is_console_class("Notepad"));
    }

    #[test]
    fn letter_bit_covers_only_letters() {
        assert_eq!(letter_bit(0x41), 1); // A
        assert_eq!(letter_bit(0x43), 1 << 2); // C
        assert_eq!(letter_bit(0x56), 1 << 21); // V
        assert_eq!(letter_bit(0x5A), 1 << 25); // Z
        assert_eq!(letter_bit(0x31), 0, "数字不该命中");
        assert_eq!(letter_bit(VK_CAPITAL), 0);
    }
}
