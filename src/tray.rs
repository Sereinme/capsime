//! 托盘图标与菜单。
//!
//! 图标内容取决于"是否暂停 + 系统亮/暗主题"两件事，状态放在本模块的 `APPEARANCE` 里：
//! - 暂停由托盘菜单切换（[`on_toggle_enabled`]）；
//! - 主题由 [`refresh_theme`] 在收到系统 `WM_SETTINGCHANGE` 广播时重读（见 [`crate::app`]）。
//!
//! 注意：tray-icon / muda 的对象内部使用 `Rc<RefCell<..>>`（`!Send + !Sync`），
//! 只能留在创建它们的主线程上，因此用 `thread_local!` 持有。
//! 菜单事件同样在主线程的消息循环里被派发（muda 在菜单窗口过程里同步调用
//! 事件处理器），所以处理器里访问这些对象是安全的。

use std::cell::RefCell;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::{autostart, hook, icon, indicator, log, theme, util};

thread_local! {
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
    static ENABLED_ITEM: RefCell<Option<CheckMenuItem>> = const { RefCell::new(None) };
    static AUTOSTART_ITEM: RefCell<Option<CheckMenuItem>> = const { RefCell::new(None) };
    static HINT_ITEM: RefCell<Option<CheckMenuItem>> = const { RefCell::new(None) };
    /// 当前图标要表达的状态（启用/暂停 + 亮/暗主题）
    static APPEARANCE: RefCell<icon::Appearance> = const {
        RefCell::new(icon::Appearance {
            style: icon::Style::Enabled,
            light: true,
        })
    };
}

/// 创建托盘。
pub fn init() -> Result<(), String> {
    APPEARANCE.with(|a| {
        let mut app = a.borrow_mut();
        app.style = if hook::is_enabled() {
            icon::Style::Enabled
        } else {
            icon::Style::Paused
        };
        app.light = theme::is_light();
    });

    let enabled_item = CheckMenuItem::new("启用大写键切换", true, hook::is_enabled(), None);
    let autostart_item = CheckMenuItem::new("开机自动启动", true, autostart::is_enabled(), None);
    let hint_item = CheckMenuItem::new("切换时显示提示", true, indicator::is_enabled(), None);
    let log_item = MenuItem::new("查看日志", true, None);
    let quit_item = MenuItem::new("退出", true, None);

    let menu = Menu::new();
    menu.append(&enabled_item).map_err(|e| e.to_string())?;
    menu.append(&autostart_item).map_err(|e| e.to_string())?;
    menu.append(&hint_item).map_err(|e| e.to_string())?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| e.to_string())?;
    menu.append(&log_item).map_err(|e| e.to_string())?;
    menu.append(&quit_item).map_err(|e| e.to_string())?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("capsime — CapsLock 切换中英文")
        .with_icon(make_icon()?)
        .build()
        .map_err(|e| e.to_string())?;

    let enabled_id = enabled_item.id().clone();
    let autostart_id = autostart_item.id().clone();
    let hint_id = hint_item.id().clone();
    let log_id = log_item.id().clone();
    let quit_id = quit_item.id().clone();

    TRAY.with(|c| *c.borrow_mut() = Some(tray));
    ENABLED_ITEM.with(|c| *c.borrow_mut() = Some(enabled_item));
    AUTOSTART_ITEM.with(|c| *c.borrow_mut() = Some(autostart_item));
    HINT_ITEM.with(|c| *c.borrow_mut() = Some(hint_item));

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let id = event.id();
        if *id == enabled_id {
            on_toggle_enabled();
        } else if *id == autostart_id {
            on_toggle_autostart();
        } else if *id == hint_id {
            on_toggle_hint();
        } else if *id == log_id {
            if let Some(p) = log::path() {
                util::open_path(p);
            }
        } else if *id == quit_id {
            log::info("用户选择退出");
            crate::app::request_quit();
        }
    }));

    Ok(())
}

/// 系统主题可能变了（主线程收到 `WM_SETTINGCHANGE` 后调用）：重读一次，变了才换图标。
pub fn refresh_theme() {
    let light = theme::is_light();
    let changed = APPEARANCE.with(|a| {
        let mut app = a.borrow_mut();
        let changed = app.light != light;
        app.light = light;
        changed
    });
    if changed {
        log::info(if light {
            "系统切到亮色模式，托盘图标已跟随"
        } else {
            "系统切到暗色模式，托盘图标已跟随"
        });
        apply_appearance();
    }
}

/// 按当前状态渲染一枚图标（按系统 DPI 直接渲染托盘实际尺寸）。
fn make_icon() -> Result<Icon, String> {
    let (rgba, size) = APPEARANCE.with(|a| icon::tray_icon(*a.borrow()));
    Icon::from_rgba(rgba, size, size).map_err(|e| e.to_string())
}

/// 用当前状态重建托盘图标（必须在主线程调用）。
fn apply_appearance() {
    TRAY.with(|c| {
        if let Some(tray) = c.borrow().as_ref() {
            if let Ok(ic) = make_icon() {
                let _ = tray.set_icon(Some(ic));
            }
        }
    });
}

fn on_toggle_enabled() {
    let now = !hook::is_enabled();
    hook::set_enabled(now);
    ENABLED_ITEM.with(|c| {
        if let Some(item) = c.borrow().as_ref() {
            item.set_checked(now);
        }
    });
    APPEARANCE.with(|a| {
        a.borrow_mut().style = if now {
            icon::Style::Enabled
        } else {
            icon::Style::Paused
        };
    });
    apply_appearance();
    log::info(if now { "已启用" } else { "已暂停" });
}

/// 切换"切换时显示提示"（光标附近的「中 / 英」小卡片）。
fn on_toggle_hint() {
    let now = !indicator::is_enabled();
    indicator::set_enabled(now);
    HINT_ITEM.with(|c| {
        if let Some(item) = c.borrow().as_ref() {
            item.set_checked(now);
        }
    });
    log::info(if now {
        "已开启切换提示"
    } else {
        "已关闭切换提示"
    });
}

fn on_toggle_autostart() {
    let now = !autostart::is_enabled();
    match autostart::set_enabled(now) {
        Ok(()) => {
            AUTOSTART_ITEM.with(|c| {
                if let Some(item) = c.borrow().as_ref() {
                    item.set_checked(now);
                }
            });
            log::info(if now {
                "已开启开机自启"
            } else {
                "已关闭开机自启"
            });
        }
        Err(code) => {
            log::error(&format!("设置开机自启失败：错误码 {code}"));
            let actual = autostart::is_enabled();
            AUTOSTART_ITEM.with(|c| {
                if let Some(item) = c.borrow().as_ref() {
                    item.set_checked(actual);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 启用/暂停 × 亮/暗 四种组合都要能构建出托盘图标。
    #[test]
    fn icon_builds_for_every_appearance() {
        for style in [icon::Style::Enabled, icon::Style::Paused] {
            for light in [true, false] {
                APPEARANCE.with(|a| {
                    *a.borrow_mut() = icon::Appearance { style, light };
                });
                assert!(make_icon().is_ok(), "图标构建失败：{style:?} light={light}");
            }
        }
    }
}
