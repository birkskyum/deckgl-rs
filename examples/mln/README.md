# deckgl-rs on mln-rs

deck layers drawn into a [mln-rs](https://github.com/louwers/mln-rs) basemap: one wgpu
device, one texture, all Rust.

[`examples/maplibre-ffi`](../maplibre-ffi) does the same thing against the C++
maplibre-native: wgpu-hal adopts the map's `MTLDevice`, an Objective-C++ shim carries the
frame over a C ABI, and the camera crosses a thread boundary through a mutex. mln-rs is a
Rust port of that same engine on the same wgpu version, so here the exchange is three
calls and no FFI at all:

| | |
| --- | --- |
| share the GPU | `mln_rs::Device::webgpu(instance, device, queue, format)` |
| render the map on it | `HeadlessFrontend::with_device(device, size, ..)` |
| reach the frame | `frontend.target().frames()` → `begin_frame()` → `as_webgpu()` → `TargetResource` |

`TargetResource` gives the colour view, the depth-stencil view and both formats, which is
everything deck needs to record into the same attachments.

## Running it

It needs a checkout of mln-rs beside this repository, which is why the crate is its own
workspace and excluded from the root one:

```sh
git clone https://github.com/louwers/mln-rs ../../../mln-rs
cargo run -p deckgl-mln-demo -- out.png     # from examples/mln
```

mln-rs is edition 2024 with a 1.98 MSRV, above this repository's own.

| variable | effect |
| --- | --- |
| `DECKGL_MLN_DEPTH=0` | draw deck on top instead of sharing the map's depth buffer |

## State

Working: the device is shared, the map renders on it, deck draws into the texture the map
rendered into, and the composite reads back through mln-rs as one image.

Not yet working: **interleaving**. With the map's depth attachment bound, every deck
fragment is rejected, because deck's clip depth range and the map's near and far planes
disagree. `deckgl_ffi::maplibre_near_far_pixels` already solves exactly this for
maplibre-native, and mln-rs is a port of that engine, so the same correction should
apply. Until then `DECKGL_MLN_DEPTH=0` is the honest setting, and the layers paint over
the basemap rather than being occluded by it.

One upstream change is also carried locally for now: `HeadlessFrontend` holds its
`LocalTarget` privately, so reaching the frame needs a `target()` accessor. It is one
line and fits mln-rs's existing "Port-only" host hooks (`Device::webgpu`,
`with_device`, `SurfaceTarget::set_overlay`), so it belongs upstream rather than in a
fork.
