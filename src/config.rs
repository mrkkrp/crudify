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
