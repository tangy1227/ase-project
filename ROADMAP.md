# ROADMAP — ranked changes for the ase-project spatializer

Date: 2026-09-12. Reads with AUDIT.md (findings F1–F15) and RESEARCH.md. Nothing below has been
implemented; this is the order I would do it in, with what each change breaks, how to prove it
helped, and rough effort for one person who knows the code.

**Rewrite verdict: no.** The measured DSP core is exact and uses 0.05–0.25 % of the block budget. Every
serious defect sits at the edges (parameter model, interpolation query, filter updates, error handling,
loading). Each is fixable inside the current nih-plug + sofar structure, and I could not find a defect
that incremental change fails on. The "replace the approach" section lists what a different approach
would buy and why none of them is justified *yet*.

Verification tools referenced: `examples/audit_bench.rs` (already in the repo; extend it),
Python `sofar`/`pyfar` for offline spectral checks, AMT toolbox localization models, SPARTA
Binauraliser as an A/B reference, a DAW with 32-sample buffers (Logic or Pro Tools) for host tests.

---

## A. Fix the existing design

### A1. Make `process()` safe for any block length — fixes F1 (crash) — effort: ½ day

**Change.** Feed input through a 64-sample staging FIFO; run the sofar renderer whenever 64 samples
have accumulated; read output from a matching FIFO. Report `context.set_latency_samples(64)` in
`initialize()`. Replace the `unwrap()` at `lib.rs:210` with: zero the output, return
`ProcessStatus::Error(..)`.

**What breaks.** Adds 64 samples (1.3 ms at 48 kHz) of reported latency; hosts compensate. Keep
`SAMPLE_ACCURATE_AUTOMATION = true` — it becomes safe.

**Verify.** In the harness, process the same signal (a) in 64-sample blocks and (b) in random block
lengths 0…1024 including 1 and 33; outputs must be bit-identical after the 64-sample shift. Host test:
32-sample buffer + automation ramp on every parameter for a minute; count crashes (must be 0).

### A2. NaN/inf guard and `reset()` — fixes F2, F7 — effort: 2 hours

**Change.** Before the downmix, replace non-finite samples with 0 (one branch per sample, or
`f32::is_finite` on the block and zero the block). Implement `Plugin::reset` to call
`self.render.reset()` and reset the parameter smoothers. Return `ProcessStatus::Tail(filter_len)`.

**What breaks.** Nothing.

**Verify.** Harness §E NaN case returns `Ok` with finite output on the poisoned block and after;
harness §H tail energy after `reset()` = 0. Host test: stop/start transport mid-note; record and check
there is no stale burst.

### A3. Replace the parameter model with azimuth / elevation (/ distance) — fixes F4 — effort: 1 day

**Change.** Parameters: `azimuth` −180…180° (positive = left, SOFA convention, or state the opposite
explicitly in the label), `elevation` −90…90°, optional `distance` 0.2…10 m. Compute the unit vector
from the angles and scale it to `sofa.lookup().radius_max` before calling `sofa.filter` (this alone
also fixes the interpolation radius, A4a). Use the smoothers: read `smoothed.next_block()` or one
value per 64-sample hop, and rebuild the filter only when the smoothed position moved (fixes F13).

**What breaks.** Parameter IDs `FrontBack`/`LeftRight`/`UpDown` and their meaning. Old sessions load
with wrong positions. There are no external users, so change the IDs and bump the version; if you want
old sessions to keep working, keep the IDs and map old values in a state-migration hook instead
(more work, not recommended).

**Verify.** Harness §C table: default (0°, 0°) must select measurement #44 (front); az 90° → #4049;
az 270° → #12059; every 1° step must move the nearest measurement monotonically. Smoothing check:
automate azimuth 0→90° over 20 ms and confirm the filter index advances every hop rather than once per
host block.

### A4. Interpolation that actually interpolates — fixes F5, F9 — effort: (a) 10 minutes, (b) 2–4 days

**(a) Immediate.** Scale the query to the measurement radius (part of A3). Optionally open an upstream
PR so `Sofar::filter` uses `find_mut` (sofar `lookup.rs:129-132`), which fixes it for everyone.

**(b) Proper.** Implement interpolation in the plugin (or a fork of sofar's `interpolate`):
1. At load, for each measurement estimate the ITD/onset (cross-correlation of 750 Hz low-passed
   ears, as SAF does, or onset detection) and store time-aligned HRIRs plus a per-ear delay.
2. For a query direction find the enclosing spherical triangle (for this regular 2° × Gauss-Legendre
   grid, bilinear in azimuth × elevation is exact and needs no triangulation; keep a convex-hull
   triangulation path for arbitrary SOFA sets) and compute barycentric/VBAP weights.
3. Interpolate the time-aligned HRIRs (or their magnitudes) with those weights, interpolate the
   delays separately, re-insert them with a fractional delay (a Lagrange or Thiran fractional delay
   per ear, or sub-sample shift in the frequency domain).
This is the SAF / Steam Audio / SUpDEq approach in RESEARCH Part 3.

**What breaks.** The filter for a given position changes slightly (better). Load time increases by the
ITD estimation (16 020 × 2 cross-correlations; estimate 50–100 ms). Memory unchanged.

**Verify.** (1) Hold-out test: for 500 random grid points, interpolate from their neighbours and
compare with the measured HRTF: log-spectral distortion in 1/3-octave bands (target < 1 dB below
8 kHz) and ITD error (target < 20 µs); the current IDW at r = 1 is the baseline. (2) Harness §D sweep:
consecutive-step L2 must be small and smooth with no zero runs. (3) Localization models from AMT on
the interpolated set vs. the measured set. (4) A/B against SPARTA Binauraliser with the same SOFA.

### A5. Click-free filter updates — fixes F6 — effort: 1 day

**Change.** Keep two renderers; when the filter changes, render the hop with both and crossfade the
outputs over the hop (64 samples) or over 128–256 samples for narrowband material. Alternatively
crossfade in the partitioned-spectrum domain (BEAR crossfades over one period; csound `hrtfmove` uses
one 128-sample buffer).

**What breaks.** Convolution cost doubles while a crossfade is active (+3.6 µs per 512 samples).

**Verify.** Harness §G: max sample-step ratio at a 90° jump must fall from +4.6 dB to under +0.5 dB.
Spectrogram of pink noise with fast azimuth automation shows no broadband transients at block
boundaries. Listen with a 1 kHz sine while sweeping.

### A6. Zero allocations on the audio thread — fixes F12 — effort: 1–2 days

**Change.** Preallocate in `initialize()`: the mono buffer, L/R buffers (sized to
`buffer_config.max_buffer_size`), the `Filter`, and any FIFO from A1. The remaining allocations are
inside `sofar::interpolate` (two `Vec`s). Options in order of preference: (1) upstream a
`filter_into(&mut Filter, &mut Scratch)` API; (2) vendor `interpolate` into the plugin (it is 250
lines) — A4b needs its own implementation anyway; (3) run HRIR lookup on a worker thread and pass the
`Filter` with a triple buffer (the sofar example's `Mutex` is not RT-safe).

**What breaks.** Nothing user-visible.

**Verify.** Debug bundle with `assert_process_allocs` (already enabled) survives a full harness pass
and a DAW session; harness allocation counter reads 0 per block; worst-case block time within 1.5× of
mean.

### A7. Load once, load safely, load anything — fixes F3, F8 — effort: 1–2 days

**Change.** (1) Drop the `Default` load: make `sofa` an `Option` or share a
`static OnceLock<Arc<Sofar>>` per sample rate so all instances share one parsed set (saves 18.9 MB
and 640 ms per extra instance). (2) Return `false` from `initialize()` on load failure instead of
panicking; show the error in the GUI. (3) Locate the SOFA relative to the bundle (ship it inside the
`.vst3`/`.clap` bundle's resources) or `include_bytes!` it (19.7 MB in the binary, `Sofar::open_data`
already accepts a byte slice), and add a persisted "custom SOFA path" setting. (4) Validate
`SimpleFreeFieldHRIR`, R = 2, one radius or handle several, and warn on `Data.Delay` ≠ 0 (A4b consumes
it).

**What breaks.** Build/bundling script changes; the first instance still pays ~650 ms.

**Verify.** Instantiate 8 instances: total load time ≈ one load; resident memory grows by < 1 MB per
extra instance. Copy the bundle to a clean user account and load it in a DAW.

### A8. Metadata, packaging, tests — fixes F14, F15 — effort: ½ day

`Vst3SubCategory::Spatial`, real URL/vendor/e-mail, drop `ClapFeature::Mono` or add a mono layout,
move unused deps to `[dev-dependencies]` or delete them (`hrtf`, `criterion`, `cpal`, `ringbuf`,
`parking_lot`, `rand`, `assert_approx_eq`, `anyhow`, `hound`, `realfft` unless A4b uses it), delete
or wire the dead modules, turn `examples/audit_bench.rs` sections into `#[test]`s with thresholds so
regressions in A1–A6 fail CI. Verify: `cargo test` runs and fails when a threshold is violated.

### Features on top of the existing design (after A1–A6)

**A9. Distance model** — effort 1–2 days. 1/r gain (clamped near 0.2 m), air-absorption low-pass
(3-band shelves are enough), optional Spagnol-2017 near-field shelving for r < 1 m, direct/reverb
ratio if A11 exists. Verify: gain vs. distance curve, spectral tilt vs. distance against the ISO 9613
absorption table at 20 °C.

**A10. Stereo input as two sources** — effort 1 day. Render L and R at ±width around the azimuth
(two renderers, 2× cost) instead of summing to mono. Verify: a hard-panned stereo test signal keeps
its width; mono-compatible signals match the mono path.

**A11. Small colorless FDN reverb** — effort 3–5 days. 4–8 delay lines, orthogonal matrix and gains
optimized offline with FLAMO (script in repo), one-pole or GEQ attenuation per line, two decorrelated
outputs, level tied to distance. Verify: echo density and spectral flatness of the impulse response
(FLAMO's own metrics), RT60 vs. the target, CPU < 0.5 % of a core, A/B against IEM FdnReverb.

**A12. Head tracking** — effort 2–3 days. Listener yaw/pitch/roll from OSC on a background thread
(nih-plug `BackgroundTask` or a small UDP thread) or from three automatable parameters; rotate the
source direction before lookup. Verify: with a sine at azimuth 0° and yaw 90° the output equals the
untracked render at azimuth −90°; latency of tracker → audio < 20 ms.

**A13. GUI** — effort 1–2 weeks. `nih_plug_egui` or `nih_plug_vizia`: azimuth/elevation pad, distance
slider, SOFA picker, level meters. Not a DSP change; sequence after A3 so the controls are angles.

---

## B. Replace the approach (not recommended now; conditions under which each becomes worth it)

### B1. Filterbank / frequency-domain renderer with magnitude+ITD interpolation (SAF-style)

**What it buys.** Interpolation in the domain where it is done anyway; cheap per-band smoothing; head
tracking and many sources at flat cost. **Why not now.** A4b gets the same interpolation quality by
producing a time-domain HRIR per update and feeding sofar; SAF's filterbank costs 1536 samples of
latency, which is worse than the current 0. **Would justify it:** > ~50 simultaneous sources or a
scene-level product. Effort 2–3 weeks; verify by A/B against SPARTA with identical SOFA.

### B2. NIIRF-style IIR rendering (per-ear shelf + 8–32 peaking biquads + fractional ITD)

**What it buys.** ~50× lower cost than FIR (irrelevant here), no partition constraint (A1 becomes
unnecessary), coefficients that vary continuously with direction so per-sample smoothing replaces
crossfades, and a natural home for neural personalization (RESEARCH 2.1). **Why not now.** Research
grade: magnitude-only modelling plus a separate ITD, no published perceptual validation of the K = 8
setting against convolution, and it requires an offline fit per dataset with the released code. The
current FIR path is exact by construction. **Would justify it:** if personalization becomes the
product and you want the plugin to consume a small learned model instead of a 20 MB SOFA.
**Verify:** LSD vs. the measured set, AMT localization models, ABX vs. convolution. Effort 3–4 weeks.

### B3. Ambisonic scene pipeline with MagLS decoding

**What it buys.** Flat cost for many sources, SH-domain head rotation, drop-in use of IEM/SAF
decoders and their KU100 MagLS sets. **Why not now.** For one source it is strictly lower quality
than direct convolution at any order ≤ 7 and no cheaper (RESEARCH 2.2). **Would justify it:** a
multi-source or ambient-bed product. Effort 2 weeks.

### B4. Swap sofar for another engine (`hrtf` crate, own convolver)

**Why not.** sofar 0.3 is pure Rust, MIT/Apache, and its convolver is exact; its three limitations
(block multiples, allocations in `filter`, IDW) are all addressed by A1, A4b and A6 without leaving the
crate, and two of them are one-line upstream fixes (`find_mut`, a scratch-buffer API). The `hrtf`
crate already in `Cargo.toml` offers barycentric spectral interpolation, but replacing the engine
would re-open every verified property above for no measured gain. **Would justify it:** upstream
refuses the fixes and A4b/A6 end up re-implementing most of the crate anyway.

---

## Suggested order and checkpoints

1. A2, A1, A8-tests (one afternoon): the plugin stops crashing; harness sections E/H become tests.
2. A3 + A4a (one day): the knobs mean what they say; interpolation stops stepping.
3. A5, A6 (two to three days): clean motion, zero audio-thread allocations, debug builds run.
4. A7 (one to two days): shareable bundle, one load per process, custom SOFA.
5. A4b (two to four days): real interpolation; re-run the hold-out LSD/ITD test.
6. Features A9–A13 as desired; revisit B2 only if personalization becomes the goal.
