//! 通用 HTML → 终端渲染器：HTML 解析 + CSS 级联 + 布局 → ratatui 单元格网格。
//!
//! 与 rsx 入口（[`crate::DioxusDocument`]）共用同一条绘制管线，仅替换节点来源：
//! - **HTML 解析**：[`blitz_html::HtmlDocument::from_html`] 经 html5ever 把 HTML 文本
//!   灌进 blitz-dom [`BaseDocument`]（DOM 树）。
//! - **CSS 级联 + 布局**：`DocumentConfig.ua_stylesheets` 注入默认 UA 样式表
//!   （`blitz_dom::DEFAULT_CSS`）；内联 `<style>` 由 blitz 在 mutation 阶段自动解析
//!   进 stylo；`<link rel="stylesheet">` 指向的**本地** CSS 由 [`html_document`] 同步读文件
//!   注入（相对 `base_dir` 解析；不加载 http(s) 外链、不执行 JS）。随后 `resolve()`
//!   完成级联计算样式 + taffy 布局。
//! - **单元格翻译**：[`crate::render::paint_html_doc`] DFS 遍历布局树写入 [`Buffer`]。
//! - **diff / ANSI 输出**：[`crate::backend::AnsiBackend`] 配合 ratatui [`Terminal`]
//!   对前后两帧 Buffer 做 diff，仅把变化单元格编码为 ANSI 序列。
//!
//! [`Terminal`]: ratatui::terminal::Terminal

use std::io::Write;
use std::path::Path;

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::HtmlDocument;
use ratatui::{Terminal, TerminalOptions, Viewport, buffer::Buffer, layout::Rect};

use crate::backend::AnsiBackend;

/// 构造文档时的缺省 `base_url`。
///
/// blitz 默认 base 是 `data:` URL，**无法** join 相对路径（rustdoc 的
/// `<link rel="stylesheet" href="../static/…css">` 会在 URL 解析处 panic）。
/// 用一个绝对 `file:///` 作 base：让 blitz 内部对 `<link>` 的 URL 解析不 panic；
/// 其异步加载走 `DummyNetProvider` 被静默丢弃，外链 CSS 改由 [`html_document`]
/// 同步读本地文件注入（见 [`local_stylesheet_hrefs`]）。
const DEFAULT_BASE_URL: &str = "file:///";

/// 判断 href 是否带协议（`http:`/`https:`/`data:` 等）——这类外链跳过，只读本地文件。
fn has_scheme(href: &str) -> bool {
    match href.find(':') {
        Some(colon) => href.find('/').is_none_or(|slash| colon < slash),
        None => false,
    }
}

/// 解析一个 `<link …>` 标签体（`<link` 之后、`>` 之前的部分）为 (小写属性名, 原值) 列表。
fn link_attrs(body: &str) -> Vec<(String, String)> {
    let bytes = body.as_bytes();
    let mut attrs = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let ns = i;
        while i < bytes.len()
            && bytes[i] != b'='
            && !(bytes[i] as char).is_ascii_whitespace()
            && bytes[i] != b'>'
        {
            i += 1;
        }
        let name = body[ns..i].to_ascii_lowercase();
        while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                i += 1;
                let vs = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                attrs.push((name, body[vs..i].to_string()));
                if i < bytes.len() {
                    i += 1;
                }
            } else {
                let vs = i;
                while i < bytes.len()
                    && !(bytes[i] as char).is_ascii_whitespace()
                    && bytes[i] != b'>'
                {
                    i += 1;
                }
                attrs.push((name, body[vs..i].to_string()));
            }
        } else if !name.is_empty() {
            attrs.push((name, String::new()));
        }
    }
    attrs
}

/// 扫描 HTML 中 `<link rel="stylesheet" href="…">` 的**本地**样式表 href（跳过带协议外链）。
fn local_stylesheet_hrefs(html: &str) -> Vec<String> {
    // to_ascii_lowercase 仅改 ASCII 字母、不改变字节长度/位置，故 lower 的偏移可切原串取属性值。
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(off) = lower[from..].find("<link") {
        let body_start = from + off + "<link".len();
        let Some(gt) = lower[body_start..].find('>') else {
            break;
        };
        let body_end = body_start + gt;
        let attrs = link_attrs(&html[body_start..body_end]);
        let is_css = attrs.iter().any(|(n, v)| {
            n == "rel"
                && v.to_ascii_lowercase()
                    .split_whitespace()
                    .any(|t| t == "stylesheet")
        });
        if is_css
            && let Some((_, href)) = attrs.iter().find(|(n, _)| n == "href")
            && !href.is_empty()
            && !has_scheme(href)
        {
            let clean = href.split('#').next().unwrap_or(href);
            let clean = clean.split('?').next().unwrap_or(clean);
            out.push(clean.to_string());
        }
        from = body_end + 1;
    }
    out
}

/// 由 HTML 文本构造一个 [`HtmlDocument`]（DOM 已建、样式表已挂，尚未 `resolve()`）。
///
/// `ua_stylesheets` 置为 `Some(vec![])` 以触发 [`HtmlDocument::from_html`] 注入默认 UA
/// 样式表；`base_url` 取 [`DEFAULT_BASE_URL`] 以免相对资源解析 panic。
///
/// 若给出 `base_dir`，则扫描 `<link rel="stylesheet">` 并把其指向的**本地** CSS 文件读出、
/// 以 UA 源注入（相对路径按 `base_dir` 解析）。不加载 http(s) 外链、不执行 JS。
#[must_use]
pub fn html_document(html: &str, base_dir: Option<&Path>) -> HtmlDocument {
    let config = DocumentConfig {
        base_url: Some(DEFAULT_BASE_URL.to_owned()),
        ua_stylesheets: Some(Vec::new()),
        ..DocumentConfig::default()
    };
    let mut doc = HtmlDocument::from_html(html, config);
    if let Some(base) = base_dir {
        for href in local_stylesheet_hrefs(html) {
            if let Ok(css) = std::fs::read_to_string(base.join(&href)) {
                doc.add_user_agent_stylesheet(&css);
            }
        }
    }
    doc
}

/// 可复用的 HTML 渲染器：持有解析后的文档，跨帧只重算 viewport / 布局，不重复解析。
#[allow(missing_debug_implementations)]
pub struct HtmlRenderer {
    doc: HtmlDocument,
}

impl HtmlRenderer {
    /// 解析 HTML 文本，构造渲染器。`base_dir` 给出时同步加载 `<link>` 本地外链 CSS。
    #[must_use]
    pub fn new(html: &str, base_dir: Option<&Path>) -> Self {
        Self {
            doc: html_document(html, base_dir),
        }
    }

    /// 只读访问底层 [`BaseDocument`]（调试 / 断言用）。
    #[must_use]
    pub fn document(&self) -> &BaseDocument {
        &self.doc
    }

    /// 一帧渲染：viewport 跟随 `buf.area` → `resolve()` → DFS 写入 `buf`。
    pub fn render(&mut self, buf: &mut Buffer) {
        let area = buf.area;
        crate::render::paint_html_doc(&mut self.doc, area.width, area.height, buf);
    }
}

/// 把 HTML 一次性渲染为 ANSI 字节流（写入 `w`），经 ratatui [`Terminal`] 做 diff。
///
/// 无终端、无交互：[`AnsiBackend`] 把目标尺寸固定为 `cols × rows`，`Terminal` 用
/// `Viewport::Fullscreen` 直接采用该尺寸；首帧与空缓冲区 diff，故全量输出。
pub fn render_html_to_ansi<W: Write>(
    html: &str,
    cols: u16,
    rows: u16,
    base_dir: Option<&Path>,
    mut w: W,
) -> std::io::Result<()> {
    let backend = AnsiBackend::new(&mut w, cols, rows);
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fullscreen,
        },
    )
    .map_err(|e| std::io::Error::other(e.to_string()))?;

    let mut renderer = HtmlRenderer::new(html, base_dir);
    // 一次性渲染：Terminal::draw 内部已 flush 并交换双缓冲，不能再调 flush()
    // （否则第二次 flush 会拿「当前=内容」对「下一个=旧空缓冲」做 diff，把内容擦成空白）。
    terminal
        .draw(|frame| renderer.render(frame.buffer_mut()))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    drop(terminal);
    w.flush()
}

/// 把 HTML 渲染到一个 [`Buffer`]（终端渲染的中间产物，测试 / 复用友好）。
#[must_use]
pub fn render_html_to_buffer(html: &str, cols: u16, rows: u16, base_dir: Option<&Path>) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, cols, rows));
    HtmlRenderer::new(html, base_dir).render(&mut buf);
    buf
}
