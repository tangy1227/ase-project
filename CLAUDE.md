# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A real-time **binaural spatializer audio plugin** written in Rust using the [nih-plug](https://github.com/robbert-vdh/nih-plug) framework. It takes stereo audio and renders each channel as its own virtual source around the listener by convolving with head-related impulse responses (HRIRs) read from a SOFA dataset, so the stereo image survives (left channel at `azimuth + width/2`, right at `azimuth − width/2`). Builds as a **VST3 + CLAP** plugin with an egui editor. Originally a Georgia Tech "Audio Software Engineering" (ASE) course group project.

## Branches (important)

- **`main`** / **`retake_and_updates`** — the plugin (VST3/CLAP).
- **`origin/GUI`** — a *separate, heavily divergent* implementation: a standalone `eframe`/`egui` desktop app (`src/main.rs`) for offline file rendering, plus a vendored C `libmysofa-sys` binding and a forked copy of the sofar `reader.rs`/`render.rs`. It is effectively a different project (crate name `proj`, not `ase-project`). Do not assume code/commands are shared across the two branches. The README's `cargo run` instruction and the "GUI ver." demo refer to this branch.

## Build / run / test

**Prerequisites:** just a Rust toolchain (edition 2021; MSRV 1.80, inherited from nih-plug). **No C toolchain is needed** — `sofar` 0.3 is a pure-Rust SOFA reader. (Older `sofar` 0.1.x wrapped the C `libmysofa` via `libmysofa-sys` and required CMake + pkg-config; the 0.3 rewrite dropped that. So if a CMake error ever appears here, check that `sofar` hasn't been pinned back to 0.1.x in `Cargo.toml`.)

```shell
# Bundle the VST3 + CLAP plugin (uses nih-plug's xtask; alias defined in .cargo/config.toml)
cargo xtask bundle ase-project --release      # output under target/bundled/

# Type-check / build the cdylib without bundling
cargo check
cargo build --release

# Tests. Use --release: the DSP tests parse the 20 MB SOFA set, which is very slow in debug.
cargo test --release -p ase-project -p spatializer-ui
cargo test --release -p ase-project hard_left   # one test by name

# Editor smoke test (Windows only, opens a window for ~1 s): spawns the editor in a parent
# window like a host would, clicks the pad, checks the begin/set/end gesture reaches the host.
cargo test --release -p ase-project editor_smoke -- --ignored --nocapture
# EDITOR_SMOKE_SCREENSHOT=editor.bmp additionally saves what the window drew

# Standalone widget prototype (eframe), and the old rotating-source demo
cargo run -p spatializer-ui --release --example position_widget
cargo run --release --example renderer -- audio/Melody_mono.wav SOFA-data/HRIR_FULL2DEG.sofa
```

Note: `cargo run` with no example does **not** work on `main` — the crate is `crate-type = ["cdylib"]` with no binary target. First build pulls `nih_plug`/`nih_plug_xtask` from git, so it is slow.

The `sofa_read.py` helper (`pip install sofar`) just inspects a SOFA file's IR/position array shapes; it is not part of the build.

**Installing for a DAW:** copy `target/bundled/ase-project.vst3` (a folder) to `C:\Program Files\Common Files\VST3\` and/or `ase-project.clap` to `...\Common Files\CLAP\`. Close the DAW first — a loaded plugin DLL is locked.

## Architecture

- **`src/lib.rs`** — `Plugin`/`ClapPlugin`/`Vst3Plugin` impls, `SpatializerParams`, exports. `initialize()` loads the SOFA set at the host rate (only when the rate changed) and builds the engine; `Default` does **not** load it (hosts call `Default` while scanning). `process()` hands the stereo buffer to the engine, then applies the smoothed gain and updates the editor's peak meter.
- **`src/dsp.rs`** — `Engine`, the whole signal path, independent of nih-plug except `permit_alloc`:
  1. Input goes through a fixed **64-sample FIFO** (`HOP`), so sofar's renderer always gets whole partitions regardless of host block size. Reported latency = 64 samples.
  2. Once per hop, the `Position { azimuth, elevation, width }` is turned into two directions (L/R sources), placed on the dataset's measurement radius, and looked up with `Sofar::filter`.
  3. Each source has **two `Renderer`s** fed the same input; on a position change the new filter goes into the spare one, the hop is crossfaded (raised cosine) from old to new, and they swap. No change → no lookup.
  4. Output = 0.5 · (left source + right source), so `width = 0` equals the old `(L+R)/2` mono path exactly.
  Non-finite input samples are zeroed; renderer errors reset the engine instead of panicking.
- **`src/editor.rs`** — the plugin editor. Implements nih-plug's `Editor` trait directly on **egui-baseview 0.7** (egui 0.36) because `nih_plug_egui` at the pinned commit is on egui 0.31 and the widget crate is on 0.36. Draws the `ui` crate's `PositionPad` (azimuth; L/R ghost dots show where each channel lands) + `ElevationStrip`, plus Width/Gain sliders and a level meter. Every change goes through `ParamSetter` begin/set/end. Key capture is off so host shortcuts (space bar) keep working.
- **`ui/`** (`spatializer-ui`) — framework-agnostic egui widgets and design tokens (see `UI.md`). Talks to the outside only through the `PositionHost` trait.
- **HRIR data:** `SOFA-data/HRIR_FULL2DEG.sofa` — Neumann KU100 spherical far-field set, 2° resolution, 128-sample IRs at 48 kHz, fixed 3.25 m radius.

**Parameters** (`#[id]` strings are the host-facing identity — renaming fields is fine, changing IDs breaks automation/presets): `gain`, `azimuth` (−180…180°, **positive = left**, SOFA convention; displayed as "30° L"/"30° R"), `elevation` (−90…90°), `width` (0…180°, default 60° = ±30°). Version 0.2.0 replaced the old `FrontBack`/`LeftRight`/`UpDown` IDs (Cartesian coordinates mislabelled as degrees), so sessions saved with 0.1 load those at defaults.

### Dead / experimental code (not compiled)

Not referenced by any `mod`, so editing them changes nothing:
- `src/lib_ola.rs` — an alternative `process()` that does manual STFT overlap-add FFT convolution (`realfft` + nih-plug's `StftHelper`) instead of sofar's `Renderer`.
- `src/spatializer_efx.rs` — a `SpatializerEfx` helper plus old `#[test]`s that render `audio/Melody_mono.wav`.

## Known issues / gotchas

- **SOFA path is resolved at build time** (`SOFA_PATH` = `CARGO_MANIFEST_DIR/SOFA-data/...`). A bundle copied to a machine without this checkout fails `initialize()` (logged, returns `false`; no longer panics). For distribution, ship the SOFA inside the bundle or `include_bytes!` it (`Sofar::open_data`).
- **sofar's interpolation allocates** two small `Vec`s per lookup. It only runs when the position changes and is wrapped in `permit_alloc`; the rest of `process()` is allocation-free.
- **Editor scaling:** on Windows/Linux the window is `640×440 × host scale factor` physical px. Hosts that never call `setContentScaleFactor` get a 1.0 frame while baseview draws at the monitor DPI; the layout is responsive, so it just renders smaller. With OpenGL, baseview follows the *process* DPI awareness.
- **Placeholder metadata:** `URL` is a rickroll and `EMAIL`/`VENDOR` are stubs in the `Plugin` impl.
- Unused deps (`hrtf`, `criterion`, …) and the `dsp` feature are still in `Cargo.toml`; some are used by examples only.
- `AUDIT.md`/`ROADMAP.md` (2026-09-12) predate 0.2.0: F1, F2, F4, F6, F7 (reset only; the tail is still not reported), F8 (partly), F10, F12 (partly), F13 and roadmap A1, A2, A3, A4a, A5, A10, A13 (first cut) are now addressed.
