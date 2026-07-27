# Release Notes

## CSI decode and firmware identify

### Fixed

- **Serialized decode startup delay** — CSI now decodes within the first 1–2 seconds of Start on the same hardware where `csi-webserver-rs` already worked. The GUI no longer sits at high raw fps with ~0% decode for ~30 seconds before recovering on its own.
- **Wire decode path** — Frames are decoded with `postcard::take_from_bytes_cobs` on raw COBS bytes, selected by chip family (classic / ESP32-C5 / ESP32-C6). The legacy `un_utf8` step and custom COBS decoder are removed from the hot path.
- **Framing** — Serialized mode uses `read_until('\0')` instead of byte-by-byte accumulation.
- **Post-command handling** — After CLI writes, one null-delimited chunk is skipped (`drop_next_chunk`) instead of discarding up to 50 ms of port data via `drain_echo`.
- **Firmware identification** — Restored `flush()` after serial writes; `read_one_frame` runs only while `ConnState::Streaming` so identify traffic is not consumed by the packet decoder; `info` is sent without marking the response for drop.
- **Merge with upstream** — Integrated esp-csi-cli-rs 0.5.0 sync (ESP-NOW peer-MAC/HT40, raw delivery) while keeping AP config fields, ESP panel UI, and the decode/identify fixes above.

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

- **Numeric field editing** — Traffic Hz, HE STBC, HE val scale, and start duration use persistent edit buffers. Backspacing no longer collapses values unexpectedly (e.g. `2|000` becoming `0`).
- **Keyboard shortcut conflict** — Pane focus shortcuts (1–9, 0) are suppressed while a text field has keyboard focus, so typing digits in config fields no longer switches panes.

### Changed

- **Enum fields use dropdowns** — Multi-value settings are now `ComboBox` widgets instead of click-to-cycle buttons (Wi-Fi mode, protocol, HT40, collection mode, log mode, PHY rate, CSI delivery).
- **HE20 is a one-button preset** — The *HE: CSI preset* row is now a single **Apply HE20 preset** button (mirroring `csi-webserver-rs`) instead of a Default/He20 ComboBox. One click sets the full HE-LTF-only layout (`EspConfig::apply_he20_preset`) and collapses the pushed config to `set-csi --preset=he20`; clicking again clears back to the default preset.
- **Channel hidden in Station mode** — The Channel field is hidden and `set-wifi --set-channel` is omitted when Wi-Fi mode is Station, since the channel follows the associated AP.

## Internal

- Removed temporary `agent_debug` NDJSON instrumentation added during the decode investigation.
- Added `savgol-rs` for Savitzky–Golay filtering.
- Added `SessionRecorder` (`src/persistence/recording.rs`) for background JSONL writes.

## Verification

- `cargo test` (38 unit tests)
- `cargo clippy -- -D warnings`
- Manual:
  - Connect → Apply (Serialized) → Start — decode % rises quickly; firmware identifies reliably.
  - Split layout 2×2 — panes remain usable.
  - M → Preprocessing… — enable Hampel + MA detrend; confirm plots change on new packets only.
  - Enable **Record to JSONL** → Start → Stop — file appears under `exports/`; Import Data replays it.
  - Long capture without recording — RSS stays stable (no unbounded packet RAM growth).
