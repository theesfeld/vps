//! Live VT grid so TUI reattach can restore the current screen.

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::Processor;

pub struct Screen {
    term: Term<VoidListener>,
    parser: Processor,
}

impl Screen {
    pub fn new(cols: u16, rows: u16) -> Self {
        let size = TermSize::new(cols.max(1) as usize, rows.max(1) as usize);
        let config = Config {
            scrolling_history: 0,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &size, VoidListener),
            parser: Processor::new(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.term
            .resize(TermSize::new(cols.max(1) as usize, rows.max(1) as usize));
    }

    /// Visible cells as SGR-less ANSI. Kept for tests; attach does not send this
    /// (it crashed iced). TUI resume uses a live size-change redraw instead.
    #[allow(dead_code)]
    pub fn dump_ansi(&self) -> Vec<u8> {
        let cols = self.term.columns();
        let rows = self.term.screen_lines();
        let mut out = Vec::with_capacity(cols * rows + 64);
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            out.extend_from_slice(b"\x1b[?1049h");
        }
        out.extend_from_slice(b"\x1b[2J\x1b[H\x1b[0m");
        let grid = self.term.grid();
        for y in 0..rows {
            let line = Line(y as i32);
            out.extend_from_slice(format!("\x1b[{};1H", y + 1).as_bytes());
            for x in 0..cols {
                let cell = &grid[line][Column(x)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                let c = if cell.c == '\0' { ' ' } else { cell.c };
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
        let cursor = grid.cursor.point;
        let cy = (cursor.line.0.max(0) as usize) + 1;
        let cx = cursor.column.0 + 1;
        out.extend_from_slice(format!("\x1b[{cy};{cx}H").as_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_contains_written_text() {
        let mut s = Screen::new(40, 8);
        s.feed(b"hello-vps\r\n");
        let bytes = s.dump_ansi();
        let dump = String::from_utf8_lossy(&bytes);
        assert!(dump.contains("hello-vps"), "{dump:?}");
    }
}
