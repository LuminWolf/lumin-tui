//! 把 ratatui 的 diff 输出编码为 ANSI 转义序列的 [`Backend`] 实现。
//!
//! 设计目的：让「HTML → 终端渲染」这条管线**无终端、可重定向**地跑通——
//! [`Terminal`] 仍负责前后两帧 [`Buffer`] 的 diff，本类型只把「变化单元格」
//! 翻译成 ANSI：光标定位（CUP）+ 前景/背景色（SGR）+ 粗体等修饰 + 字符本身。
//!
//! 写入目标是任意 [`Write`]（真实终端 stdout、内存 `Vec<u8>`、管道皆可），
//! 且 `size()` 返回构造时给定的固定尺寸，因此不查询、也不要求 TTY。
//!
//! [`Terminal`]: ratatui::Terminal
//! [`Buffer`]: ratatui::buffer::Buffer

use std::io::Write;

use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
    style::{Color, Modifier, Style},
};

/// ANSI 转义序列后端：把 `Terminal` diff 出的变化单元格写入泛型 `W`。
#[derive(Debug)]
pub struct AnsiBackend<W> {
    writer: W,
    width: u16,
    height: u16,
    /// 当前光标逻辑位置，供 `get_cursor_position` 应答（不查询真实终端）。
    cursor: Position,
}

impl<W: Write> AnsiBackend<W> {
    /// 新建固定 `width × height` 单元格的后端，输出写入 `writer`。
    pub fn new(writer: W, width: u16, height: u16) -> Self {
        Self {
            writer,
            width,
            height,
            cursor: Position::ORIGIN,
        }
    }

    /// 交还内部 writer。
    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W: Write> Backend for AnsiBackend<W> {
    type Error = std::io::Error;

    /// 绘制变化单元格：逐个定位光标 → 应用样式（相对上一个单元格做增量）→ 写字符。
    ///
    /// `content` 已是 `Terminal` diff 后的「脏单元格」迭代器，`(x, y)` 为绝对坐标。
    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        // 同一批次内复用上一次 SGR，减少冗余转义
        let mut last_style: Option<Style> = None;
        for (x, y, cell) in content {
            // CUP：行列均 1-based
            write!(self.writer, "\x1b[{};{}H", y + 1, x + 1)?;
            let style = cell.style();
            if last_style != Some(style) {
                // 先复位再叠加，避免残留修饰污染
                write!(self.writer, "\x1b[0m")?;
                write_sgr(&mut self.writer, style)?;
                last_style = Some(style);
            }
            self.writer.write_all(cell.symbol().as_bytes())?;
        }
        write!(self.writer, "\x1b[0m")?;
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        write!(self.writer, "\x1b[?25l")
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        write!(self.writer, "\x1b[?25h")
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.cursor = position.into();
        write!(
            self.writer,
            "\x1b[{};{}H",
            self.cursor.y + 1,
            self.cursor.x + 1
        )
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        write!(self.writer, "\x1b[2J")
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        // ANSI 擦除码：0=光标至屏尾，1=屏首至光标，2=整屏
        let code = match clear_type {
            ClearType::All => 2,
            ClearType::AfterCursor => 0,
            ClearType::BeforeCursor => 1,
            ClearType::CurrentLine => 2,
            ClearType::UntilNewLine => 0,
        };
        write!(self.writer, "\x1b[{code}J")
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(Size::new(self.width, self.height))
    }

    /// 像素尺寸按 1 格 = 8×16 px 估算（终端无真实像素信息，仅供需要时参考）。
    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        let cols_rows = Size::new(self.width, self.height);
        let pixels = Size::new(self.width * 8, self.height * 16);
        Ok(WindowSize {
            columns_rows: cols_rows,
            pixels,
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.writer.flush()
    }
}

/// 由 [`Style`] 的前景色 / 背景色 / 修饰位编码为 SGR 转义序列（假定已 `\x1b[0m` 复位）。
fn write_sgr<W: Write>(w: &mut W, style: Style) -> std::io::Result<()> {
    if let Some(fg) = style.fg {
        write_color(w, fg, false)?;
    }
    if let Some(bg) = style.bg {
        write_color(w, bg, true)?;
    }
    let m = style.add_modifier;
    if m.contains(Modifier::BOLD) {
        write!(w, "\x1b[1m")?;
    }
    if m.contains(Modifier::DIM) {
        write!(w, "\x1b[2m")?;
    }
    if m.contains(Modifier::ITALIC) {
        write!(w, "\x1b[3m")?;
    }
    if m.contains(Modifier::UNDERLINED) {
        write!(w, "\x1b[4m")?;
    }
    if m.contains(Modifier::REVERSED) {
        write!(w, "\x1b[7m")?;
    }
    Ok(())
}

/// 单个 [`Color`] → SGR。`bg` 决定用前景（30/38）还是背景（40/48）族。
fn write_color<W: Write>(w: &mut W, color: Color, bg: bool) -> std::io::Result<()> {
    match color {
        Color::Reset => {
            // 仅复位该分量
            write!(w, "\x1b[{}m", if bg { 49 } else { 39 })
        }
        Color::Rgb(r, g, b) => {
            let base = if bg { 48 } else { 38 };
            write!(w, "\x1b[{base};2;{r};{g};{b}m")
        }
        Color::Indexed(n) => {
            let base = if bg { 48 } else { 38 };
            write!(w, "\x1b[{base};5;{n}m")
        }
        other => {
            // 标准 / 高亮 16 色：低 8 色基码 +0..7，高 8 色基码 +60
            let (offset, bright) = ansi_offset(other);
            let base = if bg { 40 } else { 30 };
            let code = base + offset + if bright { 60 } else { 0 };
            write!(w, "\x1b[{code}m")
        }
    }
}

/// 16 命名色 → (0..=7 偏移, 是否高亮)。
fn ansi_offset(color: Color) -> (u8, bool) {
    match color {
        Color::Black => (0, false),
        Color::Red => (1, false),
        Color::Green => (2, false),
        Color::Yellow => (3, false),
        Color::Blue => (4, false),
        Color::Magenta => (5, false),
        Color::Cyan => (6, false),
        Color::Gray => (7, false),
        Color::DarkGray => (0, true),
        Color::LightRed => (1, true),
        Color::LightGreen => (2, true),
        Color::LightYellow => (3, true),
        Color::LightBlue => (4, true),
        Color::LightMagenta => (5, true),
        Color::LightCyan => (6, true),
        Color::White => (7, true),
        // 其余变体在 `write_color` 已单独处理，此处兜底为默认前景
        _ => (9, false),
    }
}
