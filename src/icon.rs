//! 托盘图标：不依赖图片资源，纯代码绘制圆角方块 + 白色"中"字。
//!
//! 渲染为 32x32 的 RGBA 像素，交给系统托盘按 DPI 缩放。

/// 图标边长（像素）。
pub const SIZE: u32 = 32;
/// 启用状态：蓝色背景。
pub const COLOR_ON: [u8; 3] = [37, 99, 235];
/// 暂停状态：灰色背景。
pub const COLOR_OFF: [u8; 3] = [130, 130, 138];

/// 渲染一枚 RGBA 图标。
pub fn render(bg: [u8; 3]) -> Vec<u8> {
    let n = SIZE as i32;
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];

    // 圆角背景，边缘做 1px 抗锯齿
    for y in 0..n {
        for x in 0..n {
            let a = corner_alpha(x as f32 + 0.5, y as f32 + 0.5, n as f32, 7.0);
            if a > 0.0 {
                put(&mut px, x, y, bg, (a * 255.0).round() as u8);
            }
        }
    }

    // 白色"中"字：竖笔 + 外框
    let white = [250u8, 250, 250];
    fill(&mut px, 14, 3, 17, 28, white); // 竖笔（比外框上下各出头）
    fill(&mut px, 8, 8, 23, 10, white); // 外框上边
    fill(&mut px, 8, 21, 23, 23, white); // 外框下边
    fill(&mut px, 8, 8, 10, 23, white); // 外框左边
    fill(&mut px, 21, 8, 23, 23, white); // 外框右边

    px
}

fn put(px: &mut [u8], x: i32, y: i32, rgb: [u8; 3], a: u8) {
    let n = SIZE as i32;
    if x < 0 || y < 0 || x >= n || y >= n {
        return;
    }
    let i = ((y * n + x) * 4) as usize;
    px[i] = rgb[0];
    px[i + 1] = rgb[1];
    px[i + 2] = rgb[2];
    px[i + 3] = a;
}

fn fill(px: &mut [u8], x0: i32, y0: i32, x1: i32, y1: i32, rgb: [u8; 3]) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            put(px, x, y, rgb, 255);
        }
    }
}

/// 圆角矩形的覆盖率，用作 alpha（返回 0.0~1.0）。
fn corner_alpha(x: f32, y: f32, size: f32, r: f32) -> f32 {
    let dx = (r - x).max(x - (size - r)).max(0.0);
    let dy = (r - y).max(y - (size - r)).max(0.0);
    let d = (dx * dx + dy * dy).sqrt();
    (r - d + 0.5).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_expected_buffer_size() {
        assert_eq!(render(COLOR_ON).len(), (SIZE * SIZE * 4) as usize);
    }

    #[test]
    fn corners_are_transparent_center_is_opaque() {
        let px = render(COLOR_ON);
        assert_eq!(px[3], 0, "左上角圆角外应透明");
        let center = (((SIZE / 2) * SIZE) + SIZE / 2) as usize * 4;
        assert_eq!(px[center + 3], 255, "中心（中字竖笔）应不透明");
    }

    #[test]
    fn on_and_off_differ_in_color() {
        assert_ne!(render(COLOR_ON), render(COLOR_OFF));
    }
}
