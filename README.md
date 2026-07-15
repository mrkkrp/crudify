# Crudify

[![CI](https://github.com/mrkkrp/crudify/actions/workflows/ci.yaml/badge.svg)](https://github.com/mrkkrp/crudify/actions/workflows/ci.yaml)

Reduce resolution and number of colors in preparation for painting.

## Usage

```console
$ crudify config.yaml [config.yaml ...]
```

Crudify takes one or more arguments: the paths to YAML configuration files,
each of which is processed in turn. All paths mentioned in a configuration
are resolved relative to the directory that contains that configuration file
itself.

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
    short_side: 48
    palette_size: 16
  - output: poster.png
    short_side: 72
    palette_size: 24
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
* `short_side` (required): Target size, in pixels, of the *shorter* of the
  two output dimensions (width or height). The longer dimension is derived
  from it so that the output preserves the aspect ratio of the input image.
* `palette_size` (required): Maximum number of distinct colors in the
  output. This is an upper bound only: fewer colors may be used when the
  image does not contain that many, and it places no constraint on *which*
  colors are used.
* `palette_strategy` (optional, default `saliency`): How the palette is
  chosen. See [Palette strategies](#palette-strategies).
* `lightness_compensation` (optional): How strongly to de-emphasize
  lightness when clustering, in the range `0.0..=1.0`. At `0.0` lightness
  counts fully; at `1.0` it is ignored, so colors are separated purely by
  hue and chroma. This keeps dark but saturated hues (such as blue) from
  being absorbed into large clusters of mid-lightness colors. When omitted,
  the value is chosen automatically from the input image so that lightness
  and hue/chroma contribute equally to clustering (in photographs, where
  lightness varies much more than hue, this lands close to `1.0`). Set an
  explicit value to override. Ignored by the `frequency` strategy.

### Palette strategies

A plain frequency-weighted quantizer spends its color budget on whatever
dominates the image by pixel count, so small but vivid accents get averaged
away and the result looks "muddy". The `saliency` strategy instead biases
palette selection toward perceptually important colors so that accents
survive, clustering in the perceptual OKLab color space where distinct hues
resist being merged.

* `frequency`: Frequency-weighted clustering. The original baseline; tends
  to average away small vivid accents.
* `saliency`: Reweights the histogram to favor vivid and rare colors, then
  clusters in OKLab. The default.

## License

Copyright 2026–present Mark Karpov

Distributed under the MIT license.
