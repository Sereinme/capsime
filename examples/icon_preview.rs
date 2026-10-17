//! 把托盘图标渲染成一张验收图：尺寸 × 启用/暂停 × 亮/暗主题。
//!
//! 运行：`cargo run --example icon_preview`
//! 输出：`target/icon_preview.png`
//!
//! 布局：5 行（16 / 24 / 32 / 48 / 64 px）× 4 列
//! （启用-亮、启用-暗、暂停-亮、暂停-暗）。亮色列铺浅色任务栏底色，
//! 暗色列铺深色任务栏底色，一眼看出"系统切模式时图标跟不跟得上"。

use capsime::icon::{self, Appearance, Style};

const CELL: u32 = 88;
const PAD: u32 = 8;
const TASKBAR_LIGHT: [u8; 3] = [243, 244, 246];
const TASKBAR_DARK: [u8; 3] = [27, 29, 33];

struct Cell {
    rgba: Vec<u8>,
    size: u32,
    bg: [u8; 3],
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sizes = [16u32, 24, 32, 48, 64];
    let cols = [
        (Appearance::new(Style::Enabled, true), TASKBAR_LIGHT),
        (Appearance::new(Style::Enabled, false), TASKBAR_DARK),
        (Appearance::new(Style::Paused, true), TASKBAR_LIGHT),
        (Appearance::new(Style::Paused, false), TASKBAR_DARK),
    ];

    let cells: Vec<Cell> = sizes
        .iter()
        .flat_map(|&size| {
            cols.iter().map(move |&(app, bg)| Cell {
                rgba: icon::render(app, size),
                size,
                bg,
            })
        })
        .collect();

    let rows = sizes.len() as u32;
    let cols_n = cols.len() as u32;
    let w = cols_n * CELL + (cols_n + 1) * PAD;
    let h = rows * CELL + (rows + 1) * PAD;
    let mut img = vec![0u8; (w * h * 4) as usize];
    for p in img.chunks_exact_mut(4) {
        p.copy_from_slice(&[138, 143, 152, 255]); // 中性灰分隔底
    }

    for (i, cell) in cells.iter().enumerate() {
        let row = (i as u32) / cols_n;
        let col = (i as u32) % cols_n;
        let ox = PAD + col * (CELL + PAD);
        let oy = PAD + row * (CELL + PAD);
        draw_cell(&mut img, w, ox, oy, cell);
    }

    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/icon_preview.png".to_string());
    let file = std::fs::File::create(&out)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(&img)?;
    println!("已生成 {out}（{w}x{h}：尺寸 × 启用/暂停 × 亮/暗）");
    Ok(())
}

/// 画一个格子：先铺任务栏底色，再按整数倍放大贴图标。
fn draw_cell(img: &mut [u8], stride: u32, ox: u32, oy: u32, cell: &Cell) {
    for y in 0..CELL {
        for x in 0..CELL {
            let mut rgb = cell.bg;
            let scale = (CELL / cell.size).max(1);
            let drawn = cell.size * scale;
            let pad = (CELL - drawn) / 2;
            if x >= pad && y >= pad && x < pad + drawn && y < pad + drawn {
                let sx = (x - pad) / scale;
                let sy = (y - pad) / scale;
                let si = ((sy * cell.size + sx) * 4) as usize;
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
