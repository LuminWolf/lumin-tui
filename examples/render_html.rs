//! 通用 HTML → 终端渲染器的命令行示例：把任意 HTML 文件渲染为 ANSI 输出到 stdout。
//!
//! 演示完整管线：`blitz-html` 解析 HTML → stylo 级联 + taffy 布局（`resolve`）→
//! DFS 翻译成 ratatui `Buffer` → `Terminal` + [`AnsiBackend`] 做 diff 并写出 ANSI。
//!
//! 当前**不加载外链 CSS**（`<link rel="stylesheet">`），仅默认 UA 样式表 + 内联
//! `<style>` 生效；也不做终端交互（暂不进入 raw mode / 不处理按键）。
//!
//! 渲染本库自身的 rustdoc：
//! ```text
//! cargo doc -p lumin-tui --no-deps
//! cargo run -p lumin-tui --example render_html -- target/doc/lumin_tui/index.html
//! ```
//!
//! [`AnsiBackend`]: lumin_tui::backend::AnsiBackend

use std::{env, fs, io, io::Write, path::Path};

use lumin_tui::html::render_html_to_ansi;

/// 缺省视口尺寸（无 COLUMNS/LINES 环境变量时）。
const DEFAULT_COLS: u16 = 100;
const DEFAULT_ROWS: u16 = 50;

/// 本库 rustdoc 的默认入口（相对工作区根）。
const DEFAULT_RUSTDOC: &str = "target/doc/lumin_tui/index.html";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let html_path = env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_RUSTDOC.to_owned());
    let html =
        fs::read_to_string(&html_path).map_err(|e| format!("读取 HTML 失败 {html_path}: {e}"))?;

    let cols = env_term_dim("COLUMNS", DEFAULT_COLS);
    let rows = env_term_dim("LINES", DEFAULT_ROWS);

    // base_dir = HTML 文件所在目录，供 `<link rel="stylesheet" href="../…">` 相对解析
    render_html_to_ansi(
        &html,
        cols,
        rows,
        Path::new(&html_path).parent(),
        io::stdout(),
    )?;
    // 收尾换行，避免 shell 提示符贴在最后一行渲染内容上
    writeln!(io::stdout())?;
    Ok(())
}

/// 从环境变量读终端维度（列/行），缺失或非法则取缺省值。
fn env_term_dim(key: &str, default: u16) -> u16 {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(default)
}
