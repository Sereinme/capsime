//! 开机自启：维护 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 下的
//! `capsime` 值。只动当前用户，不需要管理员权限。

use std::ffi::OsStr;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};

use crate::util::{wide, wide_os};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "capsime";

/// 是否已配置为开机自启（且指向当前 exe）。
pub fn is_enabled() -> bool {
    let Some(expected) = current_command() else {
        return false;
    };
    let Some(recorded) = read_value() else {
        return false;
    };
    recorded.eq_ignore_ascii_case(&expected)
}

pub fn set_enabled(enabled: bool) -> Result<(), u32> {
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        let rc = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        );
        if rc != ERROR_SUCCESS {
            return Err(rc);
        }

        let rc = if enabled {
            let Some(cmd) = current_command() else {
                RegCloseKey(key);
                return Err(1);
            };
            let data: Vec<u16> = wide_os(OsStr::new(&cmd));
            RegSetValueExW(
                key,
                wide(VALUE_NAME).as_ptr(),
                0,
                REG_SZ,
                data.as_ptr() as *const u8,
                (data.len() * 2) as u32,
            )
        } else {
            let rc = RegDeleteValueW(key, wide(VALUE_NAME).as_ptr());
            if rc == ERROR_FILE_NOT_FOUND {
                ERROR_SUCCESS
            } else {
                rc
            }
        };
        RegCloseKey(key);

        if rc == ERROR_SUCCESS { Ok(()) } else { Err(rc) }
    }
}

/// 注册表里应写入的启动命令（带引号的当前 exe 路径）。
fn current_command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(quote(exe.as_os_str()))
}

fn read_value() -> Option<String> {
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut key,
        ) != ERROR_SUCCESS
        {
            return None;
        }
        let mut ty: u32 = 0;
        let mut buf = [0u16; 1024];
        let mut cb = (buf.len() * 2) as u32;
        let rc = RegQueryValueExW(
            key,
            wide(VALUE_NAME).as_ptr(),
            std::ptr::null(),
            &mut ty,
            buf.as_mut_ptr() as *mut u8,
            &mut cb,
        );
        RegCloseKey(key);
        if rc != ERROR_SUCCESS || ty != REG_SZ {
            return None;
        }
        let count = (cb as usize / 2).saturating_sub(1); // 去掉结尾 NUL
        Some(String::from_utf16_lossy(&buf[..count.min(buf.len())]))
    }
}

fn quote(p: &OsStr) -> String {
    format!("\"{}\"", p.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_wraps_path_with_spaces() {
        let q = quote(OsStr::new(r"C:\Program Files\capsime\capsime.exe"));
        assert_eq!(q, r#""C:\Program Files\capsime\capsime.exe""#);
    }
}
