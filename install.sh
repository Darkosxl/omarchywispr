#!/bin/bash
# Build omarchywispr, fetch the model, wire the Omarchy shell pill, Hyprland
# binding and autostart. Safe to re-run.
set -euo pipefail

repo=$(cd "$(dirname "$0")" && pwd)
model_dir=${XDG_DATA_HOME:-$HOME/.local/share}/omarchywispr
model=ggml-small-q8_0.bin
plugin_id=darkwarro.wispr
plugin_dir=$HOME/.config/omarchy/plugins/$plugin_id
hypr=$HOME/.config/hypr

echo "==> building (first CUDA build takes a few minutes)"
cargo install --path "$repo" --target-dir "$repo/target" --locked --quiet

echo "==> model"
mkdir -p "$model_dir"
if [[ ! -f $model_dir/$model ]]; then
  curl -L --fail --progress-bar -o "$model_dir/$model.part" \
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$model"
  mv "$model_dir/$model.part" "$model_dir/$model"
fi

echo "==> shell plugin"
mkdir -p "$(dirname "$plugin_dir")"
[[ -L $plugin_dir || -e $plugin_dir ]] || ln -s "$repo/plugin" "$plugin_dir"
# The shell's file watcher does not follow the symlink; a restart picks up edits.
omarchy restart shell >/dev/null 2>&1 || true
if ! grep -q "\"$plugin_id\"" "$HOME/.config/omarchy/shell.json" 2>/dev/null; then
  omarchy plugin enable "$plugin_id" --section center --after omarchy.weather
fi

echo "==> hyprland"
add_line() { # file line
  grep -qF "$2" "$1" 2>/dev/null && return
  cp "$1" "$1.bak.$(date +%s)" 2>/dev/null || true
  printf '\n%s\n' "$2" >> "$1"
}
add_line "$hypr/bindings.lua" 'o.bind("SUPER + ALT + V", "Dictate (toggle)", "omarchywispr toggle")'
add_line "$hypr/bindings.lua" 'o.bind("SUPER + less", "Dictate (hold)", "omarchywispr start")'
add_line "$hypr/bindings.lua" 'o.bind("SUPER + less", "Dictate (release)", "omarchywispr stop", { release = true })'
add_line "$hypr/autostart.lua" 'o.launch_on_start("omarchywispr daemon")'
hyprctl reload >/dev/null
hyprctl configerrors

echo "==> daemon"
if ! omarchywispr status >/dev/null 2>&1; then
  setsid uwsm-app -- omarchywispr daemon >/dev/null 2>&1 < /dev/null &
  sleep 3
fi
omarchywispr status
echo "done — hold SUPER+< and speak, or toggle with SUPER+ALT+V."
