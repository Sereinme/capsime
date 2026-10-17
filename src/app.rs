//! 应用装配：单实例、钩子线程与"切换"工作线程、消息循环与生命周期。
//!
//! 线程模型：
//! - **主线程**：装钩子 + 托盘 + 消息循环。钩子回调在这个线程被系统调用，
//!   回调里只投递 `WM_SWITCH`，保证足够快。
//! - **工作线程**：自己的消息循环，收到 `WM_SWITCH` 后执行 IMM 等可能阻塞的
//!   Win32 调用，前台程序卡死也不会拖累钩子线程。

use std::sync::OnceLock;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentThreadId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
    MessageBoxW, PostMessageW, PostQuitMessage, PostThreadMessageW, RegisterClassW,
    TranslateMessage, WNDCLASSW,
};

use crate::{hook, log, switcher, tray, util};

const WM_QUIT: u32 = 0x0012;
const MB_OK: u32 = 0x0000;
const MB_ICONERROR: u32 = 0x0000_0010;
const MB_SETFOREGROUND: u32 = 0x0001_0000;

/// 正常启动：托盘常驻。
pub fn run() -> i32 {
    if !acquire_single_instance() {
        return 0; // 已有实例在运行，静默退出
    }
    log::init(&data_dir());
    log::info(&format!("capsime {} 启动", env!("CARGO_PKG_VERSION")));

    let worker = match spawn_worker() {
        Ok(w) => w,
        Err(e) => {
            log::error(&format!("工作线程启动失败：{e}"));
            fatal_box("capsime 启动失败", &format!("工作线程启动失败：{e}"));
            return 1;
        }
    };

    hook::set_target(worker.hwnd);
    if let Err(code) = hook::install() {
        log::error(&format!("安装键盘钩子失败：错误码 {code}"));
        fatal_box(
            "capsime 启动失败",
            &format!("无法安装键盘钩子（错误码 {code}）。\n可能被安全软件拦截。"),
        );
        return 1;
    }
    log::info("键盘钩子已安装");

    if let Err(e) = tray::init() {
        log::error(&format!("创建托盘图标失败：{e}"));
        hook::uninstall();
        fatal_box("capsime 启动失败", &format!("创建托盘图标失败：{e}"));
        return 1;
    }
    log::info("托盘已就绪，等待 CapsLock");

    message_loop();

    hook::uninstall();
    stop_worker(worker.tid);
    log::info("capsime 已退出");
    0
}

/// 冒烟自检：`capsime --selftest [秒数]`。
///
/// 安装钩子与工作线程、投递一次切换请求、跑消息循环若干秒后自动退出。
/// 用于验证"钩子能装上、消息链路能跑通"，不影响键盘正常使用（期间会拦截
/// CapsLock，时间很短）。
pub fn selftest(seconds: u64) -> i32 {
    util::attach_parent_console();
    log::init(&data_dir());
    log::info(&format!("selftest 开始（{seconds}s）"));

    let worker = match spawn_worker() {
        Ok(w) => w,
        Err(e) => {
            util::console_print(&format!("FAIL: 工作线程启动失败：{e}"));
            log::error(&format!("selftest: {e}"));
            return 2;
        }
    };
    hook::set_target(worker.hwnd);

    if let Err(code) = hook::install() {
        util::console_print(&format!("FAIL: 键盘钩子安装失败，错误码 {code}"));
        log::error(&format!("selftest: 钩子安装失败 {code}"));
        stop_worker(worker.tid);
        return 3;
    }
    util::console_print("OK: 键盘钩子已安装（本次自检期间会拦截 CapsLock）");
    log::info("selftest: 钩子已安装");

    // 直接投递一次切换请求，验证"钩子线程 -> 工作线程"的链路
    unsafe { PostMessageW(worker.hwnd, hook::WM_SWITCH, 0, 0) };
    util::console_print("INFO: 已投递一次切换请求（结果见日志）");

    let main_tid = unsafe { GetCurrentThreadId() };
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(seconds.max(1)));
        unsafe { PostThreadMessageW(main_tid, WM_QUIT, 0, 0) };
    });
    message_loop();

    hook::uninstall();
    stop_worker(worker.tid);
    util::console_print("OK: selftest 结束，钩子已卸载");
    log::info("selftest: 结束");
    0
}

/// 请求主线程退出（供托盘"退出"调用）。
pub fn request_quit() {
    unsafe { PostQuitMessage(0) };
}

struct Worker {
    hwnd: HWND,
    tid: u32,
}

fn spawn_worker() -> Result<Worker, String> {
    let (tx, rx) = mpsc::channel::<Result<(isize, u32), u32>>();
    thread::Builder::new()
        .name("capsime-worker".into())
        .spawn(move || {
            let hwnd = match create_message_window("capsime_worker_wnd") {
                Ok(h) => h,
                Err(code) => {
                    let _ = tx.send(Err(code));
                    return;
                }
            };
            let tid = unsafe { GetCurrentThreadId() };
            let _ = tx.send(Ok((hwnd as isize, tid)));

            let mut msg: MSG = unsafe { std::mem::zeroed() };
            loop {
                let r = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
                if r <= 0 {
                    break;
                }
                if msg.message == hook::WM_SWITCH {
                    do_switch();
                    continue;
                }
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            unsafe { DestroyWindow(hwnd) };
        })
        .map_err(|e| e.to_string())?;

    match rx.recv() {
        Ok(Ok((hwnd, tid))) => Ok(Worker {
            hwnd: hwnd as HWND,
            tid,
        }),
        Ok(Err(code)) => Err(format!("创建消息窗口失败（错误码 {code}）")),
        Err(_) => Err("工作线程提前退出".into()),
    }
}

fn stop_worker(tid: u32) {
    unsafe { PostThreadMessageW(tid, WM_QUIT, 0, 0) };
}

fn do_switch() {
    let r = switcher::toggle();
    let state = match r.now_chinese {
        Some(true) => "中文",
        Some(false) => "英文",
        None => "未知",
    };
    let note = if r.layout_reverted {
        "；注入的按键被系统当成切换输入语言，已切回原布局"
    } else {
        ""
    };
    match r.method {
        switcher::Method::None => {
            log::warn(&format!(
                "切换未成功：没有可用方式（前台窗口：{}）{note}",
                r.target
            ));
        }
        m if r.verified => {
            log::info(&format!(
                "已切换到{state}（方式：{}，已核对{note}，前台窗口：{}）",
                m.label(),
                r.target
            ));
        }
        m => {
            // 大多现代程序（TSF 输入法）读不回输入法状态，这是正常的，不是失败
            log::info(&format!(
                "已发送切换动作（方式：{}，状态读不回、未核对{note}，前台窗口：{}）",
                m.label(),
                r.target
            ));
        }
    }
}

fn message_loop() {
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        let r = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
        if r <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn create_message_window(class_name: &str) -> Result<HWND, u32> {
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        // 这两个 Vec 必须活到 RegisterClassW / CreateWindowExW 调用之后，
        // 不能写成临时值（否则 lpszClassName 会变成悬垂指针）。
        let class_name_w = util::wide(class_name);
        let title_w = util::wide("capsime");

        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name_w.as_ptr(),
        };
        RegisterClassW(&class);

        let hwnd = CreateWindowExW(
            0,
            class_name_w.as_ptr(),
            title_w.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            (-3isize) as HWND, // HWND_MESSAGE：仅消息窗口
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            Err(util::last_error())
        } else {
            Ok(hwnd)
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn acquire_single_instance() -> bool {
    static MUTEX: OnceLock<isize> = OnceLock::new();
    let handle = unsafe {
        CreateMutexW(
            std::ptr::null(),
            0,
            util::wide("capsime-single-instance").as_ptr(),
        )
    };
    if handle.is_null() {
        return true; // 建不出来就放行，宁多开不错杀
    }
    let duplicate = util::last_error() == ERROR_ALREADY_EXISTS;
    let _ = MUTEX.set(handle as isize); // 持有到进程结束
    !duplicate
}

fn data_dir() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("capsime")
}

fn fatal_box(title: &str, text: &str) {
    let title = util::wide(title);
    let text = util::wide(text);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}
