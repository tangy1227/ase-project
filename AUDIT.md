# AUDIT — ase-project binaural spatializer (main branch)

Date: 2026-09-12. Scope: `src/lib.rs` (the only compiled source), the `sofar` 0.3.0 crate it delegates
DSP to, and the nih-plug wrapper (`f36931f`) that hosts it. Measurements were taken with
`examples/audit_bench.rs` (release build, Apple M2, 48 kHz unless stated). Nothing in the plugin was
modified for this audit.

Line references use these roots:

| Prefix | Path |
|---|---|
| `lib.rs` | `src/lib.rs` |
| `sofar/…` | `~/.cargo/registry/src/index.crates.io-*/sofar-0.3.0/src/…` |
| `nih/…` | `~/.cargo/git/checkouts/nih-plug-d4210f82e920295f/f36931f/src/…` |

Assumed constraints (the request left them blank): desktop DAW plugin, 44.1–96 kHz, host blocks
anywhere from 32 to 2048 samples, one mono source, target well under 5 % of one core per instance,
added latency of a few ms acceptable. Built Feb–Apr 2024 as a Georgia Tech course project; revived
July 2026.

---

## 1. Signal chain, sample in → sample out

Everything happens in `Spatializer::process` (`lib.rs:174-224`), once per host block:

1. **Entry.** nih-plug hands a stereo `Buffer` (layout fixed at 2-in/2-out, `lib.rs:112-125`). Before
   calling `process`, the wrapper enables flush-to-zero (`nih/wrapper/util.rs:189-201`) and, because
   `SAMPLE_ACCURATE_AUTOMATION = true` (`lib.rs:130`), splits the host buffer into sub-blocks at every
   parameter-change event (`nih/wrapper/clap/wrapper.rs:1973-2030`, `nih/wrapper/vst3/wrapper.rs:1048`).
2. **Downmix.** L and R are averaged into a freshly allocated mono `Vec` (`lib.rs:184-188`).
3. **Parameters → position.** The three "degree" parameters are divided by −180 and offset by 0.001 to
   form a Cartesian point `x, y, z` (`lib.rs:192-194`). No smoothing is applied here; `.value()` is the
   raw target value. See finding F4 for why this mapping is wrong.
4. **HRIR extraction.** `Filter::new` allocates two 128-tap boxes (`lib.rs:196`, `sofar/filter.rs:15-22`);
   `Sofar::filter` (`sofar/reader.rs:283-299`) does a kd-tree nearest lookup with the radius clamped to
   the measurement sphere (`sofar/lookup.rs:115-119,135-151`), then inverse-distance-weights the nearest
   measurement with up to three neighbours, **using the un-clamped query point**
   (`sofar/interpolate.rs:38-167`). The blend is a time-domain average of raw HRIRs.
5. **Filter load.** `Renderer::set_filter` (`sofar/render.rs:261-280`) re-partitions both HRIRs into
   two 64-sample partitions and runs four 128-point real FFTs, every block, whether or not the position
   changed (`lib.rs:204`).
6. **Convolution.** Two more `Vec`s are allocated for L/R (`lib.rs:206-207`) and
   `Renderer::process_block` (`sofar/render.rs:293-332`) runs uniformly partitioned overlap-save
   convolution: 64-sample input hop, 128-point FFT, frequency-domain delay line of 2 partitions,
   one shared forward FFT per hop and one inverse FFT per ear (`sofar/render.rs:384-448`). The result
   is unwrapped with `.unwrap()` (`lib.rs:208-210`).
7. **Gain and exit.** L/R are copied back into the host buffer (`lib.rs:213-214`) and multiplied by the
   log-smoothed gain, per sample (`lib.rs:215-221`). `ProcessStatus::Normal` is returned (`lib.rs:223`).

Nothing else touches the signal: no distance model, no reverb, no EQ, no bypass, no reset.

## 2. Checklist answers (each request bullet, with where the evidence is)

| Question | Answer | Evidence |
|---|---|---|
| Spatialization method | Direct HRIR convolution of one mono point source. No VBAP, no Ambisonics, no decoder. | §1 |
| HRTF source | TH Köln Neumann KU100 "HRIR_FULL2DEG MKII" (SimpleFreeFieldHRIR, 2012), 16020 directions = 180 azimuths × 89 Gauss-Legendre elevations, 128 taps, 48 kHz, fixed 3.25 m radius. Loaded from an absolute build-time path (`lib.rs:9`). | harness §A |
| Interpolation | sofar's libmysofa-style inverse-distance weighting over ≤ 4 points, time domain. Degenerates to piecewise-constant at the radius the plugin queries. | F5 |
| ITD handling | Baked into the HRIRs (`Data.Delay` is `[0, 0]`). sofar's separate delay lines exist but are never instantiated by this plugin (`lib.rs:164-168` → `sofar/render.rs:96-110`). | F9 |
| Convolution | Uniformly partitioned overlap-save, 64-sample partitions, 128-point real FFT via `realfft`/`rustfft`. 2 partitions at 48 kHz (4 at 96 kHz). Exact: max error 3.5e-8 vs. the HRIR. | harness §E, §F |
| Cost per source per block | 3.0 µs (64) to 10.6 µs (1024), ≤ 0.23 % of the block budget. | §4 |
| Distance / near-field / air absorption / directivity | None. The radius of the query point is ignored except for the interpolation artefact in F5. | `lib.rs` has no such code |
| Room / reverb | None. | — |
| Head tracking | None. No listener-orientation input of any kind. | — |
| Position change behaviour | Once per block, hard filter swap, no crossfade; configured smoothers unused. Measured discontinuity up to +4.6 dB above steady state. | F6 |
| RT safety | 7 heap allocations per block; two `unwrap()`s that abort the host on reachable errors; no locks; no file I/O in `process`; no unbounded loops. | F1, F2, F7 |
| Denormals | Handled by nih-plug's `ScopedFtz` around every `process` call (`nih/wrapper/util.rs:191`). | — |
| NaN | Not handled. One non-finite input sample makes sofar return an error → `unwrap` → abort. | F2 |
| Sample-rate independence | Yes: `initialize()` reloads and resamples the whole set at the host rate (`lib.rs:154-168`). Costs ~650 ms per call. | F8 |
| Block-size independence | **No.** `process_block` requires a multiple of 64 samples; anything else is an error → abort. | F1 |
| Bypass / reset | No bypass parameter, no `reset()`; convolution history survives transport stops. | F7 |
| Algorithmic latency | 0 samples structural (overlap-save output aligns with input). The dataset's own onset (median 21 samples = 0.44 ms) is part of the HRIR, not reported latency. | harness §F |
| Memory per instance | 18.9 MB retained at 48 kHz (17.6 MB at 44.1 kHz, 35.3 MB at 96 kHz) + 7 KB renderer; 69 MB peak while loading; loaded twice per instantiation. | §4 |

---

## 3. Findings, ordered by severity

### Severity 0 — aborts the host process

**F1. Any block length that is not a multiple of 64 samples crashes the plugin.**
`Renderer::process_block` rejects lengths that are not multiples of the partition length
(`sofar/render.rs:302-307`); `lib.rs:208-210` unwraps that `Result`. A panic inside the `extern "C"`
process entry point cannot unwind, so the host aborts. Three independent triggers:
- `SAMPLE_ACCURATE_AUTOMATION = true` (`lib.rs:130`; nih-plug default is `false`, `nih/plugin.rs:113`)
  makes both wrappers split the buffer at every parameter-change event
  (`nih/wrapper/clap/wrapper.rs:1996-2030`, `nih/wrapper/vst3/wrapper.rs:1048-1053`). Automating *any*
  parameter, including gain, produces odd-length sub-blocks.
- The CLAP wrapper also splits on every transport event even when sample-accurate automation is off
  (`nih/wrapper/clap/wrapper.rs:2014-2018`), so tempo or loop changes trigger it too.
- Hosts that use 32-sample buffers (Pro Tools and Logic offer 32) or variable block lengths at loop
  and region boundaries.
Measured: a 32-sample block returns
`Input/Output length (32) should be multiple of block len (64)` (harness §E).
This is almost certainly the "buzzing / crash" class of bug from the 2024 commit history.

**F2. A single NaN or ±inf input sample aborts the host, and the state stays poisoned.**
Non-finite input propagates through the forward FFT; `realfft`'s inverse transform then rejects the
spectrum (`sofar/render.rs:438-439` → `Error::InternalProcessingError`), and `lib.rs:210` unwraps it.
The corrupted frequency-domain delay line persists, so even clean blocks keep failing until
`Renderer::reset()` (`sofar/render.rs:335-341`), which the plugin never calls. Measured in harness §E:
both NaN and +inf produce the error on the poisoned block and on the following clean block.

**F3. SOFA loading unwraps on the audio-plugin creation and activation paths.**
`Default::default()` (`lib.rs:42`) and `initialize()` (`lib.rs:157-160`) both `unwrap()` the file
open. The path is baked at build time (`lib.rs:9`), so a bundle copied to another machine, a moved
checkout, or a permissions problem panics during host plugin scanning. (Documented in CLAUDE.md; listed
here because it is the first thing a user outside this checkout will hit.)

### Severity 1 — wrong output

**F4. The three position parameters are Cartesian coordinates mislabelled as degrees.**
`frontback`, `leftright`, `updown` are declared −180…180 with unit " deg" (`lib.rs:75-97`) but are used
as `x = fb/−180 + 0.001`, `y = lr/−180 + 0.001`, `z = ud/−180 + 0.001` (`lib.rs:192-194`). The direction is
`atan2(y, x)`, so the knob values are not angles at all. Measured consequences (harness §C):

| Parameter setting | Rendered direction (nearest measurement) |
|---|---|
| all three at default 0 | azimuth 44°, elevation 36° (front-left-up), not front |
| frontback = −180 | front (0°, 0°) |
| frontback = +180 | back (180°) |
| leftright = +180, +90, or +45 | all identical: hard right (270°) |
| leftright = −180 | hard left (90°) |
| updown = +180 | straight down (−88.5°) |

The magnitude of a single knob never changes the direction, only the (unintended) interpolation blend
in F5. The "+0.001 offset because there is no IR at 0,0,0" comment (`lib.rs:191`) is a symptom: the
origin has no direction. Sign conventions are also inverted relative to SOFA (positive `leftright`
is right, positive `frontback` is back). The configured linear smoothers on these parameters
(`lib.rs:81,89,97`) would, if used, interpolate through the origin and flip direction mid-move.

**F5. HRIR interpolation degenerates to a piecewise-constant blend because the query radius is wrong.**
The measurement grid is at 3.25 m; the plugin's query point has radius ≤ 1.73 m. `Lookup::find`
clamps a *copy* of the point to the sphere for the nearest-neighbour search
(`sofar/lookup.rs:115-119,135-151`), but `Sofar::filter` passes the *original* point to `interpolate`
(`sofar/reader.rs:286-288`). The inverse-distance weights (`sofar/interpolate.rs:82,111-124`) are then
computed from distances of ~2.3 m to every candidate, so they are nearly uniform and do not vary as
the source moves inside a grid cell. Measured (harness §D):

| Query | Relative L2 distance of the returned filter from the raw nearest HRIR |
|---|---|
| az 30°, el 0°, r = 1.0 (what the plugin does) | 0.160 |
| az 30°, el 0°, r = 3.25 (on the sphere, on-grid) | 0.000 (exact-match path) |
| az 31°, el 0°, r = 1.0 | 0.175 |
| az 31°, el 0°, r = 3.25 (between grid points) | 0.199 |

Azimuth sweep in 0.25° steps, L2 change of the left filter between steps:
- r = 1.0: `0 0 0 0 .21 0 0 .26 0 0 0 .23 0 0 0 .39 0 0 0 .23 0 0 0 .41` → jumps only when the
  neighbour set changes, otherwise constant.
- r = 3.25: `.06 .03 .02 .02 .13 .03 .04 .05 .05 .05 .05 .19 …` → varies continuously.

So the plugin gets a fixed ~1/3 average of three adjacent HRIRs that steps every grid cell. Even with
the radius fixed, sofar's scheme is not barycentric: it always mixes in one elevation neighbour with
~18 % weight at el = 0, and it averages raw HRIRs in the time domain with the ITD baked in
(`Data.Delay = [0,0]`, onsets 10–43 samples, harness §A). At 2° spacing the ITD mismatch between
neighbours is ≈1 sample, so the comb-filtering from time-domain averaging is mild here, but it becomes
severe for any sparser SOFA set. `Lookup::find_mut` (`sofar/lookup.rs:129-132`) already exists upstream
and would fix the radius part in one line.

**F6. Position updates are block-rate hard swaps with no crossfade; the smoothers are dead config.**
`process()` reads `.value()` (`lib.rs:192-194`), never `smoothed.next()`, so the 50 ms linear smoothers
on the position parameters (`lib.rs:81,89,97`) are never advanced. `set_filter` overwrites the
partition spectra in place (`sofar/render.rs:271-272`); the next `apply_filter` applies the new
filter to the entire input history (`sofar/render.rs:420-435`), so the output jumps at the block
boundary. Measured on a 1 kHz sine, 256-sample blocks (harness §G; metric is the largest
sample-to-sample step near the swap relative to steady state):

| Jump | Hard swap (current) | With a one-block output crossfade |
|---|---|---|
| 0° → 2° (one grid step) | +0.0 dB | +0.0 dB |
| 0° → 10° | +1.3 dB | +0.2 dB |
| 0° → 90° | +4.6 dB | +0.2 dB |
| 0° → 180° | +2.3 dB | +0.4 dB |

Combined with F5, a slow automation ramp is rendered as a staircase of 2° hard swaps at block rate
(zipper), and a fast one as clicks.

**F7. No `reset()`: convolution history survives transport stop/start.**
`Plugin::reset` is not implemented (`lib.rs:103-229`; nih-plug calls it after `initialize` and on host
resets, `nih/plugin.rs:213-216`). The 128-sample time-domain and frequency-domain delay lines keep the
last audio, so the first block after a restart convolves stale input. Measured tail energy after
silence: 2.77 (L2) without reset, 0.0 after `Renderer::reset()` (harness §H). Also the tail is not
reported: `ProcessStatus::Normal` (`lib.rs:223`) instead of `Tail(filter_len)`
(`nih/wrapper/clap/wrapper.rs:3183`), so a host may suspend the plugin with 128 samples of tail unplayed.

**F8. Every activation re-reads and re-parses the 19.7 MB dataset; instantiation does it twice.**
`Default::default()` loads at 48 kHz (`lib.rs:42`), then `initialize()` loads again at the host rate
(`lib.rs:157-160`) and drops the first copy. nih-plug warns that `initialize` "may be called multiple
times in rapid succession" during state restore (`nih/plugin.rs:199-201`). Measured per load: 639 ms
at 48 kHz, 667 ms at 44.1 kHz (with resampling), 652 ms at 96 kHz; 16 481 allocations totalling
208 MB requested; 69 MB peak resident during parsing (harness §A/§B). Most of the time is
`Neighborhood::new`, which runs up to 6 × 90 kd-tree queries for each of 16 020 measurements
(`sofar/neighbors.rs:56-90,171-211`). No sharing between instances: each instance owns 18.9 MB.

**F9. ITD delay lines are silently disabled.**
`Renderer::builder` is never given `with_left_delay`/`with_right_delay` (`lib.rs:164-168`), so
`Channel.delay` is `None` (`sofar/render.rs:101-107`) and `update_delay` is a no-op
(`sofar/render.rs:121-127`). Harmless for this dataset (delays are zero) but any SOFA set that stores
ITD in `Data.Delay` (minimum-phase sets) would lose its ITD entirely. sofar's delay is also integer-
sample only (`sofar/render.rs:275-277`).

**F10. Stereo input is collapsed to mono before spatialization** (`lib.rs:184-188`). This is a design
choice (single point source), but it discards the L/R difference of any stereo track, and the CLAP
feature list advertises `Mono` (`lib.rs:239`) while only a stereo layout exists (`lib.rs:112-125`).

**F11. Headroom.** After sofar's normalization (frontal L+R energy = 2, `sofar/loudness.rs:92-127`)
the loudest HRIR has an L1 norm of 7.41 (+17.4 dB worst-case peak gain) and energies range from
−12.2 to +4.4 dB relative to frontal (harness §A). Stacking the +30 dB gain range (`lib.rs:59`) on
top means clipping is easy; a clamp or a lower default ceiling is worth considering.

### Severity 2 — real-time hygiene and resources

**F12. Seven heap allocations per block on the audio thread.**
`combined_buffer` (`lib.rs:184-188`), `Filter::new` (two boxes, `lib.rs:196` → `sofar/filter.rs:15-22`),
two `Vec`s inside `interpolate` (`sofar/interpolate.rs:117-118`), and the `left`/`right` outputs
(`lib.rs:206-207`). Measured 7 allocations and 2.8–14.3 KB per block depending on block size
(harness §E). In a debug build the `assert_process_allocs` feature (`Cargo.toml:21`;
`nih/wrapper/util.rs:194-195`) panics on the first block. Worst-case block times were 2–4× the mean
(12.7 µs vs 3.0 µs at 64 samples), consistent with allocator jitter; not a deadline risk at this
load, but it will be once reverb or several sources are added.

**F13. Redundant per-block work.** `sofa.filter` + `set_filter` run every block (`lib.rs:199-204`)
even when the position is unchanged: 1.8 µs + 0.4 µs of the 6.4 µs total at 512 samples. Cheap, but
the filter should only be rebuilt when the smoothed position moves.

**F14. Unused and mis-scoped dependencies.** `hrtf`, `cpal`, `criterion`, `rand`, `ringbuf`,
`parking_lot`, `assert_approx_eq`, `anyhow`, `hound`, `realfft` are in `[dependencies]`
(`Cargo.toml:19-31`) but only examples or dead modules use some of them; `criterion` in particular
belongs in `[dev-dependencies]`. `src/lib_ola.rs` and `src/spatializer_efx.rs` are not in the module
tree, so `cargo test` runs nothing.

### Severity 3 — metadata and packaging

**F15.** `URL` is a YouTube rickroll, `EMAIL` is "N/A", `VENDOR` is "Group 1" (`lib.rs:105-107`).
VST3 subcategory is `Fx|Tools` (`lib.rs:246-247`); nih-plug offers `Vst3SubCategory::Spatial`
(`nih/wrapper/vst3/subcategories.rs:13`). No bypass parameter, so hosts hard-switch around the
plugin (nih-plug only flags a bypass parameter the plugin declares, `nih/wrapper/clap/wrapper.rs:512-523`).

### What is fine

- The convolution core is exact (3.5e-8 error vs. the HRIR), zero-latency, and cheap.
- Sample-rate handling is correct: the set is resampled with rubato's FFT resampler
  (`sofar/resample.rs:31-127`) and the renderer is rebuilt to the new filter length.
- Denormals are handled by the framework; no locks or file I/O occur in `process`.
- The dataset choice (dense 2° KU100 set) is a good one for a non-personalized product; it is the same
  head used by IEM's binaural decoder and BEAR's BRIRs.

### Unusual choices that may be deliberate

- Mono downmix (F10) and Cartesian parameters (F4) read like course-project expedients rather than
  design decisions; both should be replaced.
- `SAMPLE_ACCURATE_AUTOMATION = true` is a reasonable goal, but the DSP cannot currently honour it (F1).
- Keeping the SOFA path build-time-absolute was a portability fix over hard-coded user paths; it is
  still not distributable (F3).

---

## 4. Measurements (release, Apple M2, `cargo run --release --example audit_bench`)

Per-block cost of an exact replica of `process()` at 48 kHz, position sweeping 0.5°/block:

| Block | Mean | Worst | Share of budget | Allocations / block |
|---|---|---|---|---|
| 32 | — | — | error, see F1 | — |
| 64 | 3.0 µs | 12.7 µs | 0.23 % | 7 (2.8 KB) |
| 128 | 3.4 µs | 8.3 µs | 0.13 % | 7 (3.6 KB) |
| 256 | 4.4 µs | 8.9 µs | 0.08 % | 7 (5.1 KB) |
| 512 | 6.4 µs | 12.8 µs | 0.06 % | 7 (8.2 KB) |
| 1024 | 10.6 µs | 19.2 µs | 0.05 % | 7 (14.3 KB) |

Breakdown at 512 samples: HRIR lookup + interpolation 1.8 µs, filter re-partition (4 FFTs) 0.4 µs,
convolution 3.6 µs (≈ 7 ns per output sample for both ears). Renderer state: 7.2 KB in 27 allocations.

Load / memory:

| Host rate | Load time | Filter taps | Partitions | Retained | Peak during load |
|---|---|---|---|---|---|
| 44.1 kHz | 667 ms | 118 | 2 | 17.6 MB | 69.2 MB |
| 48 kHz | 639 ms | 128 | 2 | 18.9 MB | 69.2 MB |
| 96 kHz | 652 ms | 256 | 4 | 35.3 MB | 69.2 MB |

Latency: output onset = HRIR onset (21 samples), peak at sample 25; structural latency 0.

Not measured (would need a host): behaviour under real DAW block splitting, GUI thread interaction.
Not verified by reading: nothing in this document relies on behaviour I could not trace in source,
except the host buffer sizes quoted in F1, which come from general DAW knowledge.
