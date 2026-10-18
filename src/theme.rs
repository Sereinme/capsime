//! 系统主题（亮/暗）检测，以及让系统绘制的菜单跟随主题。
//!
//! 读的是 Windows 个性化设置里"应用模式"对应的注册表值：
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme`
//! （1 = 亮色，0 = 暗色）。系统切换模式时会广播 `WM_SETTINGCHANGE`，
//! 由 [`crate::app`] 的主线程窗口收到后重新读这里并刷新图标——事件驱动，不轮询。
//!
//! Win32 的弹出菜单（`TrackPopupMenu`，托盘菜单就是它）**默认永远是浅色**，不会
//! 自动跟随系统深色模式；应用必须主动打开 uxtheme 的开关（见
//! [`enable_dark_menus`]），下面那组未公开序号就是做这件事的。

use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

use crate::{log, util};

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

// --- 系统菜单跟随深/浅色 -------------------------------------------------

/// `SetPreferredAppMode` 的参数（未公开）：允许深色。系统是浅色模式时菜单仍是浅色。
const ALLOW_DARK: i32 = 1;
/// uxtheme.dll 在 Win10 1903+ 的导出序号（未公开接口，缺失时安静降级）。
const ORDINAL_SET_PREFERRED_APP_MODE: u16 = 135;
const ORDINAL_FLUSH_MENU_THEMES: u16 = 136;

type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
type FlushMenuThemes = unsafe extern "system" fn();

/// 取 uxtheme 的某个导出序号；DLL 或序号不存在时返回 None（老系统上安静降级）。
unsafe fn uxtheme_ordinal<T>(ordinal: u16) -> Option<T> {
    let name = util::wide("uxtheme.dll");
    let module = unsafe { LoadLibraryW(name.as_ptr()) };
    if module.is_null() {
        return None;
    }
    let proc = unsafe { GetProcAddress(module, ordinal as usize as *const u8) };
    unsafe { std::mem::transmute_copy(&proc) }
}

/// 让系统绘制的菜单跟随系统深色模式（进程级，启动时调用一次；主题变化后可再调）。
///
/// Win32 的 `TrackPopupMenu` 默认只有浅色外观，必须通过 uxtheme 的
/// `SetPreferredAppMode(AllowDark)` + `FlushMenuThemes()` 才会跟随主题
/// ——这是微软未公开但被广泛使用的做法（Explorer 自己也走这条路）。
pub fn enable_dark_menus() {
    unsafe {
        let Some(set_mode) = uxtheme_ordinal::<SetPreferredAppMode>(ORDINAL_SET_PREFERRED_APP_MODE)
        else {
            log::warn("uxtheme 缺少 SetPreferredAppMode，托盘菜单不会跟随深色模式");
            return;
        };
        let Some(flush) = uxtheme_ordinal::<FlushMenuThemes>(ORDINAL_FLUSH_MENU_THEMES) else {
            log::warn("uxtheme 缺少 FlushMenuThemes，托盘菜单不会跟随深色模式");
            return;
        };
        let previous = set_mode(ALLOW_DARK);
        flush();
        log::info(&format!(
            "系统菜单已允许跟随深/浅色（SetPreferredAppMode：{previous} → {ALLOW_DARK}）"
        ));
    }
}

/// 只重刷菜单主题（系统主题刚变过时调用）。
pub fn flush_menu_themes() {
    unsafe {
        if let Some(flush) = uxtheme_ordinal::<FlushMenuThemes>(ORDINAL_FLUSH_MENU_THEMES) {
            flush();
        }
    }
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
