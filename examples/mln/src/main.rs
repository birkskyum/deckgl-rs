//! deckgl-rs layers drawn into an mln-rs basemap, all in Rust on one wgpu device.
//!
//! The maplibre-ffi demo next door does the same thing across a C ABI: maplibre-native
//! renders through Metal, wgpu-hal adopts its `MTLDevice`, and an Objective-C++ shim
//! carries the frame across. Here both halves are Rust on wgpu, so the whole exchange is
//! three calls: we make the device, mln-rs renders the map on it, and deck draws into the
//! texture it rendered into, against the map's own depth buffer.

use std::sync::Arc;

use deckgl::lumagl::RenderTarget;
use deckgl::{ClipDepthRange, Deck, DeckProps};
use deckgl_ffi::{maplibre_near_far_pixels, viewport_from_camera, DeckglCamera};
use deckgl_json::JsonConverter;

use mln_rs::mln_actor::RunLoop;
use mln_rs::mln_gfx::{BackendScope, ScopeType};
use mln_rs::mln_headless::HeadlessFrontend;
use mln_rs::mln_map::{CameraOptions, Map, MapMode, MapOptions};
use mln_rs::mln_util::geo::LatLng;
use mln_rs::mln_util::size::Size;
use mln_rs::mln_webgpu::draw_target::{as_webgpu, TargetResource};
use mln_rs::Device as MlnDevice;

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;

const STYLE: &str = r##"{
  "version": 8,
  "sources": {},
  "layers": [
    {"id": "bg", "type": "background", "paint": {"background-color": "#1b2432"}}
  ]
}"##;

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

/// The map's camera, as deck's.
///
/// mln-rs is a port of maplibre-native, so its projection is maplibre's: the same
/// `furthest_distance * 1.01` far plane, and the same OpenGL style perspective matrix
/// (`mat4::perspective` writes `(far + near) / (near - far)`, so clip z runs `[-1, 1]`).
/// That is why `maplibre_near_far_pixels`, written for maplibre-native across the FFI,
/// applies here unchanged.
fn camera(frontend: &HeadlessFrontend, map_camera: &CameraOptions) -> DeckglCamera {
    let centre = map_camera
        .center
        .unwrap_or_else(|| LatLng::new(0.0, 0.0).expect("the origin is a valid coordinate"));
    // The transform state carries the fov and pitch the map actually rendered with, both
    // in radians; the helper takes degrees.
    let state = frontend.get_transform_state();
    let fov_degrees = state.as_ref().map_or(36.869_897_645_844_02, |s| {
        f64::from(s.get_field_of_view()).to_degrees()
    });
    let pitch_degrees = state
        .as_ref()
        .map_or_else(|| map_camera.pitch.unwrap_or(0.0), |s| s.get_pitch().to_degrees());
    let (near, far) = maplibre_near_far_pixels(fov_degrees, pitch_degrees, f64::from(HEIGHT));

    DeckglCamera {
        longitude: centre.longitude(),
        latitude: centre.latitude(),
        zoom: map_camera.zoom.unwrap_or(0.0),
        bearing: map_camera.bearing.unwrap_or(0.0),
        pitch: pitch_degrees,
        fov_degrees,
        near_z_pixels: near,
        far_z_pixels: far,
        width: WIDTH,
        height: HEIGHT,
        pixel_ratio: 1.0,
        ..Default::default()
    }
}

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
    let mln_device = MlnDevice::webgpu(
        instance,
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
    );

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
    let map_camera = CameraOptions::default()
        .with_center(Some(LatLng::new(37.775, -122.42)?))
        .with_zoom(Some(13.5))
        .with_pitch(Some(55.0))
        .with_bearing(Some(-25.0));
    map.jump_to(&map_camera);

    // 3. The map draws its frame into its own offscreen texture, on our device. `render`
    //    is the still-image path: it pumps the run loop until the style is ready.
    frontend.render(&mut map)?;

    // 4. Reach the texture it drew into, and its depth buffer. mln-rs wants a backend
    //    scope held while its context is touched, as its own render paths do.
    let backend = frontend.get_backend();
    let _scope = BackendScope::new(backend, ScopeType::Explicit);

    // `DECKGL_MLN_DEPTH=0` draws deck on top instead of sharing the map's depth buffer,
    // the same escape hatch the maplibre-ffi demo spells `DECKGL_LOAD_DEPTH=0`.
    let interleave = std::env::var("DECKGL_MLN_DEPTH").map_or(true, |v| v != "0");

    let frames = frontend.target().frames().ok_or("the target has no frames")?;
    let draw = frames.begin_frame().ok_or("no frame to draw into")?;
    let webgpu = as_webgpu(&*draw).ok_or("not a webgpu draw target")?;
    let resource: &dyn TargetResource = webgpu.resource();

    let color_view = resource.get_color_texture_view().ok_or("no colour view")?;
    let depth_view = resource.get_depth_stencil_texture_view().ok_or("no depth view")?;
    let target = RenderTarget {
        color_format: resource.get_color_texture_format().ok_or("no colour format")?,
        depth_format: interleave
            .then(|| resource.get_depth_stencil_texture_format())
            .flatten(),
        sample_count: 1,
    };
    println!("map target: {target:?}, interleaved: {interleave}");

    // 5. deck draws into the same attachments. No clear, so the map stays underneath.
    let json = JsonConverter::new().parse(SCENE)?;
    let mut deck = Deck::new(
        &device,
        &queue,
        target,
        DeckProps {
            width: WIDTH,
            height: HEIGHT,
            layers: json.layers,
            // The map's opaque fills write the ground plane into the shared depth buffer;
            // bias deck above it so ground layers do not z-fight.
            depth_bias_base: -100,
            // mln-rs inherits maplibre's OpenGL style depth, as maplibre-native does.
            clip_depth_range: ClipDepthRange::NegativeOneToOne,
            ..Default::default()
        },
    )?;
    deck.set_viewport(viewport_from_camera(&camera(&frontend, &map_camera)));

    let mut encoder = device.create_command_encoder(&Default::default());
    deck.render(&mut encoder, &color_view, interleave.then_some(&depth_view), None)?;
    queue.submit([encoder.finish()]);

    // 6. Read the composited frame back through mln-rs: one texture holding both halves.
    let frame = frontend.read_still_image();
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "mln-deck.png".to_string());
    image::RgbaImage::from_raw(frame.size.width, frame.size.height, frame.data.to_vec())
        .ok_or("the frame is not RGBA8")?
        .save(&out)?;
    println!("wrote {out} ({}x{})", frame.size.width, frame.size.height);

    Ok(())
}
