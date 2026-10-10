# UI — audit, framework research, redesign, implemented slice

Date: 2026-09-12. Companion to AUDIT.md / RESEARCH.md / ROADMAP.md. Stack as found: nih-plug
`f36931f` (VST3 + CLAP), **no editor on `main`**; the only UI code in the project is the standalone
eframe/egui 0.27 app on the `GUI` branch. The request template left `[MIN SIZE]` blank; I designed
for **400 × 300 pt** upwards and the slice enforces that as its window minimum.

Line references: `lib.rs` = `src/lib.rs`; `GUI:main.rs` = `origin/GUI:src/main.rs` (read with
`git show`); `nih/…` = the nih-plug checkout at `~/.cargo/git/checkouts/nih-plug-*/f36931f/`.

---

## 4a. Audit of the current UI

### 4a.1 `main`: there is no editor

- `impl Plugin for Spatializer` (`lib.rs:103-229`) does not implement `editor()`, so nih-plug returns
  `None` and every host shows its generic parameter list. **Widget tree drawn by this code: none.**
- **Parameters declared vs. exposed.** Declared: `gain`, `FrontBack`, `LeftRight`, `UpDown`
  (`lib.rs:27-34`). Exposed by the generic view: all four, because nih-plug exports every `#[id]`
  field. No hidden or UI-only state exists, so there is no mismatch in either direction; the
  problem is that the exposed *labels* lie (three Cartesian coordinates displayed as "deg", AUDIT F4).
- **Audio→UI data path:** none. No meters, no shared state, therefore nothing that can block the
  audio callback today.
- **Repaint / GUI CPU:** nothing to draw; editor "open" and "closed" both cost 0.
- DPI, host resize, size persistence, open/close cycling, gestures, keyboard: not applicable yet.
  What the *framework* does for a future editor is verified in §4a.3, because that is the behaviour
  any egui editor added to this plugin inherits.

### 4a.2 The `GUI` branch app (`proj` crate, eframe/egui 0.27.2, last commit 2024-04-25)

Widget tree, all inside one `CentralPanel` (`GUI:main.rs:88`):

| Element | Lines | Notes |
|---|---|---|
| Space-bar handler printing pointer position | 91-95 | `ctx.pointer_hover_pos().unwrap()` panics when the pointer is outside the window |
| `install_image_loaders` | 97 | called every frame |
| Three rows: label, `TextEdit::singleline`, "Set X/Y/Z position" button, "Actual" labels | 101-165 | `parse::<f32>().unwrap()` at 110/132/153: typing `1,5` aborts the app |
| "Update all positions" button | 166-183 | same unwraps at 172-174 |
| Two static PNGs (`pos1.png`, `pos2.png`) as the "views" | 185-198 | 400 × 318 fixed |
| File pickers via `rfd` for WAV and SOFA | 199-235 | `pick_file().unwrap()` at 203/229: Cancel aborts the app |
| Output-name label | 237-243 | |
| "Start conversion!" button → `convert_audio` | 244-270 | runs synchronously on the UI thread (260); the chosen file names are ignored, `audio/test.wav` and `audio/test_sofa_1.sofa` are hard-coded (310-311) |
| Two `dot.png` markers painted at pixel positions | 293-298 | positions from hard-coded constants (383-401), so any window size or scale other than 820 × 600 @1× draws them in the wrong place |

- **Parameters exposed vs. declared:** the crate has no parameter struct; x/y/z exist only as
  strings in `MyApp` (`GUI:main.rs:51-65`); gain is absent; values only apply on button press.
- **Data path:** offline file render; no audio thread. The UI freezes for the duration of the render.
- **Repaint strategy:** eframe's default, repaint on input only. Measured (built with
  `CMAKE_POLICY_VERSION_MINIMUM=3.5`, Apple M2): **idle CPU 0.0 %** over six 1-second samples,
  **97 MB resident**.
- **Keyboard:** the position can only be set by typing into three `TextEdit`s. Flagged as a design
  risk (§4a.3 explains why typing is unreliable inside hosts). Nothing is reusable from this branch
  except the intent.

### 4a.3 What an egui editor inherits from the framework (verified in source)

| Concern | nih-plug `f36931f` (`nih_plug_egui`) | nice-plug-egui 0.5.0 |
|---|---|---|
| Repaint | `request_repaint()` unconditionally every frame "for meters" (`nih/nih_plug_egui/src/editor.rs:126-131`): the editor redraws at the baseview frame rate whether or not anything changed | `RepaintNotifier`, a realtime-safe handle the audio thread can poke; no forced redraw (changelog 0.3.0) |
| DPI / scale | `WindowScalePolicy::SystemScaleFactor` unless the host supplied a factor (`editor.rs:84-86`); a scale change while the window is open is refused, with a comment that Ableton Live does exactly that (`editor.rs:154-163`) | "fallback scale factors applied to window creation" plus zoom-factor callbacks (`NiceEguiApp::zoom_factor_changed`) |
| Host-driven resize | Not supported: VST3 `canResize` returns `kResultFalse` and `onSize` carries a TODO (`nih/src/wrapper/vst3/view.rs:427-430, 376-379`); only plugin-initiated resize via `EguiState::set_size` + `request_resize` (`editor.rs:110-124`) | `Editor::resize_hint()` answers CLAP `gui.can_resize` / VST3 `canResize`; Editor API rewritten for this in 0.3.0 |
| Size persistence | `EguiState` is serializable and documented for `#[persist = "…"]` (`nih/nih_plug_egui/src/lib.rs:33-34, 68`) | `EguiEditorState::from_size`, same idea |
| Open/close cycling on X11 | baseview #142 (BadDrawable panic when closing in Reaper/Linux) is **closed**, fixed by baseview PR #209 merged 2025-11-29 together with a GL-context leak. Still open: #116 (reparented X11 windows only get keyboard events while hovered, 2022), #331 (XWayland visibility-tree bug, Aug 2026), #286 (EGL over GLX). nih-plug #129 (Live crashes when opening a second window of the same plugin) — status not verified | same baseview |
| Automation gestures | `ParamSlider` emits `begin_set_parameter` on `drag_started` and `end_set_parameter` on `drag_stopped` (`nih/nih_plug_egui/src/widgets/param_slider.rs:181-184, 214-216, 100-102, 151-153`). Click-to-set and the reset paths call `set_parameter` **without** a begin/end pair (`param_slider.rs:104-113, 128-131, 199-205`), so a single click does not register as a touch for hosts in "touch" automation mode | same widget, same behaviour, unless you write your own (the slice does) |
| Keyboard / text entry | nih-plug #105 "No keyboard input for TextEdit widget" is **open**; the changelog notes VST3 `IPlugView` key events are now ignored so that "a couple more keyboard events" reach baseview; `ParamSlider`'s Alt-click text entry is commented out (`param_slider.rs:193-198`) but its `TextEdit` path still exists (`252-275`) | ships a `keyboard_focus_workaround` feature described as "a temporary workaround for keyboard input not working sometimes" — i.e. still not solved, only patched |
| Accessibility | baseview #200 "add screen reader support via AccessKit" open since 2024-11-22 with no activity; nih-plug #174 documents that egui/vizia accessibility works under winit but not baseview (Windows `WM_GETOBJECT` is ignored). **Still true**: no Rust plugin GUI is screen-reader accessible inside a DAW today, whichever toolkit you pick | same |

Conclusion for the design: no text fields anywhere, every parameter change wrapped in
begin/end gestures, plugin-initiated resize only (with host resize when on nice-plug), and no
reliance on repaint-per-frame for correctness.

---

## 4b. Rust plugin GUI landscape (verified September 2026)

### Framework

| | nih-plug (`robbert-vdh/nih-plug`) | nice-plug (`codeberg.org/RustAudio/nice-plug`) | truce (`truce-audio/truce`) |
|---|---|---|---|
| Status | Maintenance mode. BillyDM announced a maintained hard fork in issue #265 on 2026-03-29; last upstream commit in this checkout 2026-05-10 ("Fix doc generation") | The fork, now a RustAudio community project and "the recommended toolkit". Releases 0.1.0 (2026-06-04) → 0.4.1 (2026-09-10), i.e. monthly, with breaking changes each minor. README: "currently experimental … expect some bugs" | Independent (credits nih-plug's API as inspiration). CLAP/VST3 default, VST2/LV2/AU/AAX opt-in, hot reload, GUI via built-in widgets/egui/iced/slint/vizia. "Truce License 1.0" = Apache/MIT plus a commercial rider on redistributing the framework. MSRV 1.92 |
| VST3 binding | `vst3-sys`, **GPL-3.0** (not LGPL): a VST3 build of this plugin is GPL-licensed | `vst3` 0.3 (Micah Johnston), **MIT OR Apache-2.0**, with pre-generated bindings since Steinberg released VST 3 SDK 3.8 under MIT in 2025 | not verified |
| egui adapter | `nih_plug_egui`, egui 0.31 (CHANGELOG), egui-baseview git rev | `nice-plug-egui` 0.5.0 (2026-09-09): egui **0.36.1**, egui-baseview 0.7.0, features `opengl` (default) / `wgpu` / `keyboard_focus_workaround`, ISC | `truce-slint` 6.3.0 (2026-07-18): Slint software renderer blitted through wgpu over baseview; egui/iced/vizia adapters listed in README, versions not checked |
| iced adapter | `nih_plug_iced` pinned to an old iced (README warns) | `nice-plug-iced` 0.4.0: iced 0.14 via iced_baseview 0.5, current | — |
| vizia adapter | `nih_plug_vizia` on a 2024 patched vizia | third-party `vizia-plug` (MIT): vizia git rev `bbf98e6`, but pinned to **nice-plug-core 0.1.4** while nice-plug is at 0.4.x with three breaking releases in between; README builds with `cargo +nightly` | — |
| Slint adapter | none | `nice-plug-slint` listed as WIP; **not on crates.io** (404) | first-party |
| Migration cost for this plugin | — | ~250 lines: crate renames (`nih_plug`→`nice_plug`, `nih_export_*`→`nice_export_*`), `initialize`→`activate` / `InitContext`→`ActivateContext`, a new `Editor` associated type, `ProcessContext::send_event`→`try_send_event` (unused here), MSRV 1.88 (you have 1.96). Half a day including re-bundling. Pin an exact version; expect one more breaking bump per month | Rewrite of the plugin shell; not justified |

### Which toolkit for heavy custom vector drawing

| Toolkit | Custom drawing model | Fit for a spatializer UI |
|---|---|---|
| **egui** | Immediate mode; `Painter` with circles, paths, polygons, text; anti-aliased tessellation, no retained tree. A custom widget is a function; the same function runs under eframe (standalone) and egui-baseview (plugin) unchanged | Best fit. The UI is 90 % custom pads/meters and 10 % stock controls. Styling is `Style`/`Visuals` plus your own token struct (no CSS). Weak spots: no path effects or gradients along strokes, text metrics are basic, every frame re-tessellates (cheap at this size) |
| vizia | Retained, CSS-styled, skia-based canvas: the best-looking strokes/gradients of the four | Good drawing, but the adapter lags nice-plug by three breaking releases, needs nightly per its README, and skia adds a heavy prebuilt dependency. Revisit if vizia-plug catches up |
| iced | Elm architecture; `canvas` widget with tiny-skia/wgpu paths | Fine drawing, current adapter, but the message/update ceremony is heavier for a pad you drag 60 times a second, and there are few plugin examples |
| Slint | Declarative `.slint` markup; custom shapes via `Path` elements; software renderer in-host | Strong for stock-control-heavy UIs; awkward for a dense interactive custom pad; only via truce today |

### Recommendation

**Use egui, and adopt it through nice-plug-egui, migrating the plugin shell to nice-plug at the same
time.** Reasons, all from the tables above: nih-plug is in maintenance mode and its VST3 build is
GPL; nice-plug is where the adapters, host-resize support, `RepaintNotifier`, and the keyboard
workaround live; egui is the only toolkit whose custom-widget code runs identically standalone and
in-host, which is what makes the slice below testable. Cost: half a day for the migration, then
about a day to wire the widget crate into a `NiceEguiApp` (§4d.4). Cost of staying: the widget
crate compiles against egui 0.36 and nih_plug_egui is on 0.31, so staying means pinning the crate to
0.31 and living with forced per-frame repaint and `kResultFalse` on host resize.

### Accessibility

Broken in hosts for every toolkit (baseview #200 open, nih-plug #174). Standalone eframe has
AccessKit. Design consequence: don't promise accessibility for the plugin; do keep every control
operable by pointer alone with visible focus/hover states so keyboard support can be added later.

### Open-source Rust plugin UIs worth reading (2025–2026)

I could not view these visually from here, so this is a reading list, not a ranking:
`fsecada01/bus_channel_strip` (vizia, six-module channel strip, updated 2026-09-11);
`magnetophon/lamb-rs` (vizia + cyma visualizers, waveform/gain-reduction display);
`futureboard/equzx-vst3` and `RinLogs/nv-maxine-vst3` (egui, 2026); `Hornfisk/autokit`
(2D map + step sequencer, 2026); robbert-vdh's own Diopser / Spectral Compressor / Crisp (vizia,
the reference for parameter-widget conventions); `vizia/vizia-plug` + `vizia-plug-book`
(the tutorial); nice-plug's `poly_mod_synth` example (polyphonic modulation UI).

---

## 4c. Redesign

Assumptions: single mono source now, N sources later (ROADMAP A10), distance (A9), head tracking
(A12) and a small FDN room (A11) planned; window resizable by the plugin from 400 × 300 up;
no typed entry; the user is listening, not reading.

### 4c.1 The position problem

Mouse input is 2-D, the parameter is a direction (2 angles) plus a distance. Options considered:

| Option | How az / el / distance are set | Strengths | Weaknesses |
|---|---|---|---|
| **A. Map + elevation strip** (chosen, implemented) | Top-down pad: angle = azimuth, radius = distance. Vertical strip beside it: elevation | Matches the mixer's mental model (Nuendo/Logic/Atmos panners); every source is a unique point; distance and N sources fit the same pad; head-tracking is a rotated head glyph in the same frame; snapping and fine drag are natural | Elevation is a separate control, so "up" is one drag away, not zero |
| B. Hemisphere pad (IEM SpherePanner style) | One disc: angle = azimuth, radius = elevation (centre = zenith); a flip for the lower hemisphere; distance on a slider | Direction-only precision; IEM even switches to a linear-elevation mapping on double-click for accuracy near the horizon | Half the sphere is hidden behind a toggle; distance can't live in the pad; N sources on both hemispheres get confusing |
| C. Equirectangular pad | x = azimuth, y = elevation; distance on a slider | No hidden hemisphere; trivial grid snapping; best for many sources | Seam at ±180°; the poles smear across a row; no "map" intuition; head rotation is a horizontal scroll |
| D. Orbiting 3-D sphere | Drag the dot on a rendered sphere; orbit the camera | Looks impressive | Occlusion, camera state to manage, ambiguous depth; wrong tool for adjusting while listening |

A wins because the plugin's roadmap adds exactly the things the top-down pad already has slots for
(distance rings, N dots, head orientation, a room outline), and because the one thing it does worse
(elevation) is the least-used axis in binaural mixing.

### 4c.2 Layouts

**A. Pad + strip + header (implemented).**

```
┌────────────────────────────────────────────────────────┐
│ Spatializer   [KU100 2°]  [HRTF ▾]  [HT ● off]  [Binaural] │  header: status chips, one click each
├──────────────────────────────┬───────┬─────────────────┤
│         F                    │ +90   │ 1  L  ▮▮▮▮▯      │  right rail: source list
│     ╭───────────╮            │  ┃    │ 2  R  ▮▮▯▯▯      │  (collapses under 480 px width)
│  L  │  2   ●1   │  R         │  ●    │ 3     ▮▯▯▯▯      │
│     │    (◯)    │            │  ┃    │ gain  ─────●──   │
│     ╰───────────╯            │ −90   │ dist  ───●────   │
│         B     30° L +10° 1.00 m      │                  │
└──────────────────────────────┴───────┴─────────────────┘
```

**B. Pad-only compact.** Same pad, elevation on Alt-drag vertical, readout as the only text; for
≤ 400 px. Rejected as the default: modifier-only access to an axis is undiscoverable, but it is
the natural collapsed form of A when the window is narrow.

**C. Two-view.** Top view and side view side by side (Nuendo style), each a pad. Rejected: the
side view duplicates azimuth information, spends half the width on the rarely used axis, and
breaks down for N sources (two dots per source).

### 4c.3 Multi-source

Same pad; each source is a numbered dot in a categorical colour (six-colour palette, token
`palette.sources`), with its own thin meter arc and leader line. The selected source is larger,
gets the thick meter arc and the elevation tick, and is the only one that drags. Clicking a ghost
selects it (the widget reports `selected_other`; the caller owns selection). The strip shows the
others as coloured ticks. Above six sources the right rail is the primary selector and the pad
shows only labels on hover.

### 4c.4 Feedback while mixing

- Level: a meter arc around each dot (0…1 linear, or map dB in the caller).
- Clipping: the dot's outline turns `palette.danger` (source 4 in the demo).
- Out of space: distance clamped to the SOFA radius shows in `palette.warn` (not in the slice;
  the current dataset has one radius).
- Head orientation: the head glyph rotates with tracked yaw and gains a faint ring; the sources
  stay in the world frame so a turning head visibly changes what is "in front".

### 4c.5 Room

When A11 exists: draw the room as a rounded rectangle behind the disc in the same scale, the
listener offset from centre, first-order image sources as hollow dots when "show reflections" is
on. Nothing in the DSP supports this yet, so the slice draws no room.

### 4c.6 Always-visible status (header chips, one line)

HRTF name and grid (`KU100 2°` / custom SOFA file name, click = picker), head-tracking state
(`off` / `OSC :9000 ● live` / `stale`), output mode (`Binaural` now; `Ambisonic 3rd` / `7.1.4`
if ever added). Chips, not menus: state is readable without opening anything.

### 4c.7 Fine adjustment, no typing

Shift = fine (1/10 speed, relative, no jump); Alt/Option = snap (15° azimuth/elevation, 0.25 m);
double-click or Cmd/Ctrl-click = reset to the parameter default; wheel = nudge (Shift for fine);
arrow keys nudge while hovered (works standalone; hosts may eat them, so nothing depends on it).
The readout shows `30° L  +10°  1.00 m` — sign conventions never reach the user.

### 4c.8 Design tokens (`ui/src/tokens.rs`)

Expressed as egui `Visuals` + `Style` plus a `Tokens` value stored in the context; widgets read
`ui.visuals()` and `Tokens::get(ctx)`, never literals.

| Token | Dark | Light |
|---|---|---|
| bg / surface / surface_alt | `#14161A` / `#1C1F24` / `#2C3139` | `#F4F5F7` / `#FFFFFF` / `#D9DDE3` |
| ink / ink_muted | `#E8EAED` / `#8A9099` | `#1B1F24` / `#6B7280` |
| accent / accent_soft | `#FFB454` / same @ 38 % | `#D97706` / same @ 38 % |
| warn / danger | `#F5C451` / `#FF5C5C` | `#B48300` / `#D32F2F` |
| sources[0..6] | amber, sky, lime, pink, violet, teal | darker variants of the same hues |
| space unit | 8 pt (all gaps and paddings are multiples) | |
| radii sm / md / lg | 3 / 6 / 10 pt | |
| strokes hairline / thin / regular / bold | 1 / 1.5 / 2 / 3 pt | |
| type caption / label / body / title | 11 / 12 / 13 / 16 pt, plus a monospace `readout` style | |

Dark and light are separate palettes, not inversions: the light accent is darkened for contrast on
white, and the source hues are re-picked for a light ground.

---

## 4d. Implemented slice

### Files

- `ui/` — new workspace crate `spatializer-ui` (egui 0.36 only; no plugin framework dependency).
  - `src/tokens.rs` — the token set and `Tokens::install(ctx)` / `Tokens::get(ctx)`.
  - `src/position.rs` — `PositionHost` trait, `PositionPad`, `ElevationStrip`, `SourceView`, the
    mapping/formatting helpers and their unit tests.
  - `examples/position_widget.rs` — the standalone runner (eframe, glow backend so it builds
    without wgpu) with a demo host that logs automation gestures, plus `--svg` headless rendering.
- Root `Cargo.toml`: `ui` added to `workspace.members`. The plugin crate itself is untouched.

### Run

```shell
cargo run -p spatializer-ui --release --example position_widget            # window, dark
cargo run -p spatializer-ui --release --example position_widget -- --light
cargo run -p spatializer-ui --release --example position_widget -- --svg out.svg --size 400x300
cargo test -p spatializer-ui                                               # 4 mapping/format tests
```

### Measured

| | |
|---|---|
| Idle CPU, window open, no interaction | 0.0 % (eight 1-s samples, Apple M2) |
| Resident memory | 87 MB |
| Repaint strategy | on input only; `request_repaint_after(33 ms)` is issued only while "Animate meters" is on |
| Unit tests | 4/4 pass (wrap, direction round-trip, snapping, readout formatting) |
| Headless renders | dark/light at 560 × 440 and 400 × 300, zero unsupported shapes |

### What to judge

1. Does the top-down pad read instantly (front is up, L/R labelled, head glyph in the centre)?
2. Drag, then Shift-drag from the same spot: the second must not jump.
3. Alt-drag: the dot should click to 15° and 0.25 m steps.
4. Watch the gesture log: exactly one `begin` per press and one `end` per release, per parameter.
5. Resize the window down to 400 × 300: readout stays readable, ring labels disappear, nothing overlaps.
6. Toggle Light/Dark: both should look designed, not inverted.

### Wiring it into the plugin (done in 0.2.0, `src/editor.rs`)

The plan above assumed a nice-plug migration first. The plugin instead stays on nih-plug and
implements nih-plug's `Editor` trait directly on **egui-baseview 0.7** (the same egui 0.36 as this
crate), because `nih_plug_egui` at the pinned commit is on egui 0.31. That gives the widgets an
unmodified egui 0.36 inside the host, `RepaintNotifier` wake-ups (the audio thread pokes it while
the meter moves; parameter changes from the host poke it too) and no forced per-frame repaint.
Host-driven resize is still unavailable (nih-plug's VST3 wrapper refuses it), so the window is a
fixed 640 × 440 logical px, scaled by the host's content scale factor.

- `ParamHost` implements `PositionHost` over `ParamSetter`: azimuth and elevation map to the new
  parameters (ROADMAP A3 landed with it); distance is not a parameter yet, so the pad runs with
  `distance_enabled(false)` and the readout drops the distance.
- The two ghost dots are the left and right input channels (`azimuth ± width/2`), drawn with
  `select_others(false)` so clicking near one places the source instead of selecting it.
- The right rail has Width and Gain sliders written for the editor (same rules as the pad: Shift
  fine, double-click reset, every change a begin/set/end gesture) and an output level meter.
- `frame.set_key_capture(KeyCapture::IgnoreAll)`: the host keeps its keyboard shortcuts.
- Verified with `cargo test --release -p ase-project editor_smoke -- --ignored` on Windows at
  125 % scaling: the editor fills exactly the frame size reported to the host, and a click on the
  pad reaches the host as one `begin`/`set`/`end` with the expected azimuth.

Known gaps in the slice: no header chips (SOFA/tracking/output are mocked as checkboxes), no
right-rail source list, no room, selection of ghost sources is reported but not acted on, and the
keyboard nudges are best-effort.
