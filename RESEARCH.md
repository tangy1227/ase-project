# RESEARCH — what from 2024–2026 applies to this plugin, and how it compares to reference renderers

Date: 2026-09-12. Companion to AUDIT.md (findings F1–F15) and ROADMAP.md.

Frame of reference for "my architecture": one mono source, direct convolution with a dense (2°)
KU100 HRIR set via 64-sample partitioned overlap-save, ~4–11 µs per block, no room, no distance, no
head tracking. Budget assumption: under 5 % of one core per instance at 64-sample blocks, latency of
a few ms acceptable.

Evidence labels used below: **(a)** published with code I could locate; **(b)** published, no code
located; **(c)** claim to treat with skepticism. "Verified" means I read the paper body, README or
source; "abstract only" means I could not get past the abstract.

---

## Part 2.1 — Neural HRTF personalization and upsampling

**First, the fit to this project.** The bundled set already has 16 020 directions at 2°. Spatial
*upsampling* solves a problem this plugin does not have. The only reason to touch this literature is
*personalization* (replacing the KU100 with a listener-specific set), and every method below produces
its output offline. Nothing here runs in the audio callback; what changes in the code is that the
plugin must be able to load an arbitrary SOFA file at runtime (AUDIT F3) and handle sets that are
sparser than 2° well (AUDIT F5, F9). The personalization pipeline itself is a Python tool.

| Method | What it is | Status | What it would change here |
|---|---|---|---|
| **NIIRF** — Masuyama et al., ICASSP 2024, arXiv 2402.17907 | Neural field: MLP (4 × 512, GeLU on random Fourier features of direction) outputs per-ear cascades of a low-shelf, K = 8 or 32 peaking filters and a high-shelf. Trained on CIPIC (45 subj.) and HUTUBS (94 subj.); personalization by LoRA fine-tuning on a few directions. Matches magnitude-NF baselines, better when measurements are sparse. Verified (paper body). Code: github.com/merlresearch/neural-IIR-field | (a) | Offline: fit a personal set from 3–19 own measurements. Also the only method here with a *runtime* angle: K = 8 biquads per ear plus an ITD is ~50× cheaper than FIR convolution and continuous in direction (see ROADMAP R2). Inference cost not reported. |
| **RANF** — Masuyama et al., ICASSP 2025, arXiv 2501.13017 | Retrieval-augmented neural field: retrieves the closest subject from SONICOM, feeds its HRTF at the target direction into the field. Winner of LAP Challenge 2024 Task 2 (3–5 measured directions). Abstract + LAP page verified. Code: github.com/merlresearch/ranf-hrtf | (a) | Offline personalization from very few measurements. Needs the user to measure 3–5 directions, which is the practical blocker. |
| **HRIR-Former** — arXiv 2603.27998 (2026) | 3-layer transformer, D = 256, 4 heads; time-domain 256-tap 48 kHz HRIRs from M ∈ {3, 5, 19, 100} measurements; SONICOM (180 train / 20 val subjects); ITD/ILD auxiliary heads. NMSE < −6.9 dB. Verified (paper body). No code or weights mentioned. | (b) | Same use as RANF; time-domain output means ITD is preserved without a separate step. Cannot be reproduced without code. |
| **GraphNF-SCA** — Hu, Hu, Jiang (Inner Mongolia Univ.), arXiv 2511.10697 (Nov 2025) | GNN encoder–decoder for personalization plus an upsampling module and fine-tuning; claims state of the art. Only the abstract/intro was extractable; datasets, conditioning inputs and metrics not verified; no code located. | (b), leaning (c) | Nothing until code or a reproduction appears. |
| **DDPM personalization** — Politecnico di Milano, arXiv 2501.02871 (Jan 2025) | U-Net diffusion (5 down-blocks, 4-head self-attention), 600 steps, conditioned on 27 anthropometric measurements + direction; HUTUBS leave-one-out. LSD 5.1 dB — *worse* than the SHT baseline it cites (4.74 dB), better than the non-SHT one (6.06 dB). Verified (paper body). No code. | (b) and (c) | Does not beat a classical baseline and costs 600 network passes per HRIR; not a reason to change anything. |
| **HRTFformer** — arXiv 2510.01891, IEEE TMM 2026 | Transformer in the SH domain for individual upsampling with a neighbour-dissimilarity loss. Abstract only; no code located. | (b) | Upsampling only. |
| **SUpDEq / MCA interpolation** — Arend, Pörschmann, Weinzierl, Brinkmann, IEEE/ACM TASLP 31:3783 (2023); toolbox github.com/AudioGroupCologne/SUpDEq (MATLAB, Apache-2) | Classical, non-neural: remove direction-dependent parts with a rigid-sphere model, interpolate in SH, magnitude-correct. 38 measured directions → 2702. Verified (README). | (a) | This is the reference for *how to interpolate* a sparse set correctly; its time-alignment + magnitude-correction is what AUDIT F5's replacement should approximate. |
| Ear-photo / anthropometric conditioning (survey arXiv 2509.00400, Aug 2025) | Survey splits the field into explicit HRTF prediction vs. end-to-end neural binaural rendering. No released end-to-end pipeline that goes from a phone photo to a full SOFA set was found. | (c) for products, (b) for papers | Nothing for now. |
| LAP Challenge 2024 report (TechRxiv 10.36227/techrxiv.173153187) | Task 2 = upsampling from 3/5/19/100 directions with perceptual evaluation. Report PDF returned 403; results not verified beyond the winners list. | — | — |

Compute: all of these are training-time GPU jobs and seconds-to-minutes offline inference; zero
audio-thread cost. Real-time budget: not applicable, except NIIRF's IIR parameterization (ROADMAP R2).

## Part 2.2 — Binaural Ambisonic decoding (MagLS, time alignment, spatial resampling)

**Fit.** This plugin is not an Ambisonic renderer; it convolves one source with its HRIR directly.
MagLS and friends solve the problem of decoding a *band-limited SH scene* to binaural without
high-frequency comb filtering; they cannot make a single source sound better than direct convolution,
because order-N decoding is a spatial low-pass of the very HRTF set you already convolve with. They
become relevant only if the design moves to a scene pipeline (many sources, head rotation in SH).

- **MagLS** — Schörkhuber, Zaunschirm, Höldrich, DAGA 2018; **time alignment** — Zaunschirm et al.,
  JASA 2018; **BiMagLS** — Engel et al., 2021/2022 (fixed cutoff, better transition);
  **phase continuation** — DAGA 2023 (fixes group-delay/ITD errors of plain MagLS);
  **eMagLS / eMagLS2** — Deppisch, Helmholz, Ahrens, I3DA 2021, github.com/thomasdeppisch/eMagLS
  (MATLAB, non-commercial licence) — end-to-end variants for microphone-array signals.
  Status: (a) for eMagLS and the IEM/SAF implementations; the 2023 phase-continuation paper is (b).
- **What reference renderers do:** IEM BinauralDecoder ships MagLS decoders of the same KU100 head for
  orders 1–7 as 236-tap IRs (verified in `BinauralDecoder/Source/PluginProcessor.cpp`); SAF/SPARTA
  AmbiBIN offers LS, spatial resampling, time-alignment and MagLS (verified on the SPARTA docs page);
  Google's Open Binaural Renderer (2025) renders everything through 3rd-order SH with 256-tap direct
  and 8192-tap room filters (verified README).
- **Compare with mine:** cost for one source is a wash (SH encode is 16 MACs/sample at 3rd order; the
  decode is a fixed 32 short convolutions per scene, IEM halves it with mid/side symmetry). Quality
  for one source is *lower* than direct convolution at any order ≤ 7. Where it wins: N sources cost
  N × 16 MACs instead of N convolutions, and head-tracking is a 16 × 16 rotation. My direct path scales
  to ~100 sources before it exceeds 1 ms per 512-sample block on this machine (3.6 µs each), so the
  crossover is far away for a panner plugin.
- **Verdict:** not applicable to the current design; keep as the architecture if the goal becomes a
  scene renderer. What *does* transfer from this literature is the time-alignment idea: interpolate
  ITD separately from the time-aligned magnitude (ROADMAP F4b).

## Part 2.3 — Differentiable FDNs for room rendering

**Fit.** There is no reverb path to replace or augment; this would be an addition (ROADMAP A4).

| Work | What it gives | Cost | Status |
|---|---|---|---|
| **Differentiable colorless FDN** — Dal Santo, Prawda, Schlecht, Välimäki, DAFx 2023 | Optimizes the feedback matrix and I/O gains of an FDN for spectral flatness (removes metallic ringing) | Same as any FDN of the same size | (a) via FLAMO |
| **Tiny colorless FDNs** — Dal Santo et al., EURASIP JASMP 2025, arXiv 2402.11216 | Same objective with as few as 4 delay lines; spectral-flatness loss + parameter-sparsity penalty; subjective test confirms reduced coloration. Verified (abstract + Springer summary). | 4 lines: 16 MACs for the matrix + 4 attenuation filters per sample ≈ 100–200 FLOP/sample ≈ 0.2 % of a core at 48 kHz | (a) via FLAMO |
| **FLAMO** — Dal Santo et al., arXiv 2409.08723; `pip install flamo`, github.com/gdalsanto/flamo | PyTorch library: delays, FDN, biquads, SVF, GEQ, orthogonal mixing matrices, all frequency-sampled and differentiable. Verified (paper body). The optimized quantities (delay lengths, orthogonal matrix, gains, GEQ attenuation per line) map onto an ordinary time-domain FDN; confirm the parallel-GEQ export before relying on it. | Training offline; runtime is a plain FDN | (a) |
| **DiffGFDN** — Das, Dal Santo, Schlecht, Välimäki, Cvetković, IEEE/ACM TASLP (arXiv 2508.06686); github.com/orchidas/DiffGFDN | Grouped FDN (12 lines in 3 groups, 8 octave bands) with an MLP mapping Fourier-encoded source/receiver positions to gains → position- and direction-dependent late reverb for coupled rooms. ≈ 2 700 FLOP/sample. Verified (paper body). | ~5 % of a core per instance at 48 kHz if implemented naively; needs a trained model per room | (a) |
| **Directional FDN** (Alary et al. 2019), **GFDN with frequency-dependent coupling** (Das et al. 2023) | Direction-dependent decay via SH-domain transforms; multi-slope decay | Higher; scene-level | (b) |

- **Compared with convolution reverb:** a 2 s stereo BRIR at 48 kHz is 96 000 taps → ≈ 1 500
  partitions of 64, roughly 100–200 µs per 64-sample block with the sofar renderer's structure
  (extrapolated from 7 ns/sample/partition-pair measured; not benchmarked). A 4–8-line colorless FDN
  is two orders of magnitude cheaper and has no memory of a specific room. For a panner plugin the
  FDN wins; DiffGFDN's position dependence is for 6DoF scenes and is overkill here.
- **What changes in the code:** a `Fdn` struct with N delay lines, an orthogonal N × N matrix, per-line
  one-pole or GEQ attenuation, two decorrelated outputs summed with the direct path with a
  distance-dependent direct/reverb ratio. Parameters come from a FLAMO script checked into the repo.

## Part 2.4 — Real-time neural inference inside plugins

- **anira** — Ackva & Schulz, IEEE (Xplore 10704099) / arXiv 2506.12665; github.com/anira-project/anira
  (C++, LibTorch / ONNX Runtime / TensorFlow Lite backends). Verified (paper body): inference is moved
  off the audio thread to a static thread pool, which adds latency = buffer adaptation +
  ⌈max inference time / host buffer⌉ × host buffer + model latency. Benchmarked models: CNN-29k
  (TCN, 29 669 params), RNN-2k (LSTM-20, ~1.8 k params), HNN-11k (conv + LSTM-36, ~11 k params) at
  64–8192-sample buffers on i9-9980HK, M1 and i7-12800HX. Library overhead alone is
  0.05–0.38 µs/sample against a 20.8 µs/sample budget at 48 kHz. Every engine violated real-time
  rules (RadSan), anira itself did not; LibTorch was fastest for stateful models, ONNX Runtime for
  stateless. Per-model absolute runtimes are in figures I could not extract. Status (a).
- **RTNeural** — Chowdhury, arXiv 2106.03037; github.com/jatinchowdhury18/RTNeural (C++, header-only,
  Eigen/XSIMD/STL backends, compile-time-sized layers). Verified (paper body): beats the PyTorch C++
  API for small Dense/Conv1D/GRU/LSTM layers on an i7-9750H, loses as layers grow; used in shipping
  plugins (Chow Centaur, NAM-style amp models). The 2024 sample-rate-independent RNN paper (arXiv
  2406.06293) runs GRUs with 24–72 hidden units in real time at 96 kHz on an M1 with RTNeural. Status (a).
- **Viable sizes at this block size (64–512):** per-sample recurrent models of 1–30 k parameters run
  inside the callback with a few percent of a core (that is what NAM and the anira models are); anything
  needing a GPU or > 100 k params per sample must be decoupled and pays ≥ 1 buffer of latency.
- **Rust reality check:** anira and RTNeural are C++. From nih-plug you would either FFI them or use a
  Rust engine (`tract`, `candle`, `burn`), none of which is allocation-free per call, so the
  anira-style worker-thread + lock-free hand-off is mandatory. Not verified: whether a maintained Rust
  port of RTNeural exists.
- **Where a network belongs in *this* plugin:** at control rate, not audio rate. A neural field such
  as NIIRF maps direction → filter coefficients; evaluating it every 5–10 ms on a worker thread and
  handing coefficients to the audio thread costs nothing measurable and needs no RT-safe inference
  engine. Audio-rate neural rendering has no benefit over convolution for a static HRIR.

## Part 2.5 — Things that became standard practice since 2024 and are missing here

| Item | What it is | Relevance |
|---|---|---|
| **Eclipsa Audio / IAMF + Open Binaural Renderer** (Google, AOM, 2025; github.com/google/obr, C++, royalty-free licence) | Open container + open renderer: objects/channels/1st–4th-order Ambisonics → 3rd-order SH → binaural with 256-tap direct and 8192-tap room filters; YouTube ingest since 2025. Verified README. | The de-facto open reference for "what a consumer binaural renderer does"; a good A/B target and a source of default room filters. |
| **MPEG-I Immersive Audio, ISO/IEC 23090-4:2025** (published Nov 2025) | 6DoF scene renderer standard (distance, Doppler, directivity, early reflections, occlusion, diffraction, reverb, binaural output). | Defines the feature list a "complete" spatializer has; reference software is member-gated (b). |
| **SONICOM HRTF dataset (2023) and LAP Challenge (2024)**; Meta "SS2" HRTF database (AVAR 2024) | 200+ subjects with anthropometrics; the benchmark for personalization. | Datasets to test any interpolation change against held-out measured directions. |
| **libspatialaudio 0.4** (2025, C++, LGPL-2.1) | Objects → virtual loudspeakers → HOA → head rotation → binaural (libmysofa SOFA loading, MIT KEMAR default). Verified blog post. | Another small, readable reference pipeline. |
| **SOFA AES69-2022** | `SimpleFreeFieldHRIR` remains the convention every renderer above requires; Steam Audio additionally requires `Data.Delay` = 0. | The plugin's loader should validate the same constraints instead of unwrapping. |
| Practices, not papers | Head tracking over OSC (SPARTA, IEM SceneRotator), diffuse-field equalization of HRTF sets (SAF), near-field shelving from Spagnol et al. 2017 (SAF/SPARTA BinauraliserNF), one-period crossfades on filter changes (BEAR, csound `hrtfmove`), magnitude/ITD-separated interpolation (SAF, Steam Audio). | All cheap; all absent here. See ROADMAP. |
| Verification tooling | `pyfar`/`sofar` (Python), AMT toolbox localization models (may2011, baumgartner2014), Binaspect (arXiv 2510.25714, Python, CC-BY) for ITD/ILD azimuth maps, SPARTA Binauraliser as an A/B reference. | Used in ROADMAP "how to verify". |

## Summary table

| Technique | Benefit for this plugin | CPU cost | Integration effort | Maturity |
|---|---|---|---|---|
| Fix query radius + barycentric/magnitude-ITD interpolation (SAF-style) | Smooth motion, correct cues, works for sparse SOFA sets | +1–2 µs/block | 2–4 days | Standard practice |
| Output crossfade on filter change | Removes clicks/zipper | 2× convolution (≈ +4 µs/512) | 1 day | Standard practice |
| Neural personalization (NIIRF/RANF, offline) | Listener-specific HRTF | 0 in plugin | 1–2 weeks incl. tooling; needs user measurements | Published + code, research-grade UX |
| NIIRF-style IIR rendering at runtime | Continuous direction, no partition constraint, ~50× cheaper | ~40 biquads/sample | 2–3 weeks + validation | Research |
| MagLS Ambisonic pipeline | Only for many sources / SH head tracking | Flat per scene | 1–2 weeks | Mature (IEM/SAF) |
| Tiny colorless FDN (FLAMO-optimized) | Externalization/room cue at negligible cost | ≈ 0.2 % core | 3–5 days | Published + code |
| DiffGFDN | Position-dependent late reverb | ≈ 5 % core | Weeks + room data | Published + code, research |
| anira/RTNeural audio-rate nets | None for a static HRIR | 2–10 % core | FFI from Rust | Mature in C++ |
| Distance model (1/r, air absorption, DVF near-field) | Basic realism | < 1 µs/block | 1–2 days | Standard practice |
| Head tracking (OSC yaw/pitch/roll) | Externalization, VR use | negligible | 2–3 days | Standard practice |

---

## Part 3 — Comparison with reference implementations

Sources: SAF/SPARTA docs and `binauraliser.c`/`binauraliser_internal.h` (verified); IEM
`BinauralDecoder/Source/PluginProcessor.cpp` and plugin descriptions (verified); Steam Audio core
guide and `core/src/core/hrtf_database.cpp` (verified), default-HRTF provenance from a Steam
community thread (not verified in source); EBU Tech 3396 PDF text and `ebu/bear` README (verified).

| Aspect | This plugin | SAF / SPARTA Binauraliser | IEM Plugin Suite | Steam Audio | EAR Production Suite / BEAR |
|---|---|---|---|---|---|
| Rendering model | 1 mono source, direct HRIR convolution | Up to 64 (SAF) / 128 (SPARTA) sources, direct HRTF in an afSTFT filterbank | Ambisonic scene: encoders → SH → MagLS binaural decoder (KU100, orders 1–7, 236-tap IRs) | Per-source direct path + Ambisonic reflection convolution + parametric reverb | ADM objects panned to a BS.2051-based virtual loudspeaker set → BRIR convolution; HOA decoded by convolution |
| HRTF interpolation | IDW over ≤ 4 raw HRIRs, time domain, effectively piecewise-constant (AUDIT F5) | Triangular (VBAP-gain) interpolation of magnitude and ITD separately, recombined per band; only recomputed when a source moves | Not applicable (fixed MagLS decoder); no crossfade on order change | Nearest, or bilinear over the 4 closest directions on log-magnitude and unwrapped phase | Not applicable (panning gains + fractional per-ear delays derived from a weighted average of virtual-loudspeaker delays) |
| ITD | Baked in HRIRs; separate delay lines unused | Estimated by cross-correlation after a 750 Hz low-pass; re-inserted as phase | In the decoder design (time alignment) | In the unwrapped phase | Extracted from BRIRs at build time, re-added with Lagrange fractional delays |
| Convolution | 64-sample uniform partitions, 128-pt FFT, zero structural latency | afSTFT, hop 128; reported delay 12 × 128 = 1536 samples | Overlap-add FFT sized to block + 235 | FFT of frameSize + frameSize/4 + numSamples − 1 per frame | Partitioned convolution in VISR; "adds no latency beyond components" |
| Motion smoothness | Hard swap per block (AUDIT F6) | Filterbank-domain gain updates per frame | Gain-smoothed encoders | Bilinear + per-frame updates (nearest mode audibly steps, per their docs) | Crossfade over one period when the BRIR view set changes |
| Distance / near-field | None | BinauraliserNF: Spagnol 2017 shelving DVF | DistanceCompensator (delay/gain); RoomEncoder directivity + Doppler | 1/r attenuation, 3-band air absorption, dipole directivity, occlusion, transmission | Distance via ADM metadata and EAR gain laws |
| Room | None | None in Binauraliser | RoomEncoder: > 200 image-source reflections; FdnReverb | Ambisonic IR convolution (up to 2 s), baked or real-time, plus parametric reverb | Measured BRIRs (BBC listening room, Pike & Romanov 2017), 60 ms raised-cosine window, decorrelated diffuse path |
| Head tracking | None | OSC yaw/pitch/roll | SceneRotator (OSC/MIDI) | Listener orientation per frame | Quaternion listener orientation → BRIR set selection + residual rotation |
| HRTF loading | One baked-path SOFA, `unwrap` | SOFA via own reader; default set built in | Built-in decoders only | SOFA (`SimpleFreeFieldHRIR`, `Data.Delay` = 0) or built-in (CIPIC subject 124 per community sources) | Custom `.tf` data file built from BRIRs |
| Normalization | Frontal L+R energy = 2 (sofar) | Diffuse-field EQ available | Headphone EQ presets (23) | RMS or none, with a max-gain ceiling | Fixed |
| RT safety | 7 allocs/block, 2 abort paths | Allocation-free process, atomics for parameters | JUCE conventions | Reflection simulation on worker threads | Per-period processing, no allocation |

Where this plugin is **behind**: interpolation, motion smoothness, block-length robustness,
distance, room, head tracking, SOFA robustness, RT hygiene. Where it is **fine**: the convolution
core (exact, zero-latency, cheaper than SAF's 1536-sample filterbank path), sample-rate handling, the
dataset. Where it is **unusual**: Cartesian parameters and mono downmix (see AUDIT "unusual choices").
ADM/BS.2127 compliance is irrelevant for a single-source panner; it only matters if the plugin is
meant to consume ADM metadata, which nothing in the design suggests.
