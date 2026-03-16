use std::collections::VecDeque;
use std::io::{IsTerminal, Write};

use colored::Colorize;
use crossterm::{cursor, execute, terminal};

const MAX_TAIL_LINES: usize = 5;

pub struct TailDisplay {
    all_lines: Vec<String>,
    tail: VecDeque<String>,
    displayed_count: usize,
    is_tty: bool,
}

impl TailDisplay {
    pub fn new() -> Self {
        Self {
            all_lines: Vec::new(),
            tail: VecDeque::with_capacity(MAX_TAIL_LINES),
            displayed_count: 0,
            is_tty: std::io::stderr().is_terminal(),
        }
    }

    pub fn push_line(&mut self, line: &str) {
        self.all_lines.push(line.to_string());
        self.tail.push_back(line.to_string());
        if self.tail.len() > MAX_TAIL_LINES {
            self.tail.pop_front();
        }

        if self.is_tty {
            self.redraw_tail();
        }
    }

    fn redraw_tail(&mut self) {
        let mut stderr = std::io::stderr();

        // Move up to clear previous tail lines
        if self.displayed_count > 0 {
            let _ = execute!(
                stderr,
                cursor::MoveUp(self.displayed_count as u16),
                terminal::Clear(terminal::ClearType::FromCursorDown),
            );
        }

        // Print tail lines in dim style
        for line in &self.tail {
            let _ = writeln!(stderr, "    {}", line.bright_black());
        }

        self.displayed_count = self.tail.len();
    }

    pub fn clear(&mut self) {
        if !self.is_tty || self.displayed_count == 0 {
            self.displayed_count = 0;
            return;
        }

        let mut stderr = std::io::stderr();
        let _ = execute!(
            stderr,
            cursor::MoveUp(self.displayed_count as u16),
            terminal::Clear(terminal::ClearType::FromCursorDown),
        );
        self.displayed_count = 0;
    }

    pub fn dump_all_red(&self) {
        let mut stderr = std::io::stderr();
        for line in &self.all_lines {
            let _ = writeln!(stderr, "    {}", line.red());
        }
    }
}
