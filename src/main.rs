#![windows_subsystem = "windows"]

//! capsime 命令行入口。
//!
//! 默认为托盘常驻模式；另提供 `--selftest`（冒烟自检）等调试入口。

fn main() {
    std::panic::set_hook(Box::new(|info| {
        capsime::log::error(&format!("panic: {info}"));
    }));

    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => {
            capsime::util::attach_parent_console();
            capsime::util::console_print(concat!("capsime ", env!("CARGO_PKG_VERSION")));
            0
        }
        Some("--help") | Some("-h") => {
            capsime::util::attach_parent_console();
            capsime::util::console_print(CONSOLE_HELP);
            0
        }
        Some("--selftest") => {
            let seconds = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(3);
            capsime::app::selftest(seconds)
        }
        _ => capsime::app::run(),
    };
    std::process::exit(code);
}

const CONSOLE_HELP: &str = "\
capsime —— CapsLock 切换中英文输入法

用法:
  capsime                  启动（托盘常驻，CapsLock 切换中英文）
  capsime --selftest [秒]   冒烟自检：安装钩子、投递一次切换请求、自动退出（默认 3 秒）
  capsime --version        显示版本
  capsime --help           显示本帮助

日志: %LOCALAPPDATA%\\capsime\\capsime.log
托盘右键菜单: 启用/暂停、开机自动启动、查看日志、退出";
