//! 极简配置文件：`%LOCALAPPDATA%\capsime\config.ini`。
//!
//! 目前有两类配置：切换提示卡片的**底色**与相对锚点的**位置偏好**。格式是
//! `key = value`，`#` 开头为注释；改完保存即可——卡片每次显示前会看一眼文件修改时间，
//! 变了就重读，不需要重启（托盘菜单「编辑配置…」会创建模板并用默认编辑器打开它）。
//!
//! 默认配色取自 Apple 的设计系统：亮色 `#0066CC`（Action Blue）、暗色 `#0A84FF`
//! （macOS 系统蓝暗色版），都配白字。

use std::path::PathBuf;
use std::time::SystemTime;

use crate::{log, util};

/// 亮色主题的默认底色（Apple Action Blue）。
pub const DEFAULT_LIGHT: [u8; 3] = [0x00, 0x66, 0xCC];
/// 暗色主题的默认底色（macOS systemBlue 暗色版）。
pub const DEFAULT_DARK: [u8; 3] = [0x0A, 0x84, 0xFF];

/// 卡片相对锚点的位置偏好。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Placement {
    /// 默认：右下，贴边时自动翻到左侧/上方
    #[default]
    Auto,
    /// 正上方（水平居中于锚点）
    Above,
    /// 正下方（水平居中于锚点）
    Below,
    /// 左侧（垂直居中于锚点）
    Left,
    /// 右侧（垂直居中于锚点）
    Right,
}

impl Placement {
    /// 解析配置值（大小写不敏感；above/top、below/bottom 是同义词）。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "above" | "top" => Some(Self::Above),
            "below" | "bottom" => Some(Self::Below),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }

    /// 规范名（日志与模板里用）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Above => "above",
            Self::Below => "below",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// `Alt+字母 → Ctrl+字母` 的默认翻译表：只含 C 和 V（复制/粘贴）。
/// 位图，bit 0 = A … bit 25 = Z。
pub const DEFAULT_ALT_AS_CTRL: u32 = (1 << 2) | (1 << 21);

/// 从配置读出来的设置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub light: [u8; 3],
    pub dark: [u8; 3],
    pub placement: Placement,
    /// `Alt+字母 → Ctrl+字母` 的翻译表（位图，bit 0 = A … bit 25 = Z）。
    pub alt_as_ctrl: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            light: DEFAULT_LIGHT,
            dark: DEFAULT_DARK,
            placement: Placement::Auto,
            alt_as_ctrl: DEFAULT_ALT_AS_CTRL,
        }
    }
}

/// 配置文件路径。
pub fn path() -> PathBuf {
    util::data_dir().join("config.ini")
}

/// 配置文件模板（文件不存在时写出来）。
const TEMPLATE: &str = "\
# capsime 配置文件
#
# 改完保存，下一次按 CapsLock 即生效，不需要重启。

# 卡片底色：RRGGBB（可带 #）。两套主题各一档——卡面要深到能压住白字，
# 又要亮到能在对应主题的内容上跳出来。
# 默认值取自 Apple 的设计系统：
#   亮色 0066CC —— Action Blue，白字对比 3.8:1
#   暗色 0A84FF —— macOS 系统蓝暗色版，白字 3.7:1
light_color = 0066CC
dark_color = 0A84FF

# 卡片相对锚点（插入点/鼠标）的位置：
#   auto  默认，放在右下方；贴近屏幕边缘时自动翻到左侧/上方
#   above 正上方 / below 正下方（水平居中于锚点）
#   left  左侧   / right 右侧（垂直居中于锚点）
# 首选方向放不下时会自动翻到对面。
position = auto

# 像 macOS 一样用 Alt+字母 代替 Ctrl+字母（只转换列出的字母，其他组合原样放行）。
# Ctrl+C / Ctrl+V 本身不受影响，所以终端里 Ctrl+C 终止命令照常可用。
# 多个字母用逗号分隔；留空即关闭这个功能。
alt_as_ctrl = c, v
";

/// 确保配置文件存在（不存在就写模板），返回路径。
pub fn ensure_file() -> Option<PathBuf> {
    let path = path();
    if !path.exists() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, TEMPLATE) {
            log::warn(&format!("写配置模板失败：{e}"));
            return None;
        }
    }
    Some(path)
}

/// 文件修改时间（自 UNIX 纪元的秒数）；文件不存在或读不到返回 0。
pub fn stamp() -> u32 {
    std::fs::metadata(path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().min(u32::MAX as u64) as u32)
        .unwrap_or(0)
}

/// 读配置；缺项用默认值，格式错误记日志并忽略该项。
pub fn load() -> Settings {
    let mut settings = Settings::default();
    let Ok(text) = std::fs::read_to_string(path()) else {
        return settings;
    };
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "light_color" | "dark_color" => match parse_hex(value) {
                Some(rgb) => {
                    if key == "light_color" {
                        settings.light = rgb;
                    } else {
                        settings.dark = rgb;
                    }
                }
                None => log::warn(&format!(
                    "配置第 {} 行：{} 不是 RRGGBB 颜色，忽略",
                    index + 1,
                    value
                )),
            },
            "position" => match Placement::parse(value) {
                Some(placement) => settings.placement = placement,
                None => log::warn(&format!(
                    "配置第 {} 行：{} 不是位置偏好（auto/above/below/left/right），忽略",
                    index + 1,
                    value
                )),
            },
            "alt_as_ctrl" => settings.alt_as_ctrl = parse_letters(value),
            _ => {}
        }
    }
    settings
}

/// `c, v`（或 `cv`）→ 字母位图；非单个字母的片段忽略。
fn parse_letters(value: &str) -> u32 {
    value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(|token| {
            let mut chars = token.chars();
            let letter = chars.next()?;
            if chars.next().is_some() || !letter.is_ascii_alphabetic() {
                return None;
            }
            Some(1u32 << (letter.to_ascii_uppercase() as u8 - b'A'))
        })
        .fold(0, |mask, bit| mask | bit)
}

/// `0066CC` / `#0066CC` → `[0x00, 0x66, 0xCC]`。
fn parse_hex(value: &str) -> Option<[u8; 3]> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_with_and_without_hash() {
        assert_eq!(parse_hex("0066CC"), Some([0x00, 0x66, 0xCC]));
        assert_eq!(parse_hex("#0A84FF"), Some([0x0A, 0x84, 0xFF]));
        assert_eq!(parse_hex("  ff0000 "), Some([255, 0, 0]));
    }

    #[test]
    fn rejects_bad_hex() {
        for bad in ["", "06CC", "0066CCCC", "GGGGGG", "#", "0x66CC"] {
            assert_eq!(parse_hex(bad), None, "应拒绝 {bad:?}");
        }
    }

    #[test]
    fn defaults_are_apple_blue_and_auto() {
        let d = Settings::default();
        assert_eq!(d.light, [0x00, 0x66, 0xCC]);
        assert_eq!(d.dark, [0x0A, 0x84, 0xFF]);
        assert_eq!(d.placement, Placement::Auto);
        assert_eq!(
            d.alt_as_ctrl, DEFAULT_ALT_AS_CTRL,
            "默认只翻译 Alt+C / Alt+V"
        );
    }

    #[test]
    fn parses_alt_letters() {
        assert_eq!(parse_letters("c, v"), (1 << 2) | (1 << 21));
        assert_eq!(parse_letters("C, V"), (1 << 2) | (1 << 21), "大小写不敏感");
        assert_eq!(parse_letters("  x  "), 1 << 23);
        assert_eq!(parse_letters(""), 0, "空值 = 关闭");
        assert_eq!(parse_letters("cv"), 0, "只认单个字母，连写忽略");
        assert_eq!(parse_letters("c, 1, v"), (1 << 2) | (1 << 21), "忽略非字母");
    }

    #[test]
    fn parses_placement_values() {
        for (text, want) in [
            ("auto", Placement::Auto),
            ("ABOVE", Placement::Above),
            ("top", Placement::Above),
            ("below", Placement::Below),
            ("bottom", Placement::Below),
            (" left ", Placement::Left),
            ("right", Placement::Right),
        ] {
            assert_eq!(Placement::parse(text), Some(want), "解析 {text:?}");
        }
        assert_eq!(Placement::parse("middle"), None);
        assert_eq!(Placement::parse(""), None);
    }
}
