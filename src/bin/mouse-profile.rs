//! `mouse-profile <index>` — switch the Keychron mouse to a profile (0 = first).
//! `mouse-profile configure` — create or update apps.conf interactively.
//! `mouse-profile devices` — list the connected devices the profile command can go to.

use std::process::ExitCode;

const USAGE: &str = "\
usage: mouse-profile <index>     switch to a profile (0 = first, 1 = second, ...)
       mouse-profile configure   create or update ~/.config/mouse-profile/apps.conf
       mouse-profile devices     list connected Keychron mice (node, USB ID, name)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["configure"] => mouse_profiled::setup::run(),
        ["devices"] => {
            for mouse in mouse_profiled::mouse_interfaces() {
                println!("{}\t{}\t{}", mouse.node.display(), mouse.id, mouse.name);
            }
            Ok(())
        }
        [index] => match index.parse::<u8>() {
            Ok(index) => mouse_profiled::set_profile(index, &mouse_profiled::load_config()),
            Err(_) => return usage(),
        },
        _ => return usage(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mouse-profile: {err}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
