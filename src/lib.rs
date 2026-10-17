//! lumin-tui：dioxus VirtualDom（rsx! 类型化 vnode）→ blitz-dom `BaseDocument` 的薄适配层。
//!
//! 驱动器由 dioxus-native-dom 的 [`DioxusDocument`] 提供（VirtualDom ↔ BaseDocument 绑定、
//! 增量 MutationWriter、`<html><head/><body><main/></body></html>` 脚手架）；本 crate 在其
//! 之上只做 blitz → ratatui 的终端单元格绘制。
//!
//! 与 dioxus-native 官方栈的差异：
//! - 不接管事件循环：宿主（experiment-tui）自己跑 crossterm 循环，每帧经
//!   [`render::paint_frame`] 驱动一次；游戏状态由宿主持有，以根 context（快照）注入，
//!   rsx 组件是快照的纯函数，不使用 dioxus 事件系统（无监听器派发）。
//! - 每帧 `mark_all_dirty` + `Document::poll` 强制重跑读快照的组件（见
//!   [`render::paint_frame`]）→ `resolve()` 布局 → DFS 翻译成终端缓冲区；通用行样式
//!   组件见 [`widgets`]，快照注入见 [`snapshot`]。

pub mod backend;
pub mod html;
pub mod render;
pub mod snapshot;
pub mod widgets;

use blitz_traits::shell::Viewport;

/// dioxus ↔ blitz 驱动器：由 dioxus-native-dom 提供，本 crate 在其之上只做 ratatui 适配。
pub use dioxus_native_dom::DioxusDocument;

/// lumin-tui 预lude。
///
/// `rsx!` / `component!` 宏在**展开点**引用裸路径 `dioxus_core::` / `dioxus_elements::` /
/// `dioxus_signals::`；这些名字连同 `rsx` / `component` / `Element` 由 dioxus 伞型 crate 的
/// prelude 以同名再导出。本模块把它们与 [`DioxusDocument`]、blitz 的 `DocumentConfig` /
/// `Document` 一并转出：本 crate 各模块与调用方只需 `use lumin_tui::prelude::*;` 即可让
/// `rsx!` 展开出的裸路径就地解析。
pub mod prelude {
    pub use blitz_dom::{Document, DocumentConfig};
    pub use dioxus::prelude::*;
    pub use dioxus_native_dom::DioxusDocument;

    pub use crate::render::cpx;
}

/// 视口便捷构造（CSS px）。
#[must_use]
pub fn viewport(width_px: u32, height_px: u32) -> Viewport {
    Viewport {
        window_size: (width_px, height_px),
        ..Viewport::default()
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::snapshot::{SnapshotHandle, snapshot, snapshot_document};
    use ratatui::buffer::Buffer;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// 读快照计数并渲染为文本的根组件（快照纯函数）。
    fn counting_root() -> Element {
        let n = snapshot::<u32>();
        rsx! {
            div { style: "font-size:10px;line-height:1;", "{n}" }
        }
    }

    /// 取缓冲区内所有非空符号拼接（不关心具体落位，仅看渲染出的字符）。
    fn buffer_text(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let s = buf[(x, y)].symbol();
                if !s.is_empty() && s != " " {
                    out.push_str(s);
                }
            }
        }
        out
    }

    /// 首次 `initial_build` 即读到注入值（与增量路径对照的基线）。
    #[test]
    fn initial_build_reads_injected_snapshot() {
        let handle: SnapshotHandle<u32> = Rc::new(RefCell::new(7));
        let mut doc =
            snapshot_document(counting_root, Rc::clone(&handle), DocumentConfig::default());
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 8, 2));
        crate::render::paint_frame(&mut doc, 8, 2, &mut buf);
        assert!(
            buffer_text(&buf).contains('7'),
            "首帧应渲染 7，实得 {:?}",
            buffer_text(&buf)
        );
    }

    /// 回归：持久 doc 跨帧增量更新时，`render_frame` 必须让读快照的根组件重跑，
    /// 文本随快照新值变化。曾因只 `mark_dirty(ScopeId::ROOT)` 被 dioxus 外层
    /// wrapper 的 props memo 跳过用户组件重跑，画面冻结在首帧（表现为按键无反应）。
    #[test]
    fn render_frame_reflects_snapshot_changes() {
        let handle: SnapshotHandle<u32> = Rc::new(RefCell::new(0));
        let mut doc =
            snapshot_document(counting_root, Rc::clone(&handle), DocumentConfig::default());

        let mut buf0 = Buffer::empty(ratatui::layout::Rect::new(0, 0, 8, 2));
        crate::render::paint_frame(&mut doc, 8, 2, &mut buf0);
        assert!(
            buffer_text(&buf0).contains('0'),
            "首帧应渲染 0，实得 {:?}",
            buffer_text(&buf0)
        );

        // 仅改快照句柄内容，复用同一 doc（模拟按键后下一帧）
        *handle.borrow_mut() = 42;
        let mut buf1 = Buffer::empty(ratatui::layout::Rect::new(0, 0, 8, 2));
        crate::render::paint_frame(&mut doc, 8, 2, &mut buf1);
        assert!(
            buffer_text(&buf1).contains("42"),
            "增量帧应渲染 42，实得 {:?}",
            buffer_text(&buf1)
        );
    }
}
