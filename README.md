# Spumcake-Render

Spumcake's renderer for scene files: a place's 3D pre-vis, exported from its pre-vis builder as glTF 2.0 binary
(`.glb`), as Spumcake's scene contract describes. It draws a scene the way its builder drew it, so from each of
the builder's cameras its picture is the builder's published render, edges and all.

Spumcake Studio uses it as a library, to show scenes in its 3D view. Spumcake-Skills and Claude Code run its
command-line program, with no Studio installed.

## Crates

- `scene` reads a scene file: its objects, cameras, looks, and the door's swing.
- `spumcake-render` draws one frame of a scene, on wgpu, from a named camera or a free one, in a look, at a size
  and a time in the scene's animation. Each frame gives the colour image, the depth in metres, and which object
  each pixel shows.

## How it matches the builders

A pre-vis builder is a Python script with its own small renderer: it projects each face, fills it with Pillow's
`ImageDraw.polygon`, and keeps the nearest face at each pixel, in drawing order. A GPU's usual rasterization
covers other pixels along every boundary, enough to miss the published renders. So the engine fills every face
on the CPU by Pillow 12.1.1's own scanline rule, checked against Pillow itself in
`spumcake-render/tests/fixtures/`, and the GPU settles, for every pixel, which face is nearest and where the
edges fall.

## Looks

- **Grey:** a flat grey for each kind of object, shaded by the scene's light.
- **Colour-coded:** a flat colour for each kind, shaded the same way.
- **Simple:** white faces with no shading, the night beyond the door in dark grey, and only the objects the
  look keeps.

## The command line

```text
spumcake-render render <scene.glb> --camera <name> --look grey|coded|simple --size <width>x<height>
                       [--time <seconds>] --out <file.png> [--depth <file.png>] [--objects <file.png>]
spumcake-render --version
```

- `--out` is the picture, an RGB PNG.
- `--depth` is a 16-bit grey PNG in millimetres along the camera's forward axis, 0 where nothing is drawn.
- `--objects` is a 16-bit grey PNG: 0 where nothing is drawn, otherwise the object's number, its index in the
  scene plus one, with `<file.png>.json` beside it naming each number: `{"0": null, "1": "<object name>", …}`.
- `--version` prints `spumcake-render <version>`, such as `spumcake-render 0.1.0`.

It exits with 0 when the files are written, 1 when something else fails, 2 when the command is not understood,
and 3, with a message starting "no graphics device", when it finds no GPU or software renderer to draw with.

## Checks

`cargo test` checks the fill rule against Pillow. With `LAST_ORDER` set to the Last order benchmark's folder, it
also checks the engine against the diner's published pre-vis renders, records, and door-swing video, as the
scene contract asks; the door swing's check needs ffmpeg. The checks render on Mesa's lavapipe, so their result
does not depend on the GPU.

## Requirements

Rust 1.95 or later, and a GPU with Vulkan, Metal, or DirectX 12. Where there is no GPU, a software renderer
such as Mesa's lavapipe works too: set `SPUMCAKE_RENDER_ADAPTER=software` to choose it.

## License

Licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the [MIT license](LICENSE-MIT), at
your option.
