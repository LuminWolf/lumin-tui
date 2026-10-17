//! 通用 rsx 样式小组件（10px/格行高约定下的行、着色 span、边框标题条）。
//!
//! 无样式表场景：所有样式经 `style` 行内属性注入；1 终端格 = `cell_px()` CSS px
//! （见 [`crate::render::cell_px`]），字号取 `cell_px()`、行高 1，文本行
//! `white-space: pre` 原样保留行文案空格。
//!
//! 命名注意：helper 一律以 `cspan` / `line_of` 等命名，避免与 rsx 元素标签
//! （`div` / `span`）同名——裸写 `span(…)` 在 rsx 里会被解析成元素标签。

use unicode_width::UnicodeWidthChar;

// rsx! 展开在调用点引用裸路径 `dioxus_core::` / `dioxus_elements::` / `dioxus_signals::`；
// 由 dioxus 伞型 prelude 以同名再导出，故 `use ... ::*` 即可就地解析。
use dioxus::prelude::*;

use crate::render::cpx;

/// 颜色常量统一用 RGB 三元组表示。
pub type Rgb = (u8, u8, u8);

/// 文本行样式（1 格高 + 空白原样保留）。按 [`cpx`] 由配置缩放因子现算，不写死 px。
#[must_use]
pub fn line() -> String {
    format!("height:{}px;white-space:pre;", cpx(1))
}

/// 空行（占位 1 格）。按 [`cpx`] 由配置缩放因子现算。
#[must_use]
pub fn gap() -> String {
    format!("height:{}px;", cpx(1))
}

/// RGB 三元组 → `#rrggbb` 字符串（行内样式注入用）。
#[must_use]
pub fn hex(c: Rgb) -> String {
    let (r, g, b) = c;
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// 前景色样式（RGB 三元组 → `color:#rrggbb`）。
#[must_use]
pub fn fg(c: Rgb) -> String {
    format!("color:{}", hex(c))
}

/// 前景色 + 加粗。
#[must_use]
pub fn fg_bold(c: Rgb) -> String {
    format!("color:{};font-weight:bold", hex(c))
}

/// 前景 + 背景反白 + 加粗（列表选中行；背景须配合块级容器使用，
/// 行内 span 不参与背景矩形绘制）。
#[must_use]
pub fn fg_on(fg_c: Rgb, bg: Rgb) -> String {
    format!(
        "color:{};background-color:{};font-weight:bold",
        hex(fg_c),
        hex(bg)
    )
}

/// 整屏根容器样式（背景 / 前景 / 字号行高基调；调用方按需追加 padding / flex）。
/// 字号取 1 格（[`cpx(1)`]）保证一行文本恰好一格高。
#[must_use]
pub fn screen_style(bg: Rgb, fg_c: Rgb) -> String {
    format!(
        "width:100vw;height:100vh;background-color:{};color:{};font-size:{}px;line-height:1;",
        hex(bg),
        hex(fg_c),
        cpx(1),
    )
}

/// 行内着色 span（整段收拢，调用处写 `{cspan(…, …)}`）。
// `impl AsRef<str>` 仅作读取，但调用点混合传 `&str` / `String`，保留按值签名
#[allow(clippy::needless_pass_by_value)]
pub fn cspan(style: String, text: impl AsRef<str>) -> Element {
    let text = text.as_ref();
    rsx! {
        span { style: "{style}", "{text}" }
    }
}

/// 整行单 span：`div.line > span`。
pub fn line_of(style: String, text: impl AsRef<str>) -> Element {
    line_with(line(), style, text)
}

/// 整行单 span，行容器样式可定制（选中反白等背景矩形须落在块级 div 上，
/// 行内 span 不参与背景绘制；如弹框 flex 布局的 `flex-grow` / `flex-shrink`）。
pub fn line_with(
    div_style: impl std::fmt::Display,
    span_style: String,
    text: impl AsRef<str>,
) -> Element {
    let div_style = div_style.to_string();
    let text = text.as_ref();
    rsx! {
        div { style: "{div_style}", {cspan(span_style, text)} }
    }
}

/// 压在边框线上的标题条：底色（面板背景色）遮住被覆盖的边框段；宽度按文本
/// 占格现算注入。`bg` 传面板背景色（如主题底色）。
pub fn title_bar(accent: Rgb, bg: Rgb, text: &str) -> Element {
    title_bar_with(accent, bg, "", text)
}

/// 加粗标题条（存档选择 / 退出菜单 / 命令框弹框标题）。
pub fn title_bar_bold(accent: Rgb, bg: Rgb, text: &str) -> Element {
    title_bar_with(accent, bg, "font-weight:bold;", text)
}

fn title_bar_with(accent: Rgb, bg: Rgb, extra: &str, text: &str) -> Element {
    let cells: u32 = text
        .chars()
        .map(|c| c.width().unwrap_or(0).max(1) as u32)
        .sum();
    let w = cpx(cells);
    let style = format!(
        "position:absolute;left:{}px;top:0;height:{}px;white-space:pre;background-color:{};color:{};{extra}width:{w}px",
        cpx(1),
        cpx(1),
        hex(bg),
        hex(accent),
    );
    rsx! {
        div { style: "{style}", "{text}" }
    }
}

/// 行首标记：选中 `▶`，未选中占一个空格（与原版列对齐）。
#[must_use]
pub fn list_marker(selected: bool) -> &'static str {
    if selected { "▶" } else { " " }
}
