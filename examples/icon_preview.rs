//! 把托盘图标渲染成 PNG 预览图，便于肉眼检查绘制效果。
//!
//! 运行：`cargo run --example icon_preview`
//! 输出：`target/icon_preview.png`（8 倍最近邻放大，并排放启用/暂停两枚图标）

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const SCALE: u32 = 8;
    const GAP: u32 = 4;

    let size = capsime::icon::SIZE;
    let icons = [
        capsime::icon::render(capsime::icon::COLOR_ON),
        capsime::icon::render(capsime::icon::COLOR_OFF),
    ];

    let w = size * 2 + GAP;
    let h = size;
    let mut px = vec![0u8; (w * h * 4) as usize];
    for p in px.chunks_exact_mut(4) {
        p.copy_from_slice(&[245, 245, 245, 255]); // 浅灰底，方便观察圆角与间隙
    }

    // 把两枚图标叠到浅灰背景上
    for (n, icon) in icons.iter().enumerate() {
        let x0 = n as u32 * (size + GAP);
        for y in 0..size {
            for x in 0..size {
                let si = ((y * size + x) * 4) as usize;
                let di = ((y * w + x0 + x) * 4) as usize;
                let a = icon[si + 3] as u32;
                let bg = 245u32;
                for c in 0..3 {
                    px[di + c] = ((icon[si + c] as u32 * a + bg * (255 - a)) / 255) as u8;
                }
                px[di + 3] = 255;
            }
        }
    }

    // 最近邻放大
    let (uw, uh) = (w * SCALE, h * SCALE);
    let mut up = vec![0u8; (uw * uh * 4) as usize];
    for y in 0..uh {
        for x in 0..uw {
            let si = (((y / SCALE) * w + x / SCALE) * 4) as usize;
            let di = ((y * uw + x) * 4) as usize;
            up[di..di + 4].copy_from_slice(&px[si..si + 4]);
        }
    }

    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/icon_preview.png".to_string());
    let file = std::fs::File::create(&out)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), uw, uh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(&up)?;
    println!("已生成 {out}");
    Ok(())
}
