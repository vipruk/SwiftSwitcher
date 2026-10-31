#!/usr/bin/env bash
# Development setup for SwiftSwitcher on GNOME (Wayland). Idempotent.
# Installs the udev rule (needs sudo) and links + enables the GNOME extension.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
uuid="swiftswitcher@vipruk.github.io"
rule="70-swiftswitcher.rules"

echo "Installing udev rule (sudo)..."
sudo install -m 0644 "$root/packaging/udev/$rule" "/etc/udev/rules.d/$rule"
sudo udevadm control --reload
sudo udevadm trigger --action=change --subsystem-match=input --subsystem-match=misc

echo "Linking GNOME extension..."
ext_dir="$HOME/.local/share/gnome-shell/extensions"
mkdir -p "$ext_dir"
ln -sfn "$root/extension/$uuid" "$ext_dir/$uuid"

if gnome-extensions list --enabled 2>/dev/null | grep -qx "$uuid"; then
    echo "Extension already enabled."
elif gnome-extensions enable "$uuid" 2>/dev/null; then
    echo "Extension enabled."
else
    # GNOME Shell may not know a freshly linked extension until the next login;
    # record it in enabled-extensions so it loads then.
    current="$(gsettings get org.gnome.shell enabled-extensions)"
    if [[ "$current" != *"'$uuid'"* ]]; then
        if [[ "$current" == "@as []" || "$current" == "[]" ]]; then
            new="['$uuid']"
        else
            new="${current%]}, '$uuid']"
        fi
        gsettings set org.gnome.shell enabled-extensions "$new"
    fi
    echo "Extension will be enabled at next login."
fi

cat <<MSG

Done. Device access (keyboards, pointers, /dev/uinput) applies immediately.
Only the GNOME extension needs a log out and back in to be loaded.
Then run:  cargo run -p swiftswitcher -- check
MSG
