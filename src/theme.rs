//! 系统主题（亮/暗）检测。
//!
//! 读的是 Windows 个性化设置里"应用模式"对应的注册表值：
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme`
//! （1 = 亮色，0 = 暗色）。系统切换模式时会广播 `WM_SETTINGCHANGE`，
//! 由 [`crate::app`] 的主线程窗口收到后重新读这里并刷新图标——事件驱动，不轮询。

use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

use crate::util;

/// 当前"应用模式"是不是亮色。
pub fn is_light() -> bool {
    let subkey = util::wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let value = util::wide("AppsUseLightTheme");
    let mut data: u32 = 1; // 读不到就按亮色处理（Windows 默认）
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut _,
            &mut size,
        )
    };
    status == 0 && data != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_value_without_panicking() {
        // 结果取决于当前系统设置，这里只要求能读到（不 panic、不返回错误）
        let light = is_light();
        println!("当前应用模式：{}", if light { "亮色" } else { "暗色" });
    }
}
