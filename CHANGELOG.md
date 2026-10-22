# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [2.0.0] - 2026-10-22

Needs esp-csi-cli-rs 0.10.x (CLI protocol 4); 0.9 (protocol 3) is accepted
with a warning. Tested on hardware with esp-csi-cli-rs 0.10.1 on the ESP32,
ESP32-S3 and ESP32-C5.

### Added

- **Multiple devices.** Every board gets its own session (reader, history,
  preprocessing, recorder) and they all capture at once. A device panel lists
  them with state, pps and per-device Start/Stop; **Start all / Stop all**
  runs every ticked board under one shared `set-session` id and wall-clock
  epoch so their frames line up; **Apply all**, **Copy config to…** and
  one-click **ESP-NOW pairing** cover the common multi-board setups.
- **Discovery.** Ports are recognised by USB vendor (Espressif, CP210x, CH340,
  FTDI); **Auto-attach** opens every ESP32 as it appears. Boards are named by
  MAC, followed across USB re-enumeration, and their alias, colour and config
  are remembered in `devices.json`.
- **Per-pane device binding**: each pane follows the active device or is pinned
  to one. New **Devices** view (all boards in one table) and an Evolution
  **Compare devices** overlay.
- **The esp-csi-cli-rs 0.10 command surface**: threshold / decimate reporting
  with level, hold and n (`set-reporting`), `--report-to`, `set-session`,
  `set-csi-filter`, `set-protocol ax`, `--he20`, `--ap-leases`, `--ap-burst`,
  `--inject-period-us`, `--emitter-iface`, the C5/C6 HE acquisition flags and
  the built-in `he20` preset.
- `CsiData` carries `device`, `host_time_us`, `node_id`, `session_id`,
  `stream_seq`, `sequence_number`, `ppdu` and `bandwidth_mhz`, and the export
  schema has the matching columns.
- **Command palette** (Ctrl+K), **toasts** for routine events, an inline
  firmware-problem card, shortcuts `S` (start/stop all) and `D` (device panel).
- **Simulated devices** (`--simulate=N`, Devices → Add simulated device) that
  speak the real protocol and wire format.
- **`csi-gui-rs probe`**: headless identify / configure / collect / report, for
  SSH sessions and CI.
- `--port` (repeatable, or bare port arguments), `--auto-attach`,
  `--start-all`, `--data-dir`, `--version`.
- Platform config and data directories; the workspace is restored between
  launches; the showcase template is built in.
- GitHub Actions CI (Linux, Windows, macOS) and tagged release builds for
  x86_64 / aarch64 Linux, Windows and macOS.

### Changed

- **Serialized decoding** uses the esp-csi-rs wire-v1 format (`Envelope` +
  `Body`), vendored from esp-csi-rs 0.13 (`src/protocol/wire`,
  `scripts/sync-wire.sh`). 1.x decoded only the pre-0.12 layout, so every
  serialized frame from 0.9+ firmware failed to decode. The old layout is
  still decoded as a fallback.
- Firmware log text inside the serialized stream goes to the console instead
  of counting as a decode failure; `stream_seq` gaps are counted as lost
  frames and UI back-pressure as dropped frames.
- Identification asks first (`q`, `info`) and only then resets, which suits
  native USB-Serial-JTAG boards; a board that reboots mid-session is
  re-identified and its config re-applied.
- CLI commands are paced on the firmware prompt. Sent back to back, a full
  config overflowed the classic ESP32's UART RX FIFO, and esp-csi-cli-rs
  0.10.1 panics (`Rx(FifoOverflowed)`) and reboots.
- A run that ends by itself (`start --duration`) is detected.
- The config form only shows rows that apply to the chosen mode and chip,
  validates per chip, and uses drag values instead of text buffers.
- `app.rs` split into `app/` modules; new `device`, `paths`, `probe` and
  `sim` modules. Rerun entities are `csi/<device>/…`.
- Import opens a capture as a read-only replay device next to the live ones;
  export writes one file per device from its full recording.
- MSRV is 1.92 (egui 0.34).

### Removed

- `ESP_CONFIG_FIELDS` and the index-coupled panel rows, the modal firmware
  warning, `EspSharedState::should_start` / `should_reset`, and the
  `PhyRate` `*-l` duplicates (old configs still load).

### Fixed

- A serialized frame split across a read timeout is no longer dropped.
- Live export no longer ends at "Nothing to export".
- The bundled-template test no longer depends on a git-ignored file.

## [1.2.0]


Needs esp-csi-cli-rs 0.10.0 or later (`info` reports `protocol=4`). Older
firmware is refused at identify with an "unsupported protocol" dialog.

### Changed (breaking)

- **IEEE 802.11bf vocabulary** — The firmware CLI and this GUI now use the 802.11bf names. Hard cut; the old tokens are no longer sent or accepted.
  - Mode tokens: `esp-now-central` → `esp-now-initiator`, `esp-now-peripheral` → `esp-now-responder`, `esp-now-fast-source` → `esp-now-simplex-source`, `esp-now-fast-collector` → `esp-now-simplex-peer`. `WifiMode::EspNowCentral` / `EspNowPeripheral` are now `EspNowInitiator` / `EspNowResponder`.
  - `set-wifi --collection=collector|listener` → `set-wifi --reporting=report|none`. `CollectionMode` is now `Reporting` (`Report`, `None`), `EspConfig::collection` is `EspConfig::reporting`, and `WifiMode::admits_collection_choice` is `admits_reporting_choice`. The panel row reads **Reporting**.
  - `set-traffic --frequency-hz=N` → `set-measurement --periodicity-hz=N` (`--unsolicited=on|off` unchanged). `EspConfig::traffic_hz` / `traffic_unsolicited` are `periodicity_hz` / `unsolicited`; the panel rows read **Periodicity (Hz)** and **Unsolicited**.
  - `SUPPORTED_PROTOCOL` is 4.

## [1.1.0]

Needs esp-csi-cli-rs 0.8.0 or later. This release supersedes the mode, peer-MAC,
HT40 and CSI-output notes under [Operational modes and CSI output](#operational-modes-and-csi-output) below.

### Added

- **ESP-NOW modes** — The mode selector offers all nine firmware modes again: `esp-now-central`, `esp-now-peripheral` and the two ESP-NOW simplex ends (`esp-now-fast-source`, the flooding end, and `esp-now-fast-collector`, the measuring peer) join `station`, `sniffer`, `wifi-ap`, `ht20-emitter` and `ht40-emitter`.
- **Collection mode selector** — A new **Collection mode** row (`collector` / `listener`) is sent as `set-wifi --collection=<value>` for `station`, `wifi-ap`, `esp-now-central` and `esp-now-peripheral`, the modes that admit a choice. The sniffer, the emitters and the simplex ends fix it, so it is greyed out and not sent there. A listener still captures; it just does not report.
- **Peer MAC and HT40 in the ESP-NOW modes** — `--peer-mac` is the emitters' injection destination and the ESP-NOW modes' explicit peer (blank pairs automatically; when set, set it on both nodes). `--ht40` is the softAP's secondary channel in `wifi-ap` and forces the per-peer TX PHY in the ESP-NOW modes that transmit to a peer.

### Changed

- **Node model vocabulary** — Docs and labels follow the four-attribute [node model](https://github.com/csi-rs/esp-csi-rs/blob/main/docs/network-model.md): the Wi-Fi mode selector sets the operational mode, and "collector" / "listener" are collection modes, not roles.
- **CSI output is the delivery gate** — `set-csi-output --enabled=` is documented as the runtime delivery gate only; the collection mode is set separately (above).
- **PHY rate** — Documented as reporting only, except on the ESP-NOW pair, which applies it.
- **Renames** — `EspConfig`'s ESP32-C5/C6 acquisition fields are now `c5c6_csi_en`, `c5c6_csi_legacy_en`, `c5c6_csi_ht20_en`, `c5c6_csi_ht40_en` and `c5c6_val_scale_cfg`; configs saved with the previous names still load. `ChipFamily::is_c5c6`, `CsiPacketC5` / `CsiPacketC6` (field `sigb_len`) follow the same naming, and panel labels read "C5/C6: …".

### Removed

- **One `WifiProtocol` variant** that `set-protocol` never accepted (it takes `b|g|n|lr|a|ac`). A saved config naming it loads with the default protocol, `lr`.

## [1.0.0] and earlier

### CSI decode and firmware identify

### Fixed

- **Serialized decode startup delay** — CSI now decodes within the first 1–2 seconds of Start on the same hardware where `csi-webserver-rs` already worked. The GUI no longer sits at high raw fps with ~0% decode for ~30 seconds before recovering on its own.
- **Wire decode path** — Frames are decoded with `postcard::take_from_bytes_cobs` on raw COBS bytes, selected by chip family (classic / ESP32-C5 / ESP32-C6). The legacy `un_utf8` step and custom COBS decoder are removed from the hot path.
- **Framing** — Serialized mode uses `read_until('\0')` instead of byte-by-byte accumulation.
- **Post-command handling** — After CLI writes, one null-delimited chunk is skipped (`drop_next_chunk`) instead of discarding up to 50 ms of port data via `drain_echo`.
- **Firmware identification** — Restored `flush()` after serial writes; `read_one_frame` runs only while `ConnState::Streaming` so identify traffic is not consumed by the packet decoder; `info` is sent without marking the response for drop.
- **Merge with upstream** — Integrated the esp-csi-cli-rs 0.5.0 sync (peer-MAC/HT40, raw delivery) while keeping AP config fields, ESP panel UI, and the decode/identify fixes above.

### Unchanged (still in place)

- Start is disabled until config has been applied to the device.
- Log mode is re-synced immediately before each Start.
- Default reader log mode matches firmware boot (`ArrayList`) until `set-log-mode` is applied.

### Memory and recording

### Added

- **Optional JSONL recording** — Checkbox **Record to JSONL (exports/)** in the ESP panel Capture section. When enabled during collection, every post-preprocess packet is streamed to `exports/<port>_<time>.jsonl` on a background thread. Off by default.
- **Bounded in-memory history** — Live capture no longer clones every packet into RAM. Plots and tick replay use a `--history-size` window (~100 ms per tick). Full per-packet history lives on disk only when recording is enabled.

### Fixed

- **Long-session performance** — Removed per-packet `raw_history` growth during live capture and stopped copying unused `distribution_grid` snapshots into every tick. `History::get` uses binary search on tick ids.

### Preprocessing

### Added

- **Denoising stage** — Rolling mean, rolling median, Savitzky–Golay, Hampel (MAD), Butterworth low-pass, Haar wavelet DWT, and FFT delay (CIR multipath) filters.
- **Expanded preprocessing UI** — M → Preprocessing… now uses collapsible sections, ComboBoxes, contextual sliders, tooltips, and an active pipeline summary. Status bar shows the active preprocess chain when filters are on.

### Unchanged

- Pipeline order: Interpolation → Denoising → Detrending → Feature scaling.
- Filters apply at capture time only; toggling does not rewrite packets already in history.
- Segmentation (activity gating) is planned for a follow-up.

### Layout and views

### Fixed

- **Pane overflow when splitting rows/columns** — View content no longer falls off-screen in small tiled panes. The pane body uses the remaining height, scrolls when needed, and plots scale to the available space.

### ESP config panel

### Fixed

- **Numeric field editing** — Traffic Hz, the C5/C6 value scale, and start duration use persistent edit buffers. Backspacing no longer collapses values unexpectedly (e.g. `2|000` becoming `0`).
- **Keyboard shortcut conflict** — Pane focus shortcuts (1–9, 0) are suppressed while a text field has keyboard focus, so typing digits in config fields no longer switches panes.

### Changed

- **Enum fields use dropdowns** — Multi-value settings are now `ComboBox` widgets instead of click-to-cycle buttons (Wi-Fi mode, protocol, HT40, log mode, PHY rate, CSI delivery).
- **Channel hidden in Station mode** — The Channel field is hidden and `set-wifi --set-channel` is omitted when Wi-Fi mode is Station, since the channel follows the associated AP.

### Operational modes and CSI output

### Changed

- **Wi-Fi mode selector** — Offered `station`, `sniffer`, `wifi-ap`, `ht20-emitter` and `ht40-emitter`. *Superseded in 1.1.0*, which restores the four ESP-NOW modes.
- **Destination MAC** — `--peer-mac` became the emitter's injection destination (blank = broadcast). Bandwidth rides on the mode token, so there is no separate bandwidth flag. *Extended in 1.1.0* to the ESP-NOW modes.
- **HT40** — `--ht40=above|below|none` sets the softAP's secondary channel. *Extended in 1.1.0* to the ESP-NOW modes.
- **`set-csi-output` replaces the removed `set-collection-mode`** — `set-csi-output --enabled=true|false` (default `true`) gates whether captured CSI is delivered off-device. With it off the radio still captures, so the RX path keeps its timing. *In 1.1.0* the collection mode is set with `set-wifi --collection`.
- **Acquisition presets are pluggable** — Presets are no longer hard-coded. A build installs a `CsiProfile` (`src/protocol/profile.rs`) that names its own presets; the standard profile offers none and the preset row is hidden. An unresolved name falls back to the individual acquisition flags rather than pushing a token the firmware would reject.
- **`set-csi` on C5/C6** — Emission is limited to the flags the firmware accepts: `--csi`, `--csi-legacy`, `--csi-ht20`, `--csi-ht40`, `--val-scale-cfg`, `--dump-ack`, plus the C5-only `--csi-force-lltf` / `--csi-vht`.
- **Serialized data formats** — Wire discriminants 14 and 15 are `Reserved14` / `Reserved15`. The ordinals and the round-trip decode are unchanged; only the labels are neutral.

### Internal

- Removed temporary `agent_debug` NDJSON instrumentation added during the decode investigation.
- Added `savgol-rs` for Savitzky–Golay filtering.
- Added `SessionRecorder` (`src/persistence/recording.rs`) for background JSONL writes.

### Verification

- `cargo test` (40 unit tests)
- `cargo clippy -- -D warnings`
- Manual:
  - Connect → Apply (Serialized) → Start — decode % rises quickly; firmware identifies reliably.
  - Split layout 2×2 — panes remain usable.
  - M → Preprocessing… — enable Hampel + MA detrend; confirm plots change on new packets only.
  - Enable **Record to JSONL** → Start → Stop — file appears under `exports/`; Import Data replays it.
  - Long capture without recording — RSS stays stable (no unbounded packet RAM growth).
