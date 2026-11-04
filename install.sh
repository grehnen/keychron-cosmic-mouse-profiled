#!/usr/bin/env bash
# Install (or with --uninstall, remove) mouse-profile + mouse-profiled. Safe to re-run.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UDEV_RULE_PATH=/etc/udev/rules.d/70-keychron.rules
UDEV_RULE='KERNEL=="hidraw*", SUBSYSTEM=="hidraw", ATTRS{idVendor}=="3434", MODE="0660", TAG+="uaccess"'
SERVICE_NAME=mouse-profiled.service
SERVICE_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/mouse-profile"
LEGACY_SCRIPT_LINK="$HOME/.local/bin/mouse-profile"

step() { printf '\n==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

keychron_hidraw_nodes() {
    local uevent
    for uevent in /sys/class/hidraw/hidraw*/device/uevent; do
        if grep -qi '^HID_ID=.*:00003434:' "$uevent" 2>/dev/null; then
            echo "/dev/$(basename "$(dirname "$(dirname "$uevent")")")"
        fi
    done
}

remove_legacy_script_link() {
    # The Python script used to be symlinked here; it is now a cargo-installed binary.
    if [[ -L $LEGACY_SCRIPT_LINK ]]; then
        rm "$LEGACY_SCRIPT_LINK"
        echo "removed old symlink $LEGACY_SCRIPT_LINK"
    fi
}

check_prerequisites() {
    step "Checking prerequisites"
    command -v cargo >/dev/null || die "cargo not found — install Rust from https://rustup.rs"
    [[ ${XDG_SESSION_TYPE:-} == wayland ]] || warn "not a Wayland session; mouse-profiled won't detect focus (mouse-profile still works)"
    [[ ${XDG_CURRENT_DESKTOP:-} == *COSMIC* ]] || warn "desktop isn't COSMIC; mouse-profiled needs COSMIC's toplevel-info protocol"
}

install_udev_rule() {
    step "udev rule ($UDEV_RULE_PATH)"
    if [[ $(cat "$UDEV_RULE_PATH" 2>/dev/null) == "$UDEV_RULE" ]]; then
        echo "already up to date"
    else
        echo "writing rule (needs sudo)"
        echo "$UDEV_RULE" | sudo tee "$UDEV_RULE_PATH" >/dev/null
        sudo udevadm control --reload-rules
        sudo udevadm trigger --subsystem-match=hidraw
        sudo udevadm settle
    fi

    local nodes node missing_access=0
    nodes=$(keychron_hidraw_nodes)
    if [[ -z $nodes ]]; then
        warn "no Keychron device plugged in; can't verify access yet"
        return
    fi
    for node in $nodes; do
        [[ -w $node ]] || missing_access=1
    done
    if ((missing_access)); then
        warn "no write access to the Keychron hidraw devices yet — unplug and replug the receiver/cable"
    else
        echo "access to Keychron hidraw devices: ok"
    fi
}

install_binaries() {
    step "Building and installing binaries (cargo install)"
    cargo install --path "$REPO_DIR" --locked --force
    remove_legacy_script_link
}

install_config() {
    step "Config ($CONFIG_DIR/apps.conf)"
    mkdir -p "$CONFIG_DIR"
    if [[ -e $CONFIG_DIR/apps.conf ]]; then
        echo "exists, leaving it alone"
    else
        cp "$REPO_DIR/apps.conf.example" "$CONFIG_DIR/apps.conf"
        echo "created from apps.conf.example"
    fi
}

install_service() {
    step "systemd user service"
    mkdir -p "$SERVICE_DIR"
    cp "$REPO_DIR/systemd/$SERVICE_NAME" "$SERVICE_DIR/$SERVICE_NAME"
    systemctl --user daemon-reload
    systemctl --user enable "$SERVICE_NAME"
    systemctl --user restart "$SERVICE_NAME"
    systemctl --user --no-pager status "$SERVICE_NAME" | head -3 || true
}

uninstall() {
    step "Stopping and removing the service"
    systemctl --user disable --now "$SERVICE_NAME" 2>/dev/null || true
    rm -f "$SERVICE_DIR/$SERVICE_NAME"
    systemctl --user daemon-reload

    step "Removing binaries"
    cargo uninstall mouse-profiled 2>/dev/null || echo "not installed"
    remove_legacy_script_link

    step "Removing udev rule"
    if [[ -e $UDEV_RULE_PATH ]]; then
        sudo rm "$UDEV_RULE_PATH"
        sudo udevadm control --reload-rules
        sudo udevadm trigger --subsystem-match=hidraw
    else
        echo "not present"
    fi

    echo
    echo "Uninstalled. Your config was kept: $CONFIG_DIR/apps.conf"
}

main() {
    case "${1:-}" in
        --uninstall) uninstall ;;
        "")
            check_prerequisites
            install_udev_rule
            install_binaries
            install_config
            install_service
            echo
            echo "Done. Map apps in $CONFIG_DIR/apps.conf; find app IDs with:"
            echo "  journalctl --user -u mouse-profiled -f"
            ;;
        *) die "usage: $0 [--uninstall]" ;;
    esac
}

main "$@"
