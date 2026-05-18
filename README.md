# esp-gui-rs

**A high-resolution desktop GUI for live ESP32 Wi-Fi Channel-State-Information (CSI) capture and analysis.**

Plug an ESP32 running [`esp-csi-cli-rs`](https://github.com/csi-rs/esp-csi-cli-rs) into your laptop, run `esp-gui-rs`, and watch the radio channel in real time — magnitude, phase, I/Q scatter, channel impulse response, a phase-Δ spectrogram, and a long-window CSI evolution curve — all updating live, all scrubbable, and exportable to CSV or [Rerun](https://rerun.io) RRD for offline work.

---

## What problem does it solve?

CSI is a goldmine for Wi-Fi sensing — gesture detection, presence sensing, channel diagnostics, indoor localization — but the tooling is painful. You either get raw byte dumps over UART, terminal-only TUIs that flicker at 5 fps, or one-off Python notebooks that need ten lines of glue before you can see a packet.

`esp-gui-rs` is the *immediate-feedback* layer that's been missing:

- **One command** to identify the firmware, reset the chip, and start streaming.
- **High-resolution 2D plots** with axes, zoom, pan, and tooltips — not terminal art.
- **All four firmware log modes** (`text`, `array-list`, `serialized`, `esp-csi-tool`) decode to a single in-memory schema, so the visualizations look identical no matter which wire format you pick.
- **Ten built-in visualizations** that cover the workflow from "is anything moving?" to "what does subcarrier 24's phase look like across the last 400 packets?"
- **Replay any moment** with a global timeline scrubber — every pane jumps back together.
- **Export to CSV** for pandas / MATLAB, or to **Rerun** for time-anchored, scrubbable offline analysis.
- **A console pane** that shows the raw firmware UART, so you're never wondering why nothing's showing up.

It's the difference between *running an experiment* and *debugging your tooling*.

---

## Features

- **Auto-connect** to the first available serial port, with hardware DTR/RTS reset, banner identification, and graceful handling of hot-unplugs.
- **All firmware log modes decode**: `text`, `array-list`, `serialized` (COBS-framed postcard — both classic-chip and ESP32-C5/C6 schemas), and `esp-csi-tool` (Hernandez 26-column CSV).
- **Live visualizations** (any of which can be split into multiple tiles side-by-side):
  - **Dashboard** — PPS, SNR, RSSI, firmware status at a glance.
  - **Magnitude** — `|H(k)|` vs subcarrier (DC-centered), linear or dB, with three display modes: per-packet traces, mean ± σ envelope, or subcarrier × time **heatmap**.
  - **Phase** — raw / unwrapped / detrended, with the same three display modes (heatmap uses a diverging colormap for signed quantities).
  - **Polar** — current frame as a polar amplitude diagram.
  - **CIR** — channel impulse response (IDFT) over the last *N* packets, faded by age.
  - **Spectrogram** — phase-Δ or amplitude over time, smooth viridis colormap.
  - **Evolution** — long-window time series of a single scalar per packet: mean `|H|`, std-dev of `|H|` across subcarriers, frame-to-frame Δ`|H|` (textbook motion proxy), or RSSI.
  - **I/Q scatter** — every (I, Q) sample for the last *N* packets, density via alpha overlap.
  - **Console** — raw UART text capture with command echo for visibility into the firmware.
  - **Replay** — global timeline scrubber + play/pause + speed.
- **Tiling layout** — split panes horizontally / vertically, fullscreen one, save layouts as templates.
- **Global replay** — scrubber + play / pause + speed; every pane follows the same anchor.
- **CSV export** — one uniform 22-column schema regardless of the firmware's wire format.
- **Rerun integration** *(optional `--features rerun`)* — spawn a viewer, connect to a running one, or save `.rrd` files for offline replay. UI is hidden in the no-feature build, so it never clutters the menu when you don't need it.
- **Themes** — five built-in (Dark, Light, Nordic, Gruvbox, Catppuccin).

---

## Install

Prerequisites:
- Rust ≥ 1.85 (stable), edition 2024.
- An ESP32 (any variant — esp32, c3, c5, c6, s3) flashed with [`esp-csi-cli-rs`](https://github.com/connectedmotion-io/esp-csi-cli-rs).
- A USB-UART connection. Linux users need read/write on `/dev/ttyUSB*` (usually via the `dialout` group).

From source:
```sh
git clone <this repo>
cd csi-gui-rs
cargo run --release
```

With Rerun support:
```sh
cargo run --release --features rerun -- --rerun=spawn
```

---

## Quick start

1. Plug the ESP32 into a USB port.
2. `cargo run --release`
3. The GUI opens, auto-detects the port, resets the device, identifies the firmware, and lands on the Dashboard.
4. Click **ESP → Start Collection** (or pick *Start Collection* from the **M** menu).
5. Hit **Enter** to switch the focused pane to **Magnitude**, **Phase**, **Spectrogram**, **Evolution**, or any other view. Use **Shift+Arrow** to split the pane and show multiple views side-by-side.
6. Press **Enter → Replay** to add a scrubber pane. Drag the slider; all live panes rewind together. Press **R** to snap back to LIVE.
7. **File → Export Data…** writes a CSV with one row per packet to `exports/`.

```sh
# Common flags:
esp-gui-rs --baud=115200                 # default
esp-gui-rs --history-size=50000          # keep more packets in memory
esp-gui-rs --rerun=spawn                 # launch a Rerun viewer
esp-gui-rs --rerun=file:logs/run1.rrd    # record to an RRD file
esp-gui-rs --help
```

---

## Keyboard shortcuts

| Key | Action |
| --- | --- |
| `M` | Main menu |
| `H` | Help overlay |
| `Enter` | Pick the view type for the focused pane |
| `Tab` | Cycle focused pane |
| `Space` | Fullscreen the focused pane |
| `Shift+←/→` / `Shift+↑/↓` | Split the focused pane |
| `Delete` | Close the focused pane |
| `←/→` | Step the global replay anchor |
| `R` | Cancel replay, snap to LIVE |
| `T` | Cycle theme |
| `Shift+R` | Toggle Rerun live streaming *(with `--features rerun`)* |
| `Shift+L` | Toggle Rerun RRD recording *(with `--features rerun`)* |

---

## License

Dual-licensed under MIT or Apache-2.0, at your option.
