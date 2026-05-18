use crate::views::evolution::EvolutionMetric;
use crate::views::magnitude::{MagnitudeDisplay, MagnitudeScale};
use crate::views::phase::{PhaseDisplay, PhaseMode};
use crate::views::spectrogram::SpectrogramMetric;
use serde::{Deserialize, Serialize};

/// Per-pane persistent view state. Holds only knobs the user adjusts inside
/// a view; replay is global on `App::replay`, and visualizations are flat 2D
/// so there is no camera/orbit state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViewState {
    /// Console pane: auto-scroll to the most recent line on every push.
    #[serde(default = "default_true")]
    pub console_autoscroll: bool,

    /// I/Q-scatter pane: how many recent packets to overlay.
    #[serde(default = "default_iq_window")]
    pub iq_window: usize,

    /// Spectrogram pane: number of packets along the time axis.
    #[serde(default = "default_spec_window")]
    pub spectrogram_window: usize,
    /// Spectrogram pane: which metric is mapped to the colormap.
    #[serde(default, with = "spec_metric_serde")]
    pub spectrogram_metric: SpectrogramMetric,

    /// Phase pane: window length for traces / mean / heatmap.
    #[serde(default = "default_phase_window")]
    pub phase_window: usize,
    /// Phase pane: raw / unwrapped / detrended mode.
    #[serde(default, with = "phase_mode_serde")]
    pub phase_mode: PhaseMode,
    /// Phase pane: line traces, mean envelope, or heatmap.
    #[serde(default, with = "phase_display_serde")]
    pub phase_display: PhaseDisplay,

    /// Magnitude pane: window length for traces / mean / heatmap.
    #[serde(default = "default_magnitude_window")]
    pub magnitude_window: usize,
    /// Magnitude pane: linear vs dB Y axis.
    #[serde(default, with = "magnitude_scale_serde")]
    pub magnitude_scale: MagnitudeScale,
    /// Magnitude pane: line traces, mean envelope, or heatmap.
    #[serde(default, with = "magnitude_display_serde")]
    pub magnitude_display: MagnitudeDisplay,

    /// Evolution pane: rolling window length (packets).
    #[serde(default = "default_evolution_window")]
    pub evolution_window: usize,
    /// Evolution pane: which scalar to plot vs time.
    #[serde(default, with = "evolution_metric_serde")]
    pub evolution_metric: EvolutionMetric,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            console_autoscroll: true,
            iq_window: default_iq_window(),
            spectrogram_window: default_spec_window(),
            spectrogram_metric: SpectrogramMetric::default(),
            phase_window: default_phase_window(),
            phase_mode: PhaseMode::default(),
            phase_display: PhaseDisplay::default(),
            magnitude_window: default_magnitude_window(),
            magnitude_scale: MagnitudeScale::default(),
            magnitude_display: MagnitudeDisplay::default(),
            evolution_window: default_evolution_window(),
            evolution_metric: EvolutionMetric::default(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_iq_window() -> usize {
    60
}
fn default_spec_window() -> usize {
    150
}
fn default_phase_window() -> usize {
    30
}
fn default_magnitude_window() -> usize {
    30
}
fn default_evolution_window() -> usize {
    300
}

mod spec_metric_serde {
    use super::SpectrogramMetric;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(m: &SpectrogramMetric, s: S) -> Result<S::Ok, S::Error> {
        let tag = match m {
            SpectrogramMetric::PhaseDelta => "phase_delta",
            SpectrogramMetric::Amplitude => "amplitude",
        };
        s.serialize_str(tag)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SpectrogramMetric, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "amplitude" => SpectrogramMetric::Amplitude,
            _ => SpectrogramMetric::PhaseDelta,
        })
    }
}

mod phase_mode_serde {
    use super::PhaseMode;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(m: &PhaseMode, s: S) -> Result<S::Ok, S::Error> {
        let tag = match m {
            PhaseMode::Raw => "raw",
            PhaseMode::Unwrapped => "unwrapped",
            PhaseMode::Detrended => "detrended",
        };
        s.serialize_str(tag)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PhaseMode, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "raw" => PhaseMode::Raw,
            "detrended" => PhaseMode::Detrended,
            _ => PhaseMode::Unwrapped,
        })
    }
}

mod phase_display_serde {
    use super::PhaseDisplay;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(m: &PhaseDisplay, s: S) -> Result<S::Ok, S::Error> {
        let tag = match m {
            PhaseDisplay::AllTraces => "traces",
            PhaseDisplay::MeanEnvelope => "mean_envelope",
            PhaseDisplay::Heatmap => "heatmap",
        };
        s.serialize_str(tag)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PhaseDisplay, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "mean_envelope" => PhaseDisplay::MeanEnvelope,
            "heatmap" => PhaseDisplay::Heatmap,
            _ => PhaseDisplay::AllTraces,
        })
    }
}

mod magnitude_scale_serde {
    use super::MagnitudeScale;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(m: &MagnitudeScale, s: S) -> Result<S::Ok, S::Error> {
        let tag = match m {
            MagnitudeScale::Linear => "linear",
            MagnitudeScale::Db => "db",
        };
        s.serialize_str(tag)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<MagnitudeScale, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "db" => MagnitudeScale::Db,
            _ => MagnitudeScale::Linear,
        })
    }
}

mod magnitude_display_serde {
    use super::MagnitudeDisplay;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(m: &MagnitudeDisplay, s: S) -> Result<S::Ok, S::Error> {
        let tag = match m {
            MagnitudeDisplay::AllTraces => "traces",
            MagnitudeDisplay::MeanEnvelope => "mean_envelope",
            MagnitudeDisplay::Heatmap => "heatmap",
        };
        s.serialize_str(tag)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<MagnitudeDisplay, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "mean_envelope" => MagnitudeDisplay::MeanEnvelope,
            "heatmap" => MagnitudeDisplay::Heatmap,
            _ => MagnitudeDisplay::AllTraces,
        })
    }
}

mod evolution_metric_serde {
    use super::EvolutionMetric;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(m: &EvolutionMetric, s: S) -> Result<S::Ok, S::Error> {
        let tag = match m {
            EvolutionMetric::MeanAmplitude => "mean_amplitude",
            EvolutionMetric::StdAmplitude => "std_amplitude",
            EvolutionMetric::DeltaAmplitude => "delta_amplitude",
            EvolutionMetric::Rssi => "rssi",
        };
        s.serialize_str(tag)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<EvolutionMetric, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "std_amplitude" => EvolutionMetric::StdAmplitude,
            "delta_amplitude" => EvolutionMetric::DeltaAmplitude,
            "rssi" => EvolutionMetric::Rssi,
            _ => EvolutionMetric::MeanAmplitude,
        })
    }
}
