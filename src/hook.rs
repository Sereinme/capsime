//! 全局低级键盘钩子（WH_KEYBOARD_LL）。
//!
//! 设计要点：
//! - 只拦截"单独按下的 CapsLock"；带修饰键（Ctrl/Alt/Shift/Win）时一律放行，
//!   保留系统原生行为（如 Shift+CapsLock）。
//! - 吞掉 CapsLock 的按下与抬起后，系统不会翻转大写锁状态，指示灯也不会亮。
//! - 钩子回调只做最轻量的判断并投递消息（PostMessage），真正的 IME 调用由工作
//!   线程执行，避免拖慢回调触发系统的 LowLevelHooksTimeout 保护而被静默移除。

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CAPITAL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, PostMessageW,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::util;

/// 钩子线程 -> 工作线程：请求执行一次"切换中英文"。
pub const WM_SWITCH: u32 = WM_APP + 1;

/// 是否拦截 CapsLock（托盘"启用/暂停"）。
static ENABLED: AtomicBool = AtomicBool::new(true);
static HOOK: AtomicIsize = AtomicIsize::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);
/// 0 = 空闲；1 = 按下已吞掉（等待抬起）；2 = 按下时已放行。
static CAPS_STATE: AtomicU8 = AtomicU8::new(0);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
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

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code == HC_ACTION as i32 && lparam != 0 {
            let k = &*(lparam as *const KBDLLHOOKSTRUCT);
            // 只处理物理 CapsLock 键，放行程序注入的按键（LLKHF_INJECTED）
            if k.vkCode == VK_CAPITAL as u32 && (k.flags & LLKHF_INJECTED) == 0 {
                match wparam as u32 {
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
