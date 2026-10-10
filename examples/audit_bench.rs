// Audit measurement harness (added 2026-09-12 alongside AUDIT.md).
//
// Replicates the per-block work of `Spatializer::process` in src/lib.rs against the bundled SOFA
// set and prints: load time / memory, dataset facts, the parameter->direction mapping, how sofar's
// interpolation behaves at the radius the plugin queries, per-block CPU and allocation counts,
// NaN handling, structural latency, and the size of the discontinuity when the HRIR is swapped.
//
// Run: cargo run --release --example audit_bench            (uses SOFA-data/HRIR_FULL2DEG.sofa)
//      cargo run --release --example audit_bench -- other.sofa
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use sofar::reader::{Filter, OpenOptions, Sofar};
use sofar::render::Renderer;

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(l.size(), Relaxed);
        let live = LIVE.fetch_add(l.size() as isize, Relaxed) + l.size() as isize;
        PEAK.fetch_max(live, Relaxed);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Relaxed);
        System.dealloc(p, l)
    }
}
#[global_allocator]
static A: Counting = Counting;

#[derive(Clone, Copy)]
struct Snap { allocs: usize, bytes: usize, live: isize, peak: isize }
fn snap() -> Snap {
    Snap { allocs: ALLOCS.load(Relaxed), bytes: BYTES.load(Relaxed), live: LIVE.load(Relaxed), peak: PEAK.load(Relaxed) }
}
fn reset_peak() { PEAK.store(LIVE.load(Relaxed), Relaxed); }
fn mb(b: isize) -> f64 { b as f64 / 1e6 }

fn cart_to_sph(p: [f32; 3]) -> (f32, f32, f32) {
    let [x, y, z] = p;
    let r = (x * x + y * y + z * z).sqrt();
    let el = z.atan2((x * x + y * y).sqrt()).to_degrees();
    let az = (y.atan2(x).to_degrees() + 360.0) % 360.0;
    (az, el, r)
}
fn sph_to_cart(az: f32, el: f32, r: f32) -> [f32; 3] {
    let (a, e) = (az.to_radians(), el.to_radians());
    [a.cos() * e.cos() * r, a.sin() * e.cos() * r, e.sin() * r]
}
fn l2(a: &[f32]) -> f32 { a.iter().map(|v| v * v).sum::<f32>().sqrt() }
fn l2diff(a: &[f32], b: &[f32]) -> f32 { a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>().sqrt() }
fn db(x: f32) -> f32 { 20.0 * x.max(1e-12).log10() }

/// Exact replica of the plugin's per-block work (src/lib.rs:181-221).
fn plugin_block(sofa: &Sofar, render: &mut Renderer, l: &mut [f32], r: &mut [f32], x: f32, y: f32, z: f32, gain: f32) {
    let combined: Vec<f32> = l.iter().zip(r.iter()).map(|(a, b)| (*a + *b) / 2.0).collect();
    let mut filter = Filter::new(sofa.filter_len());
    sofa.filter(x, y, z, &mut filter);
    let _ = render.set_filter(&filter);
    let mut left: Vec<f32> = vec![0.0; combined.len()];
    let mut right: Vec<f32> = vec![0.0; combined.len()];
    render.process_block(&combined, &mut left, &mut right).unwrap();
    l.copy_from_slice(&left);
    r.copy_from_slice(&right);
    for (a, b) in l.iter_mut().zip(r.iter_mut()) { *a *= gain; *b *= gain; }
}

/// lib.rs:192-194 mapping from the three "degree" parameters to sofar coordinates.
fn param_to_xyz(fb: f32, lr: f32, ud: f32) -> [f32; 3] {
    [fb / -180.0 + 0.001, lr / -180.0 + 0.001, ud / -180.0 + 0.001]
}

fn nearest_info(sofa: &Sofar, p: [f32; 3]) -> (usize, f32, f32, f32) {
    let idx = sofa.lookup().find(&p).unwrap();
    let sp = &sofa.hrtf().source_position.values;
    let (az, el, r) = cart_to_sph([sp[idx * 3], sp[idx * 3 + 1], sp[idx * 3 + 2]]);
    (idx, az, el, r)
}

fn raw_ir(sofa: &Sofar, idx: usize) -> (&[f32], &[f32]) {
    let h = sofa.hrtf();
    let n = h.dimensions().n as usize;
    let r = h.dimensions().r as usize;
    let off = idx * r * n;
    (&h.data_ir.values[off..off + n], &h.data_ir.values[off + n..off + 2 * n])
}

fn onset(ir: &[f32], frac: f32) -> (usize, usize) {
    let peak = ir.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
    let pk_idx = ir.iter().position(|v| v.abs() == peak).unwrap_or(0);
    let on = ir.iter().position(|v| v.abs() >= frac * peak).unwrap_or(0);
    (on, pk_idx)
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/SOFA-data/HRIR_FULL2DEG.sofa").to_string());
    println!("=== A. Dataset load (48 kHz, native rate: no resample) ===");
    reset_peak();
    let s0 = snap();
    let t = Instant::now();
    let sofa = OpenOptions::new().sample_rate(48000.0).open(&path).unwrap();
    let dt = t.elapsed();
    let s1 = snap();
    println!("load time            : {:.1} ms", dt.as_secs_f64() * 1e3);
    println!("allocs during load   : {}  ({:.1} MB requested)", s1.allocs - s0.allocs, mb((s1.bytes - s0.bytes) as isize));
    println!("retained after load  : {:.1} MB   peak during load: {:.1} MB", mb(s1.live - s0.live), mb(s1.peak - s0.live));
    let h = sofa.hrtf();
    let d = *h.dimensions();
    println!("dims M={} R={} N={} E={} C={}   sample_rate={}", d.m, d.r, d.n, d.e, d.c, h.sample_rate());
    for k in ["Title", "DatabaseName", "ListenerShortName", "Origin", "Conventions", "SOFAConventions", "RoomType", "DateCreated"] {
        if let Some(v) = h.get_attribute(k) { println!("attr {:<18}: {}", k, v.chars().take(90).collect::<String>()); }
    }
    println!("SourcePosition Type  : {:?} Units {:?}", h.source_position.get_attribute("Type"), h.source_position.get_attribute("Units"));
    let lk = sofa.lookup();
    println!("radius min/max       : {:.3} / {:.3} m ; az {:.1}..{:.1} el {:.1}..{:.1}", lk.radius_min, lk.radius_max, lk.phi_min, lk.phi_max, lk.theta_min, lk.theta_max);
    let dl = &h.data_delay.values;
    let (dmin, dmax) = dl.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    println!("Data.Delay len={} min={} max={}  (M={}, M*R={})", dl.len(), dmin, dmax, d.m, d.m * d.r);

    // Onset / peak statistics over the whole set, plus worst-case gain.
    let n = d.n as usize; let m = d.m as usize;
    let (mut on_min, mut on_max, mut pk_min, mut pk_max) = (usize::MAX, 0, usize::MAX, 0);
    let mut on_hist = vec![0usize; n];
    let mut max_abs = 0f32; let mut max_l1 = 0f32; let mut max_energy = 0f32; let mut min_energy = f32::MAX;
    for i in 0..m {
        let (l, r) = raw_ir(&sofa, i);
        for ir in [l, r] {
            let (on, pk) = onset(ir, 0.1);
            on_min = on_min.min(on); on_max = on_max.max(on); pk_min = pk_min.min(pk); pk_max = pk_max.max(pk);
            on_hist[on] += 1;
            max_abs = max_abs.max(ir.iter().fold(0f32, |a, v| a.max(v.abs())));
            max_l1 = max_l1.max(ir.iter().map(|v| v.abs()).sum());
            let e: f32 = ir.iter().map(|v| v * v).sum();
            max_energy = max_energy.max(e); min_energy = min_energy.min(e);
        }
    }
    let median_on = { let mut acc = 0; let mut med = 0; for (i, c) in on_hist.iter().enumerate() { acc += c; if acc >= m { med = i; break; } } med };
    println!("IR onset(>=10% peak) : min {} / median {} / max {} samples ; peak idx min {} max {}", on_min, median_on, on_max, pk_min, pk_max);
    println!("after normalize      : max|h|={:.3} max L1(worst-case peak gain)={:.2} ({:+.1} dB) ; energy range {:+.1}..{:+.1} dB re frontal(=1)", max_abs, max_l1, db(max_l1), db(min_energy.sqrt()), db(max_energy.sqrt()));
    let (fidx, _, _, _) = nearest_info(&sofa, [3.25, 0.0, 0.0]);
    let (fl, fr) = raw_ir(&sofa, fidx);
    println!("frontal idx {} energy L={:.3} R={:.3}", fidx, fl.iter().map(|v| v * v).sum::<f32>(), fr.iter().map(|v| v * v).sum::<f32>());

    println!("\n=== B. Loads at other host rates (what initialize() does) ===");
    for sr in [44100.0f32, 96000.0] {
        reset_peak(); let s0 = snap(); let t = Instant::now();
        let s = OpenOptions::new().sample_rate(sr).open(&path).unwrap();
        let dt = t.elapsed(); let s1 = snap();
        println!("{:>6} Hz: load {:.0} ms, filter_len {} -> partitions(64) {}, retained {:.1} MB, peak {:.1} MB", sr, dt.as_secs_f64() * 1e3, s.filter_len(), (s.filter_len() + 63) / 64, mb(s1.live - s0.live), mb(s1.peak - s0.live));
    }

    println!("\n=== C. Parameter -> position mapping (lib.rs:192-194) ===");
    for (fb, lr, ud, label) in [(0.0, 0.0, 0.0, "all params at default 0"), (0.0, 180.0, 0.0, "leftright=+180"), (0.0, -180.0, 0.0, "leftright=-180"), (180.0, 0.0, 0.0, "frontback=+180"), (-180.0, 0.0, 0.0, "frontback=-180"), (0.0, 0.0, 180.0, "updown=+180"), (0.0, 0.0, -180.0, "updown=-180"), (-180.0, 180.0, 0.0, "fb=-180, lr=+180"), (0.0, 90.0, 0.0, "leftright=+90"), (0.0, 45.0, 0.0, "leftright=+45"), (-90.0, 90.0, 0.0, "fb=-90, lr=+90")] {
        let p = param_to_xyz(fb, lr, ud);
        let (idx, az, el, r) = nearest_info(&sofa, p);
        let (qaz, qel, qr) = cart_to_sph(p);
        println!("{:<26} xyz=({:+.3},{:+.3},{:+.3}) query az={:6.1} el={:5.1} r={:.3} -> meas #{:<5} az={:6.1} el={:5.1} r={:.2}", label, p[0], p[1], p[2], qaz, qel, qr, idx, az, el, r);
    }

    println!("\n=== D. Interpolation behaviour (sofar IDW over nearest + 6-neighbour set) ===");
    let mut f = Filter::new(sofa.filter_len());
    for (az, el) in [(30.0f32, 0.0f32), (31.0, 0.0), (90.0, 0.0), (45.0, 30.0)] {
        for r in [1.0f32, 3.25] {
            let p = sph_to_cart(az, el, r);
            let (idx, naz, nel, _) = nearest_info(&sofa, p);
            sofa.filter(p[0], p[1], p[2], &mut f);
            let (rl, rr) = raw_ir(&sofa, idx);
            let rel = (l2diff(&f.left, rl) + l2diff(&f.right, rr)) / (l2(rl) + l2(rr));
            println!("query az={:5.1} el={:4.1} r={:.2}: nearest #{} (az {:.1} el {:.1}); rel L2 dist(interp, raw nearest) = {:.3}; ldelay={} rdelay={}", az, el, r, idx, naz, nel, rel, f.ldelay, f.rdelay);
        }
    }
    println!("-- azimuth sweep at el=0, 0.25 deg steps: L2 change of the (left) filter between consecutive steps");
    for r in [1.0f32, 3.25] {
        let mut prev: Option<Vec<f32>> = None; let mut line = String::new();
        let mut steps = 0.0f32;
        while steps <= 6.0 {
            let p = sph_to_cart(steps, 0.0, r);
            sofa.filter(p[0], p[1], p[2], &mut f);
            if let Some(pv) = &prev { line.push_str(&format!("{:.3} ", l2diff(&f.left, pv))); }
            prev = Some(f.left.to_vec()); steps += 0.25;
        }
        println!("r={:.2}: {}", r, line);
    }
    println!("-- raw HRIR L2 distance between adjacent 2 deg measurements at az 30->32 el 0: {:.3} ; 30->90: {:.3}",
        { let (a, _, _, _) = nearest_info(&sofa, sph_to_cart(30.0, 0.0, 3.25)); let (b, _, _, _) = nearest_info(&sofa, sph_to_cart(32.0, 0.0, 3.25)); l2diff(raw_ir(&sofa, a).0, raw_ir(&sofa, b).0) },
        { let (a, _, _, _) = nearest_info(&sofa, sph_to_cart(30.0, 0.0, 3.25)); let (b, _, _, _) = nearest_info(&sofa, sph_to_cart(90.0, 0.0, 3.25)); l2diff(raw_ir(&sofa, a).0, raw_ir(&sofa, b).0) });

    println!("\n=== E. Per-block cost of the exact process() replica @48 kHz (release) ===");
    let sr = 48000.0f32;
    for bs in [32usize, 64, 128, 256, 512, 1024] {
        let mut render = Renderer::builder(sofa.filter_len()).with_sample_rate(sr).with_partition_len(64).build().unwrap();
        let mut l = vec![0.0f32; bs]; let mut r = vec![0.0f32; bs];
        for i in 0..bs { l[i] = (i as f32 * 0.1).sin() * 0.5; r[i] = (i as f32 * 0.13).cos() * 0.5; }
        let (src_l, src_r) = (l.clone(), r.clone());
        if bs % 64 != 0 {
            let combined = vec![0.0f32; bs]; let mut a = vec![0.0f32; bs]; let mut b = vec![0.0f32; bs];
            println!("block {:>4}: process_block -> {:?}  (lib.rs:210 unwraps this => panic/abort)", bs, render.process_block(&combined, &mut a, &mut b).err().map(|e| e.to_string()));
            continue;
        }
        let iters = 3000; let mut az = 0.0f32;
        for _ in 0..200 { l.copy_from_slice(&src_l); r.copy_from_slice(&src_r); plugin_block(&sofa, &mut render, &mut l, &mut r, 1.0, 0.0, 0.0, 1.0); }
        let s0 = snap(); let t = Instant::now(); let mut worst = 0f64;
        for _ in 0..iters {
            let p = sph_to_cart(az, 0.0, 1.0); az += 0.5;
            l.copy_from_slice(&src_l); r.copy_from_slice(&src_r);
            let t1 = Instant::now();
            plugin_block(&sofa, &mut render, &mut l, &mut r, p[0], p[1], p[2], 1.0);
            worst = worst.max(t1.elapsed().as_secs_f64());
        }
        let dt = t.elapsed().as_secs_f64() / iters as f64; let s1 = snap();
        let budget = bs as f64 / sr as f64;
        println!("block {:>4}: mean {:7.1} us  worst {:7.1} us  = {:5.2}% of the {:.2} ms budget ; allocs/block = {} ({} bytes)", bs, dt * 1e6, worst * 1e6, dt / budget * 100.0, budget * 1e3, (s1.allocs - s0.allocs) / iters, (s1.bytes - s0.bytes) / iters);
    }
    // NaN / inf robustness: what lib.rs:208-210 unwraps
    {
        let mut render = Renderer::builder(sofa.filter_len()).with_sample_rate(sr).with_partition_len(64).build().unwrap();
        let mut f = Filter::new(sofa.filter_len()); sofa.filter(1.0, 0.0, 0.0, &mut f); render.set_filter(&f).unwrap();
        for (label, bad) in [("NaN", f32::NAN), ("+inf", f32::INFINITY)] {
            let mut x = vec![0.1f32; 64]; x[5] = bad; let mut a = vec![0.0f32; 64]; let mut b = vec![0.0f32; 64];
            let res = render.process_block(&x, &mut a, &mut b);
            println!("one {} sample in a 64-block: process_block -> {:?}", label, res.err().map(|e| e.to_string()));
            let x = vec![0.1f32; 64];
            let res2 = render.process_block(&x, &mut a, &mut b);
            let out_finite = a.iter().all(|v| v.is_finite());
            println!("   next clean block after it: {:?}, output finite = {} (poisoned FDL history persists until reset)", res2.err().map(|e| e.to_string()), out_finite);
            render.reset();
        }
    }
    // Breakdown at 512
    {
        let bs = 512; let mut render = Renderer::builder(sofa.filter_len()).with_sample_rate(sr).with_partition_len(64).build().unwrap();
        let combined = vec![0.1f32; bs]; let mut a = vec![0.0f32; bs]; let mut b = vec![0.0f32; bs];
        let mut f = Filter::new(sofa.filter_len());
        let iters = 3000;
        let t = Instant::now(); for i in 0..iters { let p = sph_to_cart(i as f32 * 0.5, 0.0, 1.0); sofa.filter(p[0], p[1], p[2], &mut f); } let t_filter = t.elapsed().as_secs_f64() / iters as f64;
        let t = Instant::now(); for _ in 0..iters { let _ = render.set_filter(&f); } let t_set = t.elapsed().as_secs_f64() / iters as f64;
        let t = Instant::now(); for _ in 0..iters { render.process_block(&combined, &mut a, &mut b).unwrap(); } let t_proc = t.elapsed().as_secs_f64() / iters as f64;
        println!("breakdown @512: sofa.filter (kd-tree + IDW) {:.1} us | set_filter (4x rfft128) {:.1} us | process_block (8 sub-blocks) {:.1} us => {:.2} us per output sample of convolution", t_filter * 1e6, t_set * 1e6, t_proc * 1e6, t_proc * 1e6 / bs as f64);
        let s0 = snap(); sofa.filter(1.0, 0.0, 0.0, &mut f); let s1 = snap();
        println!("allocations inside sofa.filter() alone: {} ({} bytes)", s1.allocs - s0.allocs, s1.bytes - s0.bytes);
        let s0 = snap(); let mut _r2 = Renderer::builder(sofa.filter_len()).with_sample_rate(sr).with_partition_len(64).build().unwrap(); let s1 = snap();
        println!("Renderer memory: {:.1} KB in {} allocations", (s1.live - s0.live) as f64 / 1e3, s1.allocs - s0.allocs);
    }

    println!("\n=== F. Latency: impulse through the chain (front source) ===");
    {
        let mut render = Renderer::builder(sofa.filter_len()).with_sample_rate(sr).with_partition_len(64).build().unwrap();
        let p = sph_to_cart(0.0, 0.0, 1.0);
        let (idx, _, _, _) = nearest_info(&sofa, p);
        let mut f = Filter::new(sofa.filter_len()); sofa.filter(p[0], p[1], p[2], &mut f);
        render.set_filter(&f).unwrap();
        let bs = 256; let mut x = vec![0.0f32; bs]; x[0] = 1.0; let mut a = vec![0.0f32; bs]; let mut b = vec![0.0f32; bs];
        render.process_block(&x, &mut a, &mut b).unwrap();
        let (on_out, pk_out) = onset(&a, 0.1); let (on_ir, pk_ir) = onset(raw_ir(&sofa, idx).0, 0.1);
        let max_err = a.iter().zip(f.left.iter()).map(|(o, h)| (o - h).abs()).fold(0f32, f32::max);
        println!("output onset {} / peak {} ; HRIR onset {} / peak {} ; structural latency = {} samples ; max|out - filter| = {:.2e}", on_out, pk_out, on_ir, pk_ir, on_out as isize - on_ir as isize, max_err);
    }

    println!("\n=== G. Discontinuity when the filter is swapped at a block boundary (1 kHz sine, block 256) ===");
    {
        let run = |from: (f32, f32), to: (f32, f32), xfade: bool| -> (f32, f32) {
            let bs = 256usize; let blocks = 12; let switch_at = 8;
            let mut ra = Renderer::builder(sofa.filter_len()).with_sample_rate(sr).with_partition_len(64).build().unwrap();
            let mut rb = ra.clone();
            let mut fa = Filter::new(sofa.filter_len()); let pa = sph_to_cart(from.0, from.1, 1.0); sofa.filter(pa[0], pa[1], pa[2], &mut fa);
            let mut fb = Filter::new(sofa.filter_len()); let pb = sph_to_cart(to.0, to.1, 1.0); sofa.filter(pb[0], pb[1], pb[2], &mut fb);
            ra.set_filter(&fa).unwrap(); rb.set_filter(&fb).unwrap();
            let mut out = Vec::with_capacity(bs * blocks);
            let mut nphase = 0usize;
            for blk in 0..blocks {
                let x: Vec<f32> = (0..bs).map(|i| ((nphase + i) as f32 * 2.0 * std::f32::consts::PI * 1000.0 / sr).sin() * 0.5).collect();
                nphase += bs;
                let mut a = vec![0.0f32; bs]; let mut b = vec![0.0f32; bs]; let mut a2 = vec![0.0f32; bs]; let mut b2 = vec![0.0f32; bs];
                ra.process_block(&x, &mut a, &mut b).unwrap();
                rb.process_block(&x, &mut a2, &mut b2).unwrap();
                if blk < switch_at { out.extend_from_slice(&a); }
                else if blk == switch_at {
                    if xfade { for i in 0..bs { let w = i as f32 / bs as f32; out.push(a[i] * (1.0 - w) + a2[i] * w); } }
                    else { out.extend_from_slice(&a2); }
                } else { out.extend_from_slice(&a2); }
            }
            let steady = out[bs * 2..bs * (switch_at - 1)].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
            let at_switch = out[bs * switch_at - 64..bs * switch_at + 64].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
            (steady, at_switch)
        };
        for (from, to, label) in [((0.0, 0.0), (2.0, 0.0), "az 0 -> 2 deg (one grid step)"), ((0.0, 0.0), (10.0, 0.0), "az 0 -> 10 deg"), ((0.0, 0.0), (90.0, 0.0), "az 0 -> 90 deg"), ((0.0, 0.0), (180.0, 0.0), "az 0 -> 180 deg")] {
            let (steady, sw) = run(from, to, false); let (_, swx) = run(from, to, true);
            println!("{:<32}: max sample step steady {:.4} | at hard swap {:.4} ({:+.1} dB) | with 256-sample crossfade {:.4} ({:+.1} dB)", label, steady, sw, db(sw / steady), swx, db(swx / steady));
        }
    }

    println!("\n=== H. Renderer state across reset ===");
    {
        let mut render = Renderer::builder(sofa.filter_len()).with_sample_rate(sr).with_partition_len(64).build().unwrap();
        let mut f = Filter::new(sofa.filter_len()); sofa.filter(1.0, 0.0, 0.0, &mut f); render.set_filter(&f).unwrap();
        let x = vec![0.5f32; 64]; let z = vec![0.0f32; 64]; let mut a = vec![0.0f32; 64]; let mut b = vec![0.0f32; 64];
        render.process_block(&x, &mut a, &mut b).unwrap();
        render.process_block(&z, &mut a, &mut b).unwrap();
        let tail_no_reset = l2(&a);
        render.process_block(&x, &mut a, &mut b).unwrap(); render.reset();
        render.process_block(&z, &mut a, &mut b).unwrap();
        println!("silence after signal: tail energy without reset() = {:.4} ; after Renderer::reset() = {:.4}  (plugin never calls reset: lib.rs has no reset())", tail_no_reset, l2(&a));
    }
    println!("\nTOTAL process allocations counted: {} ; live now {:.1} MB", ALLOCS.load(Relaxed), mb(LIVE.load(Relaxed)));
}
