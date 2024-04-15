use sofar::reader::{OpenOptions, Filter};
use sofar::render::Renderer;

fn main() {

    // Open sofa file, resample HRTF data if needed to 44100
    let sofa = OpenOptions::new()
        .sample_rate(44100.0)
        .open("/Users/Owen/Documents/GitHub/ase-project/SOFA-data/HRIR_FULL2DEG.sofa")
        .unwrap();

    let filt_len = sofa.filter_len();
    let mut filter = Filter::new(filt_len);

    // Get filter at poistion
    sofa.filter(0.0, 1.0, 0.0, &mut filter);

    let mut render = Renderer::builder(filt_len)
        .with_sample_rate(44100.0)
        .with_partition_len(64)
        .build()
        .unwrap();

    let filt_len = sofa.filter_len();
    let mut filter = Filter::new(filt_len);
    
    // Get filter at poistion
    sofa.filter(0.0, 1.0, 0.0, &mut filter);
    
    let mut render = Renderer::builder(filt_len)
        .with_sample_rate(44100.0)
        .with_partition_len(64)
        .build()
        .unwrap();
    
    render.set_filter(&filter);
    
    let input = vec![0.0; 256];
    let mut left = vec![0.0; 256];
    let mut right = vec![0.0; 256];
    
    // read_input()
    
    render.process_block(&input, &mut left, &mut right).unwrap();

}

