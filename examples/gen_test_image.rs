//! Generate a small test image for manual/smoke testing.
//!
//! Usage: `cargo run --example gen_test_image -- <output.png>`

use image::{Rgb, RgbImage};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: gen_test_image <path>");
    let mut img = RgbImage::new(200, 150);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8]);
    }
    img.save(&path).expect("failed to save image");
    eprintln!("wrote {path}");
}
