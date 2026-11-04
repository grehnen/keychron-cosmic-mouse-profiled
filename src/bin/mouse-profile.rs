//! `mouse-profile <index>` — switch the Keychron mouse to a profile (0 = first).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(index) = (args.len() == 1).then(|| args[0].parse::<u8>().ok()).flatten() else {
        eprintln!("usage: mouse-profile <index>   (0 = first profile, 1 = second, ...)");
        return ExitCode::from(2);
    };
    match mouse_profiled::set_profile(index, &mouse_profiled::load_config()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mouse-profile: {err}");
            ExitCode::FAILURE
        }
    }
}
