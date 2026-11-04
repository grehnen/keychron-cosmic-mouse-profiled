//! Watches the focused window on COSMIC and switches the mouse profile
//! according to ~/.config/mouse-profile/apps.conf.

use std::{collections::HashMap, fs, path::PathBuf};

use cosmic_client_toolkit::{
    cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::State,
    sctk::{
        self,
        output::{OutputHandler, OutputState},
        registry::{ProvidesRegistryState, RegistryState},
    },
    toplevel_info::{ToplevelInfoHandler, ToplevelInfoState},
    wayland_client::{globals::registry_queue_init, protocol::wl_output, Connection, QueueHandle},
    wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
};

struct ProfileRules {
    by_app_id: HashMap<String, u8>,
    default_profile: u8,
}

fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap()).join(".config"));
    base.join("mouse-profile/apps.conf")
}

/// Format: one `app_id = profile_index` per line, `default = N` for everything else, `#` comments.
fn load_rules() -> ProfileRules {
    let mut rules = ProfileRules { by_app_id: HashMap::new(), default_profile: 0 };
    let Ok(text) = fs::read_to_string(config_path()) else { return rules };
    for line in text.lines().map(|l| l.split('#').next().unwrap().trim()) {
        let Some((key, value)) = line.split_once('=') else { continue };
        let Ok(profile) = value.trim().parse::<u8>() else { continue };
        match key.trim() {
            "default" => rules.default_profile = profile,
            app_id => {
                rules.by_app_id.insert(app_id.to_lowercase(), profile);
            }
        }
    }
    rules
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    toplevel_info_state: ToplevelInfoState,
    focused_app_id: Option<String>,
    applied_profile: Option<u8>,
}

impl App {
    fn on_toplevels_changed(&mut self) {
        let focused = self
            .toplevel_info_state
            .toplevels()
            .find(|info| info.state.contains(&State::Activated))
            .map(|info| info.app_id.clone());
        // Nothing focused (e.g. desktop or launcher overlay): keep the current profile.
        let Some(app_id) = focused else { return };
        if self.focused_app_id.as_deref() == Some(&app_id) {
            return;
        }
        let rules = load_rules();
        let profile = *rules.by_app_id.get(&app_id.to_lowercase()).unwrap_or(&rules.default_profile);
        println!("focus: {app_id} -> profile {profile}");
        self.focused_app_id = Some(app_id);
        if self.applied_profile != Some(profile) {
            self.applied_profile = apply_profile(profile).then_some(profile);
        }
    }
}

fn apply_profile(profile: u8) -> bool {
    mouse_profiled::set_profile(profile)
        .map_err(|err| eprintln!("switching to profile {profile} failed: {err}"))
        .is_ok()
}

impl ToplevelInfoHandler for App {
    fn toplevel_info_state(&mut self) -> &mut ToplevelInfoState {
        &mut self.toplevel_info_state
    }
    fn new_toplevel(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &ExtForeignToplevelHandleV1) {}
    fn update_toplevel(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &ExtForeignToplevelHandleV1) {}
    fn toplevel_closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &ExtForeignToplevelHandleV1) {}
    fn info_done(&mut self, _: &Connection, _: &QueueHandle<Self>) {
        self.on_toplevels_changed();
    }
}

// Outputs must be bound so the compositor sends complete toplevel info.
impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    sctk::registry_handlers!(OutputState);
}

sctk::delegate_output!(App);
sctk::delegate_registry!(App);
cosmic_client_toolkit::delegate_toplevel_info!(App);

fn main() {
    let conn = Connection::connect_to_env().expect("not running under a Wayland session");
    let (globals, mut event_queue) = registry_queue_init(&conn).expect("registry init failed");
    let qh = event_queue.handle();
    let registry_state = RegistryState::new(&globals);
    let mut app = App {
        output_state: OutputState::new(&globals, &qh),
        toplevel_info_state: ToplevelInfoState::try_new(&registry_state, &qh)
            .expect("compositor does not support ext-foreign-toplevel-list"),
        registry_state,
        focused_app_id: None,
        applied_profile: None,
    };
    println!("watching focus; rules from {}", config_path().display());
    loop {
        event_queue.blocking_dispatch(&mut app).expect("wayland connection lost");
    }
}
