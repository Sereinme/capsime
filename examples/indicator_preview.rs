//! 把切换提示卡片渲染成验收图：字形（中 / en）× 主题（亮 / 暗）× 尺寸阶梯。
//!
//! 运行：`cargo run --example indicator_preview`
//! 输出：`target/indicator_preview.png`
//!
//! 卡片在深色主题下铺深色底、亮色主题下铺浅色底（就是它实际会浮在的内容旁边），
//! 每格放大看清形状、发丝线、投影与字形居中；尺寸那一列按 100% DPI 的像素值
//! 排开，便于比较"多大才合适"。

use capsime::indicator;

const PAD: u32 = 10;
const BG_DARK: [u8; 3] = [32, 34, 40];
const BG_LIGHT: [u8; 3] = [244, 245, 247];

/// 预览放大倍数；尺寸阶梯（100% DPI 的卡片边长，实际也会按所在显示器 DPI 缩放）。
const SCALE: u32 = 3;
const SIZES: [i32; 4] = [32, 36, 40, 44];
/// 与 indicator.rs 的 SHADOW_MARGIN 一致（算格子大小时要算上阴影留白）。
const SHADOW_MARGIN: f32 = 0.34;

struct Cell {
    rgba: Vec<u8>,
    side: u32,
    bg: [u8; 3],
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (true, false, BG_DARK),  // 中 + 暗色主题
        (false, false, BG_DARK), // en + 暗色主题
        (true, true, BG_LIGHT),  // 中 + 亮色主题
        (false, true, BG_LIGHT), // en + 亮色主题
    ];

    let cells: Vec<Cell> = cases
        .iter()
        .flat_map(|&(chinese, light, bg)| {
            SIZES.iter().map(move |&card| {
                let (rgba, side) = indicator::preview_rgba(chinese, card, light);
                Cell {
                    rgba,
                    side: side as u32,
                    bg,
                }
            })
        })
        .collect();

    let max_side = SIZES
        .iter()
        .map(|&c| c + 2 * (c as f32 * SHADOW_MARGIN).round() as i32)
        .max()
        .unwrap_or(1) as u32;
    let cell = max_side * SCALE + 24;
    let cols = SIZES.len() as u32;
    let rows = cases.len() as u32;
    let w = cols * cell + (cols + 1) * PAD;
    let h = rows * cell + (rows + 1) * PAD;
    let mut img = vec![0u8; (w * h * 4) as usize];
    for p in img.chunks_exact_mut(4) {
        p.copy_from_slice(&[120, 124, 132, 255]); // 中性灰分隔底
    }

    for (i, c) in cells.iter().enumerate() {
        let row = (i as u32) / cols;
        let col = (i as u32) % cols;
        let ox = PAD + col * (cell + PAD);
        let oy = PAD + row * (cell + PAD);
        draw_cell(&mut img, w, ox, oy, cell, c);
    }

    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/indicator_preview.png".to_string());
    let file = std::fs::File::create(&out)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(&img)?;
    println!("已生成 {out}（{w}x{h}：字形 × 主题 × 100%/150% DPI，{SCALE} 倍放大）");
    Ok(())
}

/// 一格里先铺"卡片实际会浮在其上"的背景，再把卡片按 SCALE 倍最近邻放大居中。
fn draw_cell(img: &mut [u8], stride: u32, ox: u32, oy: u32, cell: u32, c: &Cell) {
    let drawn = c.side * SCALE;
    let pad_x = (cell.saturating_sub(drawn)) / 2;
    let pad_y = (cell.saturating_sub(drawn)) / 2;
    for y in 0..cell {
        for x in 0..cell {
            let mut rgb = c.bg;
            if x >= pad_x && y >= pad_y && x < pad_x + drawn && y < pad_y + drawn {
                let sx = (x - pad_x) / SCALE;
                let sy = (y - pad_y) / SCALE;
                let si = ((sy * c.side + sx) * 4) as usize;
                let a = c.rgba[si + 3] as u32;
                for (ch, out) in rgb.iter_mut().enumerate() {
                    *out = ((c.rgba[si + ch] as u32 * a + c.bg[ch] as u32 * (255 - a)) / 255) as u8;
                }
            }
            let di = (((oy + y) * stride) + ox + x) as usize;
            img[di * 4..di * 4 + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
}
