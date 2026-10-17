//! capsime —— 在 Windows 上用 CapsLock 切换中英文输入法。
//!
//! 模块划分：
//! - [`hook`]：全局键盘钩子，只拦截单独按下的 CapsLock
//! - [`switcher`]：中英文切换（读状态 + 注入切换键 + 逐级兜底与核对）
//! - [`tray`]：托盘图标与菜单
//! - [`indicator`]：切换时在输入位置附近弹出的「中 / en」提示卡片
//! - [`uia`]：用 UI Automation 读现代程序（浏览器/Electron/WinUI）的插入点
//! - [`autostart`]：开机自启（注册表 Run 项）
//! - [`app`]：装配、消息循环与生命周期
//! - [`log`]、[`util`]、[`icon`]、[`theme`]：基础设施

pub mod app;
pub mod autostart;
pub mod hook;
pub mod icon;
pub mod indicator;
pub mod log;
pub mod switcher;
pub mod theme;
pub mod tray;
pub mod uia;
pub mod util;
