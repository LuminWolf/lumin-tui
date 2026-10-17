//! lumin-tui 渲染层测试共享设施。
//!
//! 与 [`apps/experiment-tui/tests`]（宿主级快照渲染测试）分工：本目录只测 renderer 原语，
//! 走 [`lumin_tui::html`] 入口（不经 dioxus VirtualDom / snapshot），断言 CSS → Buffer 的
//! 翻译正确性。数值配置一律读 [`config/test/tui.json`]，不写死。
//!
//! [`apps/experiment-tui/tests`]: ../../../apps/experiment-tui/tests
//! [`config/test/tui.json`]: ../../../config/test/tui.json

#![allow(dead_code)]
// 按测试文件按需引用，逐 binary 视角存在未用导出
#![allow(unreachable_pub)]

use std::sync::OnceLock;

use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use serde::Deserialize;

use lumin_tui::{html::render_html_to_buffer, render::set_cell_px};

/// `config/test/tui.json` 的加载视图（缺省字段以 `Option` 容错，未列项由调用方默认）。
#[derive(Debug, Deserialize)]
pub struct TuiConfig {
    pub cell_px: f32,
    pub viewport: Viewport,
    pub theme: Theme,
}

#[derive(Debug, Deserialize)]
pub struct Viewport {
    pub cols: u16,
    pub rows: u16,
}

/// 调色板（与 [`apps/experiment-tui`] 的 `theme.rs` 同源，测试直接引用避免写死）。
///
/// [`apps/experiment-tui`]: ../../../apps/experiment-tui
#[derive(Debug, Deserialize)]
pub struct Theme {
    pub base_bg: Rgb,
    pub text: Rgb,
    pub black: Rgb,
    pub dark_gray: Rgb,
    pub gray: Rgb,
    pub white: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
}

/// RGB 三元组（各分量 0..=255）。
pub type Rgb = (u8, u8, u8);

/// 单例配置：`set_cell_px` 是进程内 `OnceLock`，同一测试二进制内首次注入生效，
/// 后续调用被忽略（见 `crates/lumin-tui/src/render.rs::set_cell_px` 语义）。
static CONFIG: OnceLock<TuiConfig> = OnceLock::new();

/// 读取 `config/test/tui.json`，注入 `cell_px`（首次调用生效）。
#[must_use]
pub fn config() -> &'static TuiConfig {
    CONFIG.get_or_init(|| {
        let raw = include_str!("../../../../config/test/tui.json");
        let cfg: TuiConfig =
            serde_json::from_str(raw).expect("config/test/tui.json 应能解析为 TuiConfig");
        set_cell_px(cfg.cell_px);
        cfg
    })
}

/// 测试默认缓冲区尺寸（来自配置，不写死）。
#[must_use]
pub fn viewport() -> (u16, u16) {
    let cfg = config();
    (cfg.viewport.cols, cfg.viewport.rows)
}

/// 主题色 → ratatui Color。
#[must_use]
pub fn rgb(c: Rgb) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

/// 从配置取主题色（避免测试文件直接引用 `cfg.theme.xxx` 时的样板）。
#[must_use]
pub fn theme_color(pick: impl FnOnce(&Theme) -> Rgb) -> Color {
    rgb(pick(&config().theme))
}

/// 按配置 viewport 渲染 HTML 到 Buffer。
#[must_use]
pub fn paint(html: &str) -> Buffer {
    let (cols, rows) = viewport();
    render_html_to_buffer(html, cols, rows, None)
}

/// 指定尺寸渲染 HTML（viewport 覆盖场景）。
#[must_use]
pub fn paint_at(html: &str, cols: u16, rows: u16) -> Buffer {
    let _ = config();
    render_html_to_buffer(html, cols, rows, None)
}

/// 缓冲区内的可见文本（按行拼接）。
#[must_use]
pub fn visible_text(buf: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

/// 去除全部空白（CJK 宽字符第二格为空格填充，需一并忽略）。
#[must_use]
pub fn normalize(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 找到包含 `needle` 的行，返回（行号，整行原文）。未找到 panic。
pub fn row_containing(buf: &Buffer, needle: &str) -> (u16, String) {
    let needle_norm = normalize(needle);
    for y in 0..buf.area.height {
        let line: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        if normalize(&line).contains(&needle_norm) {
            return (y, line);
        }
    }
    panic!("缓冲区中未找到「{needle}」");
}

/// 行文本是否包含 `needle`（去空白后比较）。
#[must_use]
pub fn line_contains(line: &str, needle: &str) -> bool {
    normalize(line).contains(&normalize(needle))
}

/// 缓冲区内首个符号等于 `sym` 的单元格（行优先扫描）。
#[must_use]
pub fn find_sym(buf: &Buffer, sym: &str) -> Option<(u16, u16)> {
    (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .find(|&(x, y)| buf[(x, y)].symbol() == sym)
}

/// 空缓冲区（用作断言基线）。
#[must_use]
pub fn empty_buf(cols: u16, rows: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, cols, rows))
}
