# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A real-time **binaural spatializer audio plugin** written in Rust using the [nih-plug](https://github.com/robbert-vdh/nih-plug) framework. It takes stereo audio, downmixes to mono, and re-spatializes it to a 3D position by convolving with head-related impulse responses (HRIRs) read from a SOFA dataset. Builds as a **VST3 + CLAP** plugin. Originally a Georgia Tech "Audio Software Engineering" (ASE) course group project.

## Branches (important)

- **`main`** — the plugin (VST3/CLAP). This is what you are on.
- **`origin/GUI`** — a *separate, heavily divergent* implementation: a standalone `eframe`/`egui` desktop app (`src/main.rs`) for offline file rendering, plus a vendored C `libmysofa-sys` binding and a forked copy of the sofar `reader.rs`/`render.rs`. It is effectively a different project (crate name `proj`, not `ase-project`). Do not assume code/commands are shared across the two branches. The README's `cargo run` instruction and the "GUI ver." demo refer to this branch.

## Build / run / test

**Prerequisites:** just a Rust toolchain (edition 2021; MSRV 1.80, inherited from nih-plug). **No C toolchain is needed** — `sofar` 0.3 is a pure-Rust SOFA reader. (Older `sofar` 0.1.x wrapped the C `libmysofa` via `libmysofa-sys` and required CMake + pkg-config; the 0.3 rewrite dropped that. So if a CMake error ever appears here, check that `sofar` hasn't been pinned back to 0.1.x in `Cargo.toml`.)

```shell
# Bundle the VST3 + CLAP plugin (uses nih-plug's xtask; alias defined in .cargo/config.toml)
cargo xtask bundle ase-project --release      # output under target/bundled/

# Type-check / build the cdylib without bundling
cargo check
cargo build --release

# Run the standalone rotating-source demo (reads a MONO f32 wav + a SOFA file)
cargo run --release --example renderer -- audio/Melody_mono.wav SOFA-data/HRIR_FULL2DEG.sofa

# Tests (see "dead code" note below — tests currently live in an un-compiled module)
cargo test
cargo test test_audio        # run a single test by name
```

Note: `cargo run` with no example does **not** work on `main` — the crate is `crate-type = ["cdylib"]` with no binary target. First build pulls `nih_plug`/`nih_plug_xtask` from git, so it is slow. Rust edition 2021.

The `sofa_read.py` helper (`pip install sofar`) just inspects a SOFA file's IR/position array shapes; it is not part of the build.

## Architecture

The signal flow, per audio block, lives entirely in `Spatializer::process` in `src/lib.rs`:

1. **Downmix** the stereo input buffer to mono (average of the two channels).
2. **Map parameters to a source position.** The three angle params (`frontback`, `leftright`, `updown`, each −180..180°) are converted to a normalized `x/y/z` in roughly [−1, 1] (with a tiny offset, because no IR exists exactly at the origin).
3. **Extract the HRIR** for that position from the loaded `Sofar` dataset into a `Filter` (left+right impulse responses).
4. **Convolve.** The filter is handed to a `sofar::render::Renderer` (partitioned FFT convolution), which turns the mono signal into left/right output.
5. **Apply gain** (smoothed) and write back into the buffer in place.

Key pieces:
- **HRIR data:** `SOFA-data/HRIR_FULL2DEG.sofa` — Neumann KU100 spherical far-field set, 2° azimuth/elevation resolution, 128-sample IRs, fixed 3.25 m radius. Read via the `sofar` crate (`OpenOptions`/`Sofar`/`Filter`/`Renderer`).
- **Plugin surface:** `src/lib.rs` implements `Plugin` + `ClapPlugin` + `Vst3Plugin` and exports via `nih_export_clap!`/`nih_export_vst3!`. Parameters are declared on `SpatializerParams` (`#[derive(Params)]`) with stable `#[id = "..."]` strings — renaming fields is fine, changing IDs breaks host automation/preset compatibility.
- **`src/lib.rs` is the ONLY file compiled into the library.** It declares no `mod`s.

### Dead / experimental code (not compiled)

These files are **not** referenced by any `mod` declaration, so they are excluded from the build. Treat them as reference material, and know that editing them changes nothing until they are wired in:
- `src/lib_ola.rs` — an alternative `process()` that does manual STFT overlap-add FFT convolution (`realfft` + nih-plug's `StftHelper`) instead of sofar's `Renderer`.
- `src/spatializer_efx.rs` — a `SpatializerEfx` helper struct plus the only `#[test]`s in the project (they render `audio/Melody_mono.wav` to an `output.wav`). Because the file is not in the module tree, `cargo test` currently does not run them; wire it up with `mod spatializer_efx;` (or move the tests) to actually exercise them.

## Known issues / gotchas (read before changing DSP code)

- **SOFA path is resolved at build time.** `src/lib.rs` locates the dataset via the `SOFA_PATH` constant — `concat!(env!("CARGO_MANIFEST_DIR"), "/SOFA-data/HRIR_FULL2DEG.sofa")` — so it loads from any working directory (including inside a DAW) on the machine that built it. **Distribution caveat:** this bakes in the build-time repo path, so a compiled `.vst3`/`.clap` copied to a machine without this checkout will `unwrap`-panic on load. For real distribution, bundle the SOFA into the plugin package or make the path a user setting. (`lib_ola.rs`/`spatializer_efx.rs` use the same pattern.)
- **Sample rate** now comes from the host: `initialize()` reads `buffer_config.sample_rate` and applies it to both the SOFA load (sofar resamples the HRIRs) and the `Renderer`, replacing the old mismatched 48 kHz / 44.1 kHz hardcodes.
- **`.unwrap()` on file I/O in real-time paths.** SOFA loading and block processing unwrap liberally; a bad path or unexpected format aborts the host.
- **Per-block allocation in `process()`.** `Filter::new`, the mono `Vec`, and the `left`/`right` `Vec`s are allocated every block on the audio thread (note `assert_process_allocs` is enabled in `Cargo.toml`, so this can trip in debug).
- **Placeholder metadata:** `URL` is a rickroll and `EMAIL`/`VENDOR` are stubs in the `Plugin` impl.
- The `hrtf` and `dsp`-feature entries in `Cargo.toml` are declared but unused on `main` (the pipeline uses `sofar`).
