//! Minimal terminal checkbox list, std only: ↑/↓ (or j/k) move, Space toggles, `a` toggles
//! all, Enter confirms, Esc/q keeps the current choice. Raw mode comes from the system `stty`,
//! drawing from ANSI escapes. The terminal is restored on every exit path, including Ctrl+C.

use std::{
    io::{self, Read, Write},
    process::{Command, Stdio},
};

pub struct Item {
    pub label: String,
    /// Dimmed text after the label.
    pub detail: String,
    pub checked: bool,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Enter: the checked state of every item (at least one is checked).
    Confirmed(Vec<bool>),
    /// Esc or q: leave things as they were.
    Kept,
}

const HINT: &str = "↑/↓ move · space toggle · a all · enter confirm · esc keep";

/// Shows the list until the user confirms or cancels. Fails if the terminal can't be put in
/// raw mode (e.g. no `stty`). Ctrl+C restores the terminal and exits with status 130.
pub fn run(title: &str, items: &[Item], color: bool) -> io::Result<Outcome> {
    let raw_mode = RawMode::enable()?;
    let mut list = Checklist::new(items.iter().map(|item| item.checked).collect());
    let mut out = io::stdout();
    write!(out, "\x1b[?25l")?; // hide cursor
    let mut drawn = 0;
    let mut warning = None;
    let outcome = loop {
        drawn = draw(&mut out, title, items, &list, warning, color, drawn)?;
        warning = None;
        match list.handle(read_key()?) {
            Step::Continue => {}
            Step::NeedOne => warning = Some("select at least one, or press esc to keep"),
            Step::Done(outcome) => break outcome,
            Step::Abort => {
                erase(&mut out, drawn)?;
                drop(raw_mode);
                std::process::exit(130);
            }
        }
    };
    erase(&mut out, drawn)?;
    Ok(outcome)
}

// --- state (pure, unit-tested) -------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Key {
    Up,
    Down,
    Toggle,
    ToggleAll,
    Confirm,
    Cancel,
    Abort,
    Other,
}

#[derive(Debug, PartialEq)]
enum Step {
    Continue,
    NeedOne,
    Done(Outcome),
    Abort,
}

struct Checklist {
    checked: Vec<bool>,
    cursor: usize,
}

impl Checklist {
    fn new(checked: Vec<bool>) -> Self {
        Self { checked, cursor: 0 }
    }

    fn handle(&mut self, key: Key) -> Step {
        let len = self.checked.len();
        match key {
            Key::Up => self.cursor = (self.cursor + len - 1) % len,
            Key::Down => self.cursor = (self.cursor + 1) % len,
            Key::Toggle => self.checked[self.cursor] = !self.checked[self.cursor],
            Key::ToggleAll => {
                let all = self.checked.iter().all(|&checked| checked);
                self.checked.fill(!all);
            }
            Key::Confirm if !self.checked.contains(&true) => return Step::NeedOne,
            Key::Confirm => return Step::Done(Outcome::Confirmed(self.checked.clone())),
            Key::Cancel => return Step::Done(Outcome::Kept),
            Key::Abort => return Step::Abort,
            Key::Other => {}
        }
        Step::Continue
    }
}

/// One read's worth of input. Arrow keys arrive as a single escape sequence; a lone ESC byte
/// is the Esc key.
fn parse_key(bytes: &[u8]) -> Key {
    match bytes {
        [0x1b, b'[' | b'O', b'A', ..] | [b'k', ..] => Key::Up,
        [0x1b, b'[' | b'O', b'B', ..] | [b'j', ..] => Key::Down,
        [0x1b] | [b'q', ..] => Key::Cancel,
        [b' ', ..] => Key::Toggle,
        [b'a' | b'A', ..] => Key::ToggleAll,
        [b'\r' | b'\n', ..] => Key::Confirm,
        [0x03, ..] => Key::Abort, // Ctrl+C (signals are off in raw mode)
        _ => Key::Other,
    }
}

// --- terminal ------------------------------------------------------------------------------

fn read_key() -> io::Result<Key> {
    let mut buf = [0u8; 8];
    let n = io::stdin().read(&mut buf)?;
    Ok(if n == 0 { Key::Cancel } else { parse_key(&buf[..n]) })
}

/// Redraws the list over the `previous` lines drawn last time; returns the lines drawn now.
fn draw(
    out: &mut impl Write,
    title: &str,
    items: &[Item],
    list: &Checklist,
    warning: Option<&str>,
    color: bool,
    previous: usize,
) -> io::Result<usize> {
    let paint = |code: &str, text: &str| {
        if color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    };
    erase(out, previous)?;
    writeln!(out, "  {} {title}", paint("34", "?"))?;
    let width = items.iter().map(|item| item.label.chars().count()).max().unwrap_or(0);
    for (i, item) in items.iter().enumerate() {
        let here = i == list.cursor;
        let pointer = if here { paint("36", "❯") } else { " ".into() };
        let checkbox = if list.checked[i] { paint("32", "[x]") } else { "[ ]".into() };
        let label = format!("{:<width$}", item.label);
        let label = if here { paint("1", &label) } else { label };
        writeln!(out, "    {pointer} {checkbox} {label}  {}", paint("2", &item.detail))?;
    }
    let hint = match warning {
        Some(warning) => paint("33", warning),
        None => paint("2", HINT),
    };
    writeln!(out, "    {hint}")?;
    out.flush()?;
    Ok(items.len() + 2)
}

/// Moves up over `lines` drawn lines and clears from there to the end of the screen.
fn erase(out: &mut impl Write, lines: usize) -> io::Result<()> {
    if lines > 0 {
        write!(out, "\x1b[{lines}A")?;
    }
    write!(out, "\r\x1b[J")?;
    out.flush()
}

/// Keys without line buffering or echo while alive; restores the saved terminal state (and the
/// cursor) when dropped.
struct RawMode {
    saved: String,
}

impl RawMode {
    fn enable() -> io::Result<Self> {
        let saved = stty(&["-g"])?.trim().to_string();
        stty(&["-icanon", "-echo", "-isig", "min", "1", "time", "0"])?;
        Ok(Self { saved })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        stty(&[&self.saved]).ok();
        print!("\x1b[?25h"); // show cursor
        io::stdout().flush().ok();
    }
}

/// Runs `stty` on our terminal (it acts on its stdin, so that must be inherited).
fn stty(args: &[&str]) -> io::Result<String> {
    let output = Command::new("stty").args(args).stdin(Stdio::inherit()).stderr(Stdio::null()).output()?;
    if !output.status.success() {
        return Err(io::Error::other("stty failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys() {
        assert_eq!(parse_key(b"\x1b[A"), Key::Up);
        assert_eq!(parse_key(b"\x1bOB"), Key::Down);
        assert_eq!(parse_key(b"k"), Key::Up);
        assert_eq!(parse_key(b"\x1b"), Key::Cancel);
        assert_eq!(parse_key(b"\x1b[C"), Key::Other);
        assert_eq!(parse_key(b" "), Key::Toggle);
        assert_eq!(parse_key(b"\n"), Key::Confirm);
        assert_eq!(parse_key(b"\x03"), Key::Abort);
    }

    #[test]
    fn cursor_wraps_around() {
        let mut list = Checklist::new(vec![false, false, false]);
        list.handle(Key::Up);
        assert_eq!(list.cursor, 2);
        list.handle(Key::Down);
        assert_eq!(list.cursor, 0);
    }

    #[test]
    fn toggles_and_confirms() {
        let mut list = Checklist::new(vec![true, false]);
        list.handle(Key::Down);
        list.handle(Key::Toggle);
        assert_eq!(list.handle(Key::Confirm), Step::Done(Outcome::Confirmed(vec![true, true])));
    }

    #[test]
    fn toggle_all_clears_when_everything_is_checked() {
        let mut list = Checklist::new(vec![true, false]);
        list.handle(Key::ToggleAll);
        assert_eq!(list.checked, [true, true]);
        list.handle(Key::ToggleAll);
        assert_eq!(list.checked, [false, false]);
    }

    #[test]
    fn needs_one_checked_to_confirm() {
        let mut list = Checklist::new(vec![false, false]);
        assert_eq!(list.handle(Key::Confirm), Step::NeedOne);
        assert_eq!(list.handle(Key::Cancel), Step::Done(Outcome::Kept));
    }
}
