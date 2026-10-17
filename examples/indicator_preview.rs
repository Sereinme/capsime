//! 把切换提示卡片渲染成验收图：字形（中 / en）× 主题（亮 / 暗）× 尺寸（100% / 150% DPI）。
//!
//! 运行：`cargo run --example indicator_preview`
//! 输出：`target/indicator_preview.png`
//!
//! 卡片在深色主题下铺深色底、亮色主题下铺浅色底（就是它实际会浮在的内容旁边），
//! 每格按 3 倍放大看清圆角、发丝线、投影与字形居中。

use capsime::indicator;

const PAD: u32 = 10;
const BG_DARK: [u8; 3] = [32, 34, 40];
const BG_LIGHT: [u8; 3] = [244, 245, 247];

/// 预览放大倍数与格子边长（按最大卡片算，避免 150% 那列被裁掉）。
const SCALE: u32 = 2;
const MAX_CARD_SIDE: u32 = 160;
const CELL: u32 = MAX_CARD_SIDE * SCALE + 24;

struct Cell {
    rgba: Vec<u8>,
    side: u32,
    bg: [u8; 3],
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sizes = [88i32, 132]; // 100% 与 150% DPI 下的卡片边长
    let cases = [
        (true, false, BG_DARK),  // 中 + 暗色主题
        (false, false, BG_DARK), // en + 暗色主题
        (true, true, BG_LIGHT),  // 中 + 亮色主题
        (false, true, BG_LIGHT), // en + 亮色主题
    ];

    let cells: Vec<Cell> = cases
        .iter()
        .flat_map(|&(chinese, light, bg)| {
            sizes.iter().map(move |&card| {
                let (rgba, side) = indicator::preview_rgba(chinese, card, light);
                Cell {
                    rgba,
                    side: side as u32,
                    bg,
                }
            })
        })
        .collect();

    let cols = sizes.len() as u32;
    let rows = cases.len() as u32;
    let w = cols * CELL + (cols + 1) * PAD;
    let h = rows * CELL + (rows + 1) * PAD;
    let mut img = vec![0u8; (w * h * 4) as usize];
    for p in img.chunks_exact_mut(4) {
        p.copy_from_slice(&[120, 124, 132, 255]); // 中性灰分隔底
    }

    for (i, cell) in cells.iter().enumerate() {
        let row = (i as u32) / cols;
        let col = (i as u32) % cols;
        let ox = PAD + col * (CELL + PAD);
        let oy = PAD + row * (CELL + PAD);
        draw_cell(&mut img, w, ox, oy, cell);
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
    println!("已生成 {out}（{w}x{h}：字形 × 主题 × 100%/150% DPI，3 倍放大）");
    Ok(())
}

/// 一格里先铺"卡片实际会浮在其上"的背景，再把卡片按 SCALE 倍最近邻放大居中。
fn draw_cell(img: &mut [u8], stride: u32, ox: u32, oy: u32, cell: &Cell) {
    let drawn = cell.side * SCALE;
    let pad_x = (CELL.saturating_sub(drawn)) / 2;
    let pad_y = (CELL.saturating_sub(drawn)) / 2;
    for y in 0..CELL {
        for x in 0..CELL {
            let mut rgb = cell.bg;
            if x >= pad_x && y >= pad_y && x < pad_x + drawn && y < pad_y + drawn {
                let sx = (x - pad_x) / SCALE;
                let sy = (y - pad_y) / SCALE;
                let si = ((sy * cell.side + sx) * 4) as usize;
                let a = cell.rgba[si + 3] as u32;
                for (c, out) in rgb.iter_mut().enumerate() {
                    *out = ((cell.rgba[si + c] as u32 * a + cell.bg[c] as u32 * (255 - a)) / 255)
                        as u8;
                }
            }
            let di = (((oy + y) * stride) + ox + x) as usize;
            img[di * 4..di * 4 + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
}
