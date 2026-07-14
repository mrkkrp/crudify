//! Loading and saving images.
//!
//! We deliberately support only a small set of formats (PNG and JPEG),
//! favoring simplicity and dependency availability over broad format support.
//! Images are always worked with as 8-bit RGB.
//!
//! Note that the palette reduction performed elsewhere only bounds the number
//! of distinct colors in the image we hand to the encoder. A lossy encoder
//! such as JPEG may reintroduce colors on decode, so the on-disk color count
//! of a JPEG output is not guaranteed to respect `palette_size`.

use std::path::Path;

use anyhow::{Context, Result};
use image::RgbImage;

/// Load an image from `path` as 8-bit RGB.
///
/// The format is inferred from the file contents and extension by the `image`
/// crate; in practice this means PNG and JPEG (the only formats compiled in).
pub fn load(path: impl AsRef<Path>) -> Result<RgbImage> {
    let image = image::open(path.as_ref())
        .with_context(|| format!("could not decode image {}", path.as_ref().display()))?;
    Ok(image.to_rgb8())
}

/// Save `image` to `path`, inferring the format from the file extension.
pub fn save(image: &RgbImage, path: impl AsRef<Path>) -> Result<()> {
    image
        .save(path.as_ref())
        .with_context(|| format!("could not encode image {}", path.as_ref().display()))?;
    Ok(())
}
