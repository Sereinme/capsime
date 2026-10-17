//! 极简文件日志：%LOCALAPPDATA%\capsime\capsime.log。
//!
//! 只记录启动/退出/切换/错误等低频事件，超过 512KB 自动轮转一次。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let path = dir.join("capsime.log");
    if fs::metadata(&path)
        .map(|m| m.len() > 512 * 1024)
        .unwrap_or(false)
    {
        let _ = fs::rename(&path, dir.join("capsime.old.log"));
    }
    let _ = LOG_PATH.set(path);
}

pub fn path() -> Option<&'static Path> {
    LOG_PATH.get().map(PathBuf::as_path)
}

pub fn info(msg: &str) {
    write("INFO ", msg);
}

pub fn warn(msg: &str) {
    write("WARN ", msg);
}

pub fn error(msg: &str) {
    write("ERROR", msg);
}

pub fn debug(msg: &str) {
    if std::env::var_os("CAPSIME_DEBUG").is_some() {
        write("DEBUG", msg);
    }
}

fn write(level: &str, msg: &str) {
    let Some(path) = LOG_PATH.get() else {
        return;
    };
    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(f, "[{}] {} {}", timestamp(), level, msg);
}

fn timestamp() -> String {
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    )
}
