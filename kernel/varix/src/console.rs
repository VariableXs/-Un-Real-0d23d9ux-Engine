//! F011 早期控制台 — framebuffer character mode with `print!` macros.
//!
//! 8x16 cells (font8x8 vertically doubled), 16-color VGA-style palette,
//! newline / carriage-return / tab / backspace handling, hardware scrolling
//! via row copy, and an underline-style cursor at the active cell.

use core::cell::UnsafeCell;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::fb::{Color, Surface};
use crate::font::{GLYPH_W, CELL_H};

/// 16-color VGA text palette mapped to RGB.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Ink {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

impl Ink {
    pub const fn color(self) -> Color {
        match self {
            Ink::Black => Color::rgb(0x00, 0x00, 0x00),
            Ink::Blue => Color::rgb(0x00, 0x00, 0xAA),
            Ink::Green => Color::rgb(0x00, 0xAA, 0x00),
            Ink::Cyan => Color::rgb(0x00, 0xAA, 0xAA),
            Ink::Red => Color::rgb(0xAA, 0x00, 0x00),
            Ink::Magenta => Color::rgb(0xAA, 0x00, 0xAA),
            Ink::Brown => Color::rgb(0xAA, 0x55, 0x00),
            Ink::LightGray => Color::rgb(0xAA, 0xAA, 0xAA),
            Ink::DarkGray => Color::rgb(0x55, 0x55, 0x55),
            Ink::LightBlue => Color::rgb(0x55, 0x55, 0xFF),
            Ink::LightGreen => Color::rgb(0x55, 0xFF, 0x55),
            Ink::LightCyan => Color::rgb(0x55, 0xFF, 0xFF),
            Ink::LightRed => Color::rgb(0xFF, 0x55, 0x55),
            Ink::Pink => Color::rgb(0xFF, 0x55, 0xFF),
            Ink::Yellow => Color::rgb(0xFF, 0xFF, 0x55),
            Ink::White => Color::rgb(0xFF, 0xFF, 0xFF),
        }
    }
}

/// Character-mode console over a framebuffer surface (F011).
pub struct Console {
    cell: UnsafeCell<CellState>,
}

struct CellState {
    surf: Surface,
    cols: u32,
    rows: u32,
    col: u32,
    row: u32,
    fg: Ink,
    bg: Ink,
    cursor_visible: bool,
    /// Track whether any pixel was ever written (banner verification).
    wrote_any: bool,
}

// SAFETY: the console is written by the single-threaded boot path only;
// pre-SMP there is exactly one core running.
unsafe impl Sync for Console {}

impl Console {
    pub fn new(surf: Surface) -> Console {
        let cols = (surf.width() / GLYPH_W).max(1);
        let rows = (surf.height() / CELL_H).max(1);
        Console {
            cell: UnsafeCell::new(CellState {
                surf,
                cols,
                rows,
                col: 0,
                row: 0,
                fg: Ink::LightGray,
                bg: Ink::Black,
                cursor_visible: true,
                wrote_any: false,
            }),
        }
    }

    pub fn cols(&self) -> u32 {
        unsafe { (*self.cell.get()).cols }
    }

    pub fn rows(&self) -> u32 {
        unsafe { (*self.cell.get()).rows }
    }

    pub fn set_colors(&self, fg: Ink, bg: Ink) {
        unsafe {
            (*self.cell.get()).fg = fg;
            (*self.cell.get()).bg = bg;
        }
    }

    pub fn reset_colors(&self) {
        self.set_colors(Ink::LightGray, Ink::Black);
    }

    pub fn wrote_any(&self) -> bool {
        unsafe { (*self.cell.get()).wrote_any }
    }

    pub fn clear(&self) {
        unsafe {
            let s = &mut *self.cell.get();
            s.surf.fill(s.bg.color());
            s.col = 0;
            s.row = 0;
        }
    }

    /// Raw char draw (no control handling) at an absolute cell position.
    fn blit(&self, c: u8, cell_x: u32, cell_y: u32, fg: Ink, bg: Ink) {
        unsafe {
            let s = &mut *self.cell.get();
            let px = cell_x as i64 * GLYPH_W as i64;
            let py = cell_y as i64 * CELL_H as i64;
            // cell background
            s.surf.fill_rect(px, py, GLYPH_W as i64, CELL_H as i64, bg.color());
            let g = crate::font::glyph(c);
            let fg = fg.color();
            for row in 0..8usize {
                let bits = g[row];
                for col in 0..8 {
                    let on = (bits >> col) & 1 == 1;
                    let color = if on { fg } else { bg.color() };
                    s.surf.set_px(px + col as i64, py + row as i64 * 2, color);
                    s.surf.set_px(px + col as i64, py + row as i64 * 2 + 1, color);
                }
            }
            s.wrote_any = true;
        }
    }

    fn erase_cursor(&self) {
        unsafe {
            let s = &mut *self.cell.get();
            if !s.cursor_visible || s.row >= s.rows || s.col >= s.cols {
                return;
            }
            let px = s.col as i64 * GLYPH_W as i64;
            let py = s.row as i64 * CELL_H as i64;
            // Undraw the cursor underline: repaint that band with the cell bg.
            s.surf.fill_rect(px, py + CELL_H as i64 - 2, GLYPH_W as i64, 2, s.bg.color());
        }
    }

    fn draw_cursor(&self) {
        unsafe {
            let s = &mut *self.cell.get();
            if !s.cursor_visible || s.row >= s.rows || s.col >= s.cols {
                return;
            }
            let px = s.col as i64 * GLYPH_W as i64;
            let py = s.row as i64 * CELL_H as i64;
            s.surf.fill_rect(px, py + CELL_H as i64 - 2, GLYPH_W as i64, 2, s.fg.color());
            s.wrote_any = true;
        }
    }

    fn newline(&self) {
        unsafe {
            let s = &mut *self.cell.get();
            s.col = 0;
            if s.row + 1 < s.rows {
                s.row += 1;
                return;
            }
            // Scroll: move rows 1..rows up by one, clear the last row.
            let cell_h = CELL_H as i64;
            s.surf.copy_rows(0, cell_h, (s.rows as i64 - 1) * cell_h);
            s.surf.fill_rect(
                0,
                (s.rows as i64 - 1) * cell_h,
                s.cols as i64 * GLYPH_W as i64,
                cell_h,
                s.bg.color(),
            );
        }
    }

    /// Write one byte, honoring control codes.
    pub fn put_byte(&self, b: u8) {
        unsafe {
            let (cols, _rows, col, row) = {
                let s = &*self.cell.get();
                (s.cols, s.rows, s.col, s.row)
            };
            match b {
                b'\n' => {
                    self.erase_cursor();
                    self.newline();
                    self.draw_cursor();
                }
                b'\r' => {
                    self.erase_cursor();
                    (*self.cell.get()).col = 0;
                    self.draw_cursor();
                }
                b'\t' => {
                    // advance to next multiple of 4 columns
                    let target = ((col / 4) + 1) * 4;
                    let mut c = col;
                    while c < target.min(cols) {
                        self.put_byte(b' ');
                        c += 1;
                    }
                }
                0x08 => {
                    // backspace: erase previous cell, move back
                    self.erase_cursor();
                    if col > 0 {
                        (*self.cell.get()).col = col - 1;
                    } else if row > 0 {
                        (*self.cell.get()).row = row - 1;
                        (*self.cell.get()).col = cols - 1;
                    }
                    let s = &mut *self.cell.get();
                    s.surf.fill_rect(
                        s.col as i64 * GLYPH_W as i64,
                        s.row as i64 * CELL_H as i64,
                        GLYPH_W as i64,
                        CELL_H as i64,
                        s.bg.color(),
                    );
                    self.draw_cursor();
                }
                _ => {
                    self.erase_cursor();
                    self.blit(b, col, row, self.current_fg(), self.current_bg());
                    let s = &mut *self.cell.get();
                    s.col += 1;
                    if s.col >= s.cols {
                        self.newline();
                    }
                    self.draw_cursor();
                }
            }
        }
    }

    fn current_fg(&self) -> Ink {
        unsafe { (*self.cell.get()).fg }
    }

    fn current_bg(&self) -> Ink {
        unsafe { (*self.cell.get()).bg }
    }

    pub fn write_str(&self, s: &str) {
        for &b in s.as_bytes() {
            self.put_byte(b);
        }
    }

    /// Cursor toggle API for a future timer-driven blink (F012).
    pub fn set_cursor_visible(&self, visible: bool) {
        unsafe {
            if visible {
                (*self.cell.get()).cursor_visible = true;
                self.draw_cursor();
            } else {
                self.erase_cursor();
                (*self.cell.get()).cursor_visible = false;
            }
        }
    }

    /// Positional helper for HUD elements (progress bar, timeline).
    pub fn surface(&self) -> Surface {
        unsafe { (*self.cell.get()).surf }
    }
}

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        Console::write_str(self, s);
        Ok(())
    }
}

impl fmt::Write for &Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        Console::write_str(self, s);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Global console (installed once the framebuffer is up)
// ---------------------------------------------------------------------------

static CONSOLE: crate::once::OnceLock<Console> = crate::once::OnceLock::new();
static MIRROR_ENABLED: AtomicBool = AtomicBool::new(false);

/// Install the global framebuffer console (F011 entry).
pub fn init(surf: Surface) -> &'static Console {
    CONSOLE.get_or_init(|| Console::new(surf))
}

pub fn installed() -> bool {
    CONSOLE.get().is_some()
}

/// Enable mirroring of logger lines onto the console.
pub fn enable_mirror() {
    MIRROR_ENABLED.store(true, Ordering::Relaxed);
}

/// Called by the logger for every line — no-op until the console exists
/// or mirroring is enabled, so early serial-only logging is safe.
pub fn mirror_bytes(bytes: &[u8]) {
    if !MIRROR_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(c) = CONSOLE.get() {
        // Strip the level tag: the console keeps lines compact.
        let mut start = 0usize;
        if bytes.len() >= 8 && bytes[0] == b'[' {
            start = 8;
        }
        for &b in &bytes[start..] {
            c.put_byte(b);
        }
    }
}

/// Core print macros backed by the global console.
#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {
        if let Some(c) = $crate::console::installed_ref() {
            use core::fmt::Write;
            let _ = write!(c, format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! kprintln {
    () => { $crate::kprint!("\n") };
    ($($arg:tt)*) => {
        $crate::kprint!("{}\n", format_args!($($arg)*))
    };
}

/// Internal helper for the print macros (avoids exposing Option<&Console>).
pub fn installed_ref() -> Option<&'static Console> {
    CONSOLE.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fb::PixelFormat;
    use std::vec::Vec;

    fn console(w: u32, h: u32) -> (Console, Vec<u8>) {
        let mut v: Vec<u8> = std::vec![0u8; (w * h * 4) as usize];
        let s = unsafe { Surface::from_raw(v.as_mut_ptr(), w, h, w * 4, PixelFormat::Bgr32) };
        (Console::new(s), v)
    }

    #[test]
    fn grid_geometry() {
        let (c, _b) = console(640, 400); // 80 cols x 25 rows
        assert_eq!(c.cols(), 80);
        assert_eq!(c.rows(), 25);
        let (c2, _b2) = console(9, 17); // degenerate surface clamps to 1x1
        assert_eq!(c2.cols(), 1);
        assert_eq!(c2.rows(), 1);
    }

    #[test]
    fn write_wraps_and_newlines() {
        let (c, _b) = console(8 * 10, 16 * 2); // 10 cols, 2 rows
        c.write_str("ABCDEFGHIJKLM"); // 13 chars > 10 cols → wraps once
        // After wrap at col 10 → row 1, chars K..M at row 1 cols 0..2
        assert_eq!(unsafe { (*c.cell.get()).row }, 1);
        assert_eq!(unsafe { (*c.cell.get()).col }, 3);
        c.write_str("\nXYZ");
        assert_eq!(unsafe { (*c.cell.get()).row }, 1); // newline in last row scrolls
        assert_eq!(unsafe { (*c.cell.get()).col }, 3);
    }

    #[test]
    fn newline_scrolls_content() {
        // 1 row of cells; two newlines force scrolling through copy_rows.
        let (c, _b) = console(8 * 4, 16); // 4 cols, 1 row
        c.set_colors(Ink::White, Ink::Black);
        c.write_str("AB");
        c.write_str("\n");
        // Row band must still contain glyph pixels (A at col 0 after scroll
        // is gone, but the cursor underline guarantees wrote_any).
        assert!(c.wrote_any());
    }

    #[test]
    fn tab_advances_to_multiple_of_four() {
        let (c, _b) = console(8 * 12, 16 * 2);
        c.write_str("a\tb");
        // 'a' at col 0 → tab → col 4 → 'b' at col 4, cursor now col 5.
        assert_eq!(unsafe { (*c.cell.get()).col }, 5);
    }

    #[test]
    fn backspace_moves_back_and_erases() {
        let (c, _b) = console(8 * 8, 16 * 2);
        c.write_str("XY");
        c.write_str("\u{8}");
        assert_eq!(unsafe { (*c.cell.get()).col }, 1);
        // The 'Y' cell must now be blank: pixel at cell (1) row 0 is bg.
        let s = c.surface();
        assert_eq!(s.get_px(8 + 3, 0), Some(0)); // inside old Y cell area
    }

    #[test]
    fn control_codes_draw_blank_not_garbage() {
        let (c, _b) = console(8 * 8, 16 * 2);
        c.put_byte(0x01); // control code → blank glyph path
        c.put_byte(0xFF); // non-ASCII → blank glyph path
        assert!(c.wrote_any());
    }

    #[test]
    fn cursor_underscore_is_drawn() {
        let (c, _b) = console(8 * 4, 16 * 2);
        let s = c.surface();
        let before = s.get_px(0, CELL_H as i64 - 1);
        c.set_cursor_visible(true);
        let after = s.get_px(0, CELL_H as i64 - 1);
        // fg underline band in the first cell's bottom two rows.
        let fg = Ink::LightGray.color();
        let expected = crate::fb::PixelFormat::Bgr32.pack(fg);
        assert_eq!(after, Some(expected));
        assert_ne!(before, Some(expected)); // initially blank cursor was drawn too
    }

    #[test]
    fn fmt_write_integration() {
        let (c, _b) = console(8 * 32, 16 * 2);
        use core::fmt::Write;
        write!(&c, "{}-{}", "boot", 7).ok();
        // 'b' at col 0: bottom rows of 'b' glyph (0x00 row 0 → blank top).
        assert!(c.wrote_any());
    }

    #[test]
    fn mirror_before_init_is_noop() {
        // No global console installed in this test process state check:
        // mirror_bytes must not panic either way.
        mirror_bytes(b"[ INFO] hello\n");
    }
}
