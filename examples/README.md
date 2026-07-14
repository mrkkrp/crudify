# Examples

Helper programs used for manual and smoke testing of crudify. These are not
part of the published library or executable; they are convenience tools for
development.

## `gen_test_image`

Generates a small 200×150 RGB test image (a gradient with many distinct
colors) that can be fed to crudify as an input.

```console
$ cargo run --example gen_test_image -- input.png
```

A typical end-to-end check then looks like:

```console
$ cargo run --example gen_test_image -- input.png
$ cat > config.yaml <<'EOF'
input: input.png
derivations:
  - output: out.png
    width: 64
    height: 48
    palette_size: 16
EOF
$ cargo run -- config.yaml
```
