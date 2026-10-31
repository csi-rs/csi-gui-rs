<p align="center">
  <a href="https://github.com/csi-rs">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/csi-rs/.github/main/brand/svg/csi-rs-lockup-dark-nocaption.svg">
      <img alt="csi-rs" src="https://raw.githubusercontent.com/csi-rs/.github/main/brand/svg/csi-rs-lockup-light-nocaption.svg" width="320">
    </picture>
  </a>
</p>

# csi-gui-rs

**A desktop GUI for live Wi-Fi Channel State Information (CSI) from one or many ESP32 boards.**

[![CI](https://github.com/csi-rs/csi-gui-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/csi-rs/csi-gui-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/csi-gui-rs.svg)](https://crates.io/crates/csi-gui-rs)
[![docs.rs](https://docs.rs/csi-gui-rs/badge.svg)](https://docs.rs/csi-gui-rs)

![csi-gui-rs with five ESP32s: an ESP-NOW pair and three sniffers](https://raw.githubusercontent.com/csi-rs/csi-gui-rs/main/images/gui-preview.png)

Plug in ESP32s running [`esp-csi-cli-rs`](https://github.com/csi-rs/esp-csi-cli-rs), run
`csi-gui-rs`, and watch the channel live on every board at once: magnitude, phase, I/Q,
channel impulse response, a spectrogram and long-window evolution curves. Configure each board
from typed forms, start them all under one measurement session, record every packet to disk,
and export to CSV / JSONL / Parquet or [Rerun](https://rerun.io).

The screenshot is a Raspberry Pi 4 driving five boards: two ESP32-C5s as an ESP-NOW
initiator / responder pair, and two ESP32-S3s and a classic ESP32 sniffing the same channel,
each pane bound to a different board.

---

## Highlights

- **Many boards, one window.** Every board gets its own reader, history, preprocessing and
  recorder. Attach them one by one, or let **Auto-attach** pick up every ESP32 as it is
  plugged in. Each board is named by its MAC, so it keeps its alias, colour and config across
  replugs and USB re-enumeration.
- **Start all, in sync.** One click (or `S`) starts every ticked board under a shared
  session id and wall-clock epoch (`set-session`), so frames from different boards line up.
  ESP-NOW pairing is one click too.
- **Per-pane device binding.** Each pane follows the selected device or is pinned to one; put
  board A's magnitude next to board B's. The **Devices** view tabulates every board (state,
  pps, decode rate, frames lost / dropped, session), and **Evolution → Compare devices**
  overlays them.
- **The whole 0.10 firmware surface** as a form that only shows what applies to the chosen
  mode and chip: the nine 802.11bf modes, threshold / decimate reporting, report-to,
  HE20 and `ax` on the C5/C6, AP leases and burst flood, emitter period and interface, the
  CSI filter, the classic and C5/C6 acquisition flags and the `he20` preset.
- **Every log mode decodes**: `serialized` (the esp-csi-rs wire-v1 format, plus the pre-0.12
  layout), `array-list` (with the transmitter MAC), `text` and `esp-csi-tool`. Firmware log
  lines go to a console instead of counting as decode failures; gaps in the frame counter
  are reported as lost frames.
- **Ten visualizations** in a tiling layout: Dashboard, Devices, Magnitude, Phase, Polar,
  CIR, Spectrogram, Evolution, I/Q scatter, Console, plus a Replay scrubber that moves every
  pane together.
- **Live preprocessing**: interpolation, denoising (rolling mean / median, Savitzky–Golay,
  Hampel, Butterworth, DWT, FFT delay), detrending and feature scaling, applied as packets
  arrive.
- **Self-healing.** Unplug a board, reboot it, or let its firmware crash mid-run: it is
  re-identified, re-configured and resumes the run under the same session, on whatever
  port it comes back on.
- **Quality of life**: Ctrl+K command palette, toasts instead of modal pop-ups, a
  firmware-problem card that says what to do, workspace saved between launches, and
  templates.
- **No hardware?** `--simulate=N` adds N simulated boards that speak the real protocol.
- **Headless check**: `csi-gui-rs probe` identifies, configures, collects and reports per
  board from a terminal; handy over SSH and in CI.

---

## Compatibility

| csi-gui-rs | esp-csi-cli-rs | CLI protocol | Serialized format |
|---|---|---|---|
| **2.0** | **0.10.x** (recommended), 0.9 | 4, 3 | wire v1 (esp-csi-rs 0.12+); the pre-0.12 layout still decodes |
| 1.2 | 0.10.0 | 4 | pre-0.12 only, so serialized mode fails on 0.10 firmware |
| 1.1 | 0.8 | 3 | pre-0.12 |

Chips: ESP32, ESP32-S3, ESP32-C3, ESP32-C5, ESP32-C6. Tested on hardware with
esp-csi-cli-rs 0.10.1 on the ESP32 (CH340, UART), ESP32-S3 and ESP32-C5 (native USB).
Use the plain-text firmware build: a `defmt` build (`info` says `log=defmt`) is refused
with an explanation.

---

## Install

**Prebuilt binaries** for Linux (x86_64, aarch64 / Raspberry Pi), Windows and macOS are on
the [releases page](https://github.com/csi-rs/csi-gui-rs/releases).

**From crates.io** (Rust 1.92 or newer):

```sh
cargo install csi-gui-rs                     # the GUI
cargo install csi-gui-rs --features rerun    # with the Rerun sink
```

**From source:**

```sh
git clone https://github.com/csi-rs/csi-gui-rs
cd csi-gui-rs
cargo run --release
```

### Platform notes

- **Linux**: needs `libudev` (Debian / Ubuntu: `sudo apt install libudev-dev pkg-config`,
  Fedora: `sudo dnf install systemd-devel`). Add yourself to the serial group so the ports
  open without root: `sudo usermod -aG dialout $USER` (Arch: `uucp`), then log in again.
  X11 and Wayland both work.
- **Windows**: install the [CH340](https://www.wch-ic.com/downloads/CH341SER_EXE.html) or
  [CP210x](https://www.silabs.com/developers/usb-to-uart-bridge-vcp-drivers) driver if your
  board has a USB-UART bridge; native-USB boards (S3, C3, C5, C6) need none.
- **macOS**: boards show up as `/dev/cu.usbmodem…` (native USB) or `/dev/cu.usbserial…`.

### Flashing the firmware

Build and flash [`esp-csi-cli-rs`](https://github.com/csi-rs/esp-csi-cli-rs) for your chip
(see its README), e.g. for an ESP32-S3:

```sh
cargo run --release --features esp32s3 --target xtensa-esp32s3-none-elf
```

---

## Quick start

```sh
csi-gui-rs                         # open the GUI; add boards from the Devices panel
csi-gui-rs --auto-attach           # attach every ESP32 port, now and as they appear
csi-gui-rs /dev/ttyACM0 /dev/ttyUSB0
csi-gui-rs --simulate=3            # three simulated boards, no hardware
csi-gui-rs -a --start-all          # unattended: attach everything and start collecting
```

Then:

1. Boards appear in the **Devices** panel on the right as they identify. Click one to edit
   its configuration, rename it, or pair it over ESP-NOW with another.
2. Press **Apply** (or **Apply all**). New boards get their config pushed automatically.
3. Press **▶ Start all** (or `S`). Tick **Record** first to keep every packet on disk.
4. Use each pane's device picker to choose what it shows; add a **Devices** pane for the
   overview.

### Command-line options

```
csi-gui-rs [options] [PORT...]          open the GUI
csi-gui-rs probe [options] [PORT...]    headless check: identify, collect, report

  PORT, -p PORT, --port=PORT   serial port to open (repeatable)
  -a, --auto-attach            open every ESP32-looking port, now and as they appear
  --simulate=N                 add N simulated esp-csi-cli-rs devices
  --start-all                  start every device as soon as it is ready
  -b N, --baud=N               baud for UART-bridged boards (default 115200)
  --history-size=N             plot/replay window in 100 ms ticks (default 10000)
  --data-dir=DIR               where exports, recordings and .rrd files go
  --rerun=MODE                 off | spawn | connect[:host:port] | file:PATH

probe options:
  --seconds=N  --channel=N  --log-mode=MODE  --esp-now-pair  --record
```

### Headless probe

```console
$ csi-gui-rs probe --seconds=10 --channel=6 --esp-now-pair /dev/ttyACM0 /dev/ttyACM2 /dev/ttyACM1 /dev/ttyACM3 /dev/ttyUSB0
port          mac                firmware                                mode               frames    pps  dec%  lost  session   sc
/dev/ttyACM0  10:BD:A3:CA:34:98  esp-csi-cli-rs 0.10.1 · esp32c5 · proto 4 esp-now-initiator   1601  160.1 100.0     0 0cea5939  53  OK
/dev/ttyACM2  38:44:BE:1F:FE:18  esp-csi-cli-rs 0.10.1 · esp32c5 · proto 4 esp-now-responder    882   88.2 100.0     0 0cea5939  53  OK
/dev/ttyACM1  A0:85:E3:F5:48:90  esp-csi-cli-rs 0.10.1 · esp32s3 · proto 4 sniffer             4264  426.4 100.0     0 0cea5939  64  OK
/dev/ttyACM3  A0:85:E3:F5:3F:58  esp-csi-cli-rs 0.10.1 · esp32s3 · proto 4 sniffer             1697  169.7 100.0     0 0cea5939  64  OK
/dev/ttyUSB0  78:42:1C:69:39:9C  esp-csi-cli-rs 0.10.1 · esp32 · proto 4   sniffer              322   32.2 100.0     0 0cea5939 128  OK
all collecting devices share session 0cea5939
```

The exit status is 0 when every board identified and delivered decodable CSI.

---

## Device configuration

The **Configuration** form is a typed front-end for the firmware's `set-*` commands. It only
shows what the chosen **mode** and the board's **chip** use, so a sniffer has no softAP rows
and an S3 has no C5/C6 acquisition flags. Hover a row's label for what it does. Edits stay
local until **Apply**, which validates them for that chip (5 GHz channels only on the C5,
`ax` and HE20 only on the C5/C6, a sniffer can't report `none`, …) and pushes them.

| Mode | What the node does | Mode-specific rows |
|---|---|---|
| `sniffer` | passive monitor on one channel | reporting |
| `station` | joins an AP, captures from its traffic | SSID, password, reporting, periodicity |
| `wifi-ap` | runs a softAP and floods its stations | SSID, password, DHCP, leases, burst flood, HT40, reporting |
| `ht20-emitter`, `ht40-emitter` | injects frames, captures none | peer MAC (destination), inject period, interface, HE20 |
| `esp-now-initiator` / `-responder` | an ESP-NOW pair | peer MAC, HT40, HE20, reporting, periodicity, PHY rate |
| `esp-now-simplex-source` / `-peer` | a one-way flood and its receiver | peer MAC (HT40 on the source) |

**Reporting** (`set-reporting`): `report` every measurement, `none`, `threshold` (802.11bf
threshold-based: only while the channel moves, with a level and hold time) or `decimate`
(every n-th), sent to the `host` (this GUI) or kept `local` on the device.

**Multi-device helpers**: **Copy to…** copies a config (except the peer MAC) to other boards
and applies it; **ESP-NOW pairing** makes the selected board the initiator and another the
responder on one channel, each with the other's MAC; **Apply all** pushes every board's own
config.

---

## Files

Everything lives in the platform's standard directories (override the data side with
`--data-dir`):

| | Linux | macOS | Windows |
|---|---|---|---|
| Config: templates, `devices.json` | `~/.config/csi-gui-rs` | `~/Library/Application Support/io.csi-rs.csi-gui-rs` | `%APPDATA%\csi-rs\csi-gui-rs\config` |
| Data: `exports/`, `rrd/` | `~/.local/share/csi-gui-rs` | same as config | `%APPDATA%\csi-rs\csi-gui-rs\data` |

**Recordings.** With **Record** ticked, each collecting board streams every packet (after
preprocessing) to `exports/<name>_<session>_<time>.jsonl`.

**Export** (File → Export captures…) writes one file per board in CSV, JSONL or Parquet, from
its last recording (every packet) or, without one, the in-memory history (one averaged packet
per 100 ms tick). **Import** opens a capture as a read-only replay device next to the live ones.

All three formats share one schema: `device`, `host_time_us`, `node_id`, `session_id`,
`stream_seq`, `sequence_number`, `mac` (transmitter), `rssi`, `rate`, `noise_floor`, `channel`,
`secondary_channel`, `bandwidth_mhz`, `ppdu`, `timestamp`, the 802.11 receive metadata,
`data_length`, `subcarriers` and `csi_raw_data` (interleaved values as the firmware sent them;
`[…]` space-separated in CSV and Parquet). Columns are matched by name on import, so files
written by 1.x still load.

**Templates** (File → Save / Load template) store the whole workspace: layout, pane views and
device bindings, theme and preprocessing. Mark one as default to load it at startup; otherwise
the last session's workspace comes back.

---

## Keyboard shortcuts

| Key | Action |
| --- | --- |
| `Ctrl+K` | Command palette (every action, searchable) |
| `S` | Start all / stop all |
| `D` | Toggle the device panel |
| `H` / `F1` | Help |
| `M` | Main menu |
| `Enter` | Pick the view for the focused pane |
| `Tab`, `1..9`, `0` | Focus next pane / pane by id |
| `Space` | Fullscreen the focused pane |
| `Shift+←/→`, `Shift+↑/↓` | Split the focused pane into columns / rows |
| `Delete` | Close the focused pane |
| `←` / `→` | Step the replay anchor |
| `R` | Back to live |
| `T` | Cycle theme |
| `Q` | Quit |
| `Shift+R`, `Shift+L` | Rerun live streaming / RRD recording *(with `--features rerun`)* |

---

## Library use

The crate is also a library: the device manager, reader, decoders (including the vendored
esp-csi-rs [`wire`](src/protocol/wire) contract) and capture formats can be used without the
GUI. See the [crate docs](https://docs.rs/csi-gui-rs).

## Development

```sh
cargo test                          # unit tests, simulated-device tests, real-firmware fixtures
cargo clippy --all-targets --all-features -- -D warnings
scripts/sync-wire.sh ../esp-csi-rs  # re-vendor the wire module after an esp-csi-rs release
cross build --release --target aarch64-unknown-linux-gnu   # Raspberry Pi build
```

Release notes are in [CHANGELOG.md](CHANGELOG.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. The
bundled Adwaita Sans font is SIL OFL 1.1 (`assets/fonts/LICENSE-AdwaitaSans`). The vendored
`src/protocol/wire/` module comes from [esp-csi-rs](https://github.com/csi-rs/esp-csi-rs)
under the same terms.
