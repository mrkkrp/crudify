//! Upscaling the pixelated image and overlaying a painting grid.
//!
//! The pixelated image produced by [`crate::pixelate`] is small: one pixel per
//! "original pixel". For painting it is more useful to see those pixels at
//! roughly the original resolution, separated by a grid. Each original pixel
//! is drawn as a square block of `cell` output pixels, and white grid lines
//! are drawn between blocks.
//!
//! The grid has three line thicknesses:
//!
//! * the **thinnest** (1px) lines separate every original pixel;
//! * **medium** lines divide each half of the image into quarters (the 1/4 and
//!   3/4 boundaries);
//! * the **thickest** lines divide the whole image in half, horizontally and
//!   vertically (the 1/2 boundary).
//!
//! A line at a coarser level replaces the finer line that would otherwise sit
//! at the same boundary, so each boundary is drawn exactly once at its
//! coarsest applicable thickness.

use anyhow::{Result, ensure};
use image::{Rgb, RgbImage};

/// Color of the grid lines.
const GRID_COLOR: Rgb<u8> = Rgb([255, 255, 255]);

/// Line thickness, in output pixels, for each grid level.
const THIN_WIDTH: u32 = 1;
const MEDIUM_WIDTH: u32 = 2;
const THICK_WIDTH: u32 = 3;

/// The largest number of original pixels, in either dimension, for which the
/// thin inter-pixel grid lines are still drawn. Beyond this the fine grid is
/// omitted (the coarser dividers remain).
const MAX_FINE_GRID_DIM: u32 = 100;

/// Maximum relative deviation allowed between the input aspect ratio and the
/// target `width:height` aspect ratio.
const ASPECT_TOLERANCE: f64 = 0.01;

/// Upscale `pixelated` so each of its pixels becomes a `cell`x`cell` block, and
/// overlay the painting grid. `input_dims` is the `(width, height)` of the
/// original input image, used to choose the upscale factor so the result is
/// approximately the input resolution.
///
/// Errors if the aspect ratio of `pixelated` does not match that of the input
/// image within [`ASPECT_TOLERANCE`].
pub fn render(pixelated: &RgbImage, input_dims: (u32, u32)) -> Result<RgbImage> {
    let (grid_w, grid_h) = pixelated.dimensions();
    let (in_w, in_h) = input_dims;
    ensure!(
        grid_w > 0 && grid_h > 0,
        "pixelated image must be non-empty"
    );
    ensure!(in_w > 0 && in_h > 0, "input image must be non-empty");

    check_aspect_ratio(in_w, in_h, grid_w, grid_h)?;
    let cell = cell_size(in_w, in_h, grid_w, grid_h);

    // With many original pixels, a 1px line between every pair would drown the
    // image in grid, so the finest (inter-pixel) lines are dropped past a
    // threshold; the coarser half/quarter dividers are always kept.
    let fine_grid = grid_w <= MAX_FINE_GRID_DIM && grid_h <= MAX_FINE_GRID_DIM;

    // Precompute the line thickness at each interior boundary. Boundary `i`
    // (for i in 1..grid_dim) sits between original pixels i-1 and i.
    let x_lines = boundary_widths(grid_w, fine_grid);
    let y_lines = boundary_widths(grid_h, fine_grid);

    // The total size is the blocks plus every line's pixels laid between them.
    let out_w = grid_w * cell + x_lines.iter().sum::<u32>();
    let out_h = grid_h * cell + y_lines.iter().sum::<u32>();

    // Where each column/row of blocks starts in the output, accounting for the
    // lines preceding it.
    let x_offsets = block_offsets(grid_w, cell, &x_lines);
    let y_offsets = block_offsets(grid_h, cell, &y_lines);

    let mut output = RgbImage::from_pixel(out_w, out_h, GRID_COLOR);
    for gy in 0..grid_h {
        for gx in 0..grid_w {
            let color = *pixelated.get_pixel(gx, gy);
            let x0 = x_offsets[gx as usize];
            let y0 = y_offsets[gy as usize];
            for dy in 0..cell {
                for dx in 0..cell {
                    output.put_pixel(x0 + dx, y0 + dy, color);
                }
            }
        }
    }

    Ok(output)
}

/// Error unless the target `grid_w:grid_h` matches the input `in_w:in_h` aspect
/// ratio within [`ASPECT_TOLERANCE`].
fn check_aspect_ratio(in_w: u32, in_h: u32, grid_w: u32, grid_h: u32) -> Result<()> {
    let input_ratio = in_w as f64 / in_h as f64;
    let target_ratio = grid_w as f64 / grid_h as f64;
    let deviation = (input_ratio - target_ratio).abs() / input_ratio;
    ensure!(
        deviation <= ASPECT_TOLERANCE,
        "target dimensions {grid_w}x{grid_h} (aspect {target_ratio:.4}) do not \
         preserve the input aspect ratio {input_ratio:.4} (deviation {:.2}% > {:.2}%)",
        deviation * 100.0,
        ASPECT_TOLERANCE * 100.0
    );
    Ok(())
}

/// The integer upscale factor bringing the `grid_w`x`grid_h` image back to
/// approximately the `in_w`x`in_h` input resolution. Both axes agree closely
/// because the aspect ratio has already been checked, so we average them.
fn cell_size(in_w: u32, in_h: u32, grid_w: u32, grid_h: u32) -> u32 {
    let ratio = (in_w as f64 / grid_w as f64 + in_h as f64 / grid_h as f64) / 2.0;
    (ratio.round() as u32).max(1)
}

/// The line thickness for every interior boundary of a `dim`-pixel axis.
///
/// Index `i` in the returned vector (length `dim - 1`) is the thickness of the
/// boundary between original pixels `i` and `i + 1`. The half boundary is
/// thickest, the quarter boundaries medium, all others thin. A coarser level
/// wins where boundaries coincide.
///
/// When `fine_grid` is false, the thin inter-pixel lines are omitted (width
/// `0`); only the half and quarter dividers remain.
fn boundary_widths(dim: u32, fine_grid: bool) -> Vec<u32> {
    if dim < 2 {
        return Vec::new();
    }
    // Boundary positions, as pixel counts from the start, that get thicker
    // lines. Using rounding keeps them centered when `dim` is not divisible.
    let half = ((dim as f64) / 2.0).round() as u32;
    let quarter = ((dim as f64) / 4.0).round() as u32;
    let three_quarter = ((dim as f64) * 3.0 / 4.0).round() as u32;

    (1..dim)
        .map(|i| {
            if i == half {
                THICK_WIDTH
            } else if i == quarter || i == three_quarter {
                MEDIUM_WIDTH
            } else if fine_grid {
                THIN_WIDTH
            } else {
                0
            }
        })
        .collect()
}

/// The output coordinate at which each block starts, for `count` blocks of
/// `cell` pixels separated by the given interior `lines` (length `count - 1`).
fn block_offsets(count: u32, cell: u32, lines: &[u32]) -> Vec<u32> {
    let mut offsets = Vec::with_capacity(count as usize);
    let mut pos = 0;
    for i in 0..count {
        offsets.push(pos);
        pos += cell;
        if let Some(line) = lines.get(i as usize) {
            pos += line;
        }
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: Rgb<u8>) -> RgbImage {
        RgbImage::from_pixel(w, h, c)
    }

    #[test]
    fn rejects_mismatched_aspect_ratio() {
        let px = solid(64, 64, Rgb([0, 0, 0])); // 1:1
        // Input 2:1 is far from 1:1.
        assert!(render(&px, (200, 100)).is_err());
    }

    #[test]
    fn accepts_matching_aspect_ratio() {
        let px = solid(64, 48, Rgb([1, 2, 3])); // 4:3
        assert!(render(&px, (200, 150)).is_ok()); // also 4:3
    }

    #[test]
    fn tolerates_small_aspect_deviation() {
        // 200x150 is 4:3; 64x48 is exactly 4:3; nudge input slightly.
        let px = solid(64, 48, Rgb([1, 2, 3]));
        assert!(render(&px, (201, 150)).is_ok());
    }

    #[test]
    fn output_size_is_blocks_plus_lines() {
        // 4x4 grid: 3 interior boundaries per axis, at positions 1, 2, 3.
        // quarter = 1 (medium, 2), half = 2 (thick, 3), three_quarter = 3
        // (medium, 2) => sum = 7 line pixels per axis.
        let px = solid(4, 4, Rgb([9, 9, 9]));
        let cell = cell_size(40, 40, 4, 4); // 40/4 = 10
        assert_eq!(cell, 10);
        let out = render(&px, (40, 40)).unwrap();
        // 4*10 + 7 = 47 on each axis.
        assert_eq!(out.dimensions(), (47, 47));
    }

    #[test]
    fn cell_size_is_at_least_one() {
        // Grid larger than input -> ratio < 1 -> clamps to 1.
        assert_eq!(cell_size(10, 10, 64, 64), 1);
    }

    #[test]
    fn grid_lines_are_white_and_blocks_preserve_color() {
        let mut px = RgbImage::new(2, 2);
        px.put_pixel(0, 0, Rgb([10, 20, 30]));
        px.put_pixel(1, 0, Rgb([40, 50, 60]));
        px.put_pixel(0, 1, Rgb([70, 80, 90]));
        px.put_pixel(1, 1, Rgb([100, 110, 120]));
        // 2x2 grid, one interior boundary per axis (the half boundary, thick=3).
        let out = render(&px, (20, 20)).unwrap();
        let cell = cell_size(20, 20, 2, 2); // 10
        // top-left block should be the first pixel's color.
        assert_eq!(*out.get_pixel(0, 0), Rgb([10, 20, 30]));
        // the boundary column between the two blocks should be white.
        let boundary_x = cell; // first line starts right after the first block
        assert_eq!(*out.get_pixel(boundary_x, 0), GRID_COLOR);
    }

    #[test]
    fn boundary_widths_marks_half_and_quarters() {
        // dim=8: half at 4 (thick), quarter at 2, three_quarter at 6 (medium).
        let w = boundary_widths(8, true);
        assert_eq!(w.len(), 7);
        assert_eq!(w[3], THICK_WIDTH); // boundary index 3 == position 4
        assert_eq!(w[1], MEDIUM_WIDTH); // position 2
        assert_eq!(w[5], MEDIUM_WIDTH); // position 6
        assert_eq!(w[0], THIN_WIDTH);
    }

    #[test]
    fn coarse_grid_omits_thin_lines_but_keeps_dividers() {
        // Without the fine grid, thin boundaries are 0 while the half and
        // quarter dividers still carry their thickness.
        let w = boundary_widths(8, false);
        assert_eq!(w.len(), 7);
        assert_eq!(w[3], THICK_WIDTH); // half divider kept
        assert_eq!(w[1], MEDIUM_WIDTH); // quarter divider kept
        assert_eq!(w[5], MEDIUM_WIDTH); // three-quarter divider kept
        assert_eq!(w[0], 0); // thin inter-pixel line dropped
        assert_eq!(w[2], 0);
    }

    #[test]
    fn fine_grid_dropped_past_threshold() {
        // A grid larger than the threshold in one dimension must not draw thin
        // lines. Build a 101x76 grid (~4:3) against a matching input; count the
        // thin boundaries that survive: only the dividers should remain.
        let px = solid(101, 76, Rgb([5, 5, 5]));
        let out = render(&px, (101, 76)).unwrap();
        // cell = 1, so output width = 101*1 + sum(x_lines). If the fine grid
        // were on, x_lines would sum to ~100+; with it off, only 3 dividers
        // per axis (quarter+half+three_quarter) contribute.
        let expected_x_lines = MEDIUM_WIDTH + THICK_WIDTH + MEDIUM_WIDTH;
        assert_eq!(out.dimensions().0, 101 + expected_x_lines);
    }

    #[test]
    fn fine_grid_kept_at_threshold() {
        // Exactly at the threshold (100) the fine grid is still drawn.
        let px = solid(100, 75, Rgb([5, 5, 5])); // 4:3
        let out = render(&px, (100, 75)).unwrap();
        // With the fine grid on, there are far more than the 3 divider lines,
        // so the output width exceeds 100 + the divider-only total.
        let dividers_only = 100 + MEDIUM_WIDTH + THICK_WIDTH + MEDIUM_WIDTH;
        assert!(out.dimensions().0 > dividers_only);
    }
}
