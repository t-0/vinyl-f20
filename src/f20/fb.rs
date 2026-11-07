//! A ratatui backend that draws cells into a pixel buffer with a bitmap font.
//!
//! ratatui hands the backend only the cells that changed since the last
//! frame; each one is rendered into `pixels` (0x00RRGGBB) and `flush` hands
//! the whole frame to a [`Present`] sink: the Linux framebuffer on the player,
//! or a capture buffer for previews on the desktop.
use super::font::Font;
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use ratatui::style::{Color, Modifier};
use std::io;

/// Where finished frames go.
pub trait Present {
    fn present(&mut self, pixels: &[u32], width: usize, height: usize) -> io::Result<()>;
}

/// Keeps the last frame; used for previews and tests.
#[derive(Default)]
pub struct Capture {
    pub frame: Vec<u32>,
    pub width: usize,
    pub height: usize,
    pub presented: usize,
}

impl Present for Capture {
    fn present(&mut self, pixels: &[u32], width: usize, height: usize) -> io::Result<()> {
        self.frame.clear();
        self.frame.extend_from_slice(pixels);
        self.width = width;
        self.height = height;
        self.presented += 1;
        Ok(())
    }
}

pub type Rgb = (u8, u8, u8);
pub const DEFAULT_FG: Rgb = (221, 221, 221);
pub const DEFAULT_BG: Rgb = (0, 0, 0);

pub struct FbBackend<P: Present> {
    font: Font,
    width: usize,
    height: usize,
    cols: u16,
    rows: u16,
    x0: usize,
    y0: usize,
    pixels: Vec<u32>,
    cells: Vec<char>,
    dirty: bool,
    cursor: Position,
    sink: P,
}

impl<P: Present> FbBackend<P> {
    pub fn new(font: Font, width: usize, height: usize, sink: P) -> io::Result<Self> {
        let cols = (width / font.width) as u16;
        let rows = (height / font.height) as u16;
        if cols < 20 || rows < 8 {
            return Err(io::Error::other(format!(
                "{width}x{height} pixels is too small for a {}x{} font ({cols}x{rows} cells)",
                font.width, font.height
            )));
        }
        let x0 = (width - cols as usize * font.width) / 2;
        let y0 = (height - rows as usize * font.height) / 2;
        Ok(Self {
            font,
            width,
            height,
            cols,
            rows,
            x0,
            y0,
            pixels: vec![pack(DEFAULT_BG); width * height],
            cells: vec![' '; cols as usize * rows as usize],
            dirty: true,
            cursor: Position::new(0, 0),
            sink,
        })
    }

    pub fn sink(&self) -> &P {
        &self.sink
    }

    pub fn sink_mut(&mut self) -> &mut P {
        &mut self.sink
    }

    pub fn cells(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    /// The screen as text, one line per cell row (wide glyphs count once).
    pub fn text(&self) -> String {
        let mut s = String::new();
        for r in 0..self.rows as usize {
            let row = &self.cells[r * self.cols as usize..(r + 1) * self.cols as usize];
            s.extend(row.iter());
            s.push('\n');
        }
        s
    }

    fn draw_cell(&mut self, x: u16, y: u16, cell: &Cell) {
        let (mut fg, mut bg) = (rgb(cell.fg, DEFAULT_FG), rgb(cell.bg, DEFAULT_BG));
        let m = cell.modifier;
        if m.contains(Modifier::REVERSED) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if m.contains(Modifier::BOLD) {
            fg = brighten(fg);
        }
        if m.contains(Modifier::DIM) {
            fg = dim(fg);
        }
        if m.contains(Modifier::HIDDEN) {
            fg = bg;
        }
        let ch = cell.symbol().chars().next().unwrap_or(' ');
        let (fw, fh) = (self.font.width, self.font.height);
        let mut glyph = [0u16; 32];
        glyph[..fh].copy_from_slice(self.font.glyph(ch));
        let (fgp, bgp) = (pack(fg), pack(bg));
        let px = self.x0 + x as usize * fw;
        let py = self.y0 + y as usize * fh;
        for r in 0..fh {
            let mut bits = glyph[r];
            if (m.contains(Modifier::UNDERLINED) && r == fh - 1)
                || (m.contains(Modifier::CROSSED_OUT) && r == fh / 2)
            {
                bits = 0xffff;
            }
            let base = (py + r) * self.width + px;
            for c in 0..fw {
                self.pixels[base + c] = if bits & (0x8000 >> c) != 0 { fgp } else { bgp };
            }
        }
        self.cells[y as usize * self.cols as usize + x as usize] = ch;
        self.dirty = true;
    }
}

pub fn pack((r, g, b): Rgb) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn brighten((r, g, b): Rgb) -> Rgb {
    let f = |v: u8| v.saturating_add((255 - v as u16) as u8 / 3);
    (f(r), f(g), f(b))
}

fn dim((r, g, b): Rgb) -> Rgb {
    (r / 2 + r / 4, g / 2 + g / 4, b / 2 + b / 4)
}

const ANSI: [Rgb; 16] = [
    (0, 0, 0),
    (205, 49, 49),
    (13, 188, 121),
    (229, 190, 16),
    (36, 114, 200),
    (188, 63, 188),
    (17, 168, 205),
    (204, 204, 204),
    (110, 110, 110),
    (241, 76, 76),
    (35, 209, 139),
    (245, 230, 67),
    (59, 142, 234),
    (214, 112, 214),
    (41, 184, 219),
    (255, 255, 255),
];

pub fn rgb(c: Color, default: Rgb) -> Rgb {
    match c {
        Color::Reset => default,
        Color::Black => ANSI[0],
        Color::Red => ANSI[1],
        Color::Green => ANSI[2],
        Color::Yellow => ANSI[3],
        Color::Blue => ANSI[4],
        Color::Magenta => ANSI[5],
        Color::Cyan => ANSI[6],
        Color::Gray => ANSI[7],
        Color::DarkGray => ANSI[8],
        Color::LightRed => ANSI[9],
        Color::LightGreen => ANSI[10],
        Color::LightYellow => ANSI[11],
        Color::LightBlue => ANSI[12],
        Color::LightMagenta => ANSI[13],
        Color::LightCyan => ANSI[14],
        Color::White => ANSI[15],
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(i) => indexed(i),
    }
}

fn indexed(i: u8) -> Rgb {
    match i {
        0..=15 => ANSI[i as usize],
        16..=231 => {
            let n = i - 16;
            let v = |x: u8| if x == 0 { 0 } else { 55 + 40 * x };
            (v(n / 36), v((n / 6) % 6), v(n % 6))
        }
        _ => {
            let g = 8 + 10 * (i - 232);
            (g, g, g)
        }
    }
}

impl<P: Present> Backend for FbBackend<P> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            if x < self.cols && y < self.rows {
                self.draw_cell(x, y, cell);
            }
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }

    fn set_cursor_position<Pos: Into<Position>>(&mut self, position: Pos) -> io::Result<()> {
        self.cursor = position.into();
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        self.pixels.fill(pack(DEFAULT_BG));
        self.cells.fill(' ');
        self.dirty = true;
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        match clear_type {
            ClearType::All => self.clear(),
            _ => Ok(()),
        }
    }

    fn size(&self) -> io::Result<Size> {
        Ok(Size::new(self.cols, self.rows))
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: Size::new(self.cols, self.rows),
            pixels: Size::new(self.width as u16, self.height as u16),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.dirty {
            self.sink.present(&self.pixels, self.width, self.height)?;
            self.dirty = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::style::Style;
    use ratatui::widgets::Paragraph;

    fn backend() -> FbBackend<Capture> {
        FbBackend::new(Font::named("6x12").unwrap(), 320, 240, Capture::default()).unwrap()
    }

    #[test]
    fn screen_is_53_by_20_cells_with_6x12() {
        assert_eq!(backend().cells(), (53, 20));
    }

    #[test]
    fn text_is_drawn_in_the_requested_colours() {
        let mut t = Terminal::new(backend()).unwrap();
        t.draw(|f| {
            let p = Paragraph::new("Hi").style(Style::default().fg(Color::Red).bg(Color::Blue));
            f.render_widget(p, f.area());
        })
        .unwrap();
        let b = t.backend();
        let cap = b.sink();
        assert_eq!(cap.presented, 1);
        assert_eq!((cap.width, cap.height), (320, 240));
        let red = pack(ANSI[1]);
        let blue = pack(ANSI[4]);
        assert!(cap.frame.contains(&red), "glyph pixels in red");
        assert!(cap.frame.iter().filter(|p| **p == blue).count() > 300 * 200);
        assert!(b.text().starts_with("Hi "));
    }

    #[test]
    fn only_changed_cells_trigger_a_present() {
        let mut t = Terminal::new(backend()).unwrap();
        for _ in 0..3 {
            t.draw(|f| f.render_widget(Paragraph::new("same"), f.area()))
                .unwrap();
        }
        assert_eq!(t.backend().sink().presented, 1);
    }

    #[test]
    fn indexed_colours_cover_the_cube_and_grays() {
        assert_eq!(indexed(16), (0, 0, 0));
        assert_eq!(indexed(231), (255, 255, 255));
        assert_eq!(indexed(232), (8, 8, 8));
        assert_eq!(indexed(255), (238, 238, 238));
    }
}
