//! Loading and saving images.
//!
//! We deliberately support only a small set of formats, favoring simplicity
//! and dependency availability over broad format support. Input and output may
//! be PNG or JPEG. Images are always worked with as 8-bit RGB.
//!
//! PNG is the natural output format: the whole point of the tool is to produce
//! an image with a bounded number of distinct colors, and PNG stores that
//! duplicate colors, and at low quality it produces visible ringing along the
//! hard block edges this tool creates. We avoid that in two ways:
//! lossy: on decode it turns a flat 20-color image into thousands of near-
//! palette losslessly. JPEG is also supported. The concern with JPEG is that it is
//!
//!   * We encode at high quality (see `JPEG_QUALITY`), so the DCT loss on our
//!     low-frequency, blocky images is visually negligible.
//!   * The `image` crate's JPEG encoder uses 4:4:4 chroma sampling (no chroma
//!     subsampling), so colors keep full spatial resolution and block edges do
//!     not bleed colored halos.
//!
//! The result is visually indistinguishable from the PNG. Note that the exact-
//! palette guarantee is still only preserved by PNG; if a downstream step needs
//! to recover the precise set of colors, keep a PNG copy.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result, bail};
use image::codecs::jpeg::JpegEncoder;
use image::{ImageEncoder, RgbImage};

/// Quality (1-100) used when encoding JPEG output. High enough that the lossy
/// DCT is visually imperceptible on the flat color blocks this tool produces.
const JPEG_QUALITY: u8 = 100;

/// Load an image from `path` as 8-bit RGB.
///
/// The format is inferred from the file contents and extension by the `image`
/// crate; in practice this means PNG and JPEG (the only formats compiled in).
pub fn load(path: impl AsRef<Path>) -> Result<RgbImage> {
    let image = image::open(path.as_ref())
        .with_context(|| format!("could not decode image {}", path.as_ref().display()))?;
    Ok(image.to_rgb8())
}

/// Save `image` to `path`, choosing the encoder from the path's extension.
///
/// `.png` is written losslessly; `.jpg`/`.jpeg` is written at [`JPEG_QUALITY`]
/// with 4:4:4 chroma sampling. Any other extension is an error.
pub fn save(image: &RgbImage, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    match output_format(path)? {
        OutputFormat::Png => image
            .save(path)
            .with_context(|| format!("could not encode image {}", path.display()))?,
        OutputFormat::Jpeg => {
            let file = File::create(path)
                .with_context(|| format!("could not create output file {}", path.display()))?;
            let encoder = JpegEncoder::new_with_quality(BufWriter::new(file), JPEG_QUALITY);
            encoder
                .write_image(
                    image.as_raw(),
                    image.width(),
                    image.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .with_context(|| format!("could not encode image {}", path.display()))?;
        }
    }
    Ok(())
}

/// The supported output encodings.
enum OutputFormat {
    Png,
    Jpeg,
}

/// Determine the output format from `path`'s extension, erroring on anything
/// other than PNG or JPEG.
fn output_format(path: &Path) -> Result<OutputFormat> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => Ok(OutputFormat::Png),
        Some("jpg" | "jpeg") => Ok(OutputFormat::Jpeg),
        _ => bail!(
            "output {} must have a .png, .jpg, or .jpeg extension",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_png_output() {
        assert!(matches!(
            output_format(Path::new("out.png")),
            Ok(OutputFormat::Png)
        ));
        assert!(matches!(
            output_format(Path::new("dir/OUT.PNG")),
            Ok(OutputFormat::Png)
        ));
    }

    #[test]
    fn accepts_jpeg_output() {
        assert!(matches!(
            output_format(Path::new("out.jpg")),
            Ok(OutputFormat::Jpeg)
        ));
        assert!(matches!(
            output_format(Path::new("out.jpeg")),
            Ok(OutputFormat::Jpeg)
        ));
        assert!(matches!(
            output_format(Path::new("dir/OUT.JPG")),
            Ok(OutputFormat::Jpeg)
        ));
    }

    #[test]
    fn rejects_other_output() {
        assert!(output_format(Path::new("out.gif")).is_err());
        assert!(output_format(Path::new("out")).is_err());
    }
}
