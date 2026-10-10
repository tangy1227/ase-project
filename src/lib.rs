use sofar::reader::OpenOptions;

use nih_plug::prelude::*;
use spatializer_ui::{format_azimuth, format_elevation};
use std::sync::Arc;

mod dsp;
mod editor;
#[cfg(all(test, windows))]
mod editor_smoke;

use dsp::{Engine, Position};
use editor::EditorShared;

/// Absolute path to the bundled SOFA HRIR dataset, resolved at compile time relative to the crate
/// root (via `CARGO_MANIFEST_DIR`) so the plugin loads regardless of the host's working directory.
const SOFA_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/SOFA-data/HRIR_FULL2DEG.sofa");

/// Time for the editor's peak meter to fall by 12 dB after the signal stops.
const PEAK_METER_DECAY_MS: f64 = 150.0;

struct Spatializer {
    params: Arc<SpatializerParams>,
    /// Built in `initialize()`; loading the SOFA set takes ~650 ms, so it is not done in
    /// `Default` (which hosts also call while scanning plugins).
    engine: Option<Engine>,
    /// Output peak for the editor's meter, linear gain.
    peak_meter: Arc<AtomicF32>,
    peak_meter_decay_weight: f32,
    editor: Arc<EditorShared>,
}

/// The [`Params`] derive macro gathers all of the information needed for the wrapper to know about
/// the plugin's parameters, persistent serializable fields, and nested parameter groups. You can
/// also easily implement [`Params`] by hand if you want to, for instance, have multiple instances
/// of a parameters struct for multiple identical oscillators/filters/envelopes.
#[derive(Params)]
pub struct SpatializerParams {
    /// The parameter's ID is used to identify the parameter in the wrapped plugin API. As long as
    /// these IDs remain constant, you can rename and reorder these fields as you wish. The
    /// parameters are exposed to the host in the same order they were defined. In this case, this
    /// gain parameter is stored as linear gain while the values are displayed in decibels.
    #[id = "gain"]
    pub gain: FloatParam,
    /// Degrees, positive = left (SOFA convention). Replaces the old `FrontBack`/`LeftRight`/
    /// `UpDown` parameters, which were Cartesian coordinates mislabelled as degrees.
    #[id = "azimuth"]
    pub azimuth: FloatParam,
    /// Degrees, positive = up.
    #[id = "elevation"]
    pub elevation: FloatParam,
    /// Angle between the virtual sources of the left and right input channels.
    #[id = "width"]
    pub width: FloatParam,
}

///================================================================================================///
impl Default for Spatializer {
    fn default() -> Self {
        Self {
            params: Arc::new(SpatializerParams::default()),
            engine: None,
            peak_meter: Arc::new(AtomicF32::new(0.0)),
            peak_meter_decay_weight: 1.0,
            editor: Arc::new(EditorShared::default()),
        }
    }
}

impl Default for SpatializerParams {
    fn default() -> Self {
        Self {
            // This gain is stored as linear gain. NIH-plug comes with useful conversion functions
            // to treat these kinds of parameters as if we were dealing with decibels. Storing this
            // as decibels is easier to work with, but requires a conversion for every sample.
            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-30.0),
                    max: util::db_to_gain(30.0),
                    // This makes the range appear as if it was linear when displaying the values as
                    // decibels
                    factor: FloatRange::gain_skew_factor(-30.0, 30.0),
                },
            )
            // Because the gain parameter is stored as linear gain instead of storing the value as
            // decibels, we need logarithmic smoothing
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            // There are many predefined formatters we can use here. If the gain was stored as
            // decibels instead of as a linear gain value, we could have also used the
            // `.with_step_size(0.1)` function to get internal rounding.
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            // Position changes are crossfaded inside the engine, so these need no smoother (a
            // linear smoother would also take the long way round across ±180°).
            azimuth: FloatParam::new(
                "Azimuth",
                0.0,
                FloatRange::Linear { min: -180.0, max: 180.0 },
            )
            .with_value_to_string(Arc::new(format_azimuth))
            .with_string_to_value(Arc::new(parse_azimuth)),

            elevation: FloatParam::new(
                "Elevation",
                0.0,
                FloatRange::Linear { min: -90.0, max: 90.0 },
            )
            .with_value_to_string(Arc::new(format_elevation))
            .with_string_to_value(Arc::new(parse_elevation)),

            // 60° puts L and R at ±30°, the standard stereo loudspeaker angle.
            width: FloatParam::new("Width", 60.0, FloatRange::Linear { min: 0.0, max: 180.0 })
                .with_unit("°")
                .with_value_to_string(formatters::v2s_f32_rounded(0)),
        }
    }
}

impl SpatializerParams {
    pub fn position(&self) -> Position {
        Position {
            azimuth: self.azimuth.value(),
            elevation: self.elevation.value(),
            width: self.width.value(),
        }
    }
}

/// Accepts what `format_azimuth` prints ("30° L", "30° R", "0° F", "180° B") as well as plain
/// signed numbers in the SOFA convention ("30" = left, "-30" = right).
fn parse_azimuth(s: &str) -> Option<f32> {
    let s = s.trim().to_ascii_uppercase();
    let (number, sign) = match s.chars().last()? {
        'L' | 'F' | 'B' => (&s[..s.len() - 1], 1.0),
        'R' => (&s[..s.len() - 1], -1.0),
        _ => (s.as_str(), 1.0),
    };
    parse_degrees(number).map(|v| sign * v)
}

fn parse_elevation(s: &str) -> Option<f32> {
    parse_degrees(s)
}

fn parse_degrees(s: &str) -> Option<f32> {
    s.trim().trim_end_matches('°').trim().replace('\u{2212}', "-").parse().ok()
}

impl Plugin for Spatializer {
    const NAME: &'static str = "Spatializer";
    const VENDOR: &'static str = "Group 1";
    const URL: &'static str = "https://youtu.be/dQw4w9WgXcQ"; // env!("CARGO_PKG_HOMEPAGE");
    const EMAIL: &'static str = "N/A";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // The first audio IO layout is used as the default. The other layouts may be selected either
    // explicitly or automatically by the host or the user depending on the plugin API/backend.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2), // mono input setting? default: 2
            main_output_channels: NonZeroU32::new(2),

            aux_input_ports: &[],
            aux_output_ports: &[],

            // Individual ports and the layout as a whole can be named here. By default these names
            // are generated as needed. This layout will be called 'Stereo', while the other one is
            // given the name 'Mono' based no the number of input and output channels.
            names: PortNames::const_default(),
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;

    // Safe now that the engine buffers into fixed hops: sub-blocks of any length are fine.
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    // If the plugin can send or receive SysEx messages, it can define a type to wrap around those
    // messages here. The type implements the `SysExMessage` trait, which allows conversion to and
    // from plain byte buffers.
    type SysExMessage = ();
    // More advanced plugins can use this to run expensive background tasks. See the field's
    // documentation for more information. `()` means that the plugin does not have any background
    // tasks.
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.peak_meter.clone(), self.editor.clone())
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        // Match the HRIR set and the convolution renderers to the host's actual sample rate.
        // sofar resamples the impulse responses to this rate on load.
        let sample_rate = buffer_config.sample_rate;

        // Hosts may call this several times in a row (e.g. while restoring state); only reload
        // the dataset when the rate actually changed.
        if self.engine.as_ref().map(Engine::sample_rate) != Some(sample_rate) {
            let engine = OpenOptions::new()
                .sample_rate(sample_rate)
                .open(SOFA_PATH)
                .map_err(|e| e.to_string())
                .and_then(Engine::new);
            match engine {
                Ok(engine) => self.engine = Some(engine),
                Err(err) => {
                    nih_error!("failed to load the HRIR set from {SOFA_PATH}: {err}");
                    self.engine = None;
                    return false;
                }
            }
        }

        context.set_latency_samples(dsp::HOP as u32);
        self.peak_meter_decay_weight = 0.25f64
            .powf((sample_rate as f64 * PEAK_METER_DECAY_MS / 1000.0).recip())
            as f32;

        true
    }

    fn reset(&mut self) {
        if let Some(engine) = self.engine.as_mut() {
            engine.reset();
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let Some(engine) = self.engine.as_mut() else {
            return ProcessStatus::Normal;
        };

        if let [left, right] = buffer.as_slice() {
            engine.process(left, right, self.params.position());
        }

        let editor_open = self.editor.is_open();
        let mut peak = self.peak_meter.load(std::sync::atomic::Ordering::Relaxed);
        for channel_samples in buffer.iter_samples() {
            // Smoothing is optionally built into the parameters themselves
            let gain = self.params.gain.smoothed.next();
            let mut amplitude = 0.0f32;
            for sample in channel_samples {
                *sample *= gain;
                amplitude = amplitude.max(sample.abs());
            }

            if editor_open {
                peak = if amplitude > peak {
                    amplitude
                } else {
                    peak * self.peak_meter_decay_weight
                        + amplitude * (1.0 - self.peak_meter_decay_weight)
                };
            }
        }

        if editor_open {
            let previous = self.peak_meter.swap(peak, std::sync::atomic::Ordering::Relaxed);
            // Only wake the GUI while the meter is visibly moving (-80 dBFS floor).
            if peak > 1e-4 || previous > 1e-4 {
                self.editor.request_repaint();
            }
        }

        ProcessStatus::Normal
    }

    // This can be used for cleaning up special resources like socket connections whenever the
    // plugin is deactivated. Most plugins won't need to do anything here.
    fn deactivate(&mut self) {}
}

impl ClapPlugin for Spatializer {
    const CLAP_ID: &'static str = "edu.gatech.ase-project";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A spatializer plugin");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for Spatializer {
    const VST3_CLASS_ID: [u8; 16] = *b"ASE-Spatial-Plug"; // Have to be 16 characters?
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nih_export_clap!(Spatializer);
nih_export_vst3!(Spatializer);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azimuth_strings_round_trip() {
        for v in [-180.0f32, -90.0, -30.0, 0.0, 30.0, 90.0, 180.0] {
            let parsed = parse_azimuth(&format_azimuth(v)).unwrap();
            assert!((parsed - v).abs() < 1e-3 || (v.abs() == 180.0 && parsed == 180.0), "{v} -> {parsed}");
        }
        assert_eq!(parse_azimuth("-45"), Some(-45.0));
        assert_eq!(parse_azimuth("45 r"), Some(-45.0));
    }

    #[test]
    fn elevation_strings_round_trip() {
        for v in [-90.0f32, -10.0, 0.0, 10.0, 90.0] {
            assert_eq!(parse_elevation(&format_elevation(v)), Some(v));
        }
    }
}
