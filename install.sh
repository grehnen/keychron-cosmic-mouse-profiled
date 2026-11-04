#!/usr/bin/env bash
# Install (or with --uninstall, remove) mouse-profile + mouse-profiled. Safe to re-run.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UDEV_RULE_PATH=/etc/udev/rules.d/70-keychron.rules
UDEV_RULE='KERNEL=="hidraw*", SUBSYSTEM=="hidraw", ATTRS{idVendor}=="3434", MODE="0660", TAG+="uaccess"'
SERVICE_NAME=mouse-profiled.service
SERVICE_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/mouse-profile"
BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
LEGACY_SCRIPT_LINK="$HOME/.local/bin/mouse-profile"

# --- output ------------------------------------------------------------------

if [[ -t 1 && -z ${NO_COLOR:-} ]]; then
    BOLD=$'\e[1m' DIM=$'\e[2m' RED=$'\e[31m' GREEN=$'\e[32m' YELLOW=$'\e[33m' BLUE=$'\e[34m' RESET=$'\e[0m'
else
    BOLD='' DIM='' RED='' GREEN='' YELLOW='' BLUE='' RESET=''
fi

step() { printf '\n%s▸ %s%s\n' "$BOLD$BLUE" "$*" "$RESET"; }
ok() { printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$*"; }
info() { printf '  %s·%s %s\n' "$DIM" "$RESET" "$*"; }
warn() { printf '  %s!%s %s\n' "$YELLOW" "$RESET" "$*" >&2; }
die() { printf '\n%s✗ %s%s\n' "$RED" "$*" "$RESET" >&2; exit 1; }
# Shorten paths under $HOME to ~ for display.
pretty() { printf '%s' "${1/#$HOME/\~}"; }

# --- helpers -----------------------------------------------------------------

reload_udev() {
    sudo udevadm control --reload-rules
    sudo udevadm trigger --subsystem-match=hidraw
    sudo udevadm settle
}

remove_legacy_script_link() {
    # The Python script used to be symlinked here; it is now a cargo-installed binary.
    if [[ -L $LEGACY_SCRIPT_LINK ]]; then
        rm "$LEGACY_SCRIPT_LINK"
        ok "removed old symlink $(pretty "$LEGACY_SCRIPT_LINK")"
    fi
}

# --- install steps -----------------------------------------------------------

check_prerequisites() {
    step "Prerequisites"
    command -v cargo >/dev/null || die "cargo not found — install Rust from https://rustup.rs"
    ok "$(cargo --version | cut -d' ' -f1-2)"
    if [[ ${XDG_SESSION_TYPE:-} == wayland ]]; then
        ok "Wayland session"
    else
        warn "not a Wayland session — mouse-profiled can't detect focus (mouse-profile still works)"
    fi
    if [[ ${XDG_CURRENT_DESKTOP:-} == *COSMIC* ]]; then
        ok "COSMIC desktop"
    else
        warn "desktop isn't COSMIC — mouse-profiled needs COSMIC's toplevel-info protocol"
    fi
}

install_udev_rule() {
    step "udev rule"
    if [[ $(cat "$UDEV_RULE_PATH" 2>/dev/null) == "$UDEV_RULE" ]]; then
        ok "$UDEV_RULE_PATH is up to date"
    else
        info "writing $UDEV_RULE_PATH (needs sudo)"
        echo "$UDEV_RULE" | sudo tee "$UDEV_RULE_PATH" >/dev/null
        reload_udev
        ok "rule installed and udev reloaded"
    fi

    local nodes node count=0 missing_access=0
    nodes=$("$BIN_DIR/mouse-profile" devices | cut -f1)
    if [[ -z $nodes ]]; then
        warn "no Keychron mouse connected — can't verify access yet"
        return
    fi
    for node in $nodes; do
        count=$((count + 1))
        [[ -w $node ]] || missing_access=1
    done
    if ((missing_access)); then
        warn "no write access to the Keychron mouse yet — unplug and replug the receiver/cable"
    else
        local noun="Keychron mouse interface"
        ((count == 1)) || noun+="s"
        ok "write access to $count $noun"
    fi
}

install_binaries() {
    step "Binaries"
    info "building with cargo (this can take a minute)…"
    cargo install --quiet --path "$REPO_DIR" --locked --force ||
        die "cargo install failed (see the errors above)"
    ok "installed mouse-profile and mouse-profiled to $(pretty "$BIN_DIR")"
    remove_legacy_script_link
}

install_config() {
    step "Config"
    "$BIN_DIR/mouse-profile" configure || die "mouse-profile configure failed"
}

install_service() {
    step "Service"
    mkdir -p "$SERVICE_DIR"
    cp "$REPO_DIR/systemd/$SERVICE_NAME" "$SERVICE_DIR/$SERVICE_NAME"
    systemctl --user daemon-reload
    systemctl --user enable --quiet "$SERVICE_NAME"
    systemctl --user restart "$SERVICE_NAME"
    if systemctl --user is-active --quiet "$SERVICE_NAME"; then
        ok "$SERVICE_NAME enabled and running"
    else
        warn "$SERVICE_NAME is not running — check: journalctl --user -u ${SERVICE_NAME%.service}"
    fi
}

# --- uninstall ---------------------------------------------------------------

uninstall() {
    step "Service"
    systemctl --user disable --now --quiet "$SERVICE_NAME" 2>/dev/null || true
    rm -f "$SERVICE_DIR/$SERVICE_NAME"
    systemctl --user daemon-reload
    ok "stopped and removed $SERVICE_NAME"

    step "Binaries"
    if cargo uninstall --quiet mouse-profiled 2>/dev/null; then
        ok "removed mouse-profile and mouse-profiled"
    else
        info "not installed"
    fi
    remove_legacy_script_link

    step "udev rule"
    if [[ -e $UDEV_RULE_PATH ]]; then
        info "removing $UDEV_RULE_PATH (needs sudo)"
        sudo rm "$UDEV_RULE_PATH"
        reload_udev
        ok "rule removed and udev reloaded"
    else
        info "not present"
    fi

    printf '\n%sUninstalled.%s Your config was kept: %s\n' "$BOLD$GREEN" "$RESET" "$(pretty "$CONFIG_DIR/apps.conf")"
}

main() {
    case "${1:-}" in
        --uninstall) uninstall ;;
        "")
            check_prerequisites
            install_binaries # first: the later steps run the installed mouse-profile
            install_udev_rule
            install_config
            install_service
            printf '\n%sDone.%s\n' "$BOLD$GREEN" "$RESET"
            printf '  Map apps to profiles in  %s\n' "$(pretty "$CONFIG_DIR/apps.conf")"
            printf '  Find app IDs with        %sjournalctl --user -u mouse-profiled -f%s\n' "$BOLD" "$RESET"
            ;;
        *) die "usage: $0 [--uninstall]" ;;
    esac
}

main "$@"
