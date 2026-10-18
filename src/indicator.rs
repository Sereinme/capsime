//! 切换中英文时，在输入位置附近弹出的「中 / en」提示卡片。
//!
//! ## 为什么用「分层窗口 + 自绘」而不是 GUI 框架
//!
//! 这个提示只需要"一块 24×24、会自己消失的卡片"，但要求很硬：**逐像素透明、点击穿透、
//! 绝不抢焦点**（抢焦点会让 IME 候选窗弹错位置）。原生分层窗口（`WS_EX_LAYERED` +
//! `UpdateLayeredWindow`）正好把这三点直接给到；egui/Slint 这类框架要额外引入数 MB
//! 依赖和 GPU 后端，还得跟框架搏斗才能做到不激活、逐像素透明——不划算。
//! 因此沿用项目已有的做法：自己按像素画（4×4 超采样），文字交给 GDI 画成掩膜。
//!
//! ## 位置：优先插入点，拿不到才退回光标
//!
//! macOS 的提示能贴着输入框弹，是因为它的文本系统要求每个可编辑视图上报插入点
//! （`NSTextInputClient.firstRectForCharacterRange:`）。Windows 没有这个统一契约，
//! 但可以沿三条路找，依次是：
//!
//! 1. [`InsertionPoint::Caret`]：`GetGUIThreadInfo` 的 `rcCaret`（**可跨进程**读前台
//!    线程的插入点），经典编辑控件都会提供；WinUI 这类程序只在光标显示时才给，忽有忽无；
//! 2. [`InsertionPoint::UiaCaret`]：浏览器（Firefox/Chromium）、Electron、WinUI 不创建
//!    Win32 caret，改由 [`crate::uia`] 走 UI Automation 的 `TextPattern` 读插入点
//!    （工作线程在切换时预先查好，见 `app::do_switch`）；
//! 3. [`InsertionPoint::UiaField`]：有的程序（Chromium 系、Windows Terminal）给了
//!    `TextPattern` 却不给插入点矩形，就退到**焦点控件的外框左下角**——至少贴在
//!    输入框上，而不是跑到鼠标那边；
//! 4. [`InsertionPoint::Cursor`]：都拿不到才退回鼠标位置，永远可用。
//!
//! 前两条都要过合理性校验（宽高在 (0,200] 像素内、落在外观可见的前台窗口附近、且在工作
//! 区内），个别程序填客户区坐标的情况会被挡掉。用了哪一个会写进 `CAPSIME_DEBUG` 的诊断
//! 日志，便于按程序排查。
//!
//! ## 卡片外观
//!
//! 卡面用**克莱因蓝**（International Klein Blue，`#002FA7`）+ 白字；其余照 Apple 的
//! 公开设计系统：**没有装饰性渐变**——纯色卡面 + 1px 发丝线 + 一道柔和投影，
//! 形状是超椭圆（Apple 式连续圆角方砖）。亮/暗主题只体现在发丝线、上缘高光与投影
//! 强度上；早先的白/系统灰卡面在内容上几乎"看不见"，所以改成这个高辨识度的蓝。
//!
//! ## 线程与消息
//!
//! 窗口属于**主线程**（与托盘同线程）。工作线程切换成功后往主窗口投
//! `app::WM_SHOW_INDICATOR`，主线程在这里显示；自动隐藏与淡入淡出由 `SetTimer`
//! 驱动，没有轮询。窗口惰性创建，进程退出时销毁。

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CLIP_DEFAULT_PRECIS, ClientToScreen, CreateCompatibleDC, CreateDIBSection, CreateFontW,
    DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_NOCLIP, DT_SINGLELINE,
    DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FW_BOLD, GetMonitorInfoW, HGDIOBJ,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, OUT_TT_PRECIS, SelectObject,
    SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GUITHREADINFO, GetCursorPos, GetGUIThreadInfo,
    GetSystemMetrics, KillTimer, RegisterClassW, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetTimer,
    SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::{log, theme, util};

/// 是否启用提示（托盘菜单可切换）。
static ENABLED: AtomicBool = AtomicBool::new(true);
/// 提示窗口句柄（主线程持有）。
static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// 动画当前步数。
static STEP: AtomicU32 = AtomicU32::new(0);
/// UIA 读到的输入位置（工作线程写入、主线程消费，用过即清）。
static UIA_HINT_X: AtomicIsize = AtomicIsize::new(0);
static UIA_HINT_Y: AtomicIsize = AtomicIsize::new(0);
static UIA_HINT_SET: AtomicBool = AtomicBool::new(false);
/// UIA 读到的焦点控件外框（插入点不可得时的粗锚点）。
static UIA_FIELD_X: AtomicIsize = AtomicIsize::new(0);
static UIA_FIELD_Y: AtomicIsize = AtomicIsize::new(0);
static UIA_FIELD_SET: AtomicBool = AtomicBool::new(false);

const TIMER_ID: usize = 1;
const FRAME_MS: u32 = 40;
/// 动画：3 帧淡入（120ms）→ 12 帧停留（480ms）→ 4 帧淡出（160ms），共约 760ms。
const FADE_IN_FRAMES: u32 = 3;
const HOLD_FRAMES: u32 = 12;
const FADE_OUT_FRAMES: u32 = 4;
const TOTAL_FRAMES: u32 = FADE_IN_FRAMES + HOLD_FRAMES + FADE_OUT_FRAMES;

/// 卡片设计尺寸（100% DPI 下的逻辑像素）、相对锚点的固定间距与阴影留白比例。
const CARD_LOGICAL: i32 = 24;
/// 卡片与锚点之间**看得见**的间距：固定物理像素，不随 DPI 缩放。
///
/// 注意窗口比卡片大一圈阴影留白（[`SHADOW_MARGIN`]，随 DPI 变大），摆放时要把留白
/// 从偏移里扣掉（`place(anchor, side, OFFSET_PX - margin, ..)`），否则同一个逻辑偏移
/// 在 150% 屏上看得见 18px、125% 屏上 16px，两块屏摆在一起 2K 那块明显更远。
const OFFSET_PX: i32 = 6;
const SHADOW_MARGIN: f32 = 0.34;
/// 插入点的合理性上限（像素）：超过即认为程序填的不是屏幕坐标。
const CARET_MAX_PX: i32 = 200;
/// 插入点允许落在前台窗口外的容差（像素）。
const CARET_WINDOW_MARGIN: i32 = 40;

// 最近渲染的一帧（主线程独占，动画复用，避免每帧重画）。
thread_local! {
    static LAST_FRAME: RefCell<Option<(Vec<u8>, i32)>> = const { RefCell::new(None) };
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// 工作线程在切换后调用：记下 UI Automation 读到的输入位置，供本次提示使用。
///
/// 主线程在 [`show`] 里一次性取走（用过即清），迟到的值不会影响下一次提示。
pub fn note_caret_hint(point: POINT) {
    UIA_HINT_X.store(point.x as isize, Ordering::SeqCst);
    UIA_HINT_Y.store(point.y as isize, Ordering::SeqCst);
    UIA_HINT_SET.store(true, Ordering::SeqCst);
}

/// 工作线程在切换后调用：记下焦点控件的外框（插入点不可得时的粗锚点）。
pub fn note_field_hint(point: POINT) {
    UIA_FIELD_X.store(point.x as isize, Ordering::SeqCst);
    UIA_FIELD_Y.store(point.y as isize, Ordering::SeqCst);
    UIA_FIELD_SET.store(true, Ordering::SeqCst);
}

/// 弹出提示（主线程调用）。`chinese` 为真显示「中」，否则显示「en」。
pub fn show(chinese: bool) {
    if !is_enabled() {
        return;
    }
    let Some(hwnd) = ensure_window() else {
        return;
    };
    let Some((anchor, source)) = anchor_point() else {
        log::debug("指示器：拿不到锚点（既无插入点也读不到鼠标位置），跳过");
        return;
    };

    // 先把窗口挪到锚点所在显示器，之后问到的才是那块屏的 DPI
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            anchor.x,
            anchor.y,
            1,
            1,
            SWP_NOACTIVATE,
        )
    };
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let card = scale(CARD_LOGICAL, dpi);
    let margin = (card as f32 * SHADOW_MARGIN).round() as i32;
    let side = card + margin * 2;

    let work = work_area(anchor);
    // 阴影留白是窗口的一部分，而留白随 DPI 变大：把它从偏移里扣掉，
    // 保证"看得见的间距"在任何 DPI 下都等于 OFFSET_PX（否则 150% 屏会比 125% 屏更远）
    let pos = place(anchor, side, OFFSET_PX - margin, work);
    log::debug(&format!(
        "指示器：锚点来自{}（{},{})，DPI {dpi}，卡片 {card}px 放在 ({},{})",
        match source {
            InsertionPoint::Caret => "插入点",
            InsertionPoint::UiaCaret => "输入位置（UIA）",
            InsertionPoint::UiaField => "输入框位置（UIA）",
            InsertionPoint::Cursor => "鼠标位置",
        },
        anchor.x,
        anchor.y,
        pos.x,
        pos.y
    ));

    unsafe {
        SetWindowPos(
            hwnd,
            (-1isize) as HWND, // HWND_TOPMOST：盖在普通窗口之上，但不激活
            pos.x,
            pos.y,
            side,
            side,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }

    let (pixels, side) = render_card(chinese, card, theme::is_light());
    if !blit(hwnd, &pixels, side, 255) {
        log::debug("指示器：UpdateLayeredWindow 失败，跳过本次提示");
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        return;
    }
    LAST_FRAME.with(|slot| *slot.borrow_mut() = Some((pixels, side)));

    STEP.store(1, Ordering::SeqCst);
    unsafe { SetTimer(hwnd, TIMER_ID, FRAME_MS, None) };
}

/// 供预览/测试用的直通 alpha RGBA 版本（`render_card` 产出的是预乘 BGRA）。
pub fn preview_rgba(chinese: bool, card: i32, light: bool) -> (Vec<u8>, i32) {
    let (premul, side) = render_card(chinese, card, light);
    let mut out = vec![0u8; premul.len()];
    for (dst, src) in out.chunks_exact_mut(4).zip(premul.chunks_exact(4)) {
        let a = (src[3] as u32).max(1); // 预乘位图里 alpha=0 时颜色通道本来就是 0
        for c in 0..3 {
            dst[c] = ((src[2 - c] as u32 * 255 + a / 2) / a).min(255) as u8;
        }
        dst[3] = src[3];
    }
    (out, side)
}

/// 退出时清理（主线程调用）。
pub fn shutdown() {
    let hwnd = WINDOW.swap(0, Ordering::SeqCst) as HWND;
    if !hwnd.is_null() {
        unsafe {
            KillTimer(hwnd, TIMER_ID);
            DestroyWindow(hwnd);
        }
    }
}

/// 一帧动画：淡入 → 停留 → 淡出 → 隐藏。
unsafe extern "system" fn tick(hwnd: HWND) {
    let step = STEP.fetch_add(1, Ordering::SeqCst) + 1;
    if step >= TOTAL_FRAMES {
        unsafe { KillTimer(hwnd, TIMER_ID) };
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        return;
    }
    let alpha = if step < FADE_IN_FRAMES {
        (255 * (step + 1)) / FADE_IN_FRAMES
    } else if step < FADE_IN_FRAMES + HOLD_FRAMES {
        255
    } else {
        let done = step - FADE_IN_FRAMES - HOLD_FRAMES;
        (255 * (FADE_OUT_FRAMES - done)) / FADE_OUT_FRAMES
    };
    let Some((pixels, side)) = LAST_FRAME.with(|slot| slot.borrow().clone()) else {
        return;
    };
    blit(hwnd, &pixels, side, alpha.clamp(1, 255) as u8);
}

// --- 位置（纯函数，便于单测）---------------------------------------------

/// 插入点来源，只用于诊断日志与锚点选择。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum InsertionPoint {
    /// 前台线程的 Win32 插入点（`GetGUIThreadInfo` 的 `rcCaret`）
    Caret,
    /// UI Automation 读到的插入点（浏览器/Electron/WinUI 等不创建 Win32 caret 的程序）
    UiaCaret,
    /// UI Automation 给的焦点控件外框：程序不给插入点矩形时的粗锚点
    /// （Chromium 系、Windows Terminal 就是这样）
    UiaField,
    /// 鼠标位置（最后的手段）
    Cursor,
}

/// 计算锚点：Win32 插入点 → UIA 插入点 → UIA 输入框外框 → 鼠标位置。
fn anchor_point() -> Option<(POINT, InsertionPoint)> {
    if let Some(p) = caret_point() {
        return Some((p, InsertionPoint::Caret));
    }
    if let Some(p) = take_hint(&UIA_HINT_SET, &UIA_HINT_X, &UIA_HINT_Y) {
        return Some((p, InsertionPoint::UiaCaret));
    }
    if let Some(p) = take_hint(&UIA_FIELD_SET, &UIA_FIELD_X, &UIA_FIELD_Y) {
        return Some((p, InsertionPoint::UiaField));
    }
    let mut cursor = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut cursor) } == 0 {
        return None;
    }
    Some((cursor, InsertionPoint::Cursor))
}

/// 取一次工作线程记下的 UIA 锚点（用过即清），并做与 Win32 caret 相同的合理性校验。
fn take_hint(set: &AtomicBool, x: &AtomicIsize, y: &AtomicIsize) -> Option<POINT> {
    if !set.swap(false, Ordering::SeqCst) {
        return None;
    }
    let point = POINT {
        x: x.load(Ordering::SeqCst) as i32,
        y: y.load(Ordering::SeqCst) as i32,
    };
    let fg = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    if fg.is_null() {
        return None;
    }
    let mut window: RECT = unsafe { std::mem::zeroed() };
    if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect(fg, &mut window) } == 0 {
        return None;
    }
    caret_point_on_screen(point, window, work_area(point))
}

/// 诊断用：单取一次 Win32 插入点，不改动任何状态（`caret_probe` 用）。
pub fn win32_caret_point() -> Option<POINT> {
    caret_point()
}

/// 诊断用：按提示卡片真实使用的顺序算一次锚点，并给出会走哪条路（不做任何显示）。
pub fn planned_anchor() -> Option<(POINT, &'static str)> {
    let (point, source) = anchor_point()?;
    let label = match source {
        InsertionPoint::Caret => "Win32 插入点",
        InsertionPoint::UiaCaret => "UIA 输入位置",
        InsertionPoint::UiaField => "UIA 输入框外框",
        InsertionPoint::Cursor => "鼠标位置",
    };
    Some((point, label))
}

/// 前台线程的插入点（屏幕物理坐标）。校验不过返回 None。
///
/// `GetGUIThreadInfo` 的 `rcCaret` 报的是 **`hwndCaret` 的客户区坐标**（实测：
/// 文本框里把光标放到第 20 个字符，x ≈ 223 正好是客户区横坐标），所以必须用
/// `ClientToScreen` 转换。这个转换**跨进程安全**（POINT 在我们自己的地址空间里），
/// 而且由 Windows 负责目标窗口的 DPI 映射——我们声明了 per-monitor-v2，
/// 拿到的就是屏幕物理像素，不用自己乘缩放系数。
///
/// 转换后再做合理性校验：插入点必须落在外观可见的前台窗口内、且在工作区内。
/// 万一某个程序填的不是客户区坐标（历史上出现过），这一步会把它挡掉、退回光标。
fn caret_point() -> Option<POINT> {
    let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    if unsafe { GetGUIThreadInfo(0, &mut info) } == 0 || info.hwndCaret.is_null() {
        return None;
    }
    let (w, h) = (
        info.rcCaret.right - info.rcCaret.left,
        info.rcCaret.bottom - info.rcCaret.top,
    );
    if w <= 0 || h <= 0 || w > CARET_MAX_PX || h > CARET_MAX_PX {
        return None;
    }

    let mut point = POINT {
        x: info.rcCaret.left,
        y: info.rcCaret.bottom,
    };
    if unsafe { ClientToScreen(info.hwndCaret, &mut point) } == 0 {
        return None;
    }

    let fg = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    if fg.is_null() {
        return None;
    }
    let mut window: RECT = unsafe { std::mem::zeroed() };
    if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect(fg, &mut window) } == 0 {
        return None;
    }
    caret_point_on_screen(point, window, work_area(point))
}

/// 插入点的合理性校验（纯函数，便于单测）。
pub(crate) fn caret_point_on_screen(point: POINT, window: RECT, work: RECT) -> Option<POINT> {
    if point.x < window.left - CARET_WINDOW_MARGIN
        || point.x > window.right + CARET_WINDOW_MARGIN
        || point.y < window.top - CARET_WINDOW_MARGIN
        || point.y > window.bottom + CARET_WINDOW_MARGIN
    {
        return None; // 不在前台窗口附近：坐标语义不对
    }
    if point.x < work.left || point.x > work.right || point.y < work.top || point.y > work.bottom {
        return None; // 不在工作区内
    }
    Some(point)
}

/// 锚点所在显示器的工作区（物理像素，已排除任务栏）。
fn work_area(at: POINT) -> RECT {
    let monitor = unsafe { MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST) };
    let fallback = || RECT {
        left: 0,
        top: 0,
        right: unsafe { GetSystemMetrics(0) }.max(1),
        bottom: unsafe { GetSystemMetrics(1) }.max(1),
    };
    if monitor.is_null() {
        return fallback();
    }
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return fallback();
    }
    info.rcWork
}

/// 把卡片摆到锚点旁边：默认右下，贴边时翻到左/上，最后钳进工作区。
pub(crate) fn place(anchor: POINT, side: i32, offset: i32, work: RECT) -> POINT {
    let mut x = anchor.x + offset;
    if x + side > work.right {
        x = anchor.x - side - offset; // 右边放不下 → 翻到左侧
    }
    x = x.clamp(work.left, (work.right - side).max(work.left));

    let mut y = anchor.y + offset;
    if y + side > work.bottom {
        y = anchor.y - side - offset; // 下方放不下 → 翻到上方
    }
    y = y.clamp(work.top, (work.bottom - side).max(work.top));

    POINT { x, y }
}

fn scale(logical: i32, dpi: u32) -> i32 {
    ((logical * dpi as i32) + 48) / 96
}

// --- 卡片绘制（自绘，逐像素 4×4 超采样）-----------------------------------

struct CardPalette {
    /// 卡面颜色（纯色，不用装饰性渐变——Apple 的 UI chrome 就没有渐变）
    fill: [f32; 3],
    fill_alpha: f32,
    /// 1px 发丝线
    border: [f32; 3],
    border_alpha: f32,
    /// 上缘微高光（暗色下给"玻璃边"一点精神；亮色下不需要）
    top_highlight: f32,
    /// 投影：向下偏移 + 柔和模糊
    shadow_alpha: f32,
    shadow_blur: f32,
    text: [f32; 3],
}

/// 卡片形状：**超椭圆**（Apple 式连续圆角方砖），`|u|^n + |v|^n = 1`。
/// n 越大越接近方砖；4 是连续圆角的常用近似（iOS 图标约 4~5）。
const SQUIRCLE_N: f32 = 4.0;
/// 投影相对卡片边长的下移量。
const SHADOW_DY: f32 = 0.06;

/// 卡片配色：**克莱因蓝**（International Klein Blue，`#002FA7`）作卡面、白字。
///
/// 亮/暗主题只体现在发丝线、上缘高光与投影强度上——克莱因蓝在浅色和深色背景上
/// 都足够醒目。早先的白/系统灰卡面在内容上几乎"看不见"，这正是换色的原因。
const KLEIN_BLUE: [f32; 3] = [0.0, 47.0, 167.0];

fn palette(light: bool) -> CardPalette {
    if light {
        CardPalette {
            fill: KLEIN_BLUE,
            fill_alpha: 0.97,
            border: [255.0, 255.0, 255.0],
            border_alpha: 0.12,
            top_highlight: 0.0,
            shadow_alpha: 0.24,
            shadow_blur: 0.20,
            text: [255.0, 255.0, 255.0],
        }
    } else {
        CardPalette {
            fill: KLEIN_BLUE,
            fill_alpha: 0.97,
            border: [255.0, 255.0, 255.0],
            border_alpha: 0.16,
            top_highlight: 0.10,
            shadow_alpha: 0.38,
            shadow_blur: 0.20,
            text: [255.0, 255.0, 255.0],
        }
    }
}

/// 渲染卡片，返回 (预乘 BGRA 像素, 边长)。
///
/// `UpdateLayeredWindow` + `ULW_ALPHA` 要求 **32bpp 预乘 alpha**，所以这里直接产出
/// 预乘的 BGRA，省得再转一道。
fn render_card(chinese: bool, card: i32, light: bool) -> (Vec<u8>, i32) {
    let pal = palette(light);
    let margin = (card as f32 * SHADOW_MARGIN).round() as i32;
    let side = card + margin * 2;
    let glyph = text_mask(chinese, card);

    const SAMPLES: u32 = 4;
    let inv = 1.0 / (SAMPLES * SAMPLES) as f32;
    let mut out = vec![0u8; (side * side * 4) as usize];

    for y in 0..side {
        for x in 0..side {
            let mut acc_a = 0.0f32;
            let mut acc = [0.0f32; 3];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    let (a, c) = card_sample(&pal, px, py, margin as f32, card as f32);
                    acc_a += a;
                    for i in 0..3 {
                        acc[i] += c[i] * a;
                    }
                }
            }
            if acc_a <= 0.0 {
                continue;
            }
            let mut rgb = [acc[0] / acc_a, acc[1] / acc_a, acc[2] / acc_a];
            let alpha = (acc_a * inv).clamp(0.0, 1.0);

            // 文字：掩膜是像素级的（GDI 自带灰度抗锯齿），只替换颜色、不额外改 alpha
            let inside = x >= margin && y >= margin && x < margin + card && y < margin + card;
            if inside {
                let cov = glyph
                    .get(((y - margin) * card + (x - margin)) as usize)
                    .copied()
                    .unwrap_or(0) as f32
                    / 255.0;
                if cov > 0.0 {
                    rgb = mix(rgb, pal.text, cov);
                }
            }

            let i = ((y * side + x) * 4) as usize;
            out[i] = (rgb[2] * alpha).round().clamp(0.0, 255.0) as u8; // B（预乘）
            out[i + 1] = (rgb[1] * alpha).round().clamp(0.0, 255.0) as u8; // G
            out[i + 2] = (rgb[0] * alpha).round().clamp(0.0, 255.0) as u8; // R
            out[i + 3] = (alpha * 255.0).round() as u8; // A
        }
    }
    (out, side)
}

/// 单个采样点：卡片（超椭圆卡面 + 1px 发丝线 + 上缘微高光）与外投影。坐标是像素。
fn card_sample(pal: &CardPalette, x: f32, y: f32, margin: f32, card: f32) -> (f32, [f32; 3]) {
    let center = margin + card / 2.0;
    let half = card / 2.0;
    let f = squircle_f(x, y, center, half);

    if f <= 1.0 {
        let mut c = pal.fill;
        // 边界附近 df/du ≈ n，于是 f 与"离边界的像素距离"的换算系数是 n/half
        let edge_px = (1.0 - f) * half / SQUIRCLE_N;
        if edge_px <= 1.0 {
            c = mix(c, pal.border, pal.border_alpha);
            // 只在上缘加高光：这是一道"玻璃边"，不是渐变
            if pal.top_highlight > 0.0 && y < center {
                c = mix(c, [255.0, 255.0, 255.0], pal.top_highlight);
            }
        }
        return (pal.fill_alpha, c);
    }

    // 卡片之外：向下偏移的柔和投影（Apple 产品阴影的比例，等比缩到卡片尺寸上）
    let shifted = squircle_f(x, y - card * SHADOW_DY, center, half);
    let dist_px = (shifted - 1.0) * half / SQUIRCLE_N;
    let blur = card * pal.shadow_blur;
    if dist_px >= blur {
        return (0.0, [0.0; 3]);
    }
    let t = 1.0 - (dist_px / blur).clamp(0.0, 1.0);
    let alpha = pal.shadow_alpha * (t * t * (3.0 - 2.0 * t)); // smoothstep：边缘更柔
    (alpha, [0.0, 0.0, 0.0])
}

/// 超椭圆的隐函数值：`<= 1` 表示在形状内。
///
/// 幂次固定为 4（`SQUIRCLE_N`）并直接展开成乘法，省掉每次采样的 `powf`。
fn squircle_f(x: f32, y: f32, center: f32, half: f32) -> f32 {
    let u = ((x - center) / half).abs();
    let v = ((y - center) / half).abs();
    let (u2, v2) = (u * u, v * v);
    u2 * u2 + v2 * v2
}

/// 卡片上的字形：字体、相对卡片边长的 em 比例、GDI 字重。
///
/// 两个字形的**笔画粗细要对齐**，这里用"2×面积/周长"量过（单位：像素，尺寸是卡片
/// 36px 即 150% 缩放下的实际渲染值）：
///
/// | 字形 | 字重 | 笔画 |
/// | --- | --- | --- |
/// | 中（YaHei UI） | Bold / Regular | 3.71 / 2.22 |
/// | en（Segoe UI） | Bold / Semibold / Regular | 3.64 / 2.67 / 2.18 |
///
/// 结论是"粗配粗、常规配常规"：Semibold 夹在中间，配谁都不像同一字重
/// （早先两边都写 600，实际落到不同字面，看起来就是一粗一细）。
/// 小徽章取粗的那一对，缩到 100% DPI 也不发虚。
struct Glyph {
    face: &'static str,
    em_ratio: f32,
    weight: i32,
    text: &'static str,
}

fn glyph(chinese: bool) -> Glyph {
    if chinese {
        Glyph {
            face: "Microsoft YaHei UI",
            em_ratio: 0.68,
            weight: FW_BOLD as i32,
            text: "中",
        }
    } else {
        Glyph {
            face: "Segoe UI",
            em_ratio: 0.76,
            weight: FW_BOLD as i32,
            text: "en",
        }
    }
}

/// 用系统字体把「中」/「en」画成 8 位覆盖率掩膜（白字黑底，取绿色通道）。
///
/// 画两遍：先测墨迹包围盒，再按包围盒光学居中——小尺寸下这一步决定"看起来正不正"。
fn text_mask(chinese: bool, card: i32) -> Vec<u8> {
    let size = card.max(1) as u32;
    let n = (size * size) as usize;
    let spec = glyph(chinese);
    let face = spec.face;
    let em = (size as f32 * spec.em_ratio).round().max(6.0) as i32;

    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return vec![0; n];
        }
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            ..std::mem::zeroed()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            return vec![0; n];
        }
        let old_bitmap = SelectObject(dc, bitmap as HGDIOBJ);
        let face_w = util::wide(face);
        let font = CreateFontW(
            -em,
            0,
            0,
            0,
            spec.weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_TT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            ANTIALIASED_QUALITY as u32,
            DEFAULT_PITCH as u32,
            face_w.as_ptr(),
        );
        if font.is_null() {
            SelectObject(dc, old_bitmap);
            DeleteObject(bitmap as HGDIOBJ);
            DeleteDC(dc);
            return vec![0; n];
        }
        let old_font = SelectObject(dc, font as HGDIOBJ);
        SetTextColor(dc, 0x00FF_FFFF);
        SetBkMode(dc, TRANSPARENT as i32);

        let glyph_w = util::wide(spec.text);
        let draw = |dx: i32, dy: i32| {
            std::ptr::write_bytes(bits as *mut u8, 0, n * 4);
            let mut rect = RECT {
                left: dx,
                top: dy,
                right: size as i32 + dx,
                bottom: size as i32 + dy,
            };
            DrawTextW(
                dc,
                glyph_w.as_ptr(),
                -1,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOCLIP,
            );
        };

        draw(0, 0);
        let src = std::slice::from_raw_parts(bits as *const u8, n * 4);
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (size, size, 0u32, 0u32);
        for y in 0..size {
            for x in 0..size {
                if src[((y * size + x) * 4) as usize + 1] > 8 {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        let mut mask = vec![0u8; n];
        if max_x >= min_x && max_y >= min_y {
            let cx = (min_x + max_x + 1) as f32 / 2.0;
            let cy = (min_y + max_y + 1) as f32 / 2.0;
            draw(
                (size as f32 / 2.0 - cx).round() as i32,
                (size as f32 / 2.0 - cy).round() as i32,
            );
            let src = std::slice::from_raw_parts(bits as *const u8, n * 4);
            for (i, cell) in mask.iter_mut().enumerate() {
                *cell = src[i * 4 + 1];
            }
        }

        SelectObject(dc, old_font);
        DeleteObject(font as HGDIOBJ);
        SelectObject(dc, old_bitmap);
        DeleteObject(bitmap as HGDIOBJ);
        DeleteDC(dc);
        mask
    }
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

// --- 窗口与位图 -----------------------------------------------------------

fn ensure_window() -> Option<HWND> {
    let existing = WINDOW.load(Ordering::SeqCst) as HWND;
    if !existing.is_null() {
        return Some(existing);
    }
    unsafe {
        let hinst = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null());
        let class_name = util::wide("capsime_indicator_wnd");
        let title = util::wide("capsime indicator");
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            log::debug(&format!(
                "指示器：创建窗口失败（错误码 {}）",
                util::last_error()
            ));
            return None;
        }
        WINDOW.store(hwnd as isize, Ordering::SeqCst);
        Some(hwnd)
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_TIMER && wparam == TIMER_ID {
            tick(hwnd);
        }
        windows_sys::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// 把预乘 BGRA 位图交给分层窗口；`alpha` 是整体不透明度（淡入淡出用）。
fn blit(hwnd: HWND, pixels: &[u8], side: i32, alpha: u8) -> bool {
    let bytes = (side * side * 4) as usize;
    if side <= 0 || pixels.len() < bytes {
        return false;
    }
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return false;
        }
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: side,
            biHeight: -side,
            biPlanes: 1,
            biBitCount: 32,
            ..std::mem::zeroed()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            return false;
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, bytes);
        let old = SelectObject(dc, bitmap as HGDIOBJ);
        let blend = BLENDFUNCTION {
            BlendOp: 0, // AC_SRC_OVER
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let size = SIZE { cx: side, cy: side };
        let src = POINT { x: 0, y: 0 };
        let ok = UpdateLayeredWindow(
            hwnd,
            std::ptr::null_mut(),
            std::ptr::null(),
            &size,
            dc,
            &src,
            0,
            &blend,
            ULW_ALPHA,
        );
        SelectObject(dc, old);
        DeleteObject(bitmap as HGDIOBJ);
        DeleteDC(dc);
        ok != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> RECT {
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        }
    }

    #[test]
    fn places_below_right_of_anchor() {
        let p = place(POINT { x: 500, y: 400 }, 40, 6, work());
        assert_eq!((p.x, p.y), (506, 406));
    }

    #[test]
    fn flips_left_and_up_near_edges() {
        let p = place(POINT { x: 1900, y: 1030 }, 40, 6, work());
        assert!(p.x + 40 <= 1920, "右边贴边时应翻到左侧：({}, {})", p.x, p.y);
        assert!(p.y + 40 <= 1040, "下边贴边时应翻到上方：({}, {})", p.x, p.y);
        assert!(p.x >= 0 && p.y >= 0);
    }

    #[test]
    fn clamps_into_work_area() {
        let w = RECT {
            left: 100,
            top: 50,
            right: 500,
            bottom: 400,
        };
        let p = place(POINT { x: 120, y: 60 }, 40, 6, w);
        assert!(p.x >= 100 && p.y >= 50);
        assert!(p.x + 40 <= 500 && p.y + 40 <= 400);
    }

    #[test]
    fn scales_with_dpi() {
        assert_eq!(scale(24, 96), 24);
        assert_eq!(scale(24, 144), 36);
        assert_eq!(scale(24, 192), 48);
    }

    #[test]
    fn squircle_is_rounder_than_a_square() {
        // 边长 40、中心 20、半边长 20：对角线方向在 0.83×半边长处仍在形状内，
        // 0.87 处已经出界（纯方砖要走到 1.0 才出界）——这就是连续圆角的手感
        let (center, half) = (12.0f32, 12.0f32);
        assert!(squircle_f(center - 0.83 * half, center - 0.83 * half, center, half) < 1.0);
        assert!(squircle_f(center - 0.87 * half, center - 0.87 * half, center, half) > 1.0);
        // 边的中点恰好落在边界上
        assert!((squircle_f(center, 0.0, center, half) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn caret_is_rejected_when_implausible() {
        let w = work();
        let win = RECT {
            left: 200,
            top: 150,
            right: 1200,
            bottom: 900,
        };
        // 落在窗口外（例如填了客户区坐标却当屏幕用）、或跑出工作区的，都要挡掉
        assert!(caret_point_on_screen(POINT { x: 20, y: 30 }, win, w).is_none());
        assert!(caret_point_on_screen(POINT { x: 1800, y: 200 }, win, w).is_none());
        // 窗口内、工作区内的正常插入点应通过
        let p = caret_point_on_screen(POINT { x: 300, y: 220 }, win, w).expect("应通过校验");
        assert_eq!((p.x, p.y), (300, 220));
    }

    #[test]
    fn card_has_premultiplied_alpha() {
        let (pixels, side) = render_card(true, 24, false);
        assert_eq!(side, 24 + (24f32 * SHADOW_MARGIN).round() as i32 * 2);
        assert_eq!(pixels.len(), (side * side * 4) as usize);
        let i = ((side / 2) * side + side / 2) as usize * 4;
        assert!(pixels[i + 3] > 200, "卡片中心应不透明");
        for c in 0..3 {
            assert!(
                pixels[i + c] <= pixels[i + 3],
                "预乘 alpha：颜色通道不应大于 alpha"
            );
        }
        assert_eq!(pixels[3], 0, "左上角应透明");
    }

    #[test]
    fn chinese_and_english_cards_differ() {
        let (cn, _) = render_card(true, 24, false);
        let (en, _) = render_card(false, 24, false);
        assert_ne!(cn, en, "「中」与「en」的卡片必须不同");
    }

    #[test]
    fn light_and_dark_cards_differ() {
        let (dark, _) = render_card(true, 24, false);
        let (light, _) = render_card(true, 24, true);
        assert_ne!(dark, light, "亮/暗主题的卡片必须不同");
    }

    #[test]
    fn card_face_is_klein_blue() {
        // 卡面必须是克莱因蓝 #002FA7（B 远大于 G，G 大于 R），且不是近黑
        let card = 24i32;
        let (pixels, side) = render_card(true, card, false);
        let margin = (card as f32 * SHADOW_MARGIN).round() as i32;
        let mut channels: Vec<Vec<u8>> = vec![Vec::new(); 3];
        for y in (margin + 2)..(margin + card - 2) {
            for x in (margin + 2)..(margin + card - 2) {
                let i = ((y * side + x) * 4) as usize;
                let alpha = pixels[i + 3] as u32;
                if alpha < 240 {
                    continue;
                }
                for (c, out) in channels.iter_mut().enumerate() {
                    out.push(((pixels[i + c] as u32 * 255 + alpha / 2) / alpha).min(255) as u8);
                }
            }
        }
        assert!(!channels[0].is_empty(), "卡片内部应有像素");
        // 中位数必然是卡面、不会落在白色字形上
        let mut median = [0u8; 3];
        for (c, values) in channels.iter_mut().enumerate() {
            values.sort_unstable();
            median[c] = values[values.len() / 2];
        }
        let [b, g, r] = median; // 内存里是 BGRA
        assert!(
            b >= 140 && b > g && g > r,
            "卡面应是克莱因蓝（实测 R{r} G{g} B{b}）"
        );
    }

    #[test]
    fn visible_gap_is_constant_across_dpi() {
        // 窗口带着阴影留白；把留白从偏移里扣掉后，任何 DPI 下"看得见的间距"都等于 OFFSET_PX，
        // 这样 150% 屏与 125% 屏摆在一起不会一大一小
        for dpi in [96, 120, 144, 192] {
            let card = scale(CARD_LOGICAL, dpi);
            let margin = (card as f32 * SHADOW_MARGIN).round() as i32;
            let side = card + margin * 2;
            let anchor = POINT { x: 1000, y: 700 };
            let pos = place(anchor, side, OFFSET_PX - margin, work());
            assert_eq!(pos.x + margin - anchor.x, OFFSET_PX, "DPI {dpi} 的横向间距");
            assert_eq!(pos.y + margin - anchor.y, OFFSET_PX, "DPI {dpi} 的纵向间距");
        }
    }
}
