//! 布局树的调试打印（spike / 排障用）。

use blitz_dom::{BaseDocument, NodeData};

use super::style::{absolute_color_css, color_css};

/// 按布局树打印节点缩略信息（标签/类名或文本、布局矩形）。
pub fn print_tree(doc: &BaseDocument) {
    let root_id = doc.root_element().id;
    print_node(doc, root_id, 0);
}

fn print_node(doc: &BaseDocument, node_id: usize, depth: usize) {
    let Some(node) = doc.get_node(node_id) else {
        return;
    };
    let l = node.final_layout;
    let desc = if let NodeData::Text(text) = &node.data {
        format!(
            "text {:?} @ ({},{}) {}x{}",
            text.content.trim(),
            l.location.x,
            l.location.y,
            l.size.width,
            l.size.height
        )
    } else {
        let (fg, bg) = node
            .primary_styles()
            .map(|s| {
                (
                    absolute_color_css(&s.clone_color()),
                    color_css(&s.get_background().background_color),
                )
            })
            .unwrap_or((String::new(), String::new()));
        let text = node
            .element_data()
            .and_then(|e| e.inline_layout_data.as_deref())
            .map(|tl| tl.text.clone())
            .unwrap_or_default();
        format!(
            "node fg={fg} bg={bg} @ ({},{}) {}x{} text={text:?}",
            l.location.x, l.location.y, l.size.width, l.size.height
        )
    };
    println!("{}{}", "  ".repeat(depth), desc);
    let children: Vec<usize> = node
        .layout_children
        .borrow()
        .as_ref()
        .map(Clone::clone)
        .unwrap_or_default();
    for child in children {
        print_node(doc, child, depth + 1);
    }
}
