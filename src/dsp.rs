//! Stereo binaural engine.
//!
//! Each input channel is rendered as its own virtual source, so a stereo track keeps its width:
//! the left channel sits at `azimuth + width / 2`, the right channel at `azimuth - width / 2`
//! (SOFA convention, positive azimuth = listener's left). With `width = 0` both sources coincide
//! and the output equals the old mono-downmix path.
//!
//! Audio runs through a fixed `HOP`-sample FIFO so the convolution always sees whole partitions,
//! whatever block length the host uses (sofar's renderer rejects anything that is not a multiple
//! of its partition length). This costs `HOP` samples of latency, reported to the host.
//!
//! Position changes are applied once per hop and crossfaded: every source owns two renderers fed
//! the same input, one holding the audible filter and one receiving the next filter. When the
//! position moves, the hop is rendered with both and faded from old to new, then the two swap.

use std::f32::consts::PI;

use sofar::reader::{Filter, Sofar};
use sofar::render::Renderer;

/// FIFO hop and convolution partition length, in samples. Also the reported latency.
pub const HOP: usize = 64;

/// Where the stereo image sits. Angles in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    /// −180…180, positive = left.
    pub azimuth: f32,
    /// −90…90, positive = up.
    pub elevation: f32,
    /// Angle between the left and right virtual sources.
    pub width: f32,
}

impl Position {
    /// `(azimuth, elevation)` of the left and right virtual sources, in that order.
    pub fn sources(&self) -> [(f32, f32); 2] {
        let half = 0.5 * self.width;
        [(self.azimuth + half, self.elevation), (self.azimuth - half, self.elevation)]
    }
}

/// One virtual source.
struct Voice {
    /// Holds the filter that is currently audible.
    current: Renderer,
    /// Receives the next filter; faded to and then swapped with `current`.
    next: Renderer,
}

pub struct Engine {
    sofa: Sofar,
    sample_rate: f32,
    /// Measurement radius of the dataset. Queries are placed on this sphere so sofar's
    /// interpolation weights are computed against the actual grid (AUDIT F5).
    radius: f32,
    voices: [Voice; 2],
    filter: Filter,
    applied: Option<Position>,
    input: [[f32; HOP]; 2],
    output: [[f32; HOP]; 2],
    fill: usize,
    /// Raised-cosine fade-in curve used for filter crossfades.
    fade: [f32; HOP],
}

impl Engine {
    pub fn new(sofa: Sofar) -> Result<Self, String> {
        let sample_rate = sofa.sample_rate();
        let filter_len = sofa.filter_len();
        let renderer = || {
            Renderer::builder(filter_len)
                .with_sample_rate(sample_rate)
                .with_partition_len(HOP)
                .build()
                .map_err(|e| e.to_string())
        };
        let voices = [
            Voice { current: renderer()?, next: renderer()? },
            Voice { current: renderer()?, next: renderer()? },
        ];
        let radius = sofa.lookup().radius_max;
        let radius = if radius.is_finite() && radius > 0.0 { radius } else { 1.0 };

        Ok(Self {
            sofa,
            sample_rate,
            radius,
            voices,
            filter: Filter::new(filter_len),
            applied: None,
            input: [[0.0; HOP]; 2],
            output: [[0.0; HOP]; 2],
            fill: 0,
            fade: std::array::from_fn(|i| 0.5 - 0.5 * (PI * (i as f32 + 0.5) / HOP as f32).cos()),
        })
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Clear all audio history. Filters stay loaded.
    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.current.reset();
            voice.next.reset();
        }
        self.input = [[0.0; HOP]; 2];
        self.output = [[0.0; HOP]; 2];
        self.fill = 0;
    }

    /// Render `left`/`right` in place. Output is delayed by [`HOP`] samples.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], position: Position) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            // A non-finite sample would poison the FFT history permanently (AUDIT F2).
            self.input[0][self.fill] = if l.is_finite() { *l } else { 0.0 };
            self.input[1][self.fill] = if r.is_finite() { *r } else { 0.0 };
            *l = self.output[0][self.fill];
            *r = self.output[1][self.fill];

            self.fill += 1;
            if self.fill == HOP {
                self.fill = 0;
                self.render_hop(position);
            }
        }
    }

    fn render_hop(&mut self, position: Position) {
        let changed = self.applied != Some(position);
        if changed {
            for (voice, (azimuth, elevation)) in self.voices.iter_mut().zip(position.sources()) {
                lookup(&self.sofa, self.radius, azimuth, elevation, &mut self.filter);
                // Lengths always match (both sized from `filter_len`), so this cannot fail.
                let _ = voice.next.set_filter(&self.filter);
            }
        }

        let mut current = [[0.0f32; HOP]; 2];
        let mut next = [[0.0f32; HOP]; 2];
        let mut output = [[0.0f32; HOP]; 2];
        let mut failed = false;
        for (voice, input) in self.voices.iter_mut().zip(&self.input) {
            // Both renderers always see the input so either can take over without stale history.
            let [cur_l, cur_r] = &mut current;
            let [next_l, next_r] = &mut next;
            if voice.current.process_block(input, &mut cur_l[..], &mut cur_r[..]).is_err()
                || voice.next.process_block(input, &mut next_l[..], &mut next_r[..]).is_err()
            {
                failed = true;
                break;
            }

            for k in 0..HOP {
                let (l, r) = if changed {
                    let w = self.fade[k];
                    (cur_l[k] + w * (next_l[k] - cur_l[k]), cur_r[k] + w * (next_r[k] - cur_r[k]))
                } else {
                    (cur_l[k], cur_r[k])
                };
                // Half per source keeps width = 0 identical to the old (L + R) / 2 downmix.
                output[0][k] += 0.5 * l;
                output[1][k] += 0.5 * r;
            }

            if changed {
                std::mem::swap(&mut voice.current, &mut voice.next);
            }
        }

        if failed {
            // Only reachable on an internal FFT error; recover with silence instead of aborting.
            self.reset();
            return;
        }
        self.output = output;
        if changed {
            self.applied = Some(position);
        }
    }
}

/// Fill `filter` with the HRIR pair for a direction.
fn lookup(sofa: &Sofar, radius: f32, azimuth: f32, elevation: f32, filter: &mut Filter) {
    let (az, el) = (azimuth.to_radians(), elevation.to_radians());
    let (x, y, z) = (radius * el.cos() * az.cos(), radius * el.cos() * az.sin(), radius * el.sin());
    // sofar's interpolation allocates two small Vecs. This only runs when the position changes,
    // so permit it rather than tripping `assert_process_allocs` in debug builds.
    nih_plug::util::permit_alloc(|| sofa.filter(x, y, z, filter));
}

#[cfg(test)]
mod tests {
    use super::*;
    use sofar::reader::OpenOptions;

    const SOFA_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/SOFA-data/HRIR_FULL2DEG.sofa");

    fn engine() -> Engine {
        let sofa = OpenOptions::new().sample_rate(48_000.0).open(SOFA_PATH).expect("open SOFA");
        Engine::new(sofa).expect("build engine")
    }

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect()
    }

    fn render(engine: &mut Engine, l: &[f32], r: &[f32], pos: Position, block: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (l.to_vec(), r.to_vec());
        for (lc, rc) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
            engine.process(lc, rc, pos);
        }
        (l, r)
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|s| s * s).sum()
    }

    const FRONT: Position = Position { azimuth: 0.0, elevation: 0.0, width: 60.0 };

    #[test]
    fn hard_left_input_stays_left() {
        // Old engine: a left-only signal was averaged to mono and came out centred (L == R).
        let n = 48_000;
        let sig = noise(n, 1);
        let silence = vec![0.0; n];
        let (l, r) = render(&mut engine(), &sig, &silence, FRONT, 512);
        let ratio_db = 10.0 * (energy(&l) / energy(&r)).log10();
        assert!(ratio_db > 3.0, "left-only input should stay on the left, got {ratio_db:.1} dB L/R");

        let (l, r) = render(&mut engine(), &silence, &sig, FRONT, 512);
        let ratio_db = 10.0 * (energy(&l) / energy(&r)).log10();
        assert!(ratio_db < -3.0, "right-only input should stay on the right, got {ratio_db:.1} dB L/R");
    }

    #[test]
    fn zero_width_matches_mono_downmix() {
        let n = 8192;
        let (a, b) = (noise(n, 2), noise(n, 3));
        let pos = Position { azimuth: 40.0, elevation: 10.0, width: 0.0 };
        let (l, r) = render(&mut engine(), &a, &b, pos, 256);
        let mono: Vec<f32> = a.iter().zip(&b).map(|(x, y)| 0.5 * (x + y)).collect();
        let (ml, mr) = render(&mut engine(), &mono, &mono, pos, 256);
        for i in 0..n {
            assert!((l[i] - ml[i]).abs() < 1e-5 && (r[i] - mr[i]).abs() < 1e-5, "sample {i} differs");
        }
    }

    #[test]
    fn any_block_length_is_accepted_and_bit_identical() {
        // The old process() aborted the host on any block that was not a multiple of 64.
        let n = 10_000;
        let (a, b) = (noise(n, 4), noise(n, 5));
        let (rl, rr) = render(&mut engine(), &a, &b, FRONT, 64);
        let mut e = engine();
        let (mut l, mut r) = (a.clone(), b.clone());
        let mut off = 0;
        for len in [1usize, 33, 7, 100, 1, 511, 64, 13].iter().cycle() {
            if off >= n {
                break;
            }
            let end = (off + len).min(n);
            e.process(&mut l[off..end], &mut r[off..end], FRONT);
            off = end;
        }
        assert_eq!(l, rl);
        assert_eq!(r, rr);
    }

    #[test]
    fn latency_is_one_hop() {
        let n = 1024;
        let mut imp = vec![0.0; n];
        imp[0] = 1.0;
        let (l, r) = render(&mut engine(), &imp, &imp, FRONT, 128);
        assert!(l[..HOP].iter().chain(&r[..HOP]).all(|s| *s == 0.0));
        assert!(energy(&l[HOP..]) > 0.0 && energy(&r[HOP..]) > 0.0);
    }

    #[test]
    fn non_finite_input_does_not_poison_output() {
        let n = 4096;
        let mut a = noise(n, 6);
        a[100] = f32::NAN;
        a[200] = f32::INFINITY;
        let (l, r) = render(&mut engine(), &a, &a, FRONT, 256);
        assert!(l.iter().chain(&r).all(|s| s.is_finite()));
        assert!(energy(&l[2048..]) > 0.0);
    }

    #[test]
    fn position_jump_is_crossfaded() {
        // 1 kHz sine, jump 0° → 90° mid-stream; the largest sample-to-sample step around the
        // switch must stay close to the steady-state maximum (AUDIT F6 measured +4.6 dB unfaded).
        let sr = 48_000.0;
        let n = 9600;
        let sine: Vec<f32> = (0..n).map(|i| (2.0 * PI * 1000.0 * i as f32 / sr).sin() * 0.5).collect();
        let mut e = engine();
        let a = Position { azimuth: 0.0, elevation: 0.0, width: 0.0 };
        let b = Position { azimuth: 90.0, ..a };
        let (mut l, mut r) = (sine.clone(), sine.clone());
        let half = n / 2;
        for (lc, rc) in l[..half].chunks_mut(256).zip(r[..half].chunks_mut(256)) {
            e.process(lc, rc, a);
        }
        for (lc, rc) in l[half..].chunks_mut(256).zip(r[half..].chunks_mut(256)) {
            e.process(lc, rc, b);
        }
        let max_step = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        for ch in [&l, &r] {
            let steady = max_step(&ch[half + 1024..]).max(max_step(&ch[1024..half]));
            let around = max_step(&ch[half - 256..half + 512]);
            let db = 20.0 * (around / steady).log10();
            assert!(db < 0.5, "switch transient {db:.2} dB above steady state");
        }
    }
}
