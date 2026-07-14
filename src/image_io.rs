//! Loading and saving images.
//!
//! We deliberately support only a small set of formats, favoring simplicity
//! and dependency availability over broad format support. Input may be PNG or
//! JPEG; output must be PNG. Images are always worked with as 8-bit RGB.
//!
//! Output is restricted to PNG on purpose: the whole point of the tool is to
//! produce an image with a bounded number of distinct colors, and a lossy
//! encoder such as JPEG reintroduces colors on decode (turning a 20-color
//! palette into tens of thousands of colors), silently defeating that goal. A
//! lossless format is therefore required for output.

use std::path::Path;

use anyhow::{Context, Result, bail};
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

/// Save `image` to `path` as PNG.
///
/// The output path must have a `.png` extension; any other extension is an
/// error, because only a lossless format can preserve the reduced palette.
pub fn save(image: &RgbImage, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    ensure_png_extension(path)?;
    image
        .save(path)
        .with_context(|| format!("could not encode image {}", path.display()))?;
    Ok(())
}

/// Return an error unless `path` has a `.png` extension (case-insensitive).
fn ensure_png_extension(path: &Path) -> Result<()> {
    let is_png = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"));
    if !is_png {
        bail!(
            "output {} must have a .png extension: palette reduction requires a \
             lossless format, so lossy formats such as JPEG are not allowed for output",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_png_output() {
        assert!(ensure_png_extension(Path::new("out.png")).is_ok());
        assert!(ensure_png_extension(Path::new("dir/OUT.PNG")).is_ok());
    }

    #[test]
    fn rejects_lossy_output() {
        assert!(ensure_png_extension(Path::new("out.jpg")).is_err());
        assert!(ensure_png_extension(Path::new("out.jpeg")).is_err());
        assert!(ensure_png_extension(Path::new("out")).is_err());
    }
}
