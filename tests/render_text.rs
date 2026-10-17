//! 文本渲染测试：grapheme cluster 合并（T2）+ 继承样式修饰符（T4 现状：仅 bold 生效）。
//!
//! 走 HTML 入口（`lumin_tui::html::render_html_to_buffer`），不经 dioxus 组件层，
//! 断言 paint.rs 把 stylo computed style + parley cluster 翻成 ratatui Cell 的正确性。

mod common;
use common::{
    config, find_sym, line_contains, normalize, paint, rgb, row_containing, theme_color,
    visible_text,
};

/// `white-space:pre` 单行文本按 char 原样落位（sanity，其他用例的基础）。
#[test]
fn plain_text_lands_on_buffer() {
    let html = r#"<html><body style="font-family:monospace"><div style="font-size:10px;line-height:1;white-space:pre;">hello</div></body></html>"#;
    let buf = paint(html);
    assert!(
        normalize(&visible_text(&buf)).contains("hello"),
        "hello 应可见，实际：\n{}",
        visible_text(&buf)
    );
}

/// CJK 字符占 2 格：相邻单元应为空格 pad 保背景连续。
#[test]
fn cjk_char_occupies_two_cells() {
    let html = concat!(
        r#"<html><body style="font-family:monospace">"#,
        r#"<div style="font-size:10px;line-height:1;white-space:pre;background-color:#abcdef;color:#ffffff">中A</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    let (x, _y) = find_sym(&buf, "中").expect("中 应可见");
    // 主 cell 存 "中"；相邻格应是空格（同 bg 保背景连续）
    let pad = &buf[(x + 1, _y)];
    assert_eq!(pad.symbol(), " ", "CJK 相邻格应为空格 pad");
    assert_eq!(
        pad.bg,
        rgb((0xab, 0xcd, 0xef)),
        "CJK pad 格应继承同 bg（背景连续）"
    );
    // 后续 ASCII 应跳过 pad 落在 x+2
    let (xa, _) = find_sym(&buf, "A").expect("A 应可见");
    assert_eq!(xa, x + 2, "A 应跳过 CJK 第二格落在 x+2");
}

/// T2：emoji ZWJ 序列（`👨‍👩‍👧`）作为一个 grapheme 写入单格，不被拆散。
#[test]
fn emoji_zwj_sequence_is_single_grapheme() {
    let html = concat!(
        r#"<html><body style="font-family:monospace">"#,
        // U+1F468 ZWJ U+1F469 ZWJ U+1F467
        r#"<div style="font-size:10px;line-height:1;white-space:pre;">👨‍👩‍👧A</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    // 找含 "A" 的行；"A" 应落在 cluster 起点后 2 格（family emoji 视为 2 宽）
    let (row, line) = row_containing(&buf, "A");
    let col_a = line.chars().position(|c| c == 'A').expect("A 应在行内可见") as u16;
    // 起点到 A 之间应只有一整簇 emoji（占 2 格）+ 可能的 1 格空格 pad
    assert!(
        col_a >= 1,
        "emoji cluster 应至少占 1 格，实际 A 落在 col={col_a}"
    );
    let _ = row;
    // 反向检查：emoji 主 cell 的 symbol 应包含 family 序列首字符 U+1F468
    let mut emoji_cell: Option<(u16, u16)> = None;
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            if buf[(x, y)].symbol().contains('\u{1f468}') {
                emoji_cell = Some((x, y));
                break;
            }
        }
        if emoji_cell.is_some() {
            break;
        }
    }
    let (ex, _ey) = emoji_cell.expect("emoji 首字符 U+1F468 应作为整簇 symbol 出现");
    // 单 cell 存整簇 symbol：不应有独立的 U+1F469 / U+1F467 cell
    let has_alone_woman = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .any(|(x, y)| {
            let s = buf[(x, y)].symbol();
            s.contains('\u{1f469}') && x != ex
        });
    assert!(
        !has_alone_woman,
        "U+1F469 不应作为独立 cell 出现（应合并进 emoji 簇）"
    );
}

/// T2：VS16 显式 emoji 呈现（`☀️` U+2600 + U+FE0F）作为 grapheme 宽度判为 2。
///
/// 注：parley cluster 可能将 `☀` 与 VS16 拆到不同 cluster（与 grapheme 不完全对齐），
/// 本用例只锁定 `grapheme_width(VS16)` 启发式返回 2 的结果：A 应落在 ≥ 2 列之后。
#[test]
fn vs16_emoji_presentation_merges() {
    let html = concat!(
        r#"<html><body style="font-family:monospace;margin:0;padding:0">"#,
        r#"<div style="font-size:10px;line-height:1;white-space:pre;">☀️A</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    let (_, line) = row_containing(&buf, "A");
    let col_a = line.chars().position(|c| c == 'A').expect("A 应可见") as u16;
    // ☀ 主 cell + VS16 启发式判为 2 格→ A 至少落在 col=2
    assert!(
        col_a >= 2,
        "A 应落在 VS16 簇（判为 2 格）之后，实际 col={col_a}"
    );
}

/// T2：控制字符（\n / \t）在 cell symbol 里被跳过，避免 ratatui 求 width panic。
#[test]
fn control_chars_are_skipped() {
    let html = concat!(
        r#"<html><body style="font-family:monospace">"#,
        // 手工把 \t 塞进文本；blitz 会保留原文（white-space:pre）
        r#"<div style="font-size:10px;line-height:1;white-space:pre;">a&#9;b</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    // a、b 应可见；tab cell 应不存在（paint 跳过 control）
    let (y, line) = row_containing(&buf, "a");
    assert!(line_contains(&line, "b"), "b 应与 a 同一行");
    let has_tab = (0..buf.area.width).any(|x| buf[(x, y)].symbol() == "\t");
    assert!(!has_tab, "tab 字符不应写入 cell");
}

/// T4：`font-weight:bold` 走 Modifier::BOLD（现有能力，回归基线）。
#[test]
fn bold_modifier_applied() {
    let html = concat!(
        r#"<html><body style="font-family:monospace">"#,
        r#"<div style="font-size:10px;line-height:1;white-space:pre;font-weight:bold;">K</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    let (x, y) = find_sym(&buf, "K").expect("K 应可见");
    assert!(
        buf[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD),
        "K 应带 BOLD 修饰符"
    );
}

/// T4 占位：italic / underline / strike 目前恒 `false`，显式声明也不出 Modifier。
/// 断言此"当前状态"以防未来误接入时未同步测试；下一轮接入实际提取时应反向移除本用例。
#[test]
fn italic_underline_strike_are_placeholders() {
    let html = concat!(
        r#"<html><body style="font-family:monospace">"#,
        r#"<div style="font-size:10px;line-height:1;white-space:pre;font-style:italic;text-decoration:underline line-through;">X</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    let (x, y) = find_sym(&buf, "X").expect("X 应可见");
    let m = buf[(x, y)].modifier;
    let mod_bit = ratatui::style::Modifier::ITALIC
        | ratatui::style::Modifier::UNDERLINED
        | ratatui::style::Modifier::CROSSED_OUT;
    assert!(
        !m.intersects(mod_bit),
        "T4 占位：italic / underline / strike 当前不应生效（下一轮接入时应移除本用例）"
    );
}

/// 前景色从 CSS `color:#xxx` 走 stylo → ratatui Color::Rgb。
#[test]
fn fg_color_from_css_hex() {
    let html = concat!(
        r#"<html><body style="font-family:monospace">"#,
        r#"<div style="font-size:10px;line-height:1;white-space:pre;color:#ff8800;">Z</div>"#,
        r#"</body></html>"#,
    );
    let buf = paint(html);
    let (x, y) = find_sym(&buf, "Z").expect("Z 应可见");
    assert_eq!(buf[(x, y)].fg, rgb((0xff, 0x88, 0x00)), "Z 前景应命中 hex");
}

/// 主题色常量与 config/test/tui.json 同源（避免各测试文件写死）。
#[test]
fn theme_colors_are_sourced_from_config() {
    let _ = config();
    assert_eq!(theme_color(|t| t.yellow), rgb((0xf9, 0xe2, 0xaf)));
    assert_eq!(theme_color(|t| t.cyan), rgb((0x94, 0xe2, 0xd5)));
}
