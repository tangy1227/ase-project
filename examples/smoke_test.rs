// Offline smoke test for the plugin's runtime DSP path.
//
// This exercises the same `sofar` calls that `Spatializer::process` in src/lib.rs makes —
// opening the bundled SOFA dataset, extracting an HRIR at a position, and running a convolution
// block — without needing a DAW/host. Its main purpose is to confirm the pure-Rust `sofar` 0.3
// parser actually reads `HRIR_FULL2DEG.sofa` and renders audio (the plugin `.unwrap()`s the open,
// so a parser incompatibility would otherwise only surface as a panic at plugin load).
//
// Run: cargo run --example smoke_test

use sofar::reader::{Filter, OpenOptions};
use sofar::render::Renderer;

const SOFA_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/SOFA-data/HRIR_FULL2DEG.sofa");
const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK: usize = 512; // multiple of the renderer partition length (64)

fn main() {
    // 1. Open + parse the dataset — the step most likely to break with the rewritten parser.
    let sofa = OpenOptions::new()
        .sample_rate(SAMPLE_RATE)
        .open(SOFA_PATH)
        .expect("sofar 0.3 failed to open/parse HRIR_FULL2DEG.sofa");
    let filt_len = sofa.filter_len();
    println!("[ok] opened SOFA, filter_len = {filt_len}");
    assert!(filt_len > 0, "filter length was zero");

    // 2. Extract a left-side HRIR (same call shape as lib.rs: three f32 coords).
    let mut filter = Filter::new(filt_len);
    sofa.filter(0.0, -1.0, 0.0, &mut filter);

    // 3. Build the renderer and feed it the filter.
    let mut render = Renderer::builder(filt_len)
        .with_sample_rate(SAMPLE_RATE)
        .with_partition_len(64)
        .build()
        .expect("failed to build renderer");
    render.set_filter(&filter).expect("set_filter failed");

    // 4. Render a block of a test tone through the HRIR.
    let input: Vec<f32> = (0..BLOCK).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    render
        .process_block(&input, &mut left, &mut right)
        .expect("process_block failed");

    // 5. Sanity-check the output: finite and not completely silent.
    let all_finite = left.iter().chain(right.iter()).all(|s| s.is_finite());
    let energy: f32 = left.iter().chain(right.iter()).map(|s| s * s).sum();
    println!("[ok] rendered block: all_finite = {all_finite}, output energy = {energy:.6}");
    assert!(all_finite, "output contained NaN/inf");
    assert!(energy > 0.0, "output was completely silent");

    println!("SMOKE TEST PASSED: sofar 0.3 parses the dataset and renders audio without panicking.");
}
