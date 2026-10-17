//! blitz 布局树 → ratatui 缓冲区的翻译层。
//!
//! DFS 遍历已 `resolve()` 的 [`BaseDocument`]：
//! - 元素节点：按 computed style 绘制背景色矩形与四边独立边框，并把前景色/字重
//!   下传（stylo 不为文本节点建样式数据，文本一律继承父元素，见
//!   `blitz-dom` 的 `recalc_style_at`：`if node.is_text_node() { return; }`）；
//! - 文本节点：按 grapheme cluster 迭代，用继承的前景/修饰符写入（emoji ZWJ / VS16 整簇保留）；
//! - 坐标换算：`1 单元格 = cell_px() CSS px`（[`super::cell_px`]），`final_layout`
//!   相对父节点定位，DFS 累加绝对偏移。

use blitz_dom::{BaseDocument, Node, NodeData};
use ratatui::{
    buffer::Buffer,
    style::{Color, Modifier, Style},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::style::{absolute_to_rgba, fg_to_absolute, resolve_color, to_ratatui};

/// 单个 grapheme cluster 在终端的占格宽度。
///
/// `UnicodeWidthStr::width` 对组合符 / VS16 / ZWJ 都返回 0，逐字求和无法体现
/// 终端把 emoji 序整簇合并为 2 格的行为。启发式：
/// - 含 ZWJ（`U+200D`）：家庭/职业 emoji 序列 → 2 格
/// - 含 VS16（`U+FE0F`）：显式 emoji 呈现 → 2 格
/// - 含区域指示符（`U+1F1E6..=U+1F1FF`）：国旗 pair → 2 格
/// - 其他：走 `UnicodeWidthStr::width`，最小 1（保底防 0 宽 grapheme 破坏游标推进）
fn grapheme_width(g: &str) -> usize {
    if g.contains('\u{200d}') || g.contains('\u{fe0f}') {
        return 2;
    }
    if g.chars().any(|c| matches!(c, '\u{1f1e6}'..='\u{1f1ff}')) {
        return 2;
    }
    UnicodeWidthStr::width(g).max(1)
}

/// 沿 DOM 树下传的继承样式（仅渲染所需的最小集）。
///
/// `italic` / `underline` / `strike` 目前仅占位（默认 `false`）：
/// - experiment-tui 不写 `font-style:italic` / `text-decoration`，无视觉回归；
/// - stylo `ComputedValues` 对 `FontStyle` / `TextDecorationLine` 的访问器名字与拷贝语义尚需验证，
///   下一轮接入时在 [`Inherited`] 构造点（`paint_node` / `run_style`）补上真实取值即可。
#[derive(Clone, Copy)]
struct Inherited {
    /// 前景色（sRGB, 0..=255）。
    fg: (u8, u8, u8, u8),
    /// 粗体（font-weight ≥ 600）。
    bold: bool,
    /// 斜体（font-style: italic）——占位，当前恒 `false`。
    italic: bool,
    /// 下划线（text-decoration-line: underline）——占位，当前恒 `false`。
    underline: bool,
    /// 删除线（text-decoration-line: line-through）——占位，当前恒 `false`。
    strike: bool,
}

/// 单条边的可见性与颜色（终端单格语义）。
///
/// 可见判定：`width ≥ 1px` 且 `border-style` 非 `none/hidden`；颜色完全透明（alpha=0）也视为不可见。
#[derive(Clone, Copy)]
struct BorderEdge {
    visible: bool,
    color: Option<Color>,
}

impl BorderEdge {
    #[must_use]
    fn style_for(self) -> Option<Style> {
        if self.visible {
            self.color.map(|c| Style::new().fg(c))
        } else {
            None
        }
    }
}

/// 一帧翻译：把整棵布局树画进 ratatui 缓冲区。
pub(crate) fn paint(doc: &BaseDocument, buf: &mut Buffer) {
    let root_id = doc.root_element().id;
    paint_node(
        doc,
        root_id,
        0.0,
        0.0,
        Inherited {
            fg: (0, 0, 0, 255),
            bold: false,
            italic: false,
            underline: false,
            strike: false,
        },
        buf,
    );
}

/// 递归绘制单个节点（`ox`/`oy` 为父链累加的绝对偏移）。
fn paint_node(
    doc: &BaseDocument,
    node_id: usize,
    ox: f32,
    oy: f32,
    inherited: Inherited,
    buf: &mut Buffer,
) {
    let Some(node) = doc.get_node(node_id) else {
        return;
    };
    let layout = node.final_layout;
    let x = ox + layout.location.x;
    let y = oy + layout.location.y;
    let (w, h) = (layout.size.width, layout.size.height);

    match &node.data {
        // 文本节点：blitz 不为其建样式数据，直接用继承样式绘制
        NodeData::Text(text) => {
            draw_text(buf, x, y, w, &text.content, inherited);
        }
        NodeData::Comment => {}
        // 有样式数据的节点（元素 / 匿名块 / Document 根）
        _ => {
            let next_inherited = node
                .primary_styles()
                .map(|style| {
                    // clone_color 直接返回解析后的 AbsoluteColor（currentColor 已在此级展开）
                    let fg = absolute_to_rgba(&style.clone_color());
                    let current = fg_to_absolute(fg);
                    let own = Inherited {
                        fg,
                        bold: style.clone_font_weight().is_bold(),
                        italic: false,
                        underline: false,
                        strike: false,
                    };

                    // 背景色矩形
                    if let Some(bg) = to_ratatui(resolve_color(
                        &style.get_background().background_color,
                        &current,
                    )) {
                        fill_rect(buf, x, y, w, h, Style::new().bg(bg));
                    }

                    // 四边独立边框：逐边判宽/style/color；顶颜优先作为角字符颜色（同色时无差异，异色时近似保留）。
                    let border = style.get_border();
                    let mk_edge = |w_px: f32, nh: bool, col: &style::values::computed::Color| {
                        let visible = w_px >= 1.0 && !nh;
                        let color = if visible {
                            to_ratatui(resolve_color(col, &current))
                        } else {
                            None
                        };
                        BorderEdge { visible, color }
                    };
                    let edges = [
                        mk_edge(
                            border.border_top_width.to_f32_px(),
                            border.border_top_style.none_or_hidden(),
                            &border.border_top_color,
                        ),
                        mk_edge(
                            border.border_right_width.to_f32_px(),
                            border.border_right_style.none_or_hidden(),
                            &border.border_right_color,
                        ),
                        mk_edge(
                            border.border_bottom_width.to_f32_px(),
                            border.border_bottom_style.none_or_hidden(),
                            &border.border_bottom_color,
                        ),
                        mk_edge(
                            border.border_left_width.to_f32_px(),
                            border.border_left_style.none_or_hidden(),
                            &border.border_left_color,
                        ),
                    ];
                    draw_borders(buf, x, y, w, h, edges);

                    own
                })
                .unwrap_or(inherited);

            // 文本：blitz 不进布局树的文本节点无样式，行内文本的 parley 布局
            // 存在元素的 `inline_layout_data`，逐行翻译（颜色/字重取该元素自身）。
            if let Some(tl) = node
                .element_data()
                .and_then(|e| e.inline_layout_data.as_deref())
            {
                draw_inline_layout(doc, buf, x, y, tl, next_inherited);
            }

            for child in layout_children_ids(node) {
                paint_node(doc, child, x, y, next_inherited, buf);
            }
        }
    }
}

/// 翻译 parley 行内布局：逐行逐 cluster 写入。
///
/// blitz 为每个行内元素 push 一个带 `TextBrush.id`（DOM 节点 id）的样式 span，
/// parley 据此给 cluster 分配样式——`cluster.first_style().brush.id` 即该段文本
/// 的 DOM 节点 id，解析其前景色/字重实现行内多段颜色（如状态栏、回显着色）。
///
/// 注意 1：parley 0.5 中样式切分与字体回退都可能产出多个 Run，同字体多 span
/// 的 Run 共享同一 RunData（cluster 会重复遍历），以 cluster 起始区间去重。
///
/// 注意 2：cluster 偏移按字体 advance 计（CJK 1 格），与终端「CJK 占 2 格」
/// 约定不一致，起始列由「行首到 cluster 起点的字符宽度」推算（与 [`draw_text`] 一致）。
fn draw_inline_layout(
    doc: &BaseDocument,
    buf: &mut Buffer,
    x: f32,
    y: f32,
    tl: &blitz_dom::node::TextLayout,
    inherited: Inherited,
) {
    let mut yoff = 0.0;
    for line in tl.layout.lines() {
        let metrics = line.metrics();
        let line_start = line.text_range().start;
        let mut drawn: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for run in line.runs() {
            for cluster in run.clusters() {
                let range = cluster.text_range();
                if !drawn.insert(range.start) {
                    continue;
                }
                let style = run_style(doc, cluster.first_style().brush.id, inherited);
                // 行首到 cluster 起点的字符占宽（终端格，按 grapheme 合并）
                let prefix_cells: u16 = tl.text[line_start..range.start]
                    .graphemes(true)
                    .map(|g| grapheme_width(g) as u16)
                    .sum();
                let cluster_x =
                    f32::from(prefix_cells).mul_add(super::cell_px(), x + metrics.offset);
                draw_text(
                    buf,
                    cluster_x,
                    y + yoff,
                    f32::INFINITY,
                    &tl.text[range],
                    style,
                );
            }
        }
        yoff += metrics.line_height;
    }
}

/// 按 brush 内记录的 DOM 节点 id 解析 run 样式；无样式数据时回退继承样式。
fn run_style(doc: &BaseDocument, node_id: usize, fallback: Inherited) -> Inherited {
    doc.get_node(node_id)
        .and_then(|node| node.primary_styles())
        .map(|style| Inherited {
            fg: absolute_to_rgba(&style.clone_color()),
            bold: style.clone_font_weight().is_bold(),
            italic: false,
            underline: false,
            strike: false,
        })
        .unwrap_or(fallback)
}

/// 布局子节点 id 列表（布局树经过匿名块构造，与 DOM `children` 不同）。
fn layout_children_ids(node: &Node) -> Vec<usize> {
    node.layout_children
        .borrow()
        .as_ref()
        .map(Clone::clone)
        .unwrap_or_default()
}

/// CSS px → 终端列/行（1 cell = [`super::cell_px`] px），四舍五入并保证非负。
fn cell(v: f32) -> u16 {
    (v / super::cell_px()).round().max(0.0) as u16
}

/// 用指定样式填充矩形区域（裁剪到缓冲区内）。
///
/// 填充同时写入空格符号：不透明背景按绘制顺序覆盖下层内容（DOM 序靠后者在上），
/// 叠层弹框因此天然获得清屏语义。
fn fill_rect(buf: &mut Buffer, x: f32, y: f32, w: f32, h: f32, style: Style) {
    let (x0, y0) = (cell(x), cell(y));
    let (w0, h0) = (cell(w), cell(h));
    for row in y0..y0.saturating_add(h0).min(buf.area.height) {
        for col in x0..x0.saturating_add(w0).min(buf.area.width) {
            let c = &mut buf[(col, row)];
            c.set_symbol(" ");
            c.set_style(style);
        }
    }
}

/// 逐边描线 + 相邻可见时补角字符。顶/底行先绘、左/右列后绘，角最后覆盖（保证角优先于边）。
///
/// `edges` 顺序：`[top, right, bottom, left]`，与 CSS 顺时针一致。
fn draw_borders(buf: &mut Buffer, x: f32, y: f32, w: f32, h: f32, edges: [BorderEdge; 4]) {
    if !(edges.iter().any(|e| e.style_for().is_some())) {
        return;
    }
    let (x0, y0) = (cell(x), cell(y));
    let (x1, y1) = (cell(x + w), cell(y + h));
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (xe, ye) = (x1.saturating_sub(1), y1.saturating_sub(1));
    let (top, right, bottom, left) = (edges[0], edges[1], edges[2], edges[3]);

    if let Some(s) = top.style_for() {
        for col in x0..x1.min(buf.area.width) {
            if y0 < buf.area.height {
                let c = &mut buf[(col, y0)];
                c.set_style(s);
                c.set_symbol("─");
            }
        }
    }
    if let Some(s) = bottom.style_for() {
        for col in x0..x1.min(buf.area.width) {
            if ye < buf.area.height {
                let c = &mut buf[(col, ye)];
                c.set_style(s);
                c.set_symbol("─");
            }
        }
    }
    if let Some(s) = left.style_for() {
        for row in y0..y1.min(buf.area.height) {
            if x0 < buf.area.width {
                let c = &mut buf[(x0, row)];
                c.set_style(s);
                c.set_symbol("│");
            }
        }
    }
    if let Some(s) = right.style_for() {
        for row in y0..y1.min(buf.area.height) {
            if xe < buf.area.width {
                let c = &mut buf[(xe, row)];
                c.set_style(s);
                c.set_symbol("│");
            }
        }
    }

    // 角：相邻两边都可见时，以顶/底边颜色为主画角（同色时无差异，异色时近似保留）。
    let corners = [
        ("┌", x0, y0, top, left),
        ("┐", xe, y0, top, right),
        ("└", x0, ye, bottom, left),
        ("┘", xe, ye, bottom, right),
    ];
    for (sym, cx, cy, h_edge, v_edge) in corners {
        if !(h_edge.visible && v_edge.visible) {
            continue;
        }
        if cx >= buf.area.width || cy >= buf.area.height {
            continue;
        }
        if let Some(s) = h_edge.style_for().or_else(|| v_edge.style_for()) {
            let c = &mut buf[(cx, cy)];
            c.set_symbol(sym);
            c.set_style(s);
        }
    }
}

/// 写入一行文本：按继承前景色/字重着色，按缓冲区右缘截断。
///
/// `x`/`y` 为 CSS px（内部换算为单元格）；`content` 原样绘制——空白折叠与断行
/// 已由 parley 在 `TextLayout` 阶段按 `white-space` 规则完成，此处不再重复处理；
/// CJK 等宽字符占 2 格（按 unicode width 计），超出即截断。
/// `w` 为 CSS px 宽度上限（`INFINITY` 表示不限）。
/// 按 grapheme 迭代替换原按 char 迭代的实现，避免 emoji ZWJ / VS16 序列被拆开；
/// 首格写整簇 symbol，后续 `gw-1` 格 pad 空格保背景连续；跳过含 control 的 cluster
/// （ratatui 0.30 对含控制符 symbol 求 width 会 panic）。
fn draw_text(buf: &mut Buffer, x: f32, y: f32, w: f32, content: &str, inherited: Inherited) {
    if content.is_empty() {
        return;
    }
    let col0 = cell(x);
    let row = cell(y);
    if row >= buf.area.height || col0 >= buf.area.width {
        return;
    }
    let max_cells = if w.is_infinite() {
        buf.area.width - col0
    } else {
        cell(w).min(buf.area.width - col0)
    };

    let mut style = Style::new().fg(Color::Rgb(inherited.fg.0, inherited.fg.1, inherited.fg.2));
    if inherited.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if inherited.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if inherited.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if inherited.strike {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }

    let mut used = 0u16;
    for g in content.graphemes(true) {
        // 跳过含 control 的 cluster（\n / \t 等）
        if g.chars().any(|c| c.is_control()) {
            continue;
        }
        let gw = grapheme_width(g) as u16;
        if used + gw > max_cells {
            break;
        }
        let c = &mut buf[(col0 + used, row)];
        c.set_symbol(g);
        c.set_style(style);
        used += 1;
        // 双格（或更多）用空格 pad 余下格，保证背景高亮连续（与 char 版语义一致）
        for _ in 1..gw {
            if col0 + used >= buf.area.width {
                break;
            }
            let pad = &mut buf[(col0 + used, row)];
            pad.set_symbol(" ");
            pad.set_style(style);
            used += 1;
        }
    }
}
