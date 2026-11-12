//! deckgl-rs layers drawn into an mln-rs basemap, all in Rust on one wgpu device.
//!
//! The maplibre-ffi demo next door does the same thing across a C ABI: maplibre-native
//! renders through Metal, wgpu-hal adopts its `MTLDevice`, and an Objective-C++ shim
//! carries the frame across. Here both halves are Rust on wgpu, so the whole exchange is
//! three calls: we make the device, mln-rs renders the map on it, and deck draws into the
//! texture it rendered into, against the map's own depth buffer.

use std::sync::Arc;

use deckgl::lumagl::RenderTarget;
use deckgl::{Deck, DeckProps, ViewState};
use deckgl_json::JsonConverter;

use mln_rs::mln_actor::RunLoop;
use mln_rs::mln_gfx::{BackendScope, ScopeType};
use mln_rs::mln_headless::HeadlessFrontend;
use mln_rs::mln_map::{CameraOptions, Map, MapMode, MapOptions};
use mln_rs::mln_util::geo::LatLng;
use mln_rs::mln_util::size::Size;
use mln_rs::mln_webgpu::draw_target::{as_webgpu, TargetResource};
use mln_rs::Device as MlnDevice;

/// The map's camera, as deck's view state.
///
/// Centre, zoom, pitch and bearing are enough to put the layers over the right ground.
/// Making the two agree in *depth* as well needs the map's near and far planes, the way
/// `deckgl_ffi::maplibre_near_far_pixels` does for maplibre-native; until that is wired
/// up here, geometry is in the right place but its depth is deck's own.
fn view_state(camera: &CameraOptions) -> ViewState {
    let center = camera.center.unwrap_or_else(|| LatLng::new(0.0, 0.0).expect("the origin is a valid coordinate"));
    ViewState {
        longitude: center.longitude(),
        latitude: center.latitude(),
        zoom: camera.zoom.unwrap_or(0.0),
        pitch: camera.pitch.unwrap_or(0.0),
        bearing: camera.bearing.unwrap_or(0.0),
    }
}

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;

/// A basemap with ground and water, so there is something for deck to sit on.
const STYLE: &str = r##"{
  "version": 8,
  "sources": {},
  "layers": [
    {"id": "bg", "type": "background", "paint": {"background-color": "#1b2432"}}
  ]
}"##;

/// Deck layers over the map.
const SCENE: &str = r#"[
  {
    "@@type": "ScatterplotLayer",
    "id": "points",
    "data": [
      {"position": [-122.42, 37.775]},
      {"position": [-122.40, 37.785]},
      {"position": [-122.44, 37.765]}
    ],
    "getPosition": "@@=position",
    "getRadius": 400,
    "getFillColor": [255, 140, 0, 220],
    "radiusUnits": "meters"
  },
  {
    "@@type": "SolidPolygonLayer",
    "id": "block",
    "data": [{"polygon": [[-122.425, 37.770], [-122.395, 37.770], [-122.395, 37.800], [-122.425, 37.800]]}],
    "extruded": true,
    "getPolygon": "@@=polygon",
    "getElevation": 600,
    "getFillColor": [0, 150, 255, 200]
  }
]"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. We own the GPU. deck creates the instance, adapter, device and queue.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("deckgl-rs + mln-rs"),
        ..Default::default()
    }))?;
    println!("adapter: {:?}", adapter.get_info().name);

    // 2. mln-rs renders on it. `Device::webgpu` is its hook for a host's wgpu handles;
    //    wgpu's Device and Queue are Arc-backed, so these clones are the same objects.
    let color_format = wgpu::TextureFormat::Rgba8Unorm;
    let mln_device = MlnDevice::webgpu(instance, device.clone(), queue.clone(), color_format);

    let _run_loop = RunLoop::new();
    let size = Size::new(WIDTH, HEIGHT);
    let mut frontend = HeadlessFrontend::with_device(mln_device, size, 1.0, true, Arc::default())?;

    let mut map = Map::new(
        MapOptions::new()
            .with_map_mode(MapMode::Static)
            .with_size(size)
            .with_pixel_ratio(1.0),
    )?;
    frontend.attach(&map);
    map.style_mut().load_json(STYLE);
    map.jump_to(
        &CameraOptions::default()
            .with_center(Some(LatLng::new(37.775, -122.42)?))
            .with_zoom(Some(13.5))
            .with_pitch(Some(55.0))
            .with_bearing(Some(-25.0)),
    );

    // 3. The map draws its frame into its own offscreen texture, on our device.
    //    `render` is the still-image path: it pumps the run loop until the style is
    //    ready, renders, and reads the result back.
    let map_only = frontend.render(&mut map)?.image;
    println!(
        "map alone: first pixel {:?}, non-transparent {}",
        &map_only.data[0..4],
        map_only.data.chunks(4).filter(|p| p[3] != 0).count()
    );

    // 4. Reach the texture it drew into, and its depth buffer. mln-rs wants a backend
    //    scope held while its context is touched, as its own render paths do.
    let backend = frontend.get_backend();
    let _scope = BackendScope::new(backend, ScopeType::Explicit);

    let frames = frontend.target().frames().ok_or("the target has no frames")?;
    let draw = frames.begin_frame().ok_or("no frame to draw into")?;
    let webgpu = as_webgpu(&*draw).ok_or("not a webgpu draw target")?;
    let resource: &dyn TargetResource = webgpu.resource();

    // `DECKGL_MLN_DEPTH=0` draws deck on top instead of sharing the map's depth buffer,
    // the same escape hatch the maplibre-ffi demo spells `DECKGL_LOAD_DEPTH=0`.
    let interleave = std::env::var("DECKGL_MLN_DEPTH").map_or(true, |v| v != "0");

    let color_view = resource.get_color_texture_view().ok_or("no colour view")?;
    let depth_view = resource.get_depth_stencil_texture_view().ok_or("no depth view")?;
    let target = RenderTarget {
        color_format: resource.get_color_texture_format().ok_or("no colour format")?,
        depth_format: interleave.then(|| resource.get_depth_stencil_texture_format()).flatten(),
        sample_count: 1,
    };
    println!("map target: {:?}", target);

    // 5. deck draws into the same attachments. No clear, so the map stays underneath, and
    //    the shared depth buffer is what makes this interleaved rather than painted over.
    let json = JsonConverter::new().parse(SCENE)?;
    let mut deck = Deck::new(
        &device,
        &queue,
        target,
        DeckProps {
            width: WIDTH,
            height: HEIGHT,
            layers: json.layers,
            ..Default::default()
        },
    )?;
    deck.set_view_state(view_state(&frontend.get_camera_options()));

    let mut encoder = device.create_command_encoder(&Default::default());
    deck.render(&mut encoder, &color_view, interleave.then_some(&depth_view), None)?;
    queue.submit([encoder.finish()]);

    // 6. Read the composited frame back through mln-rs: one texture holding both halves.
    let frame = frontend.read_still_image();
    println!(
        "composited: first pixel {:?}, non-transparent {}",
        &frame.data[0..4],
        frame.data.chunks(4).filter(|p| p[3] != 0).count()
    );
    let out = std::env::args().nth(1).unwrap_or_else(|| "mln-deck.png".to_string());
    image::RgbaImage::from_raw(frame.size.width, frame.size.height, frame.data.to_vec())
        .ok_or("the frame is not RGBA8")?
        .save(&out)?;
    println!("wrote {out} ({}x{})", frame.size.width, frame.size.height);

    Ok(())
}
