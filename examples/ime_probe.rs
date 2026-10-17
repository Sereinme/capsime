//! IME 探针：排查" CapsLock 按下去没有切换中英文"这类问题。
//!
//! 用法（默认只做只读检查 + 进程内实测，会短暂抢一次焦点）：
//!
//! ```text
//! cargo run --example ime_probe               # A 环境 + B 逐条路径实测
//! cargo run --example ime_probe -- --no-focus  # 不抢焦点，只做只读检查
//! cargo run --example ime_probe -- --notepad   # F 真实记事本里验证（最可信）
//! cargo run --example ime_probe -- --e2e       # E 对着另一个真实进程验证 switcher::toggle()
//! ```
//!
//! 判定"真的切了"的黄金标准：
//!
//! - **真实应用**（F，最可信）：在记事本里键入 `nihao` + 空格，看标题变成
//!   「你好」还是 `nihao`——这是最终事实，任何 API 返回值都不能替代它。
//! - **进程内**（B）：向聚焦窗口注入一次 `n` 键——收到 `WM_IME_STARTCOMPOSITION`
//!   说明输入法在组字（中文态）；只收到 `WM_CHAR('n')` 说明是英文态。
//! - **跨进程**（E）：读前台线程默认 IME 窗口的转换模式。
//!   ⚠️ 这一条只对经典窗口有效：TSF 程序（记事本等）读回来的不是真实状态，
//!   实测"打字出中文"时它仍然报英文——用它下判断会得出错误结论。
//!
//! `--notepad` 的注意事项：Win11 记事本会把新窗口并进已有实例（同一进程多窗口），
//! 所以它只用**未命名、无内容**的新窗口，测试完会清空内容再请求关闭；
//! 如果窗口没被关掉（Win11 记事本会忽略外部关闭请求），直接手动关掉即可。

use std::process::Command;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use std::os::windows::process::CommandExt;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcess, GetCurrentThreadId, OpenProcessToken,
};
use windows_sys::Win32::UI::Input::Ime::{
    IME_CHOTKEY_IME_NONIME_TOGGLE, IME_CMODE_NATIVE, ImmGetContext, ImmGetConversionStatus,
    ImmGetDefaultIMEWnd, ImmGetOpenStatus, ImmReleaseContext, ImmSetConversionStatus,
    ImmSetOpenStatus, ImmSimulateHotKey,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ActivateKeyboardLayout, GetKeyboardLayout, GetKeyboardLayoutList, INPUT, INPUT_KEYBOARD,
    KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, SetFocus, VK_A, VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_H,
    VK_I, VK_MENU, VK_N, VK_O, VK_SHIFT, VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, GUITHREADINFO, GetClassNameW,
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsWindowVisible, MSG,
    PM_REMOVE, PeekMessageW, PostMessageW, RegisterClassW, SMTO_ABORTIFHUNG, SW_SHOW,
    SendMessageTimeoutW, SetForegroundWindow, ShowWindow, TranslateMessage, WM_CHAR, WM_CLOSE,
    WM_IME_COMPOSITION, WM_IME_CONTROL, WM_IME_STARTCOMPOSITION, WM_INPUTLANGCHANGEREQUEST,
    WM_KEYDOWN, WM_SYSKEYDOWN, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use capsime::util;

const IMC_GETCONVERSIONMODE: usize = 0x0001;
const IMC_SETCONVERSIONMODE: usize = 0x0002;
const IMC_GETOPENSTATUS: usize = 0x0005;
const IMC_SETOPENSTATUS: usize = 0x0006;
const LANG_CHINESE: u16 = 0x04;
const VK_IME_ON: u16 = 0x16;
const VK_IME_OFF: u16 = 0x1A;
const VK_KANA: u16 = 0x15;

static START_COMP: AtomicU32 = AtomicU32::new(0);
static COMPOSITION: AtomicU32 = AtomicU32::new(0);
static CHARS: AtomicU32 = AtomicU32::new(0);
static KEYDOWNS: AtomicU32 = AtomicU32::new(0);
static PROBE_HWND: AtomicIsize = AtomicIsize::new(0);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Verdict {
    Cn,
    En,
    Unknown,
}

impl Verdict {
    fn text(self) -> &'static str {
        match self {
            Verdict::Cn => "中文",
            Verdict::En => "英文",
            Verdict::Unknown => "未判定",
        }
    }
}

enum Kind {
    /// 绝对设置：apply(hwnd, want_chinese)
    Absolute,
    /// 相对切换：apply(hwnd, _) 应用一次
    Toggle,
}

struct Technique {
    name: &'static str,
    kind: Kind,
    apply: Box<dyn Fn(HWND, bool)>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--as-target") {
        let path = args.get(i + 1).cloned().unwrap_or_default();
        run_as_target(&path);
        return;
    }
    phase_a_env();
    if args.iter().any(|a| a == "--e2e") {
        phase_e_e2e();
        return;
    }
    if args.iter().any(|a| a == "--notepad") {
        phase_notepad();
        return;
    }
    let no_focus = args.iter().any(|a| a == "--no-focus");
    phase_b_matrix(no_focus);
}

// ---------------------------------------------------------------------------
// A. 环境
// ---------------------------------------------------------------------------

fn phase_a_env() {
    println!("=== A. 环境 ===");
    unsafe {
        let tid = GetCurrentThreadId();
        println!(
            "本线程 tid={tid} 布局=0x{:X}",
            GetKeyboardLayout(tid) as usize
        );
        let n = GetKeyboardLayoutList(0, std::ptr::null_mut());
        if n > 0 {
            let mut list = vec![std::ptr::null_mut(); n as usize];
            GetKeyboardLayoutList(n, list.as_mut_ptr());
            let items: Vec<String> = list
                .iter()
                .map(|h| {
                    let primary = (*h as usize & 0x3FF) as u16;
                    let tag = match primary {
                        LANG_CHINESE => "中文",
                        0x09 => "英文",
                        0x11 => "日文",
                        _ => "其他",
                    };
                    format!("0x{:X}({tag})", *h as usize)
                })
                .collect();
            println!("已加载布局（{n} 个）：{}", items.join(", "));
        }
        let mut token = std::ptr::null_mut();
        let mut elevated = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0 {
            GetTokenInformation(
                token,
                TokenElevation,
                &mut elevated as *mut _ as *mut _,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            );
        }
        println!("进程已提权={}", elevated.TokenIsElevated != 0);
        println!("\n-- 当前可见顶层窗口的 IMM 上下文（跨进程能不能拿到）：");
        EnumWindows(Some(enum_proc), 0);
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> i32 {
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let (class, title) = window_desc(hwnd);
        if title.is_empty() {
            return 1;
        }
        let short: String = title.chars().take(36).collect();
        println!(
            "   [{}] class={class} imm=({}) {short}",
            exe_of(hwnd),
            imm_desc(hwnd)
        );
        1
    }
}

// ---------------------------------------------------------------------------
// B. 进程内逐条路径实测
// ---------------------------------------------------------------------------

fn phase_b_matrix(no_focus: bool) {
    println!("\n=== B. 逐条路径实测（黄金标准：注入 n 键看是否组字）===");
    let hwnd = unsafe { make_probe_window() };
    if hwnd.is_null() {
        println!("FATAL: 创建探针窗口失败");
        return;
    }
    PROBE_HWND.store(hwnd as isize, Ordering::SeqCst);

    let focused = if no_focus {
        false
    } else {
        steal_foreground(hwnd)
    };
    println!("\n[窗口] hwnd=0x{:X} 已获得焦点={focused}", hwnd as usize);
    if focused {
        unsafe { pin_chinese_layout(hwnd) };
    }

    let (base, detail) = unsafe { behavior(hwnd, focused) };
    println!(
        "\n[基线] 行为={} {detail}\n        IMM: {} | IMEWnd: {}",
        base.text(),
        unsafe { imm_desc(hwnd) },
        unsafe { imewnd_desc(hwnd) }
    );

    let mut working: Vec<&'static str> = Vec::new();
    for t in techniques() {
        if run_technique(&t, hwnd, focused) {
            working.push(t.name);
        }
    }

    println!("\n=== 结论：真正能切换输入法状态的方法 ===");
    if working.is_empty() {
        println!("(无) —— 所有方法都无法改变输入法行为");
    } else {
        for w in &working {
            println!("  ✔ {w}");
        }
    }

    if focused {
        restore(hwnd, base);
    }
    unsafe {
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
        pump(200);
    }
}

fn techniques() -> Vec<Technique> {
    vec![
        Technique {
            name: "1. [绝对] IMM ImmSetConversionStatus(NATIVE 位)",
            kind: Kind::Absolute,
            apply: Box::new(|hwnd, cn| unsafe {
                let himc = ImmGetContext(hwnd);
                if himc.is_null() {
                    return;
                }
                let mut conv = 0u32;
                let mut sentence = 0u32;
                ImmGetConversionStatus(himc, &mut conv, &mut sentence);
                let next = mode_bits(conv, cn);
                if cn {
                    ImmSetOpenStatus(himc, 1);
                }
                ImmSetConversionStatus(himc, next, sentence);
                ImmReleaseContext(hwnd, himc);
            }),
        },
        Technique {
            name: "2. [绝对] IMM ImmSetOpenStatus(IME 开/关)",
            kind: Kind::Absolute,
            apply: Box::new(|hwnd, cn| unsafe {
                let himc = ImmGetContext(hwnd);
                if himc.is_null() {
                    return;
                }
                ImmSetOpenStatus(himc, if cn { 1 } else { 0 });
                ImmReleaseContext(hwnd, himc);
            }),
        },
        Technique {
            name: "3. [绝对] 默认 IME 窗口 IMC_SETCONVERSIONMODE",
            kind: Kind::Absolute,
            apply: Box::new(|hwnd, cn| unsafe {
                let w = ImmGetDefaultIMEWnd(hwnd);
                if w.is_null() {
                    return;
                }
                let conv = send_timeout(w, WM_IME_CONTROL, IMC_GETCONVERSIONMODE, 0)
                    .map(|v| v as u32)
                    .unwrap_or(0);
                send_timeout(
                    w,
                    WM_IME_CONTROL,
                    IMC_SETCONVERSIONMODE,
                    mode_bits(conv, cn) as isize,
                );
            }),
        },
        Technique {
            name: "4. [绝对] 默认 IME 窗口 IMC_SETOPENSTATUS(开/关)",
            kind: Kind::Absolute,
            apply: Box::new(|hwnd, cn| unsafe {
                let w = ImmGetDefaultIMEWnd(hwnd);
                if w.is_null() {
                    return;
                }
                send_timeout(w, WM_IME_CONTROL, IMC_SETOPENSTATUS, if cn { 1 } else { 0 });
            }),
        },
        Technique {
            name: "5. [绝对] 注入 IME_ON / IME_OFF 虚拟键",
            kind: Kind::Absolute,
            apply: Box::new(|_hwnd, cn| send_key(if cn { VK_IME_ON } else { VK_IME_OFF })),
        },
        Technique {
            name: "6. [切换] ImmSimulateHotKey(IME_NONIME_TOGGLE)",
            kind: Kind::Toggle,
            apply: Box::new(|hwnd, _| unsafe {
                ImmSimulateHotKey(hwnd, IME_CHOTKEY_IME_NONIME_TOGGLE);
            }),
        },
        Technique {
            name: "7. [切换] 注入 Shift",
            kind: Kind::Toggle,
            apply: Box::new(|_hwnd, _| {
                send_key(VK_SHIFT);
            }),
        },
        Technique {
            name: "8. [切换] 注入单独 Ctrl",
            kind: Kind::Toggle,
            apply: Box::new(|_hwnd, _| {
                send_key(VK_CONTROL);
            }),
        },
        Technique {
            name: "9. [切换] 注入 Ctrl+Space",
            kind: Kind::Toggle,
            apply: Box::new(|_hwnd, _| {
                send_key_flags(VK_CONTROL, 0);
                send_key(VK_SPACE);
                send_key_flags(VK_CONTROL, KEYEVENTF_KEYUP);
            }),
        },
        Technique {
            name: "10. [切换] 注入 VK_KANA/VK_HANGUL(0x15)",
            kind: Kind::Toggle,
            apply: Box::new(|_hwnd, _| {
                send_key(VK_KANA);
            }),
        },
    ]
}

fn run_technique(t: &Technique, hwnd: HWND, focused: bool) -> bool {
    println!("\n-- {}", t.name);
    if !focused {
        println!("   跳过（未聚焦，行为不可判定）");
        return false;
    }
    let layout_before = unsafe { GetKeyboardLayout(GetCurrentThreadId()) };
    match t.kind {
        Kind::Absolute => {
            (t.apply)(hwnd, false);
            pump(350);
            let (v_en, d_en) = unsafe { behavior(hwnd, focused) };
            let imm_en = unsafe { imm_desc(hwnd) };
            let wnd_en = unsafe { imewnd_desc(hwnd) };

            (t.apply)(hwnd, true);
            pump(350);
            let (v_cn, d_cn) = unsafe { behavior(hwnd, focused) };
            let imm_cn = unsafe { imm_desc(hwnd) };
            let wnd_cn = unsafe { imewnd_desc(hwnd) };

            report_layout_side_effect(layout_before);
            println!("   设英文 -> 行为={} {d_en}", v_en.text());
            println!("            IMM={imm_en} | IMEWnd={wnd_en}");
            println!("   设中文 -> 行为={} {d_cn}", v_cn.text());
            println!("            IMM={imm_cn} | IMEWnd={wnd_cn}");
            let ok = v_en == Verdict::En && v_cn == Verdict::Cn;
            println!("   结论: {}", if ok { "**有效**" } else { "无效" });
            ok
        }
        Kind::Toggle => {
            let (v0, d0) = unsafe { behavior(hwnd, focused) };
            let imm0 = unsafe { imm_desc(hwnd) };
            let wnd0 = unsafe { imewnd_desc(hwnd) };

            (t.apply)(hwnd, false);
            pump(350);

            let (v1, d1) = unsafe { behavior(hwnd, focused) };
            let imm1 = unsafe { imm_desc(hwnd) };
            let wnd1 = unsafe { imewnd_desc(hwnd) };

            report_layout_side_effect(layout_before);
            println!("   行为: {} {d0}  ->  {} {d1}", v0.text(), v1.text());
            println!("   IMM:    {imm0}  ->  {imm1}");
            println!("   IMEWnd: {wnd0}  ->  {wnd1}");
            let ok = v0 != v1 && v0 != Verdict::Unknown && v1 != Verdict::Unknown;
            println!("   结论: {}", if ok { "**有效**" } else { "无效" });
            ok
        }
    }
}

/// 矩阵跑完把状态还原成基线状态（尽力而为）。
fn restore(hwnd: HWND, base: Verdict) {
    let want_cn = base == Verdict::Cn;
    unsafe {
        let himc = ImmGetContext(hwnd);
        if !himc.is_null() {
            let mut conv = 0u32;
            let mut sentence = 0u32;
            ImmGetConversionStatus(himc, &mut conv, &mut sentence);
            ImmSetConversionStatus(himc, mode_bits(conv, want_cn), sentence);
            ImmReleaseContext(hwnd, himc);
        }
        let mut v = behavior(hwnd, true).0;
        for _ in 0..3 {
            if v == base || v == Verdict::Unknown {
                break;
            }
            send_key(VK_SHIFT);
            pump(300);
            v = behavior(hwnd, true).0;
        }
        println!("\n[还原] 目标={} 实际={}", base.text(), v.text());
    }
}

fn report_layout_side_effect(layout_before: *mut core::ffi::c_void) {
    unsafe {
        let after = GetKeyboardLayout(GetCurrentThreadId());
        if after != layout_before {
            println!(
                "   !! 副作用：线程布局 0x{:X} -> 0x{:X}（这是输入语言切换，不是中英文模式切换）",
                layout_before as usize, after as usize
            );
            ActivateKeyboardLayout(layout_before, 0);
            pump(200);
        }
    }
}

// ---------------------------------------------------------------------------
// E. 对着另一个真实进程验证 capsime::switcher::toggle()
// ---------------------------------------------------------------------------

fn phase_e_e2e() {
    println!("\n=== E. 端到端验证 switcher::toggle()（目标是另一个进程的窗口）===");

    let dir = std::env::temp_dir();
    let info = dir.join("capsime_probe_target.txt");
    let _ = std::fs::remove_file(&info);

    let exe = std::env::current_exe().expect("current_exe");
    let child = Command::new(&exe)
        .args(["--as-target", info.to_str().unwrap()])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn();
    let child = match child {
        Ok(c) => c,
        Err(e) => {
            println!("   启动子进程失败：{e}");
            return;
        }
    };

    let deadline = Instant::now() + Duration::from_secs(8);
    let mut target: HWND = std::ptr::null_mut();
    while Instant::now() < deadline && target.is_null() {
        if let Ok(s) = std::fs::read_to_string(&info) {
            if let Some(hex) = s.split_whitespace().next() {
                target = usize::from_str_radix(hex, 16).unwrap_or(0) as HWND;
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    if target.is_null() {
        println!("   未拿到子进程窗口句柄");
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/F"])
            .output();
        return;
    }
    println!("   子进程窗口=0x{:X}", target as usize);

    // 先让自己成为前台，再把前台交给子进程（否则会被前台锁挡住）
    let probe = unsafe { make_probe_window() };
    if !probe.is_null() {
        PROBE_HWND.store(probe as isize, Ordering::SeqCst);
        steal_foreground(probe);
    }
    if !steal_foreground(target) {
        println!("   抢焦点失败，无法验证（换一次运行通常就好了）");
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/F"])
            .output();
        return;
    }
    println!("   前台已切到子进程窗口");

    let observe = |label: &str| -> Option<bool> {
        let ime = unsafe { ImmGetDefaultIMEWnd(target) };
        if ime.is_null() {
            println!("   [{label}] 拿不到 IME 窗口");
            return None;
        }
        let conv = send_timeout(ime, WM_IME_CONTROL, IMC_GETCONVERSIONMODE, 0).map(|v| v as u32)?;
        let open = send_timeout(ime, WM_IME_CONTROL, IMC_GETOPENSTATUS, 0).map(|v| v != 0)?;
        let cn = open && (conv & IME_CMODE_NATIVE) != 0;
        println!(
            "   [{label}] 独立观察：conv=0x{conv:04X} open={} => {}",
            if open { 1 } else { 0 },
            if cn { "中文" } else { "英文" }
        );
        Some(cn)
    };

    let mut prev = observe("初始");
    let mut flips = 0;
    for i in 1..=3 {
        let r = capsime::switcher::toggle();
        pump(400);
        let now = observe(&format!("第 {i} 次 toggle 后"));
        println!(
            "   toggle #{i}: 方式={} 报告状态={:?} verified={} 前台窗口={}",
            r.method.label(),
            r.now_chinese,
            r.verified,
            r.target
        );
        match (prev, now) {
            (Some(b), Some(a)) if a != b => {
                flips += 1;
                println!("      → 独立观察确认翻转 ✓");
            }
            (Some(_), Some(_)) => println!("      → 独立观察显示【没有翻转】 ✗"),
            _ => println!("      → 观察通道不可用，无法判断"),
        }
        prev = now;
    }
    println!("   3 次 toggle 中，独立观察确认翻转 {flips} 次");

    if !probe.is_null() {
        unsafe { PostMessageW(probe, WM_CLOSE, 0, 0) };
        pump(150);
    }
    let _ = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/F"])
        .output();
    println!("   子进程已结束");
}

/// 子进程模式：开一个普通窗口、把句柄写给父进程，然后一直跑消息循环。
/// 父进程用它当"另一个进程的前台窗口"来验证切换。
fn run_as_target(info_path: &str) {
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class_name = util::wide("capsime_probe_target");
        let title = util::wide("capsime probe target");
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(target_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&class);
        let main = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            300,
            200,
            520,
            160,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if main.is_null() {
            return;
        }
        let _ = std::fs::write(info_path, format!("{:X}", main as usize));

        let mut msg: MSG = std::mem::zeroed();
        loop {
            let r = PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE);
            if r != 0 {
                if msg.message == WM_CLOSE {
                    windows_sys::Win32::System::Threading::ExitProcess(0);
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

unsafe extern "system" fn target_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

// ---------------------------------------------------------------------------
// F. 真实应用（Windows 记事本）端到端验证
//
// 记事本会把自己文档的第一行放进窗口标题，所以"标题里出现什么"就是最终事实：
// 中文态输入 nihao+空格 会变成「你好」，英文态则是「nihao」。这条通道能独立于
// IME 状态读回值，直接回答"到底切了没有"。
// ---------------------------------------------------------------------------

fn phase_notepad() {
    println!("\n=== F. 真实应用（记事本）端到端验证 ===");
    println!("（会新开一个记事本、抢一次焦点；全程只操作这个新窗口）");
    println!("（Win11 记事本可能把新窗口并进已有实例；测试完会清空内容，窗口可能需要你手动关）");

    let existing = notepad_windows();
    let child = Command::new("notepad.exe").spawn();
    let child = match child {
        Ok(c) => c,
        Err(e) => {
            println!("   启动记事本失败：{e}");
            return;
        }
    };
    let target = match wait_for_new_notepad(&existing) {
        Some(h) => h,
        None => {
            println!("   没等到新的记事本窗口，放弃");
            let _ = Command::new("taskkill")
                .args(["/PID", &child.id().to_string(), "/F"])
                .output();
            return;
        }
    };
    println!(
        "   记事本窗口=0x{:X} 标题={:?}",
        target as usize,
        title_of(target)
    );

    if !steal_foreground(target) {
        println!("   抢焦点失败，放弃（手离开键盘后重试）");
        // 还没输入过任何东西：直接请求关闭这个空窗口
        unsafe { PostMessageW(target, WM_CLOSE, 0, 0) };
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/F"])
            .output();
        return;
    }

    // 只用"未命名/无标题"的空文档，绝不碰你正在编辑的内容
    let t = title_of(target);
    if !is_untitled(&t) {
        println!("   这个记事本窗口不是空文档（标题={t:?}），为安全起见放弃（不关它）");
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/F"])
            .output();
        return;
    }

    let (v0, d0) = notepad_type_probe(target);
    println!(
        "\n   基线（不做任何切换）: 输入结果={} {d0}\n   IME 读回值: {}",
        v0.text(),
        unsafe { imewnd_desc(target) }
    );

    // 连按三次产品代码：每次都靠"打字出来的是 你好 还是 nihao"判定
    let mut prev = v0;
    let mut flips = 0;
    for i in 1..=3 {
        let r = capsime::switcher::toggle();
        std::thread::sleep(Duration::from_millis(250));
        let (v, d) = notepad_type_probe(target);
        println!(
            "\n   toggle #{i}: 方式={} 报告状态={:?} verified={}",
            r.method.label(),
            r.now_chinese,
            r.verified
        );
        println!("     打字结果={} {d}", v.text());
        println!("     IME 读回值: {}", unsafe { imewnd_desc(target) });
        match (prev, v) {
            (a, b) if a != Verdict::Unknown && b != Verdict::Unknown => {
                if a != b {
                    flips += 1;
                    println!("     → 真的翻转了 ✓（记事本里的输入结果变了）");
                } else {
                    println!("     → 【没有翻转】 ✗");
                }
            }
            _ => println!("     → 判定不了（窗口可能丢了焦点）"),
        }
        prev = v;
    }

    println!("\n   ---- 判读 ----");
    println!("   基线={}；3 次 toggle 中真的翻转 {flips} 次", v0.text());
    if flips == 3 {
        println!("   → switcher 在真实程序里工作正常");
    } else {
        println!("   → 有问题：翻转次数不足，需要继续排查");
    }

    close_test_notepad(target, &child);
}

/// 收尾：清空测试文档再关闭窗口。
///
/// 用"清空 + WM_CLOSE"而不是 taskkill：Win11 记事本是单进程多标签，
/// 杀进程会连带关掉同一个进程里别的标签页（可能是你自己正在写的内容）；
/// 而"未命名 + 未修改"的窗口收到关闭请求会静默关掉，不会弹保存提示。
fn close_test_notepad(target: HWND, child: &std::process::Child) {
    unsafe {
        if GetForegroundWindow() == target {
            send_key_flags(VK_CONTROL, 0);
            send_key(VK_A);
            send_key_flags(VK_CONTROL, KEYEVENTF_KEYUP);
            std::thread::sleep(Duration::from_millis(60));
            send_key(VK_DELETE);
            std::thread::sleep(Duration::from_millis(120));
        }
        PostMessageW(target, WM_CLOSE, 0, 0);
    }
    std::thread::sleep(Duration::from_millis(300));
    // 进程若还挂着（例如它把窗口交给了别的实例），清掉我们起的那个
    let _ = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/F"])
        .output();
}

/// 在记事本里清空 → 输入 `nihao` + 空格 → 用标题判定中文/英文。
fn notepad_type_probe(target: HWND) -> (Verdict, String) {
    unsafe {
        // 安全阀：只在目标记事本确实是前台窗口时才动键盘
        if GetForegroundWindow() != target {
            return (Verdict::Unknown, "未聚焦(跳过)".into());
        }
        // 清空文档：Ctrl+A、Delete
        send_key_flags(VK_CONTROL, 0);
        send_key(VK_A);
        send_key_flags(VK_CONTROL, KEYEVENTF_KEYUP);
        std::thread::sleep(Duration::from_millis(80));
        send_key(VK_DELETE);
        std::thread::sleep(Duration::from_millis(150));

        for vk in [VK_N, VK_I, VK_H, VK_A, VK_O] {
            send_key(vk);
            std::thread::sleep(Duration::from_millis(70));
        }
        std::thread::sleep(Duration::from_millis(200));
        send_key(VK_SPACE); // 中文态下：上屏首选词
        std::thread::sleep(Duration::from_millis(500));

        let title = title_of(target);
        let verdict = if has_cjk(&title) {
            Verdict::Cn
        } else if title.to_ascii_lowercase().contains("nihao") {
            Verdict::En
        } else {
            Verdict::Unknown
        };
        (verdict, format!("标题={title:?}"))
    }
}

fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c))
}

fn is_untitled(title: &str) -> bool {
    let t = title.trim_start_matches('*').trim();
    t.contains("无标题") || t.to_ascii_lowercase().contains("untitled")
}

fn title_of(hwnd: HWND) -> String {
    unsafe { window_desc(hwnd).1 }
}

static NOTEPADS: std::sync::Mutex<Vec<isize>> = std::sync::Mutex::new(Vec::new());

unsafe extern "system" fn collect_notepad_proc(hwnd: HWND, _lparam: LPARAM) -> i32 {
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        if window_desc(hwnd).0 == "Notepad" {
            if let Ok(mut v) = NOTEPADS.lock() {
                v.push(hwnd as isize);
            }
        }
        1
    }
}

fn notepad_windows() -> Vec<isize> {
    unsafe { EnumWindows(Some(collect_notepad_proc), 0) };
    let mut v = NOTEPADS.lock().map(|g| g.clone()).unwrap_or_default();
    v.sort_unstable();
    v
}

fn wait_for_new_notepad(existing: &[isize]) -> Option<HWND> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let now = notepad_windows();
        if let Some(h) = now.iter().find(|h| !existing.contains(h)) {
            return Some(*h as HWND);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

// ---------------------------------------------------------------------------
// 基础设施
// ---------------------------------------------------------------------------

unsafe fn make_probe_window() -> HWND {
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class_name = util::wide("capsime_probe_wnd");
        let title = util::wide("capsime probe");
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(probe_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&class);
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            300,
            200,
            560,
            160,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        )
    }
}

unsafe extern "system" fn probe_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_IME_STARTCOMPOSITION => {
                START_COMP.fetch_add(1, Ordering::SeqCst);
            }
            WM_IME_COMPOSITION => {
                COMPOSITION.fetch_add(1, Ordering::SeqCst);
            }
            WM_CHAR => {
                CHARS.fetch_add(1, Ordering::SeqCst);
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                KEYDOWNS.fetch_add(1, Ordering::SeqCst);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// 抢前台焦点：ALT 解锁 + AttachThreadInput，失败重试几次。
fn steal_foreground(hwnd: HWND) -> bool {
    unsafe {
        for _ in 0..10 {
            if GetForegroundWindow() == hwnd {
                return true;
            }
            // 注入一次 ALT 抬起：让本进程被系统视为"有输入"，从而允许设置前台窗口
            send_key_flags(VK_MENU, 0);
            send_key_flags(VK_MENU, KEYEVENTF_KEYUP);
            pump(120); // 给系统一点时间处理这次输入再抢
            let fg = GetForegroundWindow();
            let fg_tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
            let my_tid = GetCurrentThreadId();
            let attached = fg_tid != my_tid && AttachThreadInput(my_tid, fg_tid, 1) != 0;
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
            if attached {
                // 一定要解绑成功，否则输入队列共享、消息泵会捞出别人的消息
                for _ in 0..3 {
                    if AttachThreadInput(my_tid, fg_tid, 0) != 0 {
                        break;
                    }
                }
            }
            pump(250);
        }
        GetForegroundWindow() == hwnd
    }
}

/// 把本线程布局切到中文（如果还不是）。
unsafe fn pin_chinese_layout(hwnd: HWND) {
    unsafe {
        let tid = GetCurrentThreadId();
        let current = GetKeyboardLayout(tid);
        if (current as usize & 0x3FF) as u16 == LANG_CHINESE {
            return;
        }
        let n = GetKeyboardLayoutList(0, std::ptr::null_mut());
        if n <= 0 {
            return;
        }
        let mut list = vec![std::ptr::null_mut(); n as usize];
        GetKeyboardLayoutList(n, list.as_mut_ptr());
        if let Some(&cn) = list
            .iter()
            .find(|h| (**h as usize & 0x3FF) as u16 == LANG_CHINESE)
        {
            PostMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, 0, cn as isize);
            pump(300);
            println!(
                "[布局] 请求中文布局 0x{:X} 后，本线程布局=0x{:X}",
                cn as usize,
                GetKeyboardLayout(tid) as usize
            );
        }
    }
}

/// 空转消息循环 ms 毫秒。
///
/// 只派发"本线程窗口"的消息：AttachThreadInput 会让输入队列共享，
/// 这时 PeekMessage 可能捞出别的线程/进程的窗口消息，派发它们会被阻塞。
fn pump(ms: u64) {
    let deadline = Instant::now() + Duration::from_millis(ms);
    let my_tid = unsafe { GetCurrentThreadId() };
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while Instant::now() < deadline {
        let has = unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) };
        if has != 0 {
            let mut owner = 0u32;
            unsafe { GetWindowThreadProcessId(msg.hwnd, &mut owner) };
            if owner == my_tid {
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn send_key(vk: u16) {
    send_key_flags(vk, 0);
    send_key_flags(vk, KEYEVENTF_KEYUP);
}

fn send_key_flags(vk: u16, flags: u32) {
    let mut input: INPUT = unsafe { std::mem::zeroed() };
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: vk,
        wScan: 0,
        dwFlags: flags,
        time: 0,
        dwExtraInfo: 0,
    };
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
}

fn mode_bits(conv: u32, want_chinese: bool) -> u32 {
    if want_chinese {
        conv | IME_CMODE_NATIVE
    } else {
        conv & !IME_CMODE_NATIVE
    }
}

/// 行为级判定：注入一次 `n`，看输入法是否开始组字。
unsafe fn behavior(_hwnd: HWND, focused: bool) -> (Verdict, String) {
    unsafe {
        if !focused {
            return (Verdict::Unknown, "未聚焦(跳过)".into());
        }
        // 安全阀：注入按键前确认前台窗口仍是探针窗口，绝不把按键打进别人的程序
        if GetForegroundWindow() != PROBE_HWND.load(Ordering::SeqCst) as HWND {
            return (Verdict::Unknown, "焦点已离开探针(跳过注入)".into());
        }
        START_COMP.store(0, Ordering::SeqCst);
        COMPOSITION.store(0, Ordering::SeqCst);
        CHARS.store(0, Ordering::SeqCst);
        KEYDOWNS.store(0, Ordering::SeqCst);

        send_key(VK_N);
        pump(600);

        let start = START_COMP.load(Ordering::SeqCst);
        let comp = COMPOSITION.load(Ordering::SeqCst);
        let chars = CHARS.load(Ordering::SeqCst);
        let keys = KEYDOWNS.load(Ordering::SeqCst);

        // 取消可能存在的组字，避免影响下一步
        send_key(VK_ESCAPE);
        pump(200);

        let verdict = if start > 0 {
            Verdict::Cn
        } else if chars > 0 {
            Verdict::En
        } else {
            Verdict::Unknown
        };
        (
            verdict,
            format!("(kd={keys} start={start} comp={comp} char={chars})"),
        )
    }
}

/// IMM 视角的只读描述。
unsafe fn imm_desc(hwnd: HWND) -> String {
    unsafe {
        let himc = ImmGetContext(hwnd);
        if himc.is_null() {
            return "ctx=NULL".into();
        }
        let mut conv = 0u32;
        let mut sentence = 0u32;
        let ok = ImmGetConversionStatus(himc, &mut conv, &mut sentence);
        let open = ImmGetOpenStatus(himc) != 0;
        ImmReleaseContext(hwnd, himc);
        let state = if open && (conv & IME_CMODE_NATIVE) != 0 {
            "CN"
        } else {
            "EN"
        };
        format!(
            "ctx=ok open={} conv=0x{conv:04X}({state}) ok={ok}",
            if open { 1 } else { 0 }
        )
    }
}

/// 默认 IME 窗口视角的只读描述。
unsafe fn imewnd_desc(hwnd: HWND) -> String {
    unsafe {
        let w = ImmGetDefaultIMEWnd(hwnd);
        if w.is_null() {
            return "imewnd=NULL".into();
        }
        let Some(conv) = send_timeout(w, WM_IME_CONTROL, IMC_GETCONVERSIONMODE, 0) else {
            return format!("hwnd=0x{:X} 读超时", w as usize);
        };
        let conv = conv as u32;
        let open = send_timeout(w, WM_IME_CONTROL, IMC_GETOPENSTATUS, 0)
            .map(|v| v != 0)
            .unwrap_or(false);
        let state = if open && (conv & IME_CMODE_NATIVE) != 0 {
            "CN"
        } else {
            "EN"
        };
        format!(
            "hwnd=0x{:X} conv=0x{conv:04X}({state}) open={}",
            w as usize,
            if open { 1 } else { 0 }
        )
    }
}

/// 前台线程当前拥有键盘焦点的窗口（跨进程可查）。
#[allow(dead_code)]
fn focus_desc() -> String {
    unsafe {
        let mut gti: GUITHREADINFO = std::mem::zeroed();
        gti.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(0, &mut gti) == 0 {
            return "focus=?".into();
        }
        let (class, _) = window_desc(gti.hwndFocus);
        format!("focus=0x{:X} class={class}", gti.hwndFocus as usize)
    }
}

/// 跨进程同步发消息一律带超时：目标不响应时立刻返回，绝不挂死自己。
fn send_timeout(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> Option<usize> {
    let mut out: usize = 0;
    let r =
        unsafe { SendMessageTimeoutW(hwnd, msg, wparam, lparam, SMTO_ABORTIFHUNG, 400, &mut out) };
    if r == 0 { None } else { Some(out) }
}

unsafe fn window_desc(hwnd: HWND) -> (String, String) {
    const WM_GETTEXT: u32 = 0x000D;
    unsafe {
        let mut class_buf = [0u16; 128];
        let n = GetClassNameW(hwnd, class_buf.as_mut_ptr(), class_buf.len() as i32);
        let class = String::from_utf16_lossy(&class_buf[..n.max(0) as usize]);
        // 标题一律走带超时的 WM_GETTEXT：跨进程取标题绝不阻塞
        let mut title_buf = [0u16; 512];
        let title = match send_timeout(
            hwnd,
            WM_GETTEXT,
            title_buf.len(),
            title_buf.as_mut_ptr() as isize,
        ) {
            Some(n) => String::from_utf16_lossy(&title_buf[..(n as i32).max(0) as usize]),
            None => String::new(),
        };
        (class, title)
    }
}

/// 窗口所属进程的可执行文件名。
fn exe_of(hwnd: HWND) -> String {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return "?".into();
        }
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return "?".into();
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 {
            return "?".into();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().unwrap_or("?").to_string()
    }
}
