//! The core operation: jointly reduce resolution and palette size.
//!
//! The naive approaches — resize then quantize, or quantize then resize — each
//! sacrifice one of the two constraints we care about. Resizing first discards
//! most of the original color information before the palette is ever chosen;
//! quantizing first fixes a palette that resampling then blends past
//! `palette_size`. We instead do both *together*:
//!
//! 1. A global palette is chosen from the color distribution of the *full
//!    resolution* source image, so the palette is informed by the original
//!    colors (constraint: at most `palette_size` colors).
//! 2. The output grid is laid over the source; each output cell is the average
//!    of the source pixels that map into it, then snapped to the nearest
//!    palette color (constraint: downsampled resolution).
//!
//! This is the initial, deliberately approximate implementation. Palette
//! selection is delegated to [`crate::palette`], which offers several
//! strategies for keeping vivid accent colors; the joint downsampling structure
//! here is our own and is expected to grow into a fully custom optimizer.

use anyhow::{Result, ensure};
use image::{Rgb, RgbImage};

use crate::config::PaletteStrategy;
use crate::palette;

/// How the palette for a derivation should be selected.
#[derive(Debug, Clone, Copy)]
pub struct PaletteOptions {
    /// The palette selection strategy.
    pub strategy: PaletteStrategy,
    /// Strength of the vivid/rare bias for the saliency strategies (`0..=1`).
    pub accent_strength: f64,
    /// Reserved accent slots for the reserve-accents strategies.
    pub accent_slots: Option<u32>,
    /// Lightness de-emphasis for OKLab clustering (`0..=1`); see
    /// [`crate::config::Derivation::lightness_compensation`].
    pub lightness_compensation: f64,
}

/// Produce a `width`x`height` image using at most `palette_size` distinct
/// colors, derived from `source`, choosing the palette per `options`.
pub fn pixelate(
    source: &RgbImage,
    width: u32,
    height: u32,
    palette_size: u32,
    options: PaletteOptions,
) -> Result<RgbImage> {
    ensure!(
        width > 0 && height > 0,
        "target dimensions must be non-zero"
    );
    ensure!(palette_size > 0, "palette_size must be at least 1");
    ensure!(
        source.width() > 0 && source.height() > 0,
        "source image is empty"
    );

    // (1) Build the palette from the color distribution of the *whole* source
    // image, so palette selection sees every original color.
    let palette = palette::build(
        source,
        palette_size,
        options.strategy,
        options.accent_strength,
        options.accent_slots,
        options.lightness_compensation,
    );

    // (2) Downsample: each output cell averages the source pixels that map into
    // it, then snaps to the nearest palette color.
    let mut output = RgbImage::new(width, height);
    for oy in 0..height {
        for ox in 0..width {
            let region = cell_region(source.width(), source.height(), width, height, ox, oy);
            let average = average_color(source, region);
            output.put_pixel(ox, oy, palette.nearest(average));
        }
    }

    Ok(output)
}

/// The half-open rectangle `[x0, x1) x [y0, y1)` of source pixels covered by
/// output cell `(ox, oy)`.
fn cell_region(
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    ox: u32,
    oy: u32,
) -> (u32, u32, u32, u32) {
    // Map the output cell boundaries back onto the source grid. Using u64 for
    // the multiplication avoids overflow on large images.
    let x0 = (ox as u64 * src_w as u64 / dst_w as u64) as u32;
    let x1 = ((ox as u64 + 1) * src_w as u64 / dst_w as u64) as u32;
    let y0 = (oy as u64 * src_h as u64 / dst_h as u64) as u32;
    let y1 = ((oy as u64 + 1) * src_h as u64 / dst_h as u64) as u32;
    // When the output is larger than the source a cell can be empty; guarantee
    // it covers at least one pixel so every output pixel has a source.
    (x0, x1.max(x0 + 1).min(src_w), y0, y1.max(y0 + 1).min(src_h))
}

/// Average of the source pixels in the given half-open region.
fn average_color(source: &RgbImage, region: (u32, u32, u32, u32)) -> Rgb<u8> {
    let (x0, x1, y0, y1) = region;
    let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
    let mut count = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            let Rgb([pr, pg, pb]) = *source.get_pixel(x, y);
            r += pr as u64;
            g += pg as u64;
            b += pb as u64;
            count += 1;
        }
    }
    // `cell_region` guarantees at least one pixel, so `count` is never zero.
    let count = count.max(1);
    Rgb([(r / count) as u8, (g / count) as u8, (b / count) as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_options() -> PaletteOptions {
        PaletteOptions {
            strategy: PaletteStrategy::Frequency,
            accent_strength: 0.5,
            accent_slots: None,
            lightness_compensation: 0.0,
        }
    }

    fn solid(width: u32, height: u32, color: Rgb<u8>) -> RgbImage {
        RgbImage::from_pixel(width, height, color)
    }

    fn distinct_colors(image: &RgbImage) -> usize {
        use std::collections::HashSet;
        image.pixels().map(|p| p.0).collect::<HashSet<_>>().len()
    }

    #[test]
    fn output_has_requested_dimensions() {
        let src = solid(100, 80, Rgb([10, 20, 30]));
        let out = pixelate(&src, 20, 16, 8, default_options()).unwrap();
        assert_eq!(out.dimensions(), (20, 16));
    }

    #[test]
    fn respects_palette_size_upper_bound() {
        // A noisy gradient with many distinct colors.
        let mut src = RgbImage::new(64, 64);
        for (x, y, p) in src.enumerate_pixels_mut() {
            *p = Rgb([(x * 4) as u8, (y * 4) as u8, ((x + y) * 2) as u8]);
        }
        let out = pixelate(&src, 32, 32, 4, default_options()).unwrap();
        assert!(
            distinct_colors(&out) <= 4,
            "expected at most 4 colors, got {}",
            distinct_colors(&out)
        );
    }

    #[test]
    fn degenerate_input_uses_fewer_colors() {
        // Only one color in the source, so the output cannot exceed one color
        // even though a larger palette was allowed.
        let src = solid(50, 50, Rgb([200, 100, 50]));
        let out = pixelate(&src, 10, 10, 16, default_options()).unwrap();
        assert_eq!(distinct_colors(&out), 1);
        // The single palette color should be close to the source color. It
        // need not be bit-exact: the quantizer works in a gamma-aware color
        // space, so the round trip can shift each channel by a few units.
        let Rgb([r, g, b]) = *out.get_pixel(0, 0);
        assert!(r.abs_diff(200) <= 8 && g.abs_diff(100) <= 8 && b.abs_diff(50) <= 8);
    }

    #[test]
    fn upscaling_is_supported() {
        let src = solid(4, 4, Rgb([5, 5, 5]));
        let out = pixelate(&src, 16, 16, 4, default_options()).unwrap();
        assert_eq!(out.dimensions(), (16, 16));
    }

    #[test]
    fn rejects_zero_dimensions() {
        let src = solid(10, 10, Rgb([0, 0, 0]));
        assert!(pixelate(&src, 0, 10, 4, default_options()).is_err());
        assert!(pixelate(&src, 10, 0, 4, default_options()).is_err());
        assert!(pixelate(&src, 10, 10, 0, default_options()).is_err());
    }
}
