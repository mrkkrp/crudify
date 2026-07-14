//! Palette selection.
//!
//! A frequency-weighted quantizer spends its color budget on whatever dominates
//! the image by pixel count, so small but vivid accents get averaged into the
//! nearest large cluster and the result looks "muddy". The strategies here bias
//! selection toward perceptually important colors so accents survive:
//!
//! * [`PaletteStrategy::Frequency`] — the original behavior, no bias.
//! * `Saliency*` — reweight colors by vividness and rarity before clustering.
//! * `ReserveAccents*` — detect standout accent colors and reserve palette
//!   slots for them, clustering the remaining budget for everything else.
//!
//! Each strategy can cluster either in exoquant's color space or in the
//! perceptual OKLab space, where distances match human vision and distinct hues
//! resist being merged.

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
    /// Lookup by Euclidean distance in OKLab.
    Oklab { points: Vec<Oklab> },
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
            Mapper::Oklab { points } => {
                let target = Oklab::from_srgb(color);
                let mut best = 0;
                let mut best_dist = f64::INFINITY;
                for (i, p) in points.iter().enumerate() {
                    let d = target.distance_squared(p);
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
/// strategies favor vivid and rare colors; `accent_slots`, when given, sets how
/// many slots the reserve-accents strategies dedicate to accent colors.
pub fn build(
    source: &RgbImage,
    palette_size: u32,
    strategy: PaletteStrategy,
    accent_strength: f64,
    accent_slots: Option<u32>,
) -> Palette {
    let palette_size = palette_size.max(1) as usize;
    let accent_strength = accent_strength.clamp(0.0, 1.0);
    let counts = color_counts(source);

    match strategy {
        PaletteStrategy::Frequency => {
            let colors = cluster_exoquant(&counts, palette_size, |c| c.count);
            finish_exoquant(colors)
        }
        PaletteStrategy::Saliency => {
            let colors = cluster_exoquant(&counts, palette_size, |c| {
                saliency_weight(c, accent_strength)
            });
            finish_exoquant(colors)
        }
        PaletteStrategy::SaliencyOklab => {
            let colors = cluster_oklab(&counts, palette_size, |c| {
                saliency_weight(c, accent_strength)
            });
            finish_oklab(colors)
        }
        PaletteStrategy::ReserveAccents => {
            let colors = reserve_accents(
                &counts,
                palette_size,
                accent_strength,
                accent_slots,
                Space::Exoquant,
            );
            finish_exoquant(colors)
        }
        PaletteStrategy::ReserveAccentsOklab => {
            let colors = reserve_accents(
                &counts,
                palette_size,
                accent_strength,
                accent_slots,
                Space::Oklab,
            );
            finish_oklab(colors)
        }
    }
}

/// Which color space a set of palette colors was chosen in, so we can build a
/// matching [`Mapper`].
enum Space {
    Exoquant,
    Oklab,
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
/// weight.
fn cluster_oklab(
    counts: &[ColorCount],
    palette_size: usize,
    weight: impl Fn(&ColorCount) -> u64,
) -> Vec<Rgb<u8>> {
    let points: Vec<(Oklab, f64)> = counts
        .iter()
        .map(|c| (Oklab::from_srgb(c.color), weight(c) as f64))
        .collect();
    let centers = weighted_kmeans(&points, palette_size);
    centers.into_iter().map(|o| o.to_srgb()).collect()
}

/// Weighted k-means over OKLab points. Returns up to `k` cluster centers, one
/// per distinct color when there are fewer than `k` of them.
fn weighted_kmeans(points: &[(Oklab, f64)], k: usize) -> Vec<Oklab> {
    if points.is_empty() {
        return Vec::new();
    }
    if points.len() <= k {
        return points.iter().map(|(o, _)| *o).collect();
    }

    // Seed with farthest-point sampling so initial centers are spread out; this
    // also naturally picks up isolated accent colors as seeds.
    let mut centers = farthest_point_seed(points, k);

    // A fixed, small number of Lloyd iterations: enough to settle, cheap, and
    // deterministic (no RNG, important for reproducible builds/tests).
    for _ in 0..16 {
        let mut sums = vec![(0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64); centers.len()];
        for (p, w) in points {
            let idx = nearest_center(&centers, p);
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
/// repeatedly add the color farthest (in OKLab) from any chosen center.
fn farthest_point_seed(points: &[(Oklab, f64)], k: usize) -> Vec<Oklab> {
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
                let da = min_distance_squared(&centers, &a.0);
                let db = min_distance_squared(&centers, &b.0);
                da.total_cmp(&db)
            })
            .map(|(o, _)| *o)
            .unwrap();
        centers.push(next);
    }
    centers
}

fn nearest_center(centers: &[Oklab], p: &Oklab) -> usize {
    let mut best = 0;
    let mut best_dist = f64::INFINITY;
    for (i, c) in centers.iter().enumerate() {
        let d = p.distance_squared(c);
        if d < best_dist {
            best_dist = d;
            best = i;
        }
    }
    best
}

fn min_distance_squared(centers: &[Oklab], p: &Oklab) -> f64 {
    centers
        .iter()
        .map(|c| p.distance_squared(c))
        .fold(f64::INFINITY, f64::min)
}

/// Reserve slots for detected accent colors, then cluster the rest.
///
/// Accents are the most saturated colors present, chosen to be distinct from
/// one another. The remaining budget is clustered normally (frequency-weighted)
/// so the bulk of the image is still well represented.
fn reserve_accents(
    counts: &[ColorCount],
    palette_size: usize,
    accent_strength: f64,
    accent_slots: Option<u32>,
    space: Space,
) -> Vec<Rgb<u8>> {
    let requested = accent_slots
        .map(|n| n as usize)
        .unwrap_or_else(|| (accent_strength * palette_size as f64).round() as usize);
    // Never reserve the whole budget; leave room for the bulk clusters.
    let want_accents = requested.min(palette_size.saturating_sub(1));

    let accents = detect_accents(counts, want_accents);
    let remaining = palette_size - accents.len();

    let bulk = match space {
        Space::Exoquant => cluster_exoquant(counts, remaining, |c| c.count),
        Space::Oklab => cluster_oklab(counts, remaining, |c| c.count),
    };

    let mut colors = accents;
    colors.extend(bulk);
    dedup_colors(colors)
}

/// Pick up to `n` accent colors: highly saturated colors that are mutually
/// distinct in OKLab. Returns fewer if the image has few saturated colors.
fn detect_accents(counts: &[ColorCount], n: usize) -> Vec<Rgb<u8>> {
    if n == 0 {
        return Vec::new();
    }
    // Consider only reasonably saturated colors as accent candidates.
    let mut candidates: Vec<(Oklab, f64)> = counts
        .iter()
        .map(|c| (Oklab::from_srgb(c.color), c.count as f64))
        .filter(|(o, _)| o.chroma_ratio() >= ACCENT_MIN_CHROMA)
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    // Rank by chroma so the most vivid win, then spread by farthest-point so we
    // do not pick several near-duplicates of the same accent.
    candidates.sort_by(|a, b| b.0.chroma().total_cmp(&a.0.chroma()));

    let mut chosen: Vec<Oklab> = vec![candidates[0].0];
    for (o, _) in candidates.iter().skip(1) {
        if chosen.len() >= n {
            break;
        }
        if min_distance_squared(&chosen, o) >= ACCENT_MIN_SEPARATION * ACCENT_MIN_SEPARATION {
            chosen.push(*o);
        }
    }
    chosen.into_iter().map(|o| o.to_srgb()).collect()
}

/// Minimum OKLab chroma for a color to be considered an accent candidate.
const ACCENT_MIN_CHROMA: f64 = 0.08;
/// Minimum OKLab distance between two accents so we do not pick duplicates.
const ACCENT_MIN_SEPARATION: f64 = 0.06;

/// Remove exact duplicate colors, preserving order (accents come first).
fn dedup_colors(colors: Vec<Rgb<u8>>) -> Vec<Rgb<u8>> {
    let mut seen = std::collections::HashSet::new();
    colors.into_iter().filter(|c| seen.insert(c.0)).collect()
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

/// Finish an OKLab palette: build the matching nearest-color mapper.
fn finish_oklab(colors: Vec<Rgb<u8>>) -> Palette {
    let points = colors.iter().map(|c| Oklab::from_srgb(*c)).collect();
    Palette {
        colors,
        mapper: Mapper::Oklab { points },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let p = build(&img, 2, PaletteStrategy::Frequency, 0.5, None);
        assert!(p.colors().len() <= 2);
    }

    #[test]
    fn reserve_accents_preserves_the_accent() {
        let img = accent_image();
        let p = build(&img, 4, PaletteStrategy::ReserveAccentsOklab, 0.5, Some(1));
        assert!(
            has_color_near(p.colors(), Rgb([230, 20, 20]), 40),
            "expected a red-ish accent in palette {:?}",
            p.colors()
        );
    }

    #[test]
    fn saliency_oklab_boosts_the_accent() {
        let img = accent_image();
        let p = build(&img, 4, PaletteStrategy::SaliencyOklab, 1.0, None);
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
        for strategy in [
            PaletteStrategy::Frequency,
            PaletteStrategy::Saliency,
            PaletteStrategy::SaliencyOklab,
            PaletteStrategy::ReserveAccents,
            PaletteStrategy::ReserveAccentsOklab,
        ] {
            let p = build(&img, 5, strategy, 0.5, None);
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
        let p = build(&img, 4, PaletteStrategy::SaliencyOklab, 0.5, None);
        let mapped = p.nearest(Rgb([200, 30, 30]));
        assert!(p.colors().contains(&mapped));
    }
}
