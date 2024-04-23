use sofar::reader::{Filter, OpenOptions, Sofar};
use sofar::render::Renderer;
use hound::{WavReader, WavWriter};

use nih_plug::params::enums::Enum;

// #[derive(Debug, Clone)]
pub struct SpatializerEfx {
    x: f32,
    y: f32,
    z: f32,
    sofa: Sofar,
    sample_rate: f32,
}

#[derive(Debug, Clone, Copy)]
pub enum CoordParam {
    Xcoord,
    Ycoord,
    Zcoord,
}

impl SpatializerEfx {
    pub fn new(x: f32, y: f32, z: f32, sample_rate_hz: f32) -> Self {
        let path = "/Users/Owen/Documents/GitHub/ase-project/SOFA-data/HRIR_FULL2DEG.sofa";
        let sofa = OpenOptions::new()
            .sample_rate(sample_rate_hz)
            .open(path)
            .unwrap();

        SpatializerEfx {
            x,
            y,
            z,
            sofa,
            sample_rate: sample_rate_hz,
        }

    }

    pub fn process(&mut self, buffers: &mut [&mut [f32]]) {
        let sample_rate = self.sample_rate as f32;

        let filt_len = self.sofa.filter_len();
        let mut filter = Filter::new(filt_len);

        // get filter at position
        self.sofa.filter(self.x, self.y, self.z, &mut filter);
        
        let mut left = vec![0.0; buffers[0].len()];
        let mut right = vec![0.0; buffers[0].len()];

        // sofa Renderer
        let mut render = Renderer::builder(filt_len)
            .with_sample_rate(48000.0)
            .with_partition_len(64)
            .build()
            .unwrap();  
        render.set_filter(&filter);

        // convert stereo to mono
        let num_samples = buffers[0].len();
        let mut mono_audio = Vec::with_capacity(num_samples);
        for i in 0..num_samples {
            let sample = (buffers[0][i] + buffers[1][i]) / 2.0;
            mono_audio.push(sample);
        }         

        render.process_block(&mono_audio, &mut left, &mut right).unwrap();
        dbg!(left.len());
    }

    // pub fn set_param(&mut self, param: CoordParam, valueX: f32, valueY: f32, valueZ: f32) -> Result<(), Error> {
    //     match param {
    //         CoordParam::Xcoord => valueX,
    //         CoordParam::Ycoord => valueY,
    //         CoordParam::Zcoord => valueZ,
    //     }
    // }

}

#[cfg(test)]
mod tests{
    use super::*;
    use rand::prelude::*;

    #[test]
    fn test_audio() {
        let x = 1.0; // front-back
        let y = 0.0; // left-right
        let z = 0.0; // up-down
        // let sample_rate_hz = 48000.0;
        let mut reader = WavReader::open("/Users/Owen/Documents/GitHub/ase-project/audio/Melody_mono.wav").unwrap();
        let spec = reader.spec();
        dbg!(spec.sample_rate);

        // Create a new spec with the desired sample rate and channels
        let output_spec = hound::WavSpec {
            channels: 2,                  // Stereo output
            sample_rate: 44100,           // Desired sample rate
            bits_per_sample: spec.bits_per_sample,
            sample_format: spec.sample_format,
        };
        let mut writer = WavWriter::create("output.wav", output_spec).unwrap();        

        // let left_samples = reader.samples::<f32>().len() as usize;
        let input: Vec<f32> = reader.samples::<f32>().take(44100*5).map(Result::unwrap).collect();
        let left_samples = 44100*5;
        // dbg!(input.len());
        
        let sofa = OpenOptions::new()
            .sample_rate(48000.0)
            .open("/Users/Owen/Documents/GitHub/ase-project/SOFA-data/HRIR_FULL2DEG.sofa")
            .unwrap();
        let filt_len = sofa.filter_len();
        let mut filter = Filter::new(filt_len);
        sofa.filter(x, y, z, &mut filter);
        let mut render = Renderer::builder(filt_len)
            .with_sample_rate(48000.0)
            .with_partition_len(11025)
            .build()
            .unwrap();
        render.set_filter(&filter);
        let mut left: Vec<f32> = vec![0.0; left_samples];
        let mut right: Vec<f32> = vec![0.0; left_samples];

        render.process_block(&input, &mut left, &mut right).unwrap();

        // Write interleaved samples
        for i in 0..left.len() {
            writer.write_sample(left[i]).unwrap();
            writer.write_sample(right[i]).unwrap();
        }

        // Finalize the WAV file
        writer.finalize().unwrap();

    }    

    #[test]
    fn test_run() {
        let x = 1.0;
        let y = 0.0;
        let z = 0.0;
        let sample_rate_hz = 48000.0;
        // let input = [
        //     [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0],
        //     [1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0],
        // ];

        let mut input= [[0.0; 1024]; 2];
        // Fill the vector with random values
        let mut rng = thread_rng();
        for row in input.iter_mut() {
            for val in row.iter_mut() {
                *val = rng.gen_range(-1.0..1.0); // Generate random value between -1.0 and 1.0
            }
        }     
        let mut output = input;
        let (buf0, buf1) = output.split_at_mut(1);
        let bufs: &mut[&mut [f32]] = &mut [&mut buf0[0], &mut buf1[0]];       

        let mut efx = SpatializerEfx::new(x, y, z, sample_rate_hz);
        efx.process(bufs)

    }
}