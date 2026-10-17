//! 边框渲染测试：`border_*_width` / `border_*_style` / `border_*_color` 逐边判定（T3）。
//!
//! 断言 [`lumin_tui::render::paint::draw_borders`] 的四边独立语义：
//! - 单边可见 → 只画该边、无角；
//! - 四边同 solid → 完整 box + 角字符（experiment-tui 常用，与 v1 行为等价）；
//! - 颜色透明（alpha=0）视为不可见。

mod common;
use common::{find_sym, paint};

/// 顶边单色 → 只画 `─`，不画角。
#[test]
fn single_edge_no_corner() {
    let html = concat!(
        r#"<html><body style="margin:0;padding:0">"#,
        r#"<div style="position:absolute;left:0px;top:0px;width:40px;height:20px;"#,
        r#" border-top:1px solid #ff0000;">x</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    // 顶行应含 ─
    let top_row = (0..buf.area.width)
        .map(|x| buf[(x, 0)].symbol())
        .collect::<String>();
    assert!(top_row.contains('─'), "顶行应含 ─，实际：{top_row}");
    // 底行不应有 ─（bottom border 不可见）
    let bottom_row = (0..buf.area.width)
        .map(|x| buf[(x, 1)].symbol())
        .collect::<String>();
    assert!(
        !bottom_row.contains('─'),
        "底行不应有 ─，实际：{bottom_row}"
    );
    // 不应出现角字符
    let any_corner = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .any(|(x, y)| {
            let s = buf[(x, y)].symbol();
            s == "┌" || s == "┐" || s == "└" || s == "┘"
        });
    assert!(
        !any_corner,
        "单边可见时不应有角字符（相邻两边都可见才补角）"
    );
}

/// 四边同 solid → 完整 box + 四角。
#[test]
fn four_edges_solid_draws_full_box() {
    let html = concat!(
        r#"<html><body style="margin:0;padding:0">"#,
        r#"<div style="position:absolute;left:0px;top:0px;width:40px;height:20px;"#,
        r#" box-sizing:border-box;border:1px solid #00ff00;">&nbsp;</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    assert_eq!(find_sym(&buf, "┌"), Some((0, 0)), "左上角应有 ┌");
    assert_eq!(find_sym(&buf, "└"), Some((0, 1)), "左下角应有 └");
    // 右边由 taffy 布局给出 x1；具体列宽依赖 cell_px 缩放，此处只保证符号存在
    assert!(find_sym(&buf, "┐").is_some(), "右上角 ┐ 应存在");
    assert!(find_sym(&buf, "┘").is_some(), "右下角 ┘ 应存在");
    // 颜色：顶角格 fg 命中 CSS 声明
    if let Some((x, y)) = find_sym(&buf, "┌") {
        use ratatui::style::Color;
        assert_eq!(buf[(x, y)].fg, Color::Rgb(0x00, 0xff, 0x00), "┌ 应绿");
    }
}

/// `border-style: none` 视为不可见。
#[test]
fn border_style_none_is_invisible() {
    let html = concat!(
        r#"<html><body style="margin:0;padding:0">"#,
        r#"<div style="position:absolute;left:0px;top:0px;width:40px;height:20px;"#,
        r#" border:1px none #ff0000;">y</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    let any_border_sym = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .any(|(x, y)| {
            let s = buf[(x, y)].symbol();
            s == "─" || s == "│" || s == "┌" || s == "┐" || s == "└" || s == "┘"
        });
    assert!(!any_border_sym, "border-style:none 不应画任何边框字符");
}

/// `border-style:hidden` 与 `none` 同语义（stylo `none_or_hidden` 判定）。
#[test]
fn border_style_hidden_is_invisible() {
    let html = concat!(
        r#"<html><body style="margin:0;padding:0">"#,
        r#"<div style="position:absolute;left:0px;top:0px;width:40px;height:20px;"#,
        r#" border:1px hidden #ff0000;">y</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    let any_border_sym = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .any(|(x, y)| {
            let s = buf[(x, y)].symbol();
            s == "─" || s == "│" || s == "┌" || s == "┐" || s == "└" || s == "┘"
        });
    assert!(!any_border_sym, "border-style:hidden 不应画任何边框字符");
}

/// `border-left:2px solid` 单边可见 → 只画 `│`。
#[test]
fn left_only_edge_draws_vertical() {
    let html = concat!(
        r#"<html><body style="margin:0;padding:0">"#,
        r#"<div style="position:absolute;left:0px;top:0px;width:40px;height:20px;"#,
        r#" border-left:2px solid #0000ff;">&nbsp;</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    // 首列应有 │
    let has_pipe = (0..buf.area.height)
        .map(|y| buf[(0, y)].symbol())
        .any(|s| s == "│");
    assert!(has_pipe, "首列应有 │");
    // 不应出现 ─（左右单侧不生成横线）
    let any_h = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .any(|(x, y)| buf[(x, y)].symbol() == "─");
    assert!(!any_h, "只有左边时不应有 ─");
}

/// `border` shorthand 与 `border-top:0` 覆写：底/左/右 3 边可见 + 顶不可见。
/// 顶不可见 → 顶角 ┌ ┐ 应缺失，但底角 └ ┘ 存在。
#[test]
fn override_top_to_zero_leaves_bottom_corners() {
    let html = concat!(
        r#"<html><body style="margin:0;padding:0">"#,
        r#"<div style="position:absolute;left:0px;top:0px;width:40px;height:20px;"#,
        r#" box-sizing:border-box;border:1px solid #ff0000;border-top:0;">&nbsp;</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    assert!(find_sym(&buf, "┌").is_none(), "顶边不可见时不应有 ┌");
    assert!(find_sym(&buf, "┐").is_none(), "顶边不可见时不应有 ┐");
    assert!(find_sym(&buf, "└").is_some(), "底边可见应生成 └");
    assert!(find_sym(&buf, "┘").is_some(), "底边可见应生成 ┘");
}
