//! Terminal styling via `crossterm` — the only place that crate appears.
//!
//! Falls back to plain text when stdout is not a TTY, so piped/redirected
//! output carries no escape codes.

use std::io::{stdout, IsTerminal};

use crossterm::style::{style, Color, Stylize};

use crate::ports::{Style, Styler};

pub struct CrosstermStyler;

impl Styler for CrosstermStyler {
    fn paint(&self, s: Style, text: &str) -> String {
        if !stdout().is_terminal() {
            return text.to_string();
        }
        let color = match s {
            Style::Header => Color::Cyan,
            Style::Field => Color::White,
            Style::Hint => Color::DarkGrey,
            Style::Prompt => Color::Yellow,
            Style::Success => Color::Green,
            Style::Error => Color::Red,
        };
        style(text).with(color).to_string()
    }
}
