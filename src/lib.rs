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

    for derivation in &config.derivations {
        let pixelated = pixelate::pixelate(
            &source,
            derivation.width,
            derivation.height,
            derivation.palette_size,
            pixelate::PaletteOptions {
                strategy: derivation.palette_strategy,
                accent_strength: derivation.accent_strength,
                accent_slots: derivation.accent_slots,
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
