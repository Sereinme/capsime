//! capsime —— 在 Windows 上用 CapsLock 切换中英文输入法。
//!
//! 模块划分：
//! - [`hook`]：全局键盘钩子，只拦截单独按下的 CapsLock
//! - [`switcher`]：中英文切换（IMM / 默认 IME 窗口 / 键盘布局，三级策略）
//! - [`tray`]：托盘图标与菜单
//! - [`autostart`]：开机自启（注册表 Run 项）
//! - [`app`]：装配、消息循环与生命周期
//! - [`log`]、[`util`]、[`icon`]：基础设施

pub mod app;
pub mod autostart;
pub mod hook;
pub mod icon;
pub mod log;
pub mod switcher;
pub mod tray;
pub mod util;
