//! The YAML configuration format.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Deserialize;

/// The top level configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Path to the input image, relative to the configuration file.
    pub input: PathBuf,
    /// The derivations to produce from the input image.
    pub derivations: Vec<Derivation>,
}

/// A single output to produce from the input image.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Derivation {
    /// Path to the output image, relative to the configuration file.
    pub output: PathBuf,
    /// Target width in pixels.
    pub width: u32,
    /// Target height in pixels.
    pub height: u32,
    /// The maximum number of distinct colors allowed in the output.
    ///
    /// This is an upper bound only: a derivation may use fewer colors when the
    /// (already downscaled) image does not contain that many distinct colors.
    /// It places no constraint on *which* colors may be used.
    pub palette_size: u32,
    /// How the palette is chosen. Defaults to [`PaletteStrategy::Saliency`],
    /// which favors vivid and rare colors so that accents survive.
    #[serde(default)]
    pub palette_strategy: PaletteStrategy,
    /// How strongly the [`Saliency`](PaletteStrategy::Saliency) strategy favors
    /// vivid and rare colors, in the range `0.0..=1.0`. Ignored by
    /// [`PaletteStrategy::Frequency`]. Defaults to [`default_accent_strength`].
    #[serde(default = "default_accent_strength")]
    pub accent_strength: f64,
    /// How strongly to de-emphasize lightness when clustering in OKLab, in the
    /// range `0.0..=1.0`. At `0.0` (the default) lightness counts fully; at
    /// `1.0` it is ignored, so colors are separated purely by hue and chroma.
    /// This keeps dark but saturated hues (such as blue) from being absorbed
    /// into large clusters of mid-lightness colors. Ignored by the `frequency`
    /// strategy.
    #[serde(default)]
    pub lightness_compensation: f64,
}

/// The palette selection strategy for a derivation.
///
/// Apart from [`Frequency`](Self::Frequency), the strategies cluster in the
/// perceptual OKLab color space, where distances match human vision and
/// distinct hues resist being merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteStrategy {
    /// Frequency-weighted clustering in exoquant's color space (the original
    /// behavior). The baseline; tends to average away small vivid accents.
    Frequency,
    /// Reweight the histogram to favor vivid and rare colors, then cluster in
    /// OKLab. The default.
    #[default]
    Saliency,
}

/// The default value for [`Derivation::accent_strength`].
pub fn default_accent_strength() -> f64 {
    0.5
}

impl Config {
    /// Parse a [`Config`] from the YAML file at `path`.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let contents = fs::read_to_string(path)?;
        let config = serde_yaml::from_str(&contents)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_config() {
        let yaml = "
input: photo.png
derivations:
  - output: small.png
    width: 64
    height: 48
    palette_size: 16
  - output: tiny.png
    width: 16
    height: 16
    palette_size: 4
";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.input, PathBuf::from("photo.png"));
        assert_eq!(config.derivations.len(), 2);
        assert_eq!(config.derivations[0].width, 64);
        assert_eq!(config.derivations[0].height, 48);
        assert_eq!(config.derivations[0].palette_size, 16);
        assert_eq!(config.derivations[1].output, PathBuf::from("tiny.png"));
    }

    #[test]
    fn palette_fields_default_when_absent() {
        let yaml = "
input: photo.png
derivations:
  - output: small.png
    width: 64
    height: 48
    palette_size: 16
";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let d = &config.derivations[0];
        assert_eq!(d.palette_strategy, PaletteStrategy::Saliency);
        assert_eq!(d.accent_strength, default_accent_strength());
        assert_eq!(d.lightness_compensation, 0.0);
    }

    #[test]
    fn parses_snake_case_palette_strategies() {
        for (name, expected) in [
            ("frequency", PaletteStrategy::Frequency),
            ("saliency", PaletteStrategy::Saliency),
        ] {
            let yaml = format!(
                "
input: photo.png
derivations:
  - output: small.png
    width: 64
    height: 48
    palette_size: 16
    palette_strategy: {name}
    accent_strength: 0.7
    lightness_compensation: 0.3
"
            );
            let config: Config = serde_yaml::from_str(&yaml).unwrap();
            let d = &config.derivations[0];
            assert_eq!(d.palette_strategy, expected, "strategy {name}");
            assert_eq!(d.accent_strength, 0.7);
            assert_eq!(d.lightness_compensation, 0.3);
        }
    }

    #[test]
    fn rejects_unknown_palette_strategy() {
        let yaml = "
input: photo.png
derivations:
  - output: small.png
    width: 64
    height: 48
    palette_size: 16
    palette_strategy: nonexistent
";
        assert!(serde_yaml::from_str::<Config>(yaml).is_err());
    }

    #[test]
    fn input_is_required() {
        let yaml = "
derivations: []
";
        assert!(serde_yaml::from_str::<Config>(yaml).is_err());
    }

    #[test]
    fn derivations_are_required() {
        let yaml = "
input: photo.png
";
        assert!(serde_yaml::from_str::<Config>(yaml).is_err());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let yaml = "
input: photo.png
derivations: []
extra: nonsense
";
        assert!(serde_yaml::from_str::<Config>(yaml).is_err());
    }
}
