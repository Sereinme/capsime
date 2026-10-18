//! Win32 小工具函数。

use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, HWND};
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

/// 转成以 NUL 结尾的 UTF-16，供 Win32 W 系列 API 使用。
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn wide_os(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn last_error() -> u32 {
    unsafe { GetLastError() }
}

/// 窗口类名。
pub(crate) fn hwnd_class(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

/// 窗口所属进程的可执行文件名（仅用于日志排查）。
pub(crate) fn hwnd_exe(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return None;
    }
    let handle: HANDLE = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) };
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    Some(
        path.rsplit(['\\', '/'])
            .next()
            .unwrap_or(path.as_str())
            .to_string(),
    )
}

/// 前台窗口的三个名字：标题、类名、进程可执行文件名（探针与诊断用）。
///
/// 不暴露带 `HWND` 参数的公开函数，避免把裸指针解引用的责任推给调用方。
pub fn foreground_window_names() -> Option<(String, String, Option<String>)> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    let title = if n > 0 {
        String::from_utf16_lossy(&buf[..n as usize])
    } else {
        String::new()
    };
    Some((title, hwnd_class(hwnd), hwnd_exe(hwnd)))
}

/// GUI 子系统进程默认没有控制台；从终端启动时把父控制台接上。
pub fn attach_parent_console() {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// 声明进程按显示器 DPI 感知（PER_MONITOR_AWARE_V2）。
///
/// 必须在创建任何窗口之前调用。不声明的话，Windows 会把坐标与位图按"系统 DPI"
/// 虚拟化/缩放——副屏缩放不同时，托盘图标与切换提示就会糊或错位。
/// 现代 IME 与输入法提示都按显示器缩放渲染，我们也照做；失败（老系统或已被别处
/// 设定）时忽略，退回系统缩放。
pub fn set_per_monitor_dpi_aware() {
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
    };
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// 本程序的本地数据目录：`%LOCALAPPDATA%\capsime`（日志与配置都在这里）。
pub fn data_dir() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("capsime")
}

/// 向控制台输出一行。
///
/// GUI 子系统进程里 Rust 的 stdout 句柄不可靠，直接写 CONOUT$。
pub fn console_print(line: &str) {
    if let Ok(mut f) = OpenOptions::new().write(true).open("CONOUT$") {
        let _ = writeln!(f, "{line}");
    }
}

/// 用系统默认程序打开文件或目录。
pub fn open_path(path: &Path) {
    let operation = wide("open");
    let file = wide_os(path.as_os_str());
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1, // SW_SHOWNORMAL
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_appends_nul() {
        assert_eq!(wide("ab"), vec![0x61, 0x62, 0x00]);
    }

    #[test]
    fn wide_handles_wide_chars() {
        assert_eq!(wide("中"), vec![0x4E2D, 0x00]);
    }
}
