//! 快照注入：宿主外部状态 → rsx 组件的桥接。
//!
//! 组件是快照的纯函数：宿主持有 [`SnapshotHandle`]，每帧覆写内容后调用
//! [`crate::render::paint_frame`]；组件内用 [`snapshot::<S>()`] 取当帧克隆。
//! 不使用 dioxus 事件系统（无监听器派发），游戏逻辑由宿主命令式驱动。

use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::DocumentConfig;
use dioxus::prelude::{Element, VirtualDom};

use crate::DioxusDocument;

/// 宿主与 `VirtualDom` 根 context 共享的快照句柄。
pub type SnapshotHandle<S> = Rc<RefCell<S>>;

/// 组件内取当帧快照（从根 context 克隆）。
#[must_use]
pub fn snapshot<S: Clone + 'static>() -> S {
    dioxus::dioxus_core::consume_context::<SnapshotHandle<S>>()
        .borrow()
        .clone()
}

/// 构造快照驱动的文档：`root` 为 rsx 根组件（经 [`snapshot::<S>()`] 读快照），
/// 挂载在 `<main>` 下；同时注入 UA 基线（`html` 满高、`body` 零边距——
/// 无 `<style>` 标签场景下 body 缺省 `margin:8px` 会使内容整体偏移 1 格）。
#[must_use]
pub fn snapshot_document<S: 'static>(
    root: fn() -> Element,
    handle: SnapshotHandle<S>,
    config: DocumentConfig,
) -> DioxusDocument {
    let vdom = VirtualDom::new(root).with_root_context(handle);
    let mut doc = DioxusDocument::new(vdom, config);
    doc.add_user_agent_stylesheet("html{height:100%} body{margin:0}");
    doc.initial_build();
    doc
}
