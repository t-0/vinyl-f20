//! Bitmap fonts for the 320x240 screen, parsed from BDF at startup.
//!
//! Spleen (BSD-2, `third_party/spleen`) is a monospaced pixel font that covers
//! ASCII, Latin, box drawing and block elements, which is all the UI uses.
//! A glyph is stored as one `u16` per pixel row with the leftmost pixel in
//! bit 15, so a cell is drawn with a shift per column and no lookups.
use anyhow::{Result, bail};
use std::collections::HashMap;

const SPLEEN_5X8: &str = include_str!("../../third_party/spleen/spleen-5x8.bdf");
const SPLEEN_6X12: &str = include_str!("../../third_party/spleen/spleen-6x12.bdf");
const SPLEEN_8X16: &str = include_str!("../../third_party/spleen/spleen-8x16.bdf");

/// Font names accepted in `f20.conf` and on the command line.
pub const NAMES: [&str; 3] = ["5x8", "6x12", "8x16"];
pub const DEFAULT: &str = "6x12";

pub struct Font {
    pub width: usize,
    pub height: usize,
    glyphs: HashMap<u32, Box<[u16]>>,
    fallback: Box<[u16]>,
}

impl Font {
    pub fn named(name: &str) -> Result<Font> {
        let src = match name.trim() {
            "5x8" => SPLEEN_5X8,
            "6x12" => SPLEEN_6X12,
            "8x16" => SPLEEN_8X16,
            other => bail!("unknown font `{other}` (use 5x8, 6x12 or 8x16)"),
        };
        Font::parse(src)
    }

    pub fn parse(bdf: &str) -> Result<Font> {
        let (mut fw, mut fh, mut fxo, mut fyo) = (0usize, 0usize, 0i32, 0i32);
        let mut glyphs: HashMap<u32, Box<[u16]>> = HashMap::new();
        let mut enc: Option<u32> = None;
        let mut bbx = (0usize, 0usize, 0i32, 0i32);
        let mut in_bitmap = false;
        let mut rows: Vec<u16> = Vec::new();
        let mut row_i = 0usize;
        for line in bdf.lines() {
            if in_bitmap {
                if line.trim() == "ENDCHAR" {
                    in_bitmap = false;
                    if let Some(e) = enc.take() {
                        glyphs.insert(e, rows.clone().into_boxed_slice());
                    }
                    continue;
                }
                let hex = line.trim();
                let nbits = hex.len() * 4;
                let bits = u64::from_str_radix(hex, 16).unwrap_or(0);
                let (bw, bh, bxo, byo) = bbx;
                // BDF places the glyph box relative to the baseline; the cell
                // top is at baseline + fyo + fh, the glyph top at byo + bh.
                let y = (fyo + fh as i32) - (byo + bh as i32) + row_i as i32;
                if y >= 0 && (y as usize) < fh && nbits <= 64 {
                    for c in 0..bw.min(nbits) {
                        if (bits >> (nbits - 1 - c)) & 1 == 1 {
                            let x = bxo - fxo + c as i32;
                            if (0..16).contains(&x) && (x as usize) < fw {
                                rows[y as usize] |= 0x8000 >> x;
                            }
                        }
                    }
                }
                row_i += 1;
                continue;
            }
            let mut it = line.split_whitespace();
            let num = |s: Option<&str>| s.and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
            match it.next() {
                Some("FONTBOUNDINGBOX") => {
                    fw = num(it.next()).max(0) as usize;
                    fh = num(it.next()).max(0) as usize;
                    fxo = num(it.next());
                    fyo = num(it.next());
                }
                Some("STARTCHAR") => {
                    enc = None;
                    bbx = (0, 0, 0, 0);
                }
                Some("ENCODING") => {
                    let e = num(it.next());
                    enc = (e >= 0).then_some(e as u32);
                }
                Some("BBX") => {
                    bbx = (
                        num(it.next()).max(0) as usize,
                        num(it.next()).max(0) as usize,
                        num(it.next()),
                        num(it.next()),
                    );
                }
                Some("BITMAP") => {
                    in_bitmap = true;
                    rows = vec![0u16; fh];
                    row_i = 0;
                }
                _ => {}
            }
        }
        if fw == 0 || fh == 0 || fw > 16 || glyphs.is_empty() {
            bail!("not a usable BDF font");
        }
        for cp in SYNTH {
            // Missing, or present but blank (Spleen 5x8 ships an empty ±).
            let blank = glyphs.get(&cp).is_none_or(|g| g.iter().all(|r| *r == 0));
            if blank && let Some(g) = synth(char::from_u32(cp).unwrap_or(' '), fw, fh) {
                glyphs.insert(cp, g.into_boxed_slice());
            }
        }
        let fallback = glyphs
            .get(&('?' as u32))
            .cloned()
            .unwrap_or_else(|| vec![0u16; fh].into_boxed_slice());
        Ok(Font {
            width: fw,
            height: fh,
            glyphs,
            fallback,
        })
    }

    /// Pixel rows of `c`, or of `?` when the font has no such glyph.
    pub fn glyph(&self, c: char) -> &[u16] {
        self.glyphs
            .get(&(c as u32))
            .map(|g| &**g)
            .unwrap_or(&self.fallback)
    }

    pub fn has(&self, c: char) -> bool {
        self.glyphs.contains_key(&(c as u32))
    }

    pub fn len(&self) -> usize {
        self.glyphs.len()
    }
}

/// Code points drawn geometrically when the font has no glyph for them:
/// the symbols vinyl's UI and ratatui's gauge use that Spleen lacks.
const SYNTH: [u32; 67] = [
    0x00B1, 0x2014, 0x2026, 0x2190, 0x2191, 0x2192, 0x2193, 0x2212, 0x23CE, 0x23F8, 0x2500, 0x2501,
    0x2502, 0x2503, 0x253C, 0x2580, 0x2581, 0x2582, 0x2583, 0x2584, 0x2585, 0x2586, 0x2587,
    0x2588, 0x2589, 0x258A, 0x258B, 0x258C, 0x258D, 0x258E, 0x258F, 0x2590, 0x2591, 0x2592,
    0x2593, 0x2594, 0x2595, 0x25A0, 0x25A1, 0x25AE, 0x25AF, 0x25B0, 0x25B1, 0x25B2, 0x25B6,
    0x25B8, 0x25BC, 0x25C0, 0x25C2, 0x25CB, 0x25CF, 0x26A0, 0x2714, 0x2718, 0x25B3, 0x25BD,
    0x25C6, 0x25C7, 0x2598, 0x259D, 0x2596, 0x2597, 0x259A, 0x259E, 0x2599, 0x259B, 0x259C,
];

struct Canvas {
    w: usize,
    h: usize,
    rows: Vec<u16>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, rows: vec![0; h] }
    }
    fn put(&mut self, x: i32, y: i32) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.rows[y as usize] |= 0x8000 >> x;
        }
    }
    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        for y in y0..y1 {
            for x in x0..x1 {
                self.put(x, y);
            }
        }
    }
    fn hline(&mut self, y: i32, x0: i32, x1: i32) {
        self.rect(x0, y, x1, y + 1);
    }
    fn vline(&mut self, x: i32, y0: i32, y1: i32) {
        self.rect(x, y0, x + 1, y1);
    }
    /// A triangle pointing `dir` (0 right, 1 left, 2 up, 3 down) filling the
    /// cell with a one pixel margin.
    fn triangle(&mut self, dir: u8, outline: bool) {
        let (w, h) = (self.w as i32, self.h as i32);
        let (x0, x1, y0, y1) = (1, w - 1, 1, h - 2);
        for y in y0..y1 {
            for x in x0..x1 {
                // position along the axis of the point, 0 at base .. 1 at tip
                let (t, s) = match dir {
                    0 => ((x - x0) as f32 / (x1 - x0 - 1).max(1) as f32, (y - y0) as f32 / (y1 - y0 - 1).max(1) as f32),
                    1 => ((x1 - 1 - x) as f32 / (x1 - x0 - 1).max(1) as f32, (y - y0) as f32 / (y1 - y0 - 1).max(1) as f32),
                    2 => ((y1 - 1 - y) as f32 / (y1 - y0 - 1).max(1) as f32, (x - x0) as f32 / (x1 - x0 - 1).max(1) as f32),
                    _ => ((y - y0) as f32 / (y1 - y0 - 1).max(1) as f32, (x - x0) as f32 / (x1 - x0 - 1).max(1) as f32),
                };
                let half = (1.0 - t) / 2.0;
                let inside = (s - 0.5).abs() <= half + 0.01;
                let edge = (s - 0.5).abs() >= half - 0.35 || t <= 0.01;
                if inside && (!outline || edge) {
                    self.put(x, y);
                }
            }
        }
    }
}

/// Draws symbols that bitmap fonts often lack, sized to the cell.
fn synth(c: char, w: usize, h: usize) -> Option<Vec<u16>> {
    let mut g = Canvas::new(w, h);
    let (wi, hi) = (w as i32, h as i32);
    let (cx, cy) = (wi / 2, hi / 2 - 1);
    let frac = |n: i32, of: i32, total: i32| (total * n + of - 1) / of; // ceil(total*n/of)
    match c as u32 {
        0x2580 => g.rect(0, 0, wi, hi / 2),
        0x2581..=0x2588 => {
            let n = (c as u32 - 0x2580) as i32;
            g.rect(0, hi - frac(n, 8, hi), wi, hi)
        }
        0x2589..=0x258F => {
            let n = (0x2590 - c as u32) as i32;
            g.rect(0, 0, frac(n, 8, wi), hi)
        }
        0x2590 => g.rect(wi / 2, 0, wi, hi),
        0x2591 => (0..hi).for_each(|y| (0..wi).for_each(|x| if (x + 2 * (y % 2)) % 4 == 0 { g.put(x, y) })),
        0x2592 => (0..hi).for_each(|y| (0..wi).for_each(|x| if (x + y) % 2 == 0 { g.put(x, y) })),
        0x2593 => (0..hi).for_each(|y| (0..wi).for_each(|x| if (x + 2 * (y % 2)) % 4 != 0 { g.put(x, y) })),
        0x2594 => g.hline(0, 0, wi),
        0x2595 => g.vline(wi - 1, 0, hi),
        0x2596 => g.rect(0, hi / 2, wi / 2, hi),
        0x2597 => g.rect(wi / 2, hi / 2, wi, hi),
        0x2598 => g.rect(0, 0, wi / 2, hi / 2),
        0x2599 => {
            g.rect(0, 0, wi / 2, hi);
            g.rect(0, hi / 2, wi, hi)
        }
        0x259A => {
            g.rect(0, 0, wi / 2, hi / 2);
            g.rect(wi / 2, hi / 2, wi, hi)
        }
        0x259B => {
            g.rect(0, 0, wi, hi / 2);
            g.rect(0, 0, wi / 2, hi)
        }
        0x259C => {
            g.rect(0, 0, wi, hi / 2);
            g.rect(wi / 2, 0, wi, hi)
        }
        0x259D => g.rect(wi / 2, 0, wi, hi / 2),
        0x259E => {
            g.rect(wi / 2, 0, wi, hi / 2);
            g.rect(0, hi / 2, wi / 2, hi)
        }
        0x2500 => g.hline(cy, 0, wi),
        0x2501 => g.rect(0, cy, wi, cy + 2),
        0x2502 => g.vline(cx, 0, hi),
        0x2503 => g.rect(cx, 0, cx + 2, hi),
        0x253C => {
            g.hline(cy, 0, wi);
            g.vline(cx, 0, hi)
        }
        0x2014 => g.hline(cy, 0, wi),
        0x2212 => g.hline(cy, 1, wi - 1),
        0x00B1 => {
            g.hline(cy - 1, 1, wi - 1);
            g.vline(cx, cy - 1 - (hi / 4).max(1), cy + (hi / 4).max(1));
            g.hline(hi - 2, 1, wi - 1);
        }
        0x2026 => {
            let y = hi - 3;
            g.put(0, y);
            g.put(cx, y);
            g.put(wi - 1, y);
        }
        0x2190 | 0x2192 => {
            g.hline(cy, 1, wi - 1);
            let head = if c as u32 == 0x2190 { 1 } else { wi - 2 };
            let dir = if c as u32 == 0x2190 { 1 } else { -1 };
            for i in 1..=2.min(hi / 4) {
                g.put(head + dir * i, cy - i);
                g.put(head + dir * i, cy + i);
            }
        }
        0x2191 | 0x2193 => {
            let (top, bot) = (1, hi - 2);
            g.vline(cx, top, bot);
            let (head, dir) = if c as u32 == 0x2191 { (top, 1) } else { (bot - 1, -1) };
            for i in 1..=2.min(wi / 3) {
                g.put(cx - i, head + dir * i);
                g.put(cx + i, head + dir * i);
            }
        }
        0x23CE => {
            g.vline(wi - 2, 2, cy + 1);
            g.hline(cy, 1, wi - 1);
            g.put(2, cy - 1);
            g.put(2, cy + 1);
        }
        0x23F8 => {
            g.rect(1, 2, 1 + (wi / 3).max(1), hi - 3);
            g.rect(wi - 1 - (wi / 3).max(1), 2, wi - 1, hi - 3);
        }
        0x25B6 | 0x25B8 => g.triangle(0, false),
        0x25C0 | 0x25C2 => g.triangle(1, false),
        0x25B2 => g.triangle(2, false),
        0x25BC => g.triangle(3, false),
        0x25B3 => g.triangle(2, true),
        0x25BD => g.triangle(3, true),
        0x25CF | 0x25CB => {
            let r = (wi.min(hi) / 2 - 1).max(1) as f32;
            let (ox, oy) = ((wi - 1) as f32 / 2.0, cy as f32);
            for y in 0..hi {
                for x in 0..wi {
                    let d = ((x as f32 - ox).powi(2) + (y as f32 - oy).powi(2)).sqrt();
                    if d <= r + 0.3 && (c as u32 == 0x25CF || d >= r - 0.8) {
                        g.put(x, y);
                    }
                }
            }
        }
        0x25C6 | 0x25C7 => {
            let r = (wi.min(hi) / 2 - 1).max(1);
            for y in 0..hi {
                for x in 0..wi {
                    let d = (x - cx).abs() + (y - cy).abs();
                    if d <= r && (c as u32 == 0x25C6 || d >= r - 1) {
                        g.put(x, y);
                    }
                }
            }
        }
        0x25A0 | 0x25AE | 0x25B0 => g.rect(1, 1, wi - 1, hi - 2),
        0x25A1 | 0x25AF | 0x25B1 => {
            g.rect(1, 1, wi - 1, hi - 2);
            g.rows.iter_mut().enumerate().for_each(|(y, r)| {
                if y > 1 && (y as i32) < hi - 3 {
                    *r &= 0x8000 >> 1 | 0x8000 >> (wi - 2);
                }
            });
        }
        0x26A0 => {
            g.triangle(2, true);
            g.vline(cx, hi / 3, hi * 2 / 3 - 1);
            g.put(cx, hi * 2 / 3 + 1);
        }
        0x2714 => {
            for i in 0..wi / 2 {
                g.put(wi / 4 + i, cy + i / 2);
            }
            for i in 0..hi / 2 {
                g.put(wi / 2 + i, cy + wi / 4 - i);
            }
        }
        0x2718 => {
            for i in 1..(wi - 1).min(hi - 2) {
                g.put(i, i);
                g.put(i, hi - 2 - i);
            }
        }
        _ => return None,
    }
    Some(g.rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_vinyl_uses_are_all_drawable() {
        // Everything non-ASCII in external/vinyl/src plus ratatui's gauge blocks.
        let needed = "±—…←↑→↓−⏎⏸─┃┼▁▂▃▄▅▆▇█▏▎▍▌▋▊▉░▒▓▶▸●⚠▰▱";
        for name in NAMES {
            let f = Font::named(name).unwrap();
            for c in needed.chars() {
                assert!(f.has(c), "{name} cannot draw {c:?} (U+{:04X})", c as u32);
                assert!(f.glyph(c).iter().any(|r| *r != 0), "{name} draws {c:?} blank");
            }
        }
    }

    #[test]
    fn synthesized_blocks_fill_the_right_fraction() {
        let f = Font::named("6x12").unwrap();
        let full = f.glyph('█');
        assert!(full.iter().all(|r| *r == 0xfc00), "full block fills all 6 columns");
        let lower_half = f.glyph('▄');
        assert!(lower_half[..6].iter().all(|r| *r == 0));
        assert!(lower_half[6..].iter().all(|r| *r == 0xfc00));
        let left_eighth = f.glyph('▏');
        assert!(left_eighth.iter().all(|r| *r == 0x8000));
    }

    #[test]
    fn all_fonts_parse_with_expected_cells() {
        for (name, w, h) in [("5x8", 5, 8), ("6x12", 6, 12), ("8x16", 8, 16)] {
            let f = Font::named(name).unwrap();
            assert_eq!((f.width, f.height), (w, h), "{name}");
            assert!(f.len() > 400, "{name} has {} glyphs", f.len());
            // Printable ASCII, box drawing and block elements are all there.
            for c in (' '..='~').chain("─│┌┐└┘├┤┬┴┼█▌▐▀▄░▒▓".chars()) {
                assert!(f.has(c), "{name} lacks {c:?}");
            }
            assert!(f.glyph('A').iter().any(|r| *r != 0));
            assert!(f.glyph(' ').iter().all(|r| *r == 0));
        }
    }

    #[test]
    fn glyph_pixels_stay_inside_the_cell() {
        let f = Font::named("6x12").unwrap();
        let mask: u16 = !(0xffffu16 >> f.width);
        for c in ' '..='~' {
            for row in f.glyph(c) {
                assert_eq!(row & !mask, 0, "{c:?} draws outside its 6 columns");
            }
        }
    }

    #[test]
    fn unknown_glyph_falls_back_to_question_mark() {
        let f = Font::named("6x12").unwrap();
        assert_eq!(f.glyph('\u{1F600}'), f.glyph('?'));
    }
}
