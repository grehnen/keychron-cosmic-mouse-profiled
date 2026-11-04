//! `mouse-profile configure`: creates `apps.conf` (asking for the profile count) and lets the
//! user pick which devices get the profile command. In an existing config only `device` lines
//! are changed.

use std::{
    fs,
    io::{self, BufRead, IsTerminal, Write},
    path::Path,
};

use crate::{
    checklist::{self, Item, Outcome},
    config_path, mouse_interfaces, UsbId, DEFAULT_PROFILE_COUNT, MAX_PROFILE_COUNT,
};

const EXAMPLE_CONFIG: &str = include_str!("../apps.conf.example");

pub fn run() -> Result<(), String> {
    let ui = Ui::new();
    let path = config_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => {
            ui.ok(&format!("using existing {}", pretty_path(&path)));
            text
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let count = ask_profile_count(&ui);
            let text = with_profile_count(EXAMPLE_CONFIG, count);
            write_config(&path, &text)?;
            ui.ok(&format!("created {} with {count} profiles", pretty_path(&path)));
            text
        }
        Err(err) => return Err(format!("{}: {err}", path.display())),
    };

    let text = match pick_devices(&ui, &text) {
        Some(new_text) if new_text != text => {
            write_config(&path, &new_text)?;
            new_text
        }
        _ => text,
    };

    let saved = saved_devices(&text);
    if saved.is_empty() {
        ui.ok("profile command goes to every connected Keychron mouse");
    } else {
        let names: Vec<&str> = saved.iter().map(|(_, name)| name.as_str()).collect();
        ui.ok(&format!("profile command goes to: {}", names.join(", ")));
    }
    Ok(())
}

fn write_config(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    // Writing in place keeps the file's permissions.
    fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
}

fn pretty_path(path: &Path) -> String {
    let path = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => format!("~{}", &path[home.len()..]),
        _ => path,
    }
}

// --- prompts -------------------------------------------------------------------------------

fn ask_profile_count(ui: &Ui) -> u8 {
    if !ui.interactive {
        ui.info(&format!(
            "not interactive — using {DEFAULT_PROFILE_COUNT} profiles (change 'profiles' in apps.conf)"
        ));
        return DEFAULT_PROFILE_COUNT;
    }
    loop {
        let answer = ui.ask(&format!(
            "How many profiles are set up on the mouse (\"Levels\" in the launcher, 1–{MAX_PROFILE_COUNT})? [{DEFAULT_PROFILE_COUNT}]"
        ));
        if answer.is_empty() {
            return DEFAULT_PROFILE_COUNT;
        }
        match answer.parse::<u8>() {
            Ok(count) if (1..=MAX_PROFILE_COUNT).contains(&count) => return count,
            _ => ui.warn(&format!("enter a whole number from 1 to {MAX_PROFILE_COUNT}")),
        }
    }
}

/// Shows the checkbox picker when any device is known (connected or saved) and returns the updated config text, or
/// `None` if the user kept the current choice (or wasn't asked). The ticked devices are always
/// saved as `device` lines, even when all are ticked, so they stay listed while unplugged.
fn pick_devices(ui: &Ui, text: &str) -> Option<String> {
    let connected: Vec<(UsbId, String)> =
        mouse_interfaces().into_iter().map(|mouse| (mouse.id, mouse.name)).collect();
    let choices = device_choices(&saved_devices(text), &connected);
    if choices.is_empty() || !ui.interactive || !io::stdout().is_terminal() {
        return None;
    }
    let items: Vec<Item> = choices
        .iter()
        .map(|choice| Item {
            label: choice.name.clone(),
            detail: format!("{}{}", choice.id, if choice.connected { "" } else { "  (not connected)" }),
            checked: choice.selected,
        })
        .collect();
    match checklist::run("Which devices should switch profiles?", &items, ui.color) {
        Ok(Outcome::Kept) => None,
        Ok(Outcome::Confirmed(checked)) => {
            let lines: Vec<String> = choices
                .iter()
                .zip(checked)
                .filter(|(_, checked)| *checked)
                .map(|(choice, _)| format!("device = {}  # {}", choice.id, choice.name))
                .collect();
            Some(with_device_lines(text, &lines))
        }
        Err(_) => {
            ui.warn("can't read keys in this terminal — edit the device lines in apps.conf instead");
            None
        }
    }
}

// --- config text helpers (pure, unit-tested) -----------------------------------------------

#[derive(Debug, PartialEq)]
struct Choice {
    id: UsbId,
    name: String,
    connected: bool,
    selected: bool,
}

/// Connected devices first (deduplicated), then saved ones that aren't plugged in. Saved
/// devices are selected; with nothing saved, everything is selected ("all").
fn device_choices(saved: &[(UsbId, String)], connected: &[(UsbId, String)]) -> Vec<Choice> {
    let mut choices: Vec<Choice> = Vec::new();
    for (id, name) in connected {
        if choices.iter().any(|choice| choice.id == *id) {
            continue;
        }
        let selected = saved.is_empty() || saved.iter().any(|(saved_id, _)| saved_id == id);
        choices.push(Choice { id: *id, name: name.clone(), connected: true, selected });
    }
    for (id, name) in saved {
        if !choices.iter().any(|choice| choice.id == *id) {
            choices.push(Choice { id: *id, name: name.clone(), connected: false, selected: true });
        }
    }
    choices
}

/// `device = …` line (not commented out)?
fn is_device_line(line: &str) -> bool {
    line.trim_start().strip_prefix("device").is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// The commented example `# device = …` line in apps.conf.example?
fn is_example_device_line(line: &str) -> bool {
    line.trim_start().strip_prefix('#').is_some_and(is_device_line)
}

/// Saved `device = vendor:product  # name` lines, in order, without duplicates.
fn saved_devices(text: &str) -> Vec<(UsbId, String)> {
    let mut devices: Vec<(UsbId, String)> = Vec::new();
    for line in text.lines().filter(|line| is_device_line(line)) {
        let value = line.split_once('=').map_or("", |(_, value)| value);
        let (id, name) = value.split_once('#').unwrap_or((value, ""));
        let Some(id) = UsbId::parse(id) else { continue };
        if devices.iter().all(|(saved, _)| *saved != id) {
            let name = name.trim();
            devices.push((id, if name.is_empty() { "unknown device".into() } else { name.into() }));
        }
    }
    devices
}

/// Replaces all `device` lines with `new_lines` (empty removes them). New lines go where the
/// first old one was, else below the commented example line, else at the end.
fn with_device_lines(text: &str, new_lines: &[String]) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.iter().position(|line| is_device_line(line));
    let anchor = first.is_none().then(|| lines.iter().position(|line| is_example_device_line(line))).flatten();
    let mut out: Vec<&str> = Vec::new();
    for (i, line) in lines.iter().copied().enumerate() {
        if is_device_line(line) {
            if Some(i) == first {
                out.extend(new_lines.iter().map(String::as_str));
            }
            continue;
        }
        out.push(line);
        if Some(i) == anchor {
            out.extend(new_lines.iter().map(String::as_str));
        }
    }
    if first.is_none() && anchor.is_none() && !new_lines.is_empty() {
        if out.last().is_some_and(|line| !line.trim().is_empty()) {
            out.push("");
        }
        out.extend(new_lines.iter().map(String::as_str));
    }
    let mut result = out.join("\n");
    result.push('\n');
    result
}

/// Turns the example's commented `# profiles = 2` into an active `profiles = N` line.
fn with_profile_count(text: &str, count: u8) -> String {
    let mut found = false;
    let mut result: String = text
        .lines()
        .map(|line| {
            if !found && line.trim() == format!("# profiles = {DEFAULT_PROFILE_COUNT}") {
                found = true;
                format!("profiles = {count}\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect();
    if !found {
        result.push_str(&format!("\nprofiles = {count}\n"));
    }
    result
}

// --- output --------------------------------------------------------------------------------

/// Same look as install.sh: two-space indent, colored ✓ · ! ? markers on a terminal.
struct Ui {
    color: bool,
    interactive: bool,
}

impl Ui {
    fn new() -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        Self { color: io::stdout().is_terminal() && !no_color, interactive: io::stdin().is_terminal() }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn ok(&self, message: &str) {
        println!("  {} {message}", self.paint("32", "✓"));
    }

    fn info(&self, message: &str) {
        println!("  {} {message}", self.paint("2", "·"));
    }

    fn warn(&self, message: &str) {
        eprintln!("  {} {message}", self.paint("33", "!"));
    }

    /// `? question [default]` prompt on one line; returns the trimmed answer.
    fn ask(&self, question: &str) -> String {
        self.prompt(&format!("  {} {question}", self.paint("34", "?")))
    }

    /// Prints `text ` and reads one line (empty on EOF).
    fn prompt(&self, text: &str) -> String {
        print!("{text} ");
        io::stdout().flush().ok();
        let mut answer = String::new();
        io::stdin().lock().read_line(&mut answer).ok();
        answer.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECEIVER: UsbId = UsbId { vendor: 0x3434, product: 0xd05a };
    const WIRED: UsbId = UsbId { vendor: 0x3434, product: 0xd0ab };

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn reads_saved_devices() {
        let text = "default = 0\ndevice = 3434:D05A  # Keychron TurboLink 8K\n# device = 3434:0001\n\
                    device = 3434:d0ab\ndevice = 3434:d05a  # duplicate\ndevice = junk\n";
        assert_eq!(
            saved_devices(text),
            [(RECEIVER, "Keychron TurboLink 8K".to_string()), (WIRED, "unknown device".to_string())]
        );
    }

    #[test]
    fn nothing_saved_selects_all_connected() {
        let connected = [(RECEIVER, "R".to_string()), (WIRED, "W".to_string()), (WIRED, "W".to_string())];
        let choices = device_choices(&[], &connected);
        assert_eq!(choices.len(), 2);
        assert!(choices.iter().all(|choice| choice.selected && choice.connected));
    }

    #[test]
    fn saved_devices_stay_selected_even_when_unplugged() {
        let choices = device_choices(&[(WIRED, "W".to_string())], &[(RECEIVER, "R".to_string())]);
        assert_eq!(
            choices,
            [
                Choice { id: RECEIVER, name: "R".into(), connected: true, selected: false },
                Choice { id: WIRED, name: "W".into(), connected: false, selected: true },
            ]
        );
    }

    #[test]
    fn replaces_existing_device_lines_in_place() {
        let text = "a = 1\ndevice = 3434:d05a  # R\nb = 1\ndevice = 3434:d0ab  # W\nc = 1\n";
        assert_eq!(
            with_device_lines(text, &lines(&["device = 3434:d0ab  # W"])),
            "a = 1\ndevice = 3434:d0ab  # W\nb = 1\nc = 1\n"
        );
    }

    #[test]
    fn inserts_below_example_line() {
        let text = "# device = 3434:d05a  # example\n\ndefault = 0\n";
        assert_eq!(
            with_device_lines(text, &lines(&["device = 3434:d05a  # R"])),
            "# device = 3434:d05a  # example\ndevice = 3434:d05a  # R\n\ndefault = 0\n"
        );
    }

    #[test]
    fn appends_when_there_is_no_place_for_device_lines() {
        assert_eq!(
            with_device_lines("default = 0\ngamescope = 1\n", &lines(&["device = 3434:d05a  # R"])),
            "default = 0\ngamescope = 1\n\ndevice = 3434:d05a  # R\n"
        );
    }

    #[test]
    fn empty_selection_removes_device_lines() {
        assert_eq!(with_device_lines("default = 0\ndevice = 3434:d05a  # R\n", &[]), "default = 0\n");
    }

    #[test]
    fn sets_profile_count_in_example() {
        let text = with_profile_count(EXAMPLE_CONFIG, 4);
        assert!(text.lines().any(|line| line == "profiles = 4"));
        assert!(!text.contains("# profiles = 2"));
        assert_eq!(crate::parse_config(&text).profile_count, 4);
    }
}
