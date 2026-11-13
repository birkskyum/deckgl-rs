# deck over an mln-rs basemap, live in the browser

The same interop as [`examples/mln`](../mln), which does it headless, but against a canvas
and every frame: one WebGPU device, one frame, the map underneath and deck's layers drawn
into it, depth tested against the map's own buildings.

```sh
./build.sh                       # needs wasm-bindgen-cli 0.2.130, exactly
python3 -m http.server -d www    # then open the page
```

mln-rs comes from crates.io, so unlike `examples/mln` this needs no sibling checkout. The
wasm-bindgen CLI has to be the version mln-rs pins:

```sh
cargo install wasm-bindgen-cli --version 0.2.130 --locked
```

## How it fits together

The page makes the device; mln-rs takes it with `Device::from_wgpu`; deck draws into the
frame the map rendered into.

| | |
| --- | --- |
| share the GPU | `mln_webgpu::Device::from_wgpu(instance, device, queue, format)` |
| draw the map | `SurfaceTarget` on the page's canvas, drawn by `Renderer` |
| draw deck into it | `SurfaceTarget::set_overlay`, after the map and before it is presented |

`set_overlay` hands an overlay only the colour view, so the map's depth buffer is taken
separately: `begin_frame()` gives the frame's draw target, and `as_webgpu(..).resource()`
is a `TargetResource` with both views on it. deck gets the depth view through a cell the
overlay reads. Without it deck could only paint over the map rather than interleave with it.

Two things have to agree for the shared depth buffer to mean anything, both of them
inherited from maplibre and both the same as in the native demo:

- the clip depth convention is OpenGL's `[-1, 1]`, so deck needs `ClipDepthRange::NegativeOneToOne`
- the far plane is maplibre's `furthest_distance * 1.01`, so deck is given the same near and far

## Differences from mln-rs's own browser example

That one runs the map in a Web Worker on an `OffscreenCanvas`, with a pool of further
workers parsing tiles over a `SerializingTransport`. This runs the map on the page's thread
against an ordinary canvas, and lets the renderer's own in-process transport parse tiles on
the cooperative scheduler, a slice at a time in `tick`. Simpler, and enough for a demo;
their arrangement is the one to copy for anything that needs the parallelism.

The bootstrap otherwise follows `examples/web/src/map.rs` in mln-rs, which is BSD-2-Clause.

## Scenes

The eight geographic scenes from `examples/json`, the ones a basemap underneath adds
something to: `san-francisco`, `vancouver-blocks`, `maplibre-arcs`, `heathrow-flights`,
`shadows`, `data-filter`, `osm-tiles` and `terrain`. `build.sh` copies them into `www/scenes`.
