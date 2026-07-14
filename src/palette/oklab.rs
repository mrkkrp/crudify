//! Conversion between 8-bit sRGB and the perceptual OKLab color space.
//!
//! OKLab (Björn Ottosson, 2020) is a perceptual color space in which Euclidean
//! distance approximates perceived color difference, and where averaging colors
//! does not desaturate them the way averaging in gamma-encoded sRGB does. We use
//! it so that distinct hues resist being merged and vivid accents stay vivid.

use image::Rgb;

/// A color in the OKLab space: perceptual lightness `l` and the opponent axes
/// `a` (green–red) and `b` (blue–yellow).
#[derive(Debug, Clone, Copy)]
pub struct Oklab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

impl Oklab {
    /// Convert an 8-bit sRGB color to OKLab.
    pub fn from_srgb(color: Rgb<u8>) -> Self {
        let Rgb([r, g, b]) = color;
        let r = srgb_to_linear(r);
        let g = srgb_to_linear(g);
        let b = srgb_to_linear(b);

        // Linear sRGB -> LMS.
        let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
        let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
        let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;

        let l = l.cbrt();
        let m = m.cbrt();
        let s = s.cbrt();

        Oklab {
            l: 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
            a: 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
            b: 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
        }
    }

    /// Convert back from OKLab to an 8-bit sRGB color, clamping to gamut.
    pub fn to_srgb(self) -> Rgb<u8> {
        let l = self.l + 0.396_337_777_4 * self.a + 0.215_803_757_3 * self.b;
        let m = self.l - 0.105_561_345_8 * self.a - 0.063_854_172_8 * self.b;
        let s = self.l - 0.089_484_177_5 * self.a - 1.291_485_548_0 * self.b;

        let l = l * l * l;
        let m = m * m * m;
        let s = s * s * s;

        let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
        let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
        let b = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s;

        Rgb([linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b)])
    }

    /// Chroma: the distance from the neutral axis in OKLab (`hypot(a, b)`).
    pub fn chroma(self) -> f64 {
        self.a.hypot(self.b)
    }

    /// Chroma relative to lightness, a rough saturation measure in `0.0..~1.0`.
    /// Neutral colors are near 0; vivid colors are larger.
    pub fn chroma_ratio(self) -> f64 {
        if self.l <= f64::EPSILON {
            0.0
        } else {
            (self.chroma() / self.l).min(1.0)
        }
    }

    /// Squared OKLab distance with the lightness axis scaled by `l_weight`.
    ///
    /// With `l_weight == 1.0` this is the plain Euclidean distance. Lowering it
    /// de-emphasizes lightness so colors are separated more by hue and chroma;
    /// at `l_weight == 0.0` lightness is ignored entirely, which keeps dark but
    /// saturated hues (such as blue) from being absorbed into large clusters of
    /// mid-lightness colors.
    pub fn distance_squared_weighted(self, other: &Oklab, l_weight: f64) -> f64 {
        let dl = self.l - other.l;
        let da = self.a - other.a;
        let db = self.b - other.b;
        l_weight * dl * dl + da * da + db * db
    }
}

/// sRGB gamma decode of a single 8-bit channel to linear `0.0..=1.0`.
fn srgb_to_linear(channel: u8) -> f64 {
    let c = channel as f64 / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB gamma encode of a linear channel back to an 8-bit value, clamped.
fn linear_to_srgb(channel: f64) -> u8 {
    let c = channel.clamp(0.0, 1.0);
    let encoded = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(color: Rgb<u8>) {
        let back = Oklab::from_srgb(color).to_srgb();
        for i in 0..3 {
            assert!(
                (back.0[i] as i32 - color.0[i] as i32).abs() <= 1,
                "roundtrip {color:?} -> {back:?} differs at channel {i}"
            );
        }
    }

    #[test]
    fn srgb_roundtrip_is_stable() {
        roundtrip(Rgb([0, 0, 0]));
        roundtrip(Rgb([255, 255, 255]));
        roundtrip(Rgb([230, 20, 20]));
        roundtrip(Rgb([18, 52, 200]));
        roundtrip(Rgb([128, 128, 128]));
        roundtrip(Rgb([10, 200, 90]));
    }

    #[test]
    fn vivid_colors_have_higher_chroma_than_gray() {
        let gray = Oklab::from_srgb(Rgb([128, 128, 128]));
        let red = Oklab::from_srgb(Rgb([230, 20, 20]));
        assert!(red.chroma_ratio() > gray.chroma_ratio());
        assert!(gray.chroma_ratio() < 0.02);
    }

    #[test]
    fn lightness_weight_scales_the_lightness_axis() {
        // Two colors that differ only in lightness.
        let a = Oklab {
            l: 0.3,
            a: 0.1,
            b: 0.1,
        };
        let b = Oklab {
            l: 0.7,
            a: 0.1,
            b: 0.1,
        };
        // Full weight sees the lightness difference; zero weight sees none.
        assert!(a.distance_squared_weighted(&b, 1.0) > 0.0);
        assert_eq!(a.distance_squared_weighted(&b, 0.0), 0.0);
        // Halving the weight halves the (purely lightness) squared distance.
        let full = a.distance_squared_weighted(&b, 1.0);
        let half = a.distance_squared_weighted(&b, 0.5);
        assert!((half - full / 2.0).abs() < 1e-12);
    }

    #[test]
    fn lightness_weight_leaves_hue_distance_untouched() {
        // Colors that differ only in the a/b (hue/chroma) axes: the weight on
        // lightness must not change their distance.
        let a = Oklab {
            l: 0.5,
            a: 0.2,
            b: -0.1,
        };
        let b = Oklab {
            l: 0.5,
            a: -0.1,
            b: 0.2,
        };
        let full = a.distance_squared_weighted(&b, 1.0);
        let none = a.distance_squared_weighted(&b, 0.0);
        assert!((full - none).abs() < 1e-12);
    }
}
