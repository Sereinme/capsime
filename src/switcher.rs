//! 中英文切换核心。
//!
//! # 结论先行：唯一可靠的动作是"注入输入法自己的切换键"
//!
//! 这是在 Windows 11 + 微软拼音 + 记事本上逐条实测出来的（判据是**打字结果**，
//! 而不是任何 API 的返回值）：
//!
//! - `ImmGetContext` 对**别的进程**一律返回 NULL（Notepad / Qt / Electron /
//!   资源管理器 / UWP 全是），既读不到状态也设不了。
//! - 前台线程的默认 IME 窗口（`ImmGetDefaultIMEWnd`）**读回来的不是真实状态**：
//!   记事本明明在中文态（打字出「你好」），`IMC_GETCONVERSIONMODE` 却给出
//!   `0x0001`、`IMC_GETOPENSTATUS` 给出 0（看上去是英文），而且注入切换键后
//!   它**纹丝不动**。往这个窗口发 `IMC_SETCONVERSIONMODE` 更糟：只改它自己的
//!   读回值，前台程序毫无变化——这正是"日志说切了、实际没切"的根源。
//! - 注入 `Ctrl+Space`（输入法自带的中英文切换键）**真的有效**：记事本里能从
//!   英文切成中文（`nihao` → 「你好」），对经典窗口和跨进程同样有效。按键由输入法
//!   在**目标程序自己的上下文**里处理，所以对任何程序都适用。
//!
//! # 因此的策略
//!
//! 1. **有 IMM 上下文就精确设置**（本进程窗口一定有，个别系统上跨进程也能拿到）：
//!    读状态 → 设目标态 → 复核，全程可核对。
//! 2. **否则注入一次切换键**，然后**不做任何重试**。没有可信的状态通道，
//!    重试只会把状态再翻回去（净效果为零，表现出来就是"按了没反应"）。
//!    如果系统把这次按键当成"切换输入语言"（线程键盘布局变了）：
//!    切到中文/英文布局算成功（这正是用户要的效果）；切到无关语言（如日文）
//!    立刻切回并转用布局兜底。
//! 3. **键盘布局切换**只在第 2 步被误当成语言切换且回滚之后才用（需要系统装有
//!    英文布局）。
//!
//! 状态读不回来时，日志如实标注"未核对"，绝不虚报。

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::Ime::{
    IME_CMODE_NATIVE, ImmGetContext, ImmGetConversionStatus, ImmGetDefaultIMEWnd, ImmGetOpenStatus,
    ImmReleaseContext, ImmSetConversionStatus, ImmSetOpenStatus,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardLayoutList, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    SendInput, VK_CONTROL, VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, SMTO_ABORTIFHUNG,
    SendMessageTimeoutW, WM_IME_CONTROL, WM_INPUTLANGCHANGEREQUEST,
};

use crate::util;

/// `WM_IME_CONTROL` 的 wParam（imm.h）。
const IMC_GETCONVERSIONMODE: usize = 0x0001;
const IMC_GETOPENSTATUS: usize = 0x0005;
/// 主语言 ID（winnt.h）。
const LANG_CHINESE: u16 = 0x04;
const LANG_ENGLISH: u16 = 0x09;
/// 跨进程同步消息的超时（毫秒）：目标不响应就放弃，绝不拖住工作线程。
const IME_MSG_TIMEOUT_MS: u32 = 250;
/// 注入切换键后，等系统和输入法把这次按键处理完再看键盘布局。
const KEY_SETTLE_MS: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// IMM 上下文（只可能用于本进程窗口）：读状态、设状态都可核对
    Imm,
    /// 注入输入法切换键（对任何程序都适用的主路径）
    Hotkey,
    /// 键盘布局切换（需要系统装有英文布局）
    Hkl,
    /// 无可用方式
    None,
}

impl Method {
    /// 日志里显示的方式名。
    pub fn label(self) -> &'static str {
        match self {
            Method::Imm => "IMM",
            Method::Hotkey => "输入法切换键",
            Method::Hkl => "键盘布局",
            Method::None => "无",
        }
    }
}

#[derive(Debug)]
pub struct SwitchResult {
    pub method: Method,
    /// 切换后的状态：`Some(true)` = 中文，`Some(false)` = 英文，`None` = 读不到
    pub now_chinese: Option<bool>,
    /// 是否有独立证据证明状态确实变了（IMM 写后读回；或键盘布局变化）
    pub verified: bool,
    /// 注入的按键被系统当成"切换输入语言"，且已切回原布局
    pub layout_reverted: bool,
    /// 前台窗口描述（写日志用）
    pub target: String,
}

impl SwitchResult {
    fn new(method: Method, now_chinese: Option<bool>, verified: bool, target: String) -> Self {
        Self {
            method,
            now_chinese,
            verified,
            layout_reverted: false,
            target,
        }
    }
}

/// 对前台窗口执行一次"中英文切换"。
pub fn toggle() -> SwitchResult {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return SwitchResult::new(Method::None, None, false, "无前台窗口".into());
    }
    let target = describe(hwnd);

    // 1. 有 IMM 上下文就精确设置：读状态 → 设目标态 → 复核
    if let Some(chinese) = imm_state(hwnd) {
        let want = !chinese;
        if let Some(now) = imm_set(hwnd, want) {
            return SwitchResult::new(Method::Imm, Some(now), now == want, target);
        }
    }

    // 2. 注入输入法切换键（主路径）：只按一次，不做无依据的重试
    let tid = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    let hkl_before = unsafe { GetKeyboardLayout(tid) };
    // 参考值：经典窗口的 IMM 兼容层会跟着键切换走，TSF 程序（如记事本）不跟。
    // 只用来在"能跟上的程序"里给日志一个"已核对"，绝不参与决策。
    let reference_before = ime_wnd_state(hwnd);

    inject_switch_key();
    std::thread::sleep(std::time::Duration::from_millis(KEY_SETTLE_MS));

    let hkl_after = unsafe { GetKeyboardLayout(tid) };
    if hkl_after != hkl_before {
        return handle_language_switch(hwnd, hkl_before as isize, hkl_after as isize, target);
    }

    let reference_after = ime_wnd_state(hwnd);
    let verified = matches!((reference_before, reference_after), (Some(b), Some(a)) if a != b);
    SwitchResult::new(
        Method::Hotkey,
        if verified { reference_after } else { None },
        verified,
        target,
    )
}

/// 注入的按键被系统当成了"切换输入语言"。切到中文/英文布局视为成功（这正是
/// 用户要的效果）；切到无关语言（如日文）则切回原布局并转用布局兜底。
fn handle_language_switch(
    hwnd: HWND,
    hkl_before: isize,
    hkl_after: isize,
    target: String,
) -> SwitchResult {
    match language_of(hkl_after) {
        LANG_ENGLISH | LANG_CHINESE => SwitchResult::new(
            Method::Hotkey,
            Some(language_of(hkl_after) == LANG_CHINESE),
            true,
            target,
        ),
        _ => {
            // 无关语言：切回去，然后走布局兜底
            unsafe { PostMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, 0, hkl_before) };
            if let Some(now) = try_hkl(hwnd) {
                let mut r = SwitchResult::new(Method::Hkl, Some(now), false, target);
                r.layout_reverted = true;
                return r;
            }
            let mut r = SwitchResult::new(Method::None, None, false, target);
            r.layout_reverted = true;
            r
        }
    }
}

/// 由 (IME 打开状态, 转换模式) 判定是否中文态。
pub(crate) fn is_chinese(open: bool, conv: u32) -> bool {
    open && (conv & IME_CMODE_NATIVE) != 0
}

/// 在转换模式里置/清 `IME_CMODE_NATIVE` 位，其余位原样保留。
pub(crate) fn mode_bits(conv: u32, want_chinese: bool) -> u32 {
    if want_chinese {
        conv | IME_CMODE_NATIVE
    } else {
        conv & !IME_CMODE_NATIVE
    }
}

fn language_of(hkl: isize) -> u16 {
    (hkl as usize & 0x3FF) as u16
}

/// 读 IMM 上下文里的中文态（拿不到上下文或读失败返回 None）。
fn imm_state(hwnd: HWND) -> Option<bool> {
    let himc = unsafe { ImmGetContext(hwnd) };
    if himc.is_null() {
        return None;
    }
    unsafe {
        let mut conv = 0u32;
        let mut sentence = 0u32;
        let ok = ImmGetConversionStatus(himc, &mut conv, &mut sentence);
        let open = ImmGetOpenStatus(himc) != 0;
        ImmReleaseContext(hwnd, himc);
        if ok == 0 {
            None
        } else {
            Some(is_chinese(open, conv))
        }
    }
}

/// 用 IMM 把前台窗口设成目标状态，返回设置后的实际状态。
fn imm_set(hwnd: HWND, want_chinese: bool) -> Option<bool> {
    let himc = unsafe { ImmGetContext(hwnd) };
    if himc.is_null() {
        return None;
    }
    unsafe {
        let mut conv = 0u32;
        let mut sentence = 0u32;
        if ImmGetConversionStatus(himc, &mut conv, &mut sentence) == 0 {
            ImmReleaseContext(hwnd, himc);
            return None;
        }
        let next = mode_bits(conv, want_chinese);
        if want_chinese {
            // 切到中文：先确保 IME 是打开状态
            ImmSetOpenStatus(himc, 1);
        }
        ImmSetConversionStatus(himc, next, sentence);
        let mut conv2 = 0u32;
        let mut sentence2 = 0u32;
        let ok = ImmGetConversionStatus(himc, &mut conv2, &mut sentence2);
        let open = ImmGetOpenStatus(himc) != 0;
        ImmReleaseContext(hwnd, himc);
        if ok == 0 {
            None
        } else {
            Some(is_chinese(open, conv2))
        }
    }
}

/// 参考读值：前台线程默认 IME 窗口报告的转换模式。
///
/// **注意**：TSF 程序（记事本等现代应用）这里读到的不是真实状态，所以它只用于
/// 给"能跟上的程序"补一个核对结果，绝不用于决策。
fn ime_wnd_state(hwnd: HWND) -> Option<bool> {
    let ime_wnd = unsafe { ImmGetDefaultIMEWnd(hwnd) };
    if ime_wnd.is_null() {
        return None;
    }
    let conv = send_timeout(ime_wnd, WM_IME_CONTROL, IMC_GETCONVERSIONMODE, 0)? as u32;
    let open = send_timeout(ime_wnd, WM_IME_CONTROL, IMC_GETOPENSTATUS, 0)? != 0;
    Some(is_chinese(open, conv))
}

/// 注入输入法切换键：Ctrl + Space。
fn inject_switch_key() {
    send_key(VK_CONTROL, false);
    send_key(VK_SPACE, false);
    send_key(VK_SPACE, true);
    send_key(VK_CONTROL, true);
}

fn send_key(vk: u16, up: bool) {
    let mut input: INPUT = unsafe { std::mem::zeroed() };
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: vk,
        wScan: 0,
        dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
        time: 0,
        dwExtraInfo: 0,
    };
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
}

/// 跨进程同步发消息一律带超时：目标不响应时立刻返回，绝不挂死自己。
fn send_timeout(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> Option<usize> {
    let mut out: usize = 0;
    let r = unsafe {
        SendMessageTimeoutW(
            hwnd,
            msg,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG,
            IME_MSG_TIMEOUT_MS,
            &mut out,
        )
    };
    if r == 0 { None } else { Some(out) }
}

fn try_hkl(hwnd: HWND) -> Option<bool> {
    unsafe {
        let tid = GetWindowThreadProcessId(hwnd, std::ptr::null_mut());
        let current = GetKeyboardLayout(tid) as isize;
        let count = GetKeyboardLayoutList(0, std::ptr::null_mut());
        if count <= 0 {
            return None;
        }
        let mut list: Vec<isize> = vec![0; count as usize];
        GetKeyboardLayoutList(count, list.as_mut_ptr() as *mut _);

        let want_chinese = language_of(current) != LANG_CHINESE;
        let target = list.iter().copied().find(|&hkl| {
            let p = language_of(hkl);
            if want_chinese {
                p == LANG_CHINESE
            } else {
                p == LANG_ENGLISH
            }
        })?;

        PostMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, 0, target);
        Some(want_chinese)
    }
}

fn describe(hwnd: HWND) -> String {
    let class = util::hwnd_class(hwnd);
    match util::hwnd_exe(hwnd) {
        Some(exe) => format!("{exe} [{class}]"),
        None => format!("[{class}]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_chinese_needs_ime_open_and_native_bit() {
        assert!(is_chinese(true, IME_CMODE_NATIVE));
        assert!(!is_chinese(false, IME_CMODE_NATIVE), "IME 关闭时不算中文态");
        assert!(!is_chinese(true, 0));
    }

    #[test]
    fn mode_bits_clears_native_for_english() {
        assert_eq!(mode_bits(IME_CMODE_NATIVE, false) & IME_CMODE_NATIVE, 0);
    }

    #[test]
    fn mode_bits_sets_native_for_chinese() {
        assert_ne!(mode_bits(0, true) & IME_CMODE_NATIVE, 0);
    }

    #[test]
    fn mode_bits_preserves_unrelated_mode_bits() {
        let other = 0x0400; // 例如 IME_CMODE_FULLSHAPE
        assert_eq!(mode_bits(other, false) & other, other);
        assert_eq!(mode_bits(other | IME_CMODE_NATIVE, false) & other, other);
        assert_eq!(mode_bits(other, true) & other, other);
    }

    #[test]
    fn language_of_extracts_primary_language() {
        assert_eq!(language_of(0x0804_0804), LANG_CHINESE);
        assert_eq!(language_of(0x0411_0411), 0x11);
        assert_eq!(language_of(0x0409_0409), LANG_ENGLISH);
    }
}
