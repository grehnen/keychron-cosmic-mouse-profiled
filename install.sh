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

# Prints "<hidraw node> <vendor:product> <name>" for every hidraw node the profile command can
# go to: Keychron devices (vendor 0x3434) whose report descriptor has the vendor usage page
# 0xFFC1 (bytes 06 C1 FF). Mirrors src/lib.rs.
mouse_interfaces() {
    local dir uevent id name
    for dir in /sys/class/hidraw/hidraw*/device; do
        uevent=$(cat "$dir/uevent" 2>/dev/null) || continue
        [[ $uevent =~ HID_ID=[0-9A-Fa-f]+:0000([0-9A-Fa-f]{4}):0000([0-9A-Fa-f]{4}) ]] || continue
        id="${BASH_REMATCH[1],,}:${BASH_REMATCH[2],,}"
        [[ $id == 3434:* ]] || continue
        od -An -tx1 -v "$dir/report_descriptor" 2>/dev/null | tr -d '\n' | grep -q ' 06 c1 ff' || continue
        # Names repeat the vendor ("Keychron Keychron TurboLink 8K"); drop the duplicate.
        name=$(sed -nE 's/^HID_NAME=//p' <<<"$uevent" | sed -E 's/^([^ ]+) \1 /\1 /')
        printf '%s %s %s\n' "/dev/$(basename "$(dirname "$dir")")" "$id" "${name:-unknown device}"
    done
}

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
    nodes=$(mouse_interfaces | cut -d" " -f1)
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
    ok "installed mouse-profile and mouse-profiled to $(pretty "${CARGO_HOME:-$HOME/.cargo}/bin")"
    remove_legacy_script_link
}

install_config() {
    step "Config"
    mkdir -p "$CONFIG_DIR"
    if [[ -e $CONFIG_DIR/apps.conf ]]; then
        ok "using existing $(pretty "$CONFIG_DIR/apps.conf")"
    else
        local count
        count=$(ask_profile_count)
        sed "s/^# profiles = 2\$/profiles = $count/" "$REPO_DIR/apps.conf.example" >"$CONFIG_DIR/apps.conf"
        grep -q "^profiles = $count\$" "$CONFIG_DIR/apps.conf" ||
            printf '\nprofiles = %s\n' "$count" >>"$CONFIG_DIR/apps.conf"
        ok "created $(pretty "$CONFIG_DIR/apps.conf") with $count profiles"
    fi
    configure_devices "$CONFIG_DIR/apps.conf"
}

# Lets the user pick which devices get the profile command and saves the choice as
# `device = vendor:product  # name` lines. Only asks when 2+ devices are known (connected
# or already saved); saved devices start out selected. Only `device` lines are changed.
configure_devices() {
    local conf=$1 node id name i answer
    local -a ids=() names=() connected=() selected=()
    local saved_count=0

    # Devices saved in the config, connected or not.
    local -A saved_names=()
    local -a saved_order=()
    while read -r id name; do
        id=${id,,}
        [[ -v saved_names[$id] ]] && continue
        saved_names[$id]=${name:-unknown device}
        saved_order+=("$id")
    done < <(sed -nE 's/^[[:space:]]*device[[:space:]]*=[[:space:]]*([0-9A-Fa-f]{4}:[0-9A-Fa-f]{4})[[:space:]]*(#[[:space:]]*)?(.*)$/\1 \3/p' "$conf")
    saved_count=${#saved_order[@]}

    # Connected devices first (one device can expose several matching interfaces), then saved
    # ones that aren't plugged in. With nothing saved, everything starts out selected ("all").
    local -A listed=()
    while read -r node id name; do
        [[ -v listed[$id] ]] && continue
        listed[$id]=1
        ids+=("$id") names+=("$name") connected+=(1)
        if ((saved_count == 0)) || [[ -v saved_names[$id] ]]; then selected+=(1); else selected+=(0); fi
    done < <(mouse_interfaces)
    for id in "${saved_order[@]}"; do
        [[ -v listed[$id] ]] && continue
        ids+=("$id") names+=("${saved_names[$id]}") connected+=(0) selected+=(1)
    done

    if ((${#ids[@]} >= 2)) && [[ -t 0 ]]; then
        local width=0
        for name in "${names[@]}"; do ((${#name} > width)) && width=${#name}; done
        printf '  %s?%s Which devices should switch profiles?\n' "$BLUE" "$RESET"
        for i in "${!ids[@]}"; do
            printf '      %d) [%s] %-*s  %s%s%s\n' $((i + 1)) "$( ((selected[i])) && echo x || echo ' ')" \
                "$width" "${names[i]}" "$DIM" "${ids[i]}$( ((connected[i])) || echo '  (not connected)')" "$RESET"
        done
        while true; do
            printf '    Numbers separated by spaces, a = all, Enter = keep: '
            read -r answer || answer=''
            [[ -z $answer ]] && break
            if [[ $answer == a ]]; then
                saved_count=0
                write_device_lines "$conf" ""
                break
            fi
            local -a picked=()
            local valid=1 n
            for n in $answer; do
                if [[ $n =~ ^[0-9]+$ ]] && ((10#$n >= 1 && 10#$n <= ${#ids[@]})); then
                    picked[10#$n - 1]=1
                else
                    valid=0
                fi
            done
            if ((valid)); then
                local lines=''
                saved_count=0
                for i in "${!ids[@]}"; do
                    selected[i]=${picked[i]:-0}
                    ((selected[i])) || continue
                    lines+="device = ${ids[i]}  # ${names[i]}"$'\n'
                    saved_count=$((saved_count + 1))
                done
                write_device_lines "$conf" "${lines%$'\n'}"
                break
            fi
            warn "enter numbers from 1 to ${#ids[@]}, a, or nothing"
        done
    fi

    if ((saved_count == 0)); then
        ok "profile command goes to every connected Keychron mouse"
    else
        local list=()
        for i in "${!ids[@]}"; do ((selected[i])) && list+=("${names[i]}"); done
        ok "profile command goes to: $(printf '%s, ' "${list[@]}" | sed 's/, $//')"
    fi
}

# Replaces all active `device =` lines in the config with $2 (empty removes them). New lines
# go where the first old one was, else below the commented example line, else at the end.
write_device_lines() {
    local conf=$1 block=$2 tmp
    tmp=$(mktemp "$conf.XXXXXX")
    BLOCK=$block awk '
        { line[NR] = $0 }
        END {
            block = ENVIRON["BLOCK"]
            active = "^[[:space:]]*device[[:space:]]*="
            for (i = 1; i <= NR && !first; i++) if (line[i] ~ active) first = i
            for (i = 1; i <= NR && !first && !anchor; i++) if (line[i] ~ /^#[[:space:]]*device[[:space:]]*=/) anchor = i
            for (i = 1; i <= NR; i++) {
                if (line[i] ~ active) {
                    if (i == first && block != "") print block
                    continue
                }
                print line[i]
                if (i == anchor && block != "") print block
            }
            if (!first && !anchor && block != "") print "\n" block
        }
    ' "$conf" >"$tmp"
    # Copy back instead of mv so the file keeps its permissions.
    cat "$tmp" >"$conf"
    rm "$tmp"
}

# Prints the number of profiles to stdout; prompts on stderr when run interactively.
ask_profile_count() {
    local answer
    if [[ ! -t 0 ]]; then
        info "not interactive — using 2 profiles (change 'profiles' in apps.conf)" >&2
        echo 2
        return
    fi
    while true; do
        printf '  %s?%s How many profiles are set up on the mouse ("Levels" in the launcher, 1–5)? [2] ' \
            "$BLUE" "$RESET" >&2
        read -r answer || answer=''
        answer=${answer:-2}
        if [[ $answer =~ ^[0-9]+$ ]] && ((10#$answer >= 1 && 10#$answer <= 5)); then
            echo $((10#$answer))
            return
        fi
        warn "enter a whole number from 1 to 5"
    done
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
            install_udev_rule
            install_binaries
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
