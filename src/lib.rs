//! Crudify reduces the resolution and the number of distinct colors of an
//! image in preparation for painting.
//!
//! The entry point is [`run`], which reads a YAML configuration file
//! describing an input image and a list of *derivations* to produce from it.
//! Almost all of the logic lives in this library; the executable is only a
//! thin wrapper around [`run`].

pub mod config;
pub mod grid;
pub mod image_io;
pub mod palette;
pub mod pixelate;

use std::path::Path;

use anyhow::{Context, Result};

use crate::config::Config;

/// Run crudify against the configuration file at `config_path`.
///
/// The configuration file is a YAML document (see [`config::Config`]). Every
/// path mentioned in it is resolved relative to the directory that contains
/// the configuration file itself.
pub fn run(config_path: impl AsRef<Path>) -> Result<()> {
    let config_path = config_path.as_ref();
    let config = Config::from_file(config_path)
        .with_context(|| format!("failed to read config file {}", config_path.display()))?;

    // Paths in the config are relative to the directory containing the config
    // file, defaulting to the current directory when there is no parent.
    let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));

    let input_path = base_dir.join(&config.input);
    let source = image_io::load(&input_path)
        .with_context(|| format!("failed to load input image {}", input_path.display()))?;

    let input_dims = source.dimensions();

    // The default lightness compensation is derived once from the input image
    // so that lightness and hue/chroma contribute equally to clustering; a
    // derivation may override it with an explicit value.
    let default_lightness_compensation = palette::adaptive_lightness_compensation(&source);

    let (input_width, input_height) = input_dims;

    for derivation in &config.derivations {
        // The user specifies only the shorter output dimension; the longer one
        // is scaled from it to preserve the input's aspect ratio. Rounding is
        // done so the derived dimension is never smaller than `short_side`.
        let (width, height) = if input_width <= input_height {
            let height = scale_dimension(derivation.short_side, input_height, input_width);
            (derivation.short_side, height)
        } else {
            let width = scale_dimension(derivation.short_side, input_width, input_height);
            (width, derivation.short_side)
        };

        let pixelated = pixelate::pixelate(
            &source,
            width,
            height,
            derivation.palette_size,
            pixelate::PaletteOptions {
                strategy: derivation.palette_strategy,
                accent_strength: derivation.accent_strength,
                lightness_compensation: derivation
                    .lightness_compensation
                    .unwrap_or(default_lightness_compensation),
            },
        )
        .with_context(|| {
            format!(
                "failed to process derivation for output {}",
                derivation.output.display()
            )
        })?;

        // Upscale back to approximately the input resolution and overlay the
        // painting grid.
        let output = grid::render(&pixelated, input_dims).with_context(|| {
            format!(
                "failed to render grid for output {}",
                derivation.output.display()
            )
        })?;

        let output_path = base_dir.join(&derivation.output);
        image_io::save(&output, &output_path)
            .with_context(|| format!("failed to write output image {}", output_path.display()))?;
    }

    Ok(())
}

/// Scale `short_side` by the input aspect ratio `long / short` to obtain the
/// longer output dimension, rounding to the nearest pixel.
///
/// The result is clamped to be at least `short_side` so that the shorter side
/// stays the shorter side even for a square input (`long == short`).
fn scale_dimension(short_side: u32, long: u32, short: u32) -> u32 {
    let scaled = (short_side as u64 * long as u64 + short as u64 / 2) / short as u64;
    (scaled as u32).max(short_side)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_the_longer_dimension_by_aspect_ratio() {
        // 3:2 landscape input, short side 200 -> long side 300.
        assert_eq!(scale_dimension(200, 3000, 2000), 300);
        // 16:9 input, short side 90 -> 160.
        assert_eq!(scale_dimension(90, 1600, 900), 160);
    }

    #[test]
    fn rounds_to_the_nearest_pixel() {
        // 100 * 101 / 100 = 101, exact.
        assert_eq!(scale_dimension(100, 101, 100), 101);
        // 3 * 5 / 3 = 5.
        assert_eq!(scale_dimension(3, 5, 3), 5);
        // 7 * 10 / 3 = 23.33 -> 23.
        assert_eq!(scale_dimension(7, 10, 3), 23);
    }

    #[test]
    fn never_shrinks_below_the_short_side() {
        // Square input: long == short, result must stay at short_side.
        assert_eq!(scale_dimension(200, 500, 500), 200);
    }
}
