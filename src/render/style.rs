//! stylo 计算样式 → sRGB 字节色的解析层（渲染与调试共用）。

use style::color::{AbsoluteColor, ColorSpace};
use style::values::computed::Color as ComputedColor;

/// 把 stylo 绝对色（sRGB）转为字节四元组（r, g, b, a，0..=255）。
///
/// stylo 对 srgb 分量统一存 0..=1 浮点，个别 legacy 路径可能给出 0..=255，
/// 按「存在 > 1 的分量即按 0..=255」兜底判别。
pub(crate) fn absolute_to_rgba(abs: &AbsoluteColor) -> (u8, u8, u8, u8) {
    let (r, g, b) = (abs.components.0, abs.components.1, abs.components.2);
    let scale = if r > 1.0 || g > 1.0 || b > 1.0 {
        255.0
    } else {
        1.0
    };
    let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    (
        to_u8(r / scale),
        to_u8(g / scale),
        to_u8(b / scale),
        to_u8(abs.alpha.clamp(0.0, 1.0)),
    )
}

/// 把 stylo 计算色解析为 sRGB 四元组（r, g, b, a，0..=255）。
///
/// `resolve_to_absolute` 已处理 `currentColor` / `color-mix` 等展开。
pub(crate) fn resolve_color(
    color: &ComputedColor,
    current_color: &AbsoluteColor,
) -> (u8, u8, u8, u8) {
    absolute_to_rgba(&color.resolve_to_absolute(current_color))
}

/// 前景字节色 → currentColor 绝对色（供 `resolve_to_absolute` 展开 currentcolor 用）。
pub(crate) fn fg_to_absolute(fg: (u8, u8, u8, u8)) -> AbsoluteColor {
    AbsoluteColor::new(
        ColorSpace::Srgb,
        f32::from(fg.0) / 255.0,
        f32::from(fg.1) / 255.0,
        f32::from(fg.2) / 255.0,
        f32::from(fg.3) / 255.0,
    )
}

/// 全透明返回 `None`（不画）。
pub(crate) fn to_ratatui(color: (u8, u8, u8, u8)) -> Option<ratatui::style::Color> {
    if color.3 == 0 {
        None
    } else {
        Some(ratatui::style::Color::Rgb(color.0, color.1, color.2))
    }
}

/// 调试用：把 stylo 计算色序列化为 `rgba(r,g,b,a)` 字符串。
#[allow(dead_code)]
pub(crate) fn color_css(color: &ComputedColor) -> String {
    let (r, g, b, a) = resolve_color(
        color,
        &AbsoluteColor::new(ColorSpace::Srgb, 0.0, 0.0, 0.0, 1.0),
    );
    format!("rgba({r},{g},{b},{a})")
}

/// 调试用：把 stylo 绝对色序列化为 `rgba(r,g,b,a)` 字符串。
#[allow(dead_code)]
pub(crate) fn absolute_color_css(color: &AbsoluteColor) -> String {
    let (r, g, b, a) = absolute_to_rgba(color);
    format!("rgba({r},{g},{b},{a})")
}
