//! 托盘图标与菜单。
//!
//! 注意：tray-icon / muda 的对象内部使用 `Rc<RefCell<..>>`（`!Send + !Sync`），
//! 只能留在创建它们的主线程上，因此用 `thread_local!` 持有。
//! 菜单事件同样在主线程的消息循环里被派发（muda 在菜单窗口过程里同步调用
//! 事件处理器），所以处理器里访问这些对象是安全的。

use std::cell::RefCell;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::{autostart, hook, icon, log, util};

thread_local! {
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
    static ENABLED_ITEM: RefCell<Option<CheckMenuItem>> = const { RefCell::new(None) };
    static AUTOSTART_ITEM: RefCell<Option<CheckMenuItem>> = const { RefCell::new(None) };
}

pub fn init() -> Result<(), String> {
    let enabled_item = CheckMenuItem::new("启用大写键切换", true, hook::is_enabled(), None);
    let autostart_item = CheckMenuItem::new("开机自动启动", true, autostart::is_enabled(), None);
    let log_item = MenuItem::new("查看日志", true, None);
    let quit_item = MenuItem::new("退出", true, None);

    let menu = Menu::new();
    menu.append(&enabled_item).map_err(|e| e.to_string())?;
    menu.append(&autostart_item).map_err(|e| e.to_string())?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| e.to_string())?;
    menu.append(&log_item).map_err(|e| e.to_string())?;
    menu.append(&quit_item).map_err(|e| e.to_string())?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("capsime — CapsLock 切换中英文")
        .with_icon(make_icon(true)?)
        .build()
        .map_err(|e| e.to_string())?;

    let enabled_id = enabled_item.id().clone();
    let autostart_id = autostart_item.id().clone();
    let log_id = log_item.id().clone();
    let quit_id = quit_item.id().clone();

    TRAY.with(|c| *c.borrow_mut() = Some(tray));
    ENABLED_ITEM.with(|c| *c.borrow_mut() = Some(enabled_item));
    AUTOSTART_ITEM.with(|c| *c.borrow_mut() = Some(autostart_item));

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let id = event.id();
        if *id == enabled_id {
            on_toggle_enabled();
        } else if *id == autostart_id {
            on_toggle_autostart();
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

fn make_icon(enabled: bool) -> Result<Icon, String> {
    let color = if enabled {
        icon::COLOR_ON
    } else {
        icon::COLOR_OFF
    };
    Icon::from_rgba(icon::render(color), icon::SIZE, icon::SIZE).map_err(|e| e.to_string())
}

fn on_toggle_enabled() {
    let now = !hook::is_enabled();
    hook::set_enabled(now);
    ENABLED_ITEM.with(|c| {
        if let Some(item) = c.borrow().as_ref() {
            item.set_checked(now);
        }
    });
    TRAY.with(|c| {
        if let Some(tray) = c.borrow().as_ref() {
            if let Ok(ic) = make_icon(now) {
                let _ = tray.set_icon(Some(ic));
            }
        }
    });
    log::info(if now { "已启用" } else { "已暂停" });
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
