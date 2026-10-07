# omarchywispr

Local push-to-talk dictation for [Omarchy](https://omarchy.org) — a tiny
Wispr Flow. Press a key, speak, press again: the text is typed into whatever
has focus. Runs whisper.cpp on the GPU, nothing leaves the machine.

- `SUPER + ALT + V` toggles recording (edit `~/.config/hypr/bindings.lua`).
- A small level meter appears in the top bar (right of the weather icon)
  while recording and pulses while transcribing (shell plugin `darkwarro.wispr`).
- Default model: OpenAI Whisper `small`, 8-bit (`ggml-small-q8_0.bin`,
  264 MB), language `tr` — Turkish with English mixed in works. Swap models or
  language with flags on the autostart line.

## Install

```bash
./install.sh
```

Needs: Omarchy ≥ 4 (Hyprland, PipeWire, Quickshell shell), Rust toolchain,
CUDA toolkit (`cuda` package) for GPU inference, `wtype`, `pw-record`.

## Usage

```
omarchywispr daemon [--model PATH] [--lang tr|en|auto] [--prompt TEXT] [--device PW_NODE]
omarchywispr toggle | start | stop | status [--follow]
```

`--prompt` seeds the decoder, e.g. `--prompt "Türkçe ve İngilizce karışık teknik konuşma."`
`--device` is a PipeWire node name from `pactl list sources short`.

Models live in `~/.local/share/omarchywispr/`. Other whisper.cpp ggml files
from <https://huggingface.co/ggerganov/whisper.cpp> drop in:
`medium-q5_0` (539 MB, better Turkish) or `large-v3-turbo-q5_0` (574 MB, best).

## How it works

`daemon` loads the model once and listens on `$XDG_RUNTIME_DIR/omarchywispr.sock`.
`toggle` is a one-shot client. Recording spawns `pw-record` (16 kHz mono f32);
stopping runs whisper and pipes the text to `wtype -` (falls back to `wl-copy`).
The bar widget connects to the same socket and sends `follow` to stream
`{"phase":"recording","level":0.42}` lines.
