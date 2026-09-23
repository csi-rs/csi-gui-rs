# Release Notes

## 1.1.0

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

## CSI decode and firmware identify

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

## Memory and recording

### Added

- **Optional JSONL recording** — Checkbox **Record to JSONL (exports/)** in the ESP panel Capture section. When enabled during collection, every post-preprocess packet is streamed to `exports/<port>_<time>.jsonl` on a background thread. Off by default.
- **Bounded in-memory history** — Live capture no longer clones every packet into RAM. Plots and tick replay use a `--history-size` window (~100 ms per tick). Full per-packet history lives on disk only when recording is enabled.

### Fixed

- **Long-session performance** — Removed per-packet `raw_history` growth during live capture and stopped copying unused `distribution_grid` snapshots into every tick. `History::get` uses binary search on tick ids.

## Preprocessing

### Added

- **Denoising stage** — Rolling mean, rolling median, Savitzky–Golay, Hampel (MAD), Butterworth low-pass, Haar wavelet DWT, and FFT delay (CIR multipath) filters.
- **Expanded preprocessing UI** — M → Preprocessing… now uses collapsible sections, ComboBoxes, contextual sliders, tooltips, and an active pipeline summary. Status bar shows the active preprocess chain when filters are on.

### Unchanged

- Pipeline order: Interpolation → Denoising → Detrending → Feature scaling.
- Filters apply at capture time only; toggling does not rewrite packets already in history.
- Segmentation (activity gating) is planned for a follow-up.

## Layout and views

### Fixed

- **Pane overflow when splitting rows/columns** — View content no longer falls off-screen in small tiled panes. The pane body uses the remaining height, scrolls when needed, and plots scale to the available space.

## ESP config panel

### Fixed

- **Numeric field editing** — Traffic Hz, the C5/C6 value scale, and start duration use persistent edit buffers. Backspacing no longer collapses values unexpectedly (e.g. `2|000` becoming `0`).
- **Keyboard shortcut conflict** — Pane focus shortcuts (1–9, 0) are suppressed while a text field has keyboard focus, so typing digits in config fields no longer switches panes.

### Changed

- **Enum fields use dropdowns** — Multi-value settings are now `ComboBox` widgets instead of click-to-cycle buttons (Wi-Fi mode, protocol, HT40, log mode, PHY rate, CSI delivery).
- **Channel hidden in Station mode** — The Channel field is hidden and `set-wifi --set-channel` is omitted when Wi-Fi mode is Station, since the channel follows the associated AP.

## Operational modes and CSI output

### Changed

- **Wi-Fi mode selector** — Offered `station`, `sniffer`, `wifi-ap`, `ht20-emitter` and `ht40-emitter`. *Superseded in 1.1.0*, which restores the four ESP-NOW modes.
- **Destination MAC** — `--peer-mac` became the emitter's injection destination (blank = broadcast). Bandwidth rides on the mode token, so there is no separate bandwidth flag. *Extended in 1.1.0* to the ESP-NOW modes.
- **HT40** — `--ht40=above|below|none` sets the softAP's secondary channel. *Extended in 1.1.0* to the ESP-NOW modes.
- **`set-csi-output` replaces the removed `set-collection-mode`** — `set-csi-output --enabled=true|false` (default `true`) gates whether captured CSI is delivered off-device. With it off the radio still captures, so the RX path keeps its timing. *In 1.1.0* the collection mode is set with `set-wifi --collection`.
- **Acquisition presets are pluggable** — Presets are no longer hard-coded. A build installs a `CsiProfile` (`src/protocol/profile.rs`) that names its own presets; the standard profile offers none and the preset row is hidden. An unresolved name falls back to the individual acquisition flags rather than pushing a token the firmware would reject.
- **`set-csi` on C5/C6** — Emission is limited to the flags the firmware accepts: `--csi`, `--csi-legacy`, `--csi-ht20`, `--csi-ht40`, `--val-scale-cfg`, `--dump-ack`, plus the C5-only `--csi-force-lltf` / `--csi-vht`.
- **Serialized data formats** — Wire discriminants 14 and 15 are `Reserved14` / `Reserved15`. The ordinals and the round-trip decode are unchanged; only the labels are neutral.

## Internal

- Removed temporary `agent_debug` NDJSON instrumentation added during the decode investigation.
- Added `savgol-rs` for Savitzky–Golay filtering.
- Added `SessionRecorder` (`src/persistence/recording.rs`) for background JSONL writes.

## Verification

- `cargo test` (40 unit tests)
- `cargo clippy -- -D warnings`
- Manual:
  - Connect → Apply (Serialized) → Start — decode % rises quickly; firmware identifies reliably.
  - Split layout 2×2 — panes remain usable.
  - M → Preprocessing… — enable Hampel + MA detrend; confirm plots change on new packets only.
  - Enable **Record to JSONL** → Start → Stop — file appears under `exports/`; Import Data replays it.
  - Long capture without recording — RSS stays stable (no unbounded packet RAM growth).
