//! 输入位置探针：看看当前前台程序的插入点能不能读到、会走哪条路。
//!
//! 运行：`cargo run --example caret_probe`
//!
//! 用法：**先把光标点进一个输入框**（浏览器地址栏、网页输入框、编辑器、聊天框都行），
//! 再运行本探针。输出顺序与 capsime 提示卡片实际选锚点的顺序一致：
//! Win32 插入点 → UIA 输入位置 → UIA 输入框外框 → 鼠标位置。

use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

use capsime::{indicator, uia, util};

fn main() {
    println!("capsime 输入位置探针（先点进一个输入框再看结果）\n");
    util::set_per_monitor_dpi_aware();

    match util::foreground_window_names() {
        Some((title, class, exe)) => println!(
            "前台窗口：{title} [{class}] {}",
            exe.unwrap_or_else(|| "?".into())
        ),
        None => {
            println!("没有前台窗口");
            return;
        }
    }

    let win32 = indicator::win32_caret_point();
    println!(
        "1. Win32 插入点：{}",
        match win32 {
            Some(p) => format!("({}, {})", p.x, p.y),
            None => "无（该程序不创建 Win32 caret，浏览器/Electron/WinUI 常见）".into(),
        }
    );

    let rect = uia::caret_rect();
    let anchors = uia::anchors();
    let kind = uia::focused_control_type()
        .map(|id| format!("{}（{}）", uia::control_type_name(id), id))
        .unwrap_or_else(|| "读不到".into());
    println!("   焦点控件类型：{kind}");
    println!(
        "2. UIA 输入位置：{}",
        match rect {
            Some(r) => format!(
                "({:.0}, {:.0}) 矩形 {:.0}×{:.0} → 锚点 ({:.0}, {:.0})",
                r[0],
                r[1],
                r[2],
                r[3],
                r[0],
                r[1] + r[3].max(0.0)
            ),
            None => "无（焦点不在文本上，或该程序没实现 TextPattern）".into(),
        }
    );
    println!(
        "3. UIA 输入框外框：{}",
        match anchors.field {
            Some(p) => format!("({}, {})", p.x, p.y),
            None => "无".into(),
        }
    );

    let mut cursor = POINT { x: 0, y: 0 };
    let cursor_ok = unsafe { GetCursorPos(&mut cursor) } != 0;
    println!(
        "4. 鼠标位置：{}",
        if cursor_ok {
            format!("({}, {})", cursor.x, cursor.y)
        } else {
            "读不到".into()
        }
    );

    // 把这次 UIA 读数交给 indicator（模拟切换时的工作线程），再看它真实的选择：
    // 这一行与卡片显示时用的判断完全同源（含"必须在工作区内、前台窗口附近"的校验）
    if let Some(p) = anchors.caret {
        indicator::note_caret_hint(p);
    } else if let Some(p) = anchors.field {
        indicator::note_field_hint(p);
    }
    match indicator::planned_anchor() {
        Some((p, label)) => {
            println!("\n提示卡片会用的锚点：{label}（{}, {}）", p.x, p.y);
            println!("（卡片显示在锚点的右下方；贴近屏幕边缘时自动翻到左上）");
        }
        None => println!("\n没有可用锚点（提示会跳过）"),
    }
}
