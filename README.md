# Crudify

Reduce resolution and number of colors in preparation for painting.

## Usage

```console
$ crudify config.yaml
```

Crudify takes a single argument: the path to a YAML configuration file. All
paths mentioned in the configuration are resolved relative to the directory
that contains the configuration file itself.

For each *derivation*, crudify reduces the input image to the requested
resolution and palette, then upscales the result back to approximately the
input resolution and overlays a painting grid.

## Configuration

The configuration file is a YAML document with two required top-level
fields:

```yaml
input: photo.png
derivations:
  - output: small.png
    width: 64
    height: 48
    palette_size: 16
  - output: poster.png
    width: 96
    height: 72
    palette_size: 24
    palette_strategy: saliency
    accent_strength: 0.7
```

### Top-level fields

* `input` (required): Path to the input image, relative to the configuration
  file. PNG and JPEG are supported.
* `derivations` (required): A list of derivations to produce from the input
  image. An empty list is accepted and produces no output.

### Derivation fields

Each entry in `derivations` describes one output image.

* `output` (required): Path to the output image, relative to the
  configuration file. Must have a `.png` extension. Output is restricted
  to PNG because lossy formats such as JPEG reintroduce colors on decode,
  which would defeat the palette reduction; a `.jpg`/`.jpeg` output is
  rejected.
* `width` (required): Target width.
* `height` (required): Target height.
* `palette_size` (required): Maximum number of distinct colors in the
  output. This is an upper bound only: fewer colors may be used when the
  image does not contain that many, and it places no constraint on *which*
  colors are used.
* `palette_strategy` (optional, default `frequency`): How the palette is
  chosen. See [Palette strategies](#palette-strategies).
* `accent_strength` (optional, default `0.5`): How strongly the saliency
  strategies favor vivid and rare colors, in the range `0.0..=1.0`. Ignored
  by the `frequency` strategy.
* `accent_slots` (optional, default strategy-chosen): Number of palette
  slots reserved for detected accent colors. Used only by the
  `reserve_accents` strategies. When absent, a strategy-chosen number is
  used.
* `lightness_compensation` (optional, default `0.0`): How strongly to
  de-emphasize lightness when clustering, in the range `0.0..=1.0`. At
  `0.0` lightness counts fully; at `1.0` it is ignored, so colors are
  separated purely by hue and chroma. This keeps dark but saturated hues
  (such as blue) from being absorbed into large clusters of
  mid-lightness colors. Ignored by the `frequency` strategy.

`width` and `height` must preserve the aspect ratio of the input image
(within a small tolerance); otherwise the derivation is rejected. The
upscale factor that brings the result back to approximately the input
resolution is derived automatically from that ratio.

### Palette strategies

A plain frequency-weighted quantizer spends its color budget on whatever
dominates the image by pixel count, so small but vivid accents get averaged
away and the result looks "muddy". The alternative strategies bias palette
selection toward perceptually important colors so that accents survive.
Apart from `frequency`, they cluster in the perceptual OKLab color space,
where distinct hues resist being merged.

* `frequency`: Frequency-weighted clustering. The default; tends to average
  away small vivid accents.
* `saliency`: Reweights the histogram to favor vivid (`accent_strength`) and
  rare colors, then clusters in OKLab.
* `reserve_accents`: Reserves `accent_slots` slots for detected accent
  colors, then clusters the remaining budget for everything else in OKLab.

## License

Copyright 2026–present Mark Karpov

Distributed under the MIT license.
