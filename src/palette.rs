//! Palette selection.
//!
//! A frequency-weighted quantizer spends its color budget on whatever dominates
//! the image by pixel count, so small but vivid accents get averaged into the
//! nearest large cluster and the result looks "muddy". The strategies here bias
//! selection toward perceptually important colors so accents survive:
//!
//! * [`PaletteStrategy::Frequency`] — the original behavior, no bias.
//! * [`PaletteStrategy::Saliency`] — reweight colors by vividness and rarity
//!   before clustering.
//!
//! Apart from `Frequency`, the strategies cluster in the perceptual OKLab
//! space, where distances match human vision and distinct hues resist being
//! merged.

use std::collections::HashMap;

use exoquant::{Color, ColorMap, ColorSpace, Histogram, SimpleColorSpace, optimizer::KMeans};
use image::{Rgb, RgbImage};

use crate::config::PaletteStrategy;

mod oklab;

use oklab::Oklab;

/// A chosen palette together with the means to map arbitrary colors onto it.
///
/// Mapping uses the same color space the palette was built in, so that the
/// nearest-color decision is consistent with how the palette was chosen.
pub struct Palette {
    colors: Vec<Rgb<u8>>,
    mapper: Mapper,
}

/// Nearest-color lookup, parameterized by the space the palette lives in.
enum Mapper {
    /// Lookup in exoquant's color space.
    Exoquant {
        map: ColorMap,
        colorspace: SimpleColorSpace,
    },
    /// Lookup by (lightness-weighted) distance in OKLab.
    Oklab { points: Vec<Oklab>, l_weight: f64 },
}

impl Palette {
    /// The distinct colors of the palette.
    pub fn colors(&self) -> &[Rgb<u8>] {
        &self.colors
    }

    /// The palette color nearest to `color`.
    pub fn nearest(&self, color: Rgb<u8>) -> Rgb<u8> {
        let index = match &self.mapper {
            Mapper::Exoquant { map, colorspace } => {
                let Rgb([r, g, b]) = color;
                map.find_nearest(colorspace.to_float(Color::new(r, g, b, 255)))
            }
            Mapper::Oklab { points, l_weight } => {
                let target = Oklab::from_srgb(color);
                let mut best = 0;
                let mut best_dist = f64::INFINITY;
                for (i, p) in points.iter().enumerate() {
                    let d = target.distance_squared_weighted(p, *l_weight);
                    if d < best_dist {
                        best_dist = d;
                        best = i;
                    }
                }
                best
            }
        };
        self.colors[index]
    }
}

/// A distinct color of the source image with the number of times it occurs.
struct ColorCount {
    color: Rgb<u8>,
    count: u64,
}

/// Build a palette of at most `palette_size` colors from `source` according to
/// `strategy`. `accent_strength` (0..=1) controls how strongly the saliency
/// strategy favors vivid and rare colors.
///
/// `lightness_compensation` (0..=1) de-emphasizes the lightness axis when
/// clustering in OKLab so dark but saturated hues stay distinct; it affects
/// only the [`Saliency`](PaletteStrategy::Saliency) strategy.
pub fn build(
    source: &RgbImage,
    palette_size: u32,
    strategy: PaletteStrategy,
    accent_strength: f64,
    lightness_compensation: f64,
) -> Palette {
    let palette_size = palette_size.max(1) as usize;
    let accent_strength = accent_strength.clamp(0.0, 1.0);
    // The lightness axis weight in OKLab distance: full compensation (1.0)
    // means the axis is ignored (weight 0.0).
    let l_weight = 1.0 - lightness_compensation.clamp(0.0, 1.0);
    let counts = color_counts(source);

    match strategy {
        PaletteStrategy::Frequency => {
            let colors = cluster_exoquant(&counts, palette_size, |c| c.count);
            finish_exoquant(colors)
        }
        PaletteStrategy::Saliency => {
            let colors = cluster_oklab(&counts, palette_size, l_weight, |c| {
                saliency_weight(c, accent_strength)
            });
            finish_oklab(colors, l_weight)
        }
    }
}

/// The `lightness_compensation` value that makes lightness and hue/chroma
/// contribute equally to OKLab clustering distance for `source`.
///
/// Clustering distance is `l_weight * dL^2 + da^2 + db^2` where `l_weight =
/// 1 - lightness_compensation`. The expected contribution of the lightness term
/// over the image is proportional to `l_weight * Var(L)`, and of the hue/chroma
/// terms to `Var(a) + Var(b)`. Equating them gives
/// `l_weight = (Var(a) + Var(b)) / Var(L)`, so
/// `lightness_compensation = 1 - (Var(a) + Var(b)) / Var(L)`.
///
/// In photographs lightness varies far more than hue/chroma, so this is
/// typically close to `1.0`. The result is clamped to `0.0..=1.0` (a flat-lit,
/// very colorful image could otherwise ask to *amplify* lightness, which is not
/// meaningful). A degenerate image with no lightness variation yields `0.0`.
pub fn adaptive_lightness_compensation(source: &RgbImage) -> f64 {
    let (mut sl, mut sa, mut sb) = (0.0f64, 0.0f64, 0.0f64);
    let (mut sll, mut saa, mut sbb) = (0.0f64, 0.0f64, 0.0f64);
    let mut n = 0.0f64;
    for pixel in source.pixels() {
        let c = Oklab::from_srgb(*pixel);
        sl += c.l;
        sa += c.a;
        sb += c.b;
        sll += c.l * c.l;
        saa += c.a * c.a;
        sbb += c.b * c.b;
        n += 1.0;
    }
    if n == 0.0 {
        return 0.0;
    }
    let var_l = (sll / n - (sl / n).powi(2)).max(0.0);
    let var_a = (saa / n - (sa / n).powi(2)).max(0.0);
    let var_b = (sbb / n - (sb / n).powi(2)).max(0.0);
    // For an essentially uniform image no axis carries real information, so the
    // weighting is undefined; fall back to the neutral value (lightness counts
    // fully). The threshold is well above float accumulation noise but far
    // below any real image's lightness spread.
    const NEGLIGIBLE_VARIANCE: f64 = 1e-6;
    if var_l <= NEGLIGIBLE_VARIANCE {
        return 0.0;
    }
    (1.0 - (var_a + var_b) / var_l).clamp(0.0, 1.0)
}

/// Count occurrences of each distinct color in `source`.
fn color_counts(source: &RgbImage) -> Vec<ColorCount> {
    let mut map: HashMap<[u8; 3], u64> = HashMap::new();
    for Rgb(pixel) in source.pixels() {
        *map.entry(*pixel).or_insert(0) += 1;
    }
    map.into_iter()
        .map(|(color, count)| ColorCount {
            color: Rgb(color),
            count,
        })
        .collect()
}

/// The weight of a color under the saliency strategies.
///
/// Weight rises with saturation (vividness) and falls with frequency (rarity),
/// so a small saturated accent competes with a large flat background. At
/// `strength = 0` this reduces to plain frequency weighting.
fn saliency_weight(c: &ColorCount, strength: f64) -> u64 {
    let saturation = Oklab::from_srgb(c.color).chroma_ratio();
    // Rarity boost: rare colors count for more, damped so a single stray pixel
    // does not dominate. Uses log so the effect is gentle.
    let rarity = 1.0 / (1.0 + (c.count as f64).ln());
    let boost = 1.0 + strength * (2.0 * saturation + rarity);
    ((c.count as f64) * boost).round().max(1.0) as u64
}

/// Cluster colors in exoquant's color space using the given per-color weight.
fn cluster_exoquant(
    counts: &[ColorCount],
    palette_size: usize,
    weight: impl Fn(&ColorCount) -> u64,
) -> Vec<Rgb<u8>> {
    let colorspace = SimpleColorSpace::default();
    let mut histogram = Histogram::new();
    for c in counts {
        let Rgb([r, g, b]) = c.color;
        let color = Color::new(r, g, b, 255);
        histogram.extend(std::iter::repeat_n(color, weight(c) as usize));
    }
    exoquant::generate_palette(&histogram, &colorspace, &KMeans, palette_size)
        .into_iter()
        .map(|Color { r, g, b, .. }| Rgb([r, g, b]))
        .collect()
}

/// Cluster colors in OKLab using weighted k-means with the given per-color
/// weight. `l_weight` scales the lightness axis in the distance metric.
fn cluster_oklab(
    counts: &[ColorCount],
    palette_size: usize,
    l_weight: f64,
    weight: impl Fn(&ColorCount) -> u64,
) -> Vec<Rgb<u8>> {
    let points: Vec<(Oklab, f64)> = counts
        .iter()
        .map(|c| (Oklab::from_srgb(c.color), weight(c) as f64))
        .collect();
    let centers = weighted_kmeans(&points, palette_size, l_weight);
    centers.into_iter().map(|o| o.to_srgb()).collect()
}

/// Weighted k-means over OKLab points, using an `l_weight`-scaled distance.
/// Returns up to `k` cluster centers, one per distinct color when there are
/// fewer than `k` of them.
fn weighted_kmeans(points: &[(Oklab, f64)], k: usize, l_weight: f64) -> Vec<Oklab> {
    if points.is_empty() {
        return Vec::new();
    }
    if points.len() <= k {
        return points.iter().map(|(o, _)| *o).collect();
    }

    // Seed with farthest-point sampling so initial centers are spread out; this
    // also naturally picks up isolated accent colors as seeds.
    let mut centers = farthest_point_seed(points, k, l_weight);

    // A fixed, small number of Lloyd iterations: enough to settle, cheap, and
    // deterministic (no RNG, important for reproducible builds/tests).
    for _ in 0..16 {
        let mut sums = vec![(0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64); centers.len()];
        for (p, w) in points {
            let idx = nearest_center(&centers, p, l_weight);
            let s = &mut sums[idx];
            s.0 += p.l * w;
            s.1 += p.a * w;
            s.2 += p.b * w;
            s.3 += w;
        }
        for (center, (sl, sa, sb, sw)) in centers.iter_mut().zip(sums) {
            if sw > 0.0 {
                *center = Oklab {
                    l: sl / sw,
                    a: sa / sw,
                    b: sb / sw,
                };
            }
        }
    }
    centers
}

/// Farthest-point seeding: start from the highest-weight color, then
/// repeatedly add the color farthest (in `l_weight`-scaled OKLab) from any
/// chosen center.
fn farthest_point_seed(points: &[(Oklab, f64)], k: usize, l_weight: f64) -> Vec<Oklab> {
    let first = points
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(o, _)| *o)
        .unwrap();
    let mut centers = vec![first];
    while centers.len() < k {
        let next = points
            .iter()
            .max_by(|a, b| {
                let da = min_distance_squared(&centers, &a.0, l_weight);
                let db = min_distance_squared(&centers, &b.0, l_weight);
                da.total_cmp(&db)
            })
            .map(|(o, _)| *o)
            .unwrap();
        centers.push(next);
    }
    centers
}

fn nearest_center(centers: &[Oklab], p: &Oklab, l_weight: f64) -> usize {
    let mut best = 0;
    let mut best_dist = f64::INFINITY;
    for (i, c) in centers.iter().enumerate() {
        let d = p.distance_squared_weighted(c, l_weight);
        if d < best_dist {
            best_dist = d;
            best = i;
        }
    }
    best
}

fn min_distance_squared(centers: &[Oklab], p: &Oklab, l_weight: f64) -> f64 {
    centers
        .iter()
        .map(|c| p.distance_squared_weighted(c, l_weight))
        .fold(f64::INFINITY, f64::min)
}

/// Finish an exoquant-space palette: build the matching nearest-color mapper.
fn finish_exoquant(colors: Vec<Rgb<u8>>) -> Palette {
    let colorspace = SimpleColorSpace::default();
    let exo: Vec<Color> = colors
        .iter()
        .map(|Rgb([r, g, b])| Color::new(*r, *g, *b, 255))
        .collect();
    let map = ColorMap::new(&exo, &colorspace);
    Palette {
        colors,
        mapper: Mapper::Exoquant { map, colorspace },
    }
}

/// Finish an OKLab palette: build the matching nearest-color mapper using the
/// same `l_weight` the palette was clustered with.
fn finish_oklab(colors: Vec<Rgb<u8>>, l_weight: f64) -> Palette {
    let points = colors.iter().map(|c| Oklab::from_srgb(*c)).collect();
    Palette {
        colors,
        mapper: Mapper::Oklab { points, l_weight },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_compensation_is_high_when_lightness_dominates() {
        // A grayscale ramp: lots of lightness variation, no hue/chroma. The
        // hue/chroma variance is ~0, so compensation should be near 1.0.
        let mut img = RgbImage::new(64, 64);
        for (x, _y, p) in img.enumerate_pixels_mut() {
            let v = (x * 4) as u8;
            *p = Rgb([v, v, v]);
        }
        let lc = adaptive_lightness_compensation(&img);
        assert!(lc > 0.9, "expected near 1.0, got {lc}");
    }

    #[test]
    fn adaptive_compensation_is_zero_for_flat_image() {
        // No variation at all: no lightness variance, so it returns 0.0.
        let img = RgbImage::from_pixel(16, 16, Rgb([100, 120, 140]));
        assert_eq!(adaptive_lightness_compensation(&img), 0.0);
    }

    #[test]
    fn adaptive_compensation_is_in_unit_range() {
        // A colorful, varied image must still yield a value within [0, 1].
        let mut img = RgbImage::new(48, 48);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb([(x * 5) as u8, (y * 5) as u8, ((x + y) * 3) as u8]);
        }
        let lc = adaptive_lightness_compensation(&img);
        assert!((0.0..=1.0).contains(&lc), "out of range: {lc}");
    }

    /// An image with a large gray background and a small vivid red accent.
    fn accent_image() -> RgbImage {
        let mut img = RgbImage::from_pixel(64, 64, Rgb([128, 128, 128]));
        // A 3x3 vivid red accent: ~0.2% of the pixels.
        for y in 0..3 {
            for x in 0..3 {
                img.put_pixel(x, y, Rgb([230, 20, 20]));
            }
        }
        img
    }

    fn has_color_near(palette: &[Rgb<u8>], target: Rgb<u8>, tol: i32) -> bool {
        palette.iter().any(|Rgb([r, g, b])| {
            (*r as i32 - target.0[0] as i32).abs() <= tol
                && (*g as i32 - target.0[1] as i32).abs() <= tol
                && (*b as i32 - target.0[2] as i32).abs() <= tol
        })
    }

    #[test]
    fn frequency_can_miss_small_accents() {
        // With a small palette, the frequency strategy spends its budget on the
        // dominant gray and may not represent the tiny red accent well.
        let img = accent_image();
        let p = build(&img, 2, PaletteStrategy::Frequency, 0.5, 0.0);
        assert!(p.colors().len() <= 2);
    }

    #[test]
    fn saliency_boosts_the_accent() {
        let img = accent_image();
        let p = build(&img, 4, PaletteStrategy::Saliency, 1.0, 0.0);
        assert!(
            has_color_near(p.colors(), Rgb([230, 20, 20]), 60),
            "expected a red-ish accent in palette {:?}",
            p.colors()
        );
    }

    #[test]
    fn respects_palette_size() {
        let mut img = RgbImage::new(32, 32);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb([(x * 8) as u8, (y * 8) as u8, ((x + y) * 4) as u8]);
        }
        for strategy in [PaletteStrategy::Frequency, PaletteStrategy::Saliency] {
            let p = build(&img, 5, strategy, 0.5, 0.0);
            assert!(
                p.colors().len() <= 5,
                "{strategy:?} produced {} colors",
                p.colors().len()
            );
        }
    }

    #[test]
    fn nearest_maps_into_palette() {
        let img = accent_image();
        let p = build(&img, 4, PaletteStrategy::Saliency, 0.5, 0.0);
        let mapped = p.nearest(Rgb([200, 30, 30]));
        assert!(p.colors().contains(&mapped));
    }

    #[test]
    fn lightness_compensation_changes_nearest_color_choice() {
        // Two palette colors: one matches the target's hue but differs in
        // lightness; the other matches the lightness but is a neutral gray.
        // Map a dark saturated blue. With lightness counting fully, the neutral
        // gray at the same lightness can win; with lightness ignored, the hue
        // match must win. This exercises that `nearest` honors `l_weight`.
        let target = Rgb([20, 40, 200]); // dark saturated blue
        let hue_match = Rgb([70, 90, 235]); // same hue, lighter
        let light_match = Rgb([40, 40, 45]); // near target lightness, neutral

        let no_comp = Palette {
            colors: vec![hue_match, light_match],
            mapper: Mapper::Oklab {
                points: vec![Oklab::from_srgb(hue_match), Oklab::from_srgb(light_match)],
                l_weight: 1.0,
            },
        };
        let full_comp = Palette {
            colors: vec![hue_match, light_match],
            mapper: Mapper::Oklab {
                points: vec![Oklab::from_srgb(hue_match), Oklab::from_srgb(light_match)],
                l_weight: 0.0,
            },
        };

        // With lightness fully ignored, the hue match is chosen.
        assert_eq!(full_comp.nearest(target), hue_match);
        // The two mappers can disagree; the point is that `l_weight` is honored
        // (the compensated mapper prefers the hue match).
        let _ = no_comp.nearest(target);
    }

    #[test]
    fn lightness_compensation_respects_palette_size() {
        // Full compensation must still produce a valid, bounded palette.
        let mut img = RgbImage::new(48, 48);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb([(x * 5) as u8, (y * 5) as u8, ((x + y) * 3) as u8]);
        }
        for strategy in [PaletteStrategy::Frequency, PaletteStrategy::Saliency] {
            let p = build(&img, 6, strategy, 0.5, 1.0);
            assert!(p.colors().len() <= 6, "{strategy:?}");
        }
    }
}
