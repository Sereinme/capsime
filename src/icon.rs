//! 托盘图标：不依赖图片资源，按设计稿纯代码绘制"变形 C"。
//!
//! 设计稿见 `assets/icon/capsime-icon.svg`（512 单位），这里按同样比例复现。
//!
//! ## 图形含义
//!
//! 字母 **C**（CapsLock 的 C），环上带一笔书法收锋：
//!
//! - 圆环主体是**几何**的（印刷／拉丁字母）；
//! - 环的下端不是平截的终端，而是顺势扫出、由粗到细**出锋收尖**的一笔，
//!   染成**朱砂红**——传统印泥的颜色（中文书写的意象）；
//! - 上端保持几何圆头横切。一个字形里"几何 + 墨"并置，缩小到 16px 也还是个 C。
//!
//! ## 材质（参考 Apple 应用图标的做法）
//!
//! - **超椭圆 squircle** 底（连续曲率，不是普通圆角矩形）；
//! - **竖向渐变 + 顶部径向柔光**，模拟来自上方的光；
//! - **顶缘高光**勾出边界（代替投影）；
//! - 图形底下压一层**偏移的深色副本**当柔和投影，让它"浮"在底色上；
//! - 两套主题跟随系统：亮色 = 纸白底墨字，暗色 = 靛蓝底白字，朱砂笔锋两侧都用。
//!
//! 渲染用逐像素超采样（每像素 4x4 个采样点）抗锯齿；小于 24px 时省掉高光与投影
//! （那点像素只会糊掉图形），并把图形整体放大一点做光学补偿。

use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};

/// 设计基准尺寸（像素）。托盘按系统 DPI 取更精确的尺寸，见 [`tray_icon`]。
pub const SIZE: u32 = 32;

/// 图标状态：启用（拦截中）/ 暂停。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Enabled,
    Paused,
}

/// 一次渲染的参数：状态 + 系统主题。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Appearance {
    pub style: Style,
    /// 系统是否处于亮色应用模式
    pub light: bool,
}

impl Appearance {
    pub fn new(style: Style, light: bool) -> Self {
        Self { style, light }
    }
}

struct Palette {
    /// 底面竖向渐变（顶 / 底）
    bg: [[f32; 3]; 2],
    /// 字母圆环颜色
    glyph: [f32; 3],
    /// 朱砂笔锋的渐变（左下 / 右上）
    brush: [[f32; 3]; 2],
    /// 图形投影颜色与强度
    shadow: [f32; 3],
    shadow_alpha: f32,
    /// 顶部柔光强度
    glow: f32,
}

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    [r as f32, g as f32, b as f32]
}

impl Appearance {
    fn palette(self) -> Palette {
        match (self.style, self.light) {
            // 启用 + 暗色：靛蓝底、白色 C、朱砂笔锋
            (Style::Enabled, false) => Palette {
                bg: [rgb(110, 123, 255), rgb(42, 51, 190)],
                glyph: rgb(255, 255, 255),
                brush: [rgb(255, 139, 99), rgb(238, 84, 46)],
                shadow: rgb(10, 15, 46),
                shadow_alpha: 0.30,
                glow: 0.14,
            },
            // 启用 + 亮色：纸白底、墨色 C、朱砂笔锋
            (Style::Enabled, true) => Palette {
                bg: [rgb(255, 255, 255), rgb(232, 234, 239)],
                glyph: rgb(29, 29, 31),
                brush: [rgb(255, 139, 99), rgb(232, 74, 36)],
                shadow: rgb(0, 0, 0),
                shadow_alpha: 0.20,
                glow: 0.05,
            },
            // 暂停：整套冷灰（亮色底稍亮、暗色底稍暗）
            (Style::Paused, true) => Palette {
                bg: [rgb(246, 246, 245), rgb(226, 227, 226)],
                glyph: rgb(138, 144, 153),
                brush: [rgb(206, 211, 219), rgb(176, 183, 193)],
                shadow: rgb(0, 0, 0),
                shadow_alpha: 0.16,
                glow: 0.04,
            },
            (Style::Paused, false) => Palette {
                bg: [rgb(74, 80, 96), rgb(38, 42, 52)],
                glyph: rgb(201, 206, 214),
                brush: [rgb(150, 157, 168), rgb(118, 125, 136)],
                shadow: rgb(6, 9, 22),
                shadow_alpha: 0.28,
                glow: 0.10,
            },
        }
    }
}

// --- 几何（按 512 单位设计稿归一化）---------------------------------------

/// 超椭圆（squircle）：半轴与指数。指数越大越接近方形，4.6 接近 iOS 图标。
const SQUIRCLE_R: f32 = 248.0 / 512.0;
const SQUIRCLE_N: f32 = 4.6;
/// 顶缘高光：内缩一圈的超椭圆（宽度差即高光带宽）。
const RIM_R: f32 = 240.0 / 512.0;
const RIM_ALPHA: f32 = 0.18;
/// 顶部柔光的中心与半径。
const GLOW_CX: f32 = 0.5;
const GLOW_CY: f32 = 0.25;
const GLOW_RADIUS: f32 = 0.60;

/// 字母 C：圆环中心、内外半径与开口半角（开口朝右）。
const C_CX: f32 = 256.0;
const C_CY: f32 = 256.0;
const C_R_OUT: f32 = 172.0;
const C_R_IN: f32 = 114.0;
const C_OPEN_DEG: f32 = 42.0;
/// 上端（几何侧）的圆心与半径：圆头横切。
const C_CAP_CX: f32 = 362.33;
const C_CAP_CY: f32 = 160.30;
const C_CAP_R: f32 = 29.0;

/// 下端（中文侧）的书法收锋多边形：由设计稿"折线 + 半宽"采样而来（42 点）。
const C_TAIL: [[f32; 2]; 42] = [
    [380.5, 329.1],
    [386.1, 332.8],
    [391.8, 336.4],
    [397.4, 340.1],
    [403.1, 343.7],
    [408.7, 347.4],
    [414.3, 351.1],
    [420.0, 354.7],
    [425.6, 358.4],
    [431.3, 362.0],
    [439.2, 367.7],
    [442.1, 375.1],
    [445.0, 382.5],
    [448.0, 389.9],
    [450.9, 397.3],
    [453.8, 404.6],
    [456.7, 412.0],
    [459.6, 419.4],
    [462.5, 426.8],
    [465.4, 434.1],
    [466.3, 443.7],
    [456.7, 443.2],
    [449.2, 440.6],
    [441.7, 438.0],
    [434.2, 435.4],
    [426.8, 432.7],
    [419.3, 430.1],
    [411.8, 427.5],
    [404.3, 424.9],
    [396.8, 422.3],
    [389.3, 419.6],
    [386.9, 416.9],
    [382.1, 412.2],
    [377.4, 407.5],
    [372.6, 402.7],
    [367.8, 398.0],
    [363.1, 393.2],
    [358.3, 388.5],
    [353.6, 383.7],
    [348.8, 379.0],
    [344.0, 374.2],
    [380.5, 329.1],
];
/// 笔锋渐变的对角线参数（沿左下 → 右上）。
const TAIL_X0: f32 = 344.0;
const TAIL_Y1: f32 = 444.0;
const TAIL_SPAN: f32 = (467.0 - 344.0) + (444.0 - 329.0);

/// 图形投影的偏移（512 空间）。
const SHADOW_DX: f32 = 4.0 / 512.0;
const SHADOW_DY: f32 = 11.0 / 512.0;
/// 小尺寸光学补偿：以图形中心放大，让 16px 下更结实。
const SMALL_SCALE: f32 = 1.06;
const SMALL_CENTER: [f32; 2] = [272.0 / 512.0, 272.0 / 512.0];

/// 渲染一枚 RGBA 图标（`size` × `size` 像素）。
pub fn render(app: Appearance, size: u32) -> Vec<u8> {
    let size = size.clamp(8, 256);
    let pal = app.palette();
    // 小尺寸省掉高光与投影：16px 下它们只有零点几像素，只会糊掉图形
    let detail = size >= 24;
    let n = size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];

    const SAMPLES: u32 = 4; // 每轴 4 个采样点 → 每像素 16 个
    let inv = 1.0 / (SAMPLES * SAMPLES) as f32;

    for y in 0..size {
        for x in 0..size {
            let mut acc_a = 0.0f32;
            let mut acc = [0.0f32; 3];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / n;
                    let py = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / n;
                    let (a, c) = sample(&pal, px, py, detail);
                    acc_a += a;
                    for i in 0..3 {
                        acc[i] += c[i] * a;
                    }
                }
            }
            let i = ((y * size + x) * 4) as usize;
            if acc_a > 0.0 {
                for c in 0..3 {
                    out[i + c] = (acc[c] / acc_a).round().clamp(0.0, 255.0) as u8;
                }
                out[i + 3] = ((acc_a * inv).min(1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}

/// 按系统 DPI 渲染托盘图标，返回 (RGBA 数据, 边长)。
///
/// 直接按托盘实际需要的像素尺寸渲染（100% 缩放 16px、150% 24px、200% 32px），
/// 避免系统把我们画的 32px 图再缩放一次造成的模糊。
pub fn tray_icon(app: Appearance) -> (Vec<u8>, u32) {
    let dpi = unsafe { GetDpiForSystem() }.max(96) as u32;
    let base = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    let size = ((base * dpi + 48) / 96).clamp(16, 64);
    (render(app, size), size)
}

/// 单个采样点：返回 (覆盖率, 颜色)。
fn sample(pal: &Palette, x: f32, y: f32, detail: bool) -> (f32, [f32; 3]) {
    if !inside_squircle(x, y, SQUIRCLE_R) {
        return (0.0, [0.0; 3]); // 底之外：透明
    }

    // 底色：竖向渐变 + 顶部径向柔光
    let t = (y / 2.0).clamp(0.0, 1.0);
    let mut color = mix(pal.bg[0], pal.bg[1], t);
    let gd = ((x - GLOW_CX).powi(2) + (y - GLOW_CY).powi(2)).sqrt() / GLOW_RADIUS;
    color = mix(
        color,
        [255.0, 255.0, 255.0],
        pal.glow * (1.0 - gd.clamp(0.0, 1.0)),
    );

    if detail {
        // 顶缘高光
        if !inside_squircle(x, y, RIM_R) {
            color = mix(color, [255.0, 255.0, 255.0], RIM_ALPHA);
        }
        // 图形投影：偏移的深色副本
        if inside_glyph(x - SHADOW_DX, y - SHADOW_DY) {
            color = mix(color, pal.shadow, pal.shadow_alpha);
        }
    }

    // 小尺寸：把采样点映射进"放大的图形坐标系"
    let (gx, gy) = if detail {
        (x, y)
    } else {
        (
            SMALL_CENTER[0] + (x - SMALL_CENTER[0]) / SMALL_SCALE,
            SMALL_CENTER[1] + (y - SMALL_CENTER[1]) / SMALL_SCALE,
        )
    };
    if inside_c_ring(gx, gy) {
        color = pal.glyph;
    } else if inside_polygon(gx * 512.0, gy * 512.0) {
        // 朱砂笔锋：沿左下 → 右上的渐变
        let bt = ((gx * 512.0 - TAIL_X0) + (TAIL_Y1 - gy * 512.0)) / TAIL_SPAN;
        color = mix(pal.brush[0], pal.brush[1], bt.clamp(0.0, 1.0));
    }
    (1.0, color)
}

/// 字母 C 的几何部分：圆环 + 上端圆头。
fn inside_c_ring(x: f32, y: f32) -> bool {
    let px = x * 512.0;
    let py = y * 512.0;
    let dx = px - C_CX;
    let dy = py - C_CY;
    let r = (dx * dx + dy * dy).sqrt();
    // 上端圆头（几何侧的横切）
    let cdx = px - C_CAP_CX;
    let cdy = py - C_CAP_CY;
    if cdx * cdx + cdy * cdy <= C_CAP_R * C_CAP_R {
        return true;
    }
    if r < C_R_IN || r > C_R_OUT {
        return false;
    }
    // 开口朝右：屏幕 y 向下，取负换成数学角，开口区间不计入
    let angle = (-dy).atan2(dx).to_degrees();
    angle <= -C_OPEN_DEG || angle >= C_OPEN_DEG
}

/// 整个图形（圆环 + 笔锋），用于投影。
fn inside_glyph(x: f32, y: f32) -> bool {
    inside_c_ring(x, y) || inside_polygon(x * 512.0, y * 512.0)
}

/// 超椭圆：|dx/r|^n + |dy/r|^n ≤ 1。
fn inside_squircle(x: f32, y: f32, r: f32) -> bool {
    let dx = ((x - 0.5) / r).abs();
    let dy = ((y - 0.5) / r).abs();
    dx.powf(SQUIRCLE_N) + dy.powf(SQUIRCLE_N) <= 1.0
}

/// 点是否在多边形内（射线法）。
fn inside_polygon(x: f32, y: f32) -> bool {
    let mut inside = false;
    for (i, &[xi, yi]) in C_TAIL.iter().enumerate() {
        let [xj, yj] = C_TAIL[if i == 0 { C_TAIL.len() - 1 } else { i - 1 }];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
    }
    inside
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    /// 指定颜色占的比例（只看不透明像素）。
    fn color_ratio(rgba: &[u8], size: u32, target: [f32; 3], tol: f32) -> f32 {
        let mut hits = 0;
        let mut total = 0;
        for y in 0..size {
            for x in 0..size {
                let p = px(rgba, size, x, y);
                if p[3] < 200 {
                    continue;
                }
                total += 1;
                let d = (0..3)
                    .map(|c| (p[c] as f32 - target[c]).abs())
                    .fold(0.0f32, f32::max);
                if d < tol {
                    hits += 1;
                }
            }
        }
        hits as f32 / total.max(1) as f32
    }

    fn enabled_dark() -> Appearance {
        Appearance::new(Style::Enabled, false)
    }

    #[test]
    fn renders_expected_buffer_size() {
        assert_eq!(render(enabled_dark(), 32).len(), (32 * 32 * 4) as usize);
        assert_eq!(
            render(Appearance::new(Style::Paused, true), 16).len(),
            (16 * 16 * 4) as usize
        );
    }

    #[test]
    fn squircle_corners_are_transparent() {
        let d = render(enabled_dark(), 32);
        assert_eq!(px(&d, 32, 0, 0)[3], 0, "超椭圆之外应透明");
        assert_eq!(px(&d, 32, 31, 0)[3], 0, "右上角应透明");
    }

    #[test]
    fn ring_is_white_on_indigo() {
        let d = render(enabled_dark(), 32);
        let bg = px(&d, 32, 2, 16);
        assert!(
            bg[2] as i32 - bg[0] as i32 > 60,
            "底色应是靛蓝，实测 {bg:?}"
        );
        let ring = px(&d, 32, 7, 16); // 环左侧（约 512 空间的 112,256）
        assert!(ring[0] > 200 && ring[1] > 200, "环应是白色，实测 {ring:?}");
    }

    #[test]
    fn ring_opens_to_the_right() {
        let d = render(enabled_dark(), 32);
        let gap = px(&d, 32, 24, 16); // 开口中间（约 512 空间的 384,256）
        assert!(gap[0] < 200, "开口处不应是白色环，实测 {gap:?}");
    }

    #[test]
    fn tail_is_cinnabar() {
        let d = render(enabled_dark(), 32);
        let ratio = color_ratio(&d, 32, [244.0, 106.0, 66.0], 90.0);
        assert!(ratio > 0.005, "应能找到朱砂笔锋，实测占比 {ratio}");
    }

    #[test]
    fn glyph_casts_a_soft_shadow() {
        // 不猜坐标：在测试里自己挑出"只有投影、没有图形"的像素，
        // 再和该高度上的纯底色（渐变值）比较，变暗即说明投影生效。
        let app = enabled_dark();
        let d = render(app, 32);
        let pal = app.palette();
        let mut darkened = 0;
        for y in 2..30u32 {
            for x in 2..30u32 {
                let p = [(x as f32 + 0.5) / 32.0, (y as f32 + 0.5) / 32.0];
                let shadowed = inside_glyph(p[0] - SHADOW_DX, p[1] - SHADOW_DY);
                if !shadowed || inside_glyph(p[0], p[1]) {
                    continue; // 这里看不到投影（或本身就是图形本体）
                }
                let actual = px(&d, 32, x, y);
                let t = (p[1] / 2.0).clamp(0.0, 1.0);
                let tile_blue = pal.bg[0][2] + (pal.bg[1][2] - pal.bg[0][2]) * t;
                if (actual[2] as f32) + 20.0 < tile_blue {
                    darkened += 1;
                }
            }
        }
        assert!(darkened > 3, "应有多处像素因投影变暗，实测 {darkened} 处");
    }

    #[test]
    fn light_theme_has_ink_ring() {
        let d = render(Appearance::new(Style::Enabled, true), 32);
        let tile = px(&d, 32, 7, 3); // 顶部中央：纸白底
        assert!(tile[0] > 200, "亮色主题底色应接近白，实测 {tile:?}");
        let ring = px(&d, 32, 7, 16);
        assert!(ring[0] < 120, "亮色主题的环应是墨色，实测 {ring:?}");
    }

    #[test]
    fn paused_is_cool_gray() {
        let d = render(Appearance::new(Style::Paused, false), 32);
        let bg = px(&d, 32, 2, 16);
        let spread = bg[..3].iter().map(|&v| v as i32).max().unwrap()
            - bg[..3].iter().map(|&v| v as i32).min().unwrap();
        assert!(spread <= 30, "暂停态底色应是冷灰，实测色差 {spread}");
    }

    #[test]
    fn styles_and_themes_differ() {
        assert_ne!(
            render(enabled_dark(), 24),
            render(Appearance::new(Style::Paused, false), 24)
        );
        assert_ne!(
            render(Appearance::new(Style::Enabled, true), 24),
            render(enabled_dark(), 24)
        );
    }

    #[test]
    fn tray_size_is_sane_on_any_dpi() {
        let (rgba, size) = tray_icon(enabled_dark());
        assert!((16..=64).contains(&size), "托盘尺寸应在合理范围：{size}");
        assert_eq!(rgba.len(), (size * size * 4) as usize);
    }

    #[test]
    fn small_sizes_keep_the_letter_readable() {
        let d = render(enabled_dark(), 16);
        let bg = px(&d, 16, 1, 8);
        let ring = px(&d, 16, 3, 8);
        assert!(
            ring[0] as i32 - bg[0] as i32 > 80,
            "16px 下 C 应清晰，实测 ring={ring:?} bg={bg:?}"
        );
    }
}
