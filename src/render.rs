//! blitz 布局树 → ratatui 缓冲区的渲染管线（dioxus-tui 内建）。
//!
//! 职责链：rsx 组件树经 [`crate::DioxusDocument`] 写入 blitz 文档（dioxus 增量
//! mutation → `BaseDocument`）→ `Document::resolve()` 完成 stylo 样式 + taffy
//! 布局 → DFS 遍历布局树、把节点翻译成 ratatui 缓冲区单元格。
//! 与 blitz-dom / stylo 的所有接触点都收敛在本模块内。

mod debug;
mod paint;
mod style;

use std::sync::OnceLock;

pub use debug::print_tree;

use ratatui::buffer::Buffer;

use blitz_dom::Document;

use crate::{DioxusDocument, viewport};

/// 缩放约定：**1 终端格 = `CELL_PX_DEFAULT` CSS px**（缺省值，实际值可经
/// [`set_cell_px`] 由宿主配置覆盖，见 experiment-tui 的 `config.json`）。
///
/// 默认 UA 字号 16px / 行高 ~19px，若按 1px=1cell 一行文本会占约 19 格，
/// 无法实用；按 10px/cell 折算后一行文本 ≈ 2 格高、布局尺寸 ≈ 终端格数 × 10。
/// 这是「连续 px 空间 ↔ 离散 cell 网格」的唯一换算旋钮，px↔cell 双向都读它。
const CELL_PX_DEFAULT: f32 = 10.0;

/// 运行时缩放因子（1 cell = ? CSS px）。进程内单例，由宿主在启动时 [`set_cell_px`] 注入。
static CELL_PX: OnceLock<f32> = OnceLock::new();

/// 当前缩放因子（1 cell = ? CSS px）；未注入时取缺省 [`CELL_PX_DEFAULT`]。
#[must_use]
pub fn cell_px() -> f32 {
    *CELL_PX.get().unwrap_or(&CELL_PX_DEFAULT)
}

/// 设置缩放因子（1 cell = ? CSS px）。首次调用生效，之后调用被忽略（`OnceLock` 语义），
/// 由宿主在渲染开始前一次性注入配置值。
pub fn set_cell_px(px: f32) {
    _ = CELL_PX.set(px);
}

/// N 终端格 → CSS px（四舍五入）。供样式生成端把「按格书写」的意图折算为 px，
/// 与 [`cell_px`] 反向换算保持单一来源，避免两侧各自写死 10。
#[must_use]
pub fn cpx(cells: u32) -> u32 {
    (cells as f32 * cell_px()).round() as u32
}

/// 终端格数 → CSS px 视口（内部按 [`cell_px`] 换算）。
#[must_use]
pub fn viewport_cells(cols: u16, rows: u16) -> blitz_traits::shell::Viewport {
    viewport(
        (f32::from(cols) * cell_px()) as u32,
        (f32::from(rows) * cell_px()) as u32,
    )
}

/// 一帧渲染：viewport 跟随终端格数 → 强制重跑并落增量 mutation → resolve 布局 → ratatui Buffer。
///
/// `mark_all_dirty` 把每个 scope（含读快照的用户 `root`）入脏队；`Document::poll` 随即经
/// `wait_for_work()`（脏队非空即 Ready）走 `render_immediate`，把增量 mutation 写入文档。
pub fn paint_frame(doc: &mut DioxusDocument, cols: u16, rows: u16, buf: &mut Buffer) {
    doc.set_viewport(viewport_cells(cols, rows));
    doc.vdom.mark_all_dirty();
    Document::poll(doc, None);
    doc.resolve(0.0);
    paint::paint(&*doc, buf);
}

/// 把已解析的 HTML 文档（blitz [`BaseDocument`]）渲染进 ratatui Buffer。
///
/// 与 [`paint_frame`] 的区别：HTML 入口没有 dioxus VirtualDom，节点树已由
/// `blitz-html` 填充，这里只按终端格数设 viewport → `resolve()`（stylo 样式级联
/// + taffy 布局）→ DFS 翻译成单元格。
pub fn paint_html_doc(doc: &mut blitz_dom::BaseDocument, cols: u16, rows: u16, buf: &mut Buffer) {
    doc.set_viewport(viewport_cells(cols, rows));
    doc.resolve(0.0);
    paint::paint(doc, buf);
}
