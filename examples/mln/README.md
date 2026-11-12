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

Working: the device is shared, the map renders on it, deck draws into the texture and the
depth buffer the map rendered into, and the composite reads back through mln-rs as one
image.

Sharing the depth buffer took two corrections, both of which fall out of mln-rs being a
faithful port. Its `mat4::perspective` is the OpenGL one, writing `(far + near) / (near -
far)`, so clip z runs `[-1, 1]` and deck needs `ClipDepthRange::NegativeOneToOne`. And its
far plane is maplibre's `furthest_distance * 1.01`, so `maplibre_near_far_pixels` from the
FFI crate gives deck the same frustum. Without those, every deck fragment was rejected and
the layers vanished.

**Not yet demonstrated: occlusion.** The style here is a bare background, which maplibre
draws with depth `Always` and `ReadOnly`, so the map writes nothing into the depth buffer
and there is nothing for it to hide. Interleaved and `DECKGL_MLN_DEPTH=0` output is
byte-identical for this scene. Showing deck's ground circles notched out by 3D buildings,
the way the maplibre-native GLFW host does, needs a style with `fill-extrusion` and
therefore vector tiles over the network.

One upstream change is also carried locally for now: `HeadlessFrontend` holds its
`LocalTarget` privately, so reaching the frame needs a `target()` accessor. It is one
line and fits mln-rs's existing "Port-only" host hooks (`Device::webgpu`,
`with_device`, `SurfaceTarget::set_overlay`), so it belongs upstream rather than in a
fork.
