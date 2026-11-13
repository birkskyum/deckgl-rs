//! deck layers over an mln-rs basemap in the browser, on one WebGPU device.
//!
//! The native `examples/mln` demo does this headless: it makes the device, mln-rs renders
//! the map on it, and deck draws into the texture and the depth buffer the map rendered
//! into. Here the same exchange happens against a canvas, every frame.
//!
//! The bootstrap follows mln-rs's own browser example (`examples/web/src/map.rs` in that
//! repository, BSD-2-Clause), with two simplifications. The map runs on the page's thread
//! against an ordinary canvas rather than in a Web Worker against an `OffscreenCanvas`,
//! and the tiles use the renderer's own in-process transport rather than a pool of Web
//! Workers: wasm has no threads, so the cooperative scheduler runs that work between
//! frames instead. What is new is the overlay, where deck draws into the map's frame.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use deckgl::lumagl::RenderTarget;
use deckgl::viewport::{WebMercatorViewportOptions, Viewport};
use deckgl::{ClipDepthRange, Deck, DeckProps};
use deckgl_json::JsonConverter;
use mln_actor::RunLoop;
use mln_actor::cooperative_scheduler::CooperativeScheduler;
use mln_actor::scheduler;
use mln_gfx::{BackendScope, FrameTarget, RendererBackend, ScopeType};
use mln_map::{CameraOptions, Map, MapEvent, MapOptions};
use mln_renderer::Renderer;
use mln_util::enum_::Enum;
use mln_util::geo::LatLng;
use mln_util::logging::{Event, EventSeverity, Log, Observer};
use mln_util::size::Size;
use mln_webgpu::draw_target::{TargetResource, as_webgpu};
use wasm_bindgen::prelude::*;

type Target = mln_webgpu::SurfaceTarget;

#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
    // No threads in the browser, so the map's background work runs cooperatively, a slice
    // at a time in `tick`. This has to be installed before any map or file source exists.
    scheduler::install_cooperative(Arc::new(CooperativeScheduler::new()));
    // Without this the map's own errors go nowhere, and a style that fails to load looks
    // exactly like one that is still loading.
    Log::set_observer(Box::new(ConsoleLog));
}

/// The map's log records, in the browser console.
struct ConsoleLog;

impl Observer for ConsoleLog {
    fn on_record(&self, severity: EventSeverity, event: Event, _code: i64, message: &str) -> bool {
        let line = JsValue::from_str(&format!("[mln {}] {message}", event.to_string()));
        match severity {
            EventSeverity::Error => web_sys::console::error_1(&line),
            EventSeverity::Warning => web_sys::console::warn_1(&line),
            _ => web_sys::console::log_1(&line),
        }
        true
    }
}

fn js_error(message: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&message.to_string())
}

fn physical(size: Size, pixel_ratio: f64) -> Size {
    Size::new(
        (f64::from(size.width) * pixel_ratio).round().max(1.0) as u32,
        (f64::from(size.height) * pixel_ratio).round().max(1.0) as u32,
    )
}

/// maplibre's near and far planes, in pixels, from the camera the map rendered with.
///
/// deck has to use the same frustum or the shared depth buffer means nothing. This is the
/// calculation `TransformState` makes, and the same one `deckgl-ffi` carries over the C
/// ABI for the maplibre-native demo.
fn near_far_pixels(fov_degrees: f64, pitch_degrees: f64, height: f64) -> (f64, f64) {
    let fov = fov_degrees.to_radians();
    let pitch = pitch_degrees.clamp(0.0, 89.0).to_radians();
    let camera_to_center = 0.5 * height / (fov / 2.0).tan();
    let tan_fov_above_center = (fov / 2.0).tan();
    let tan_multiple = (tan_fov_above_center * pitch.tan()).clamp(0.0, 0.99);
    let furthest = camera_to_center / (1.0 - tan_multiple);
    ((0.1 * camera_to_center).floor().max(1.0), furthest * 1.01)
}

/// The map's frame, and deck drawn into it before it is presented.
struct Frontend {
    renderer: Renderer,
    target: Rc<Target>,
    backend: Rc<dyn RendererBackend>,
    updates: mln_map::UpdateReader,
    drawn_update: u64,
    redraw: bool,
}

impl Frontend {
    /// Draws the latest update if it is new.
    ///
    /// The depth view is taken once the frame has begun and left where the overlay can
    /// reach it: `set_overlay` hands an overlay only the colour view, so deck would have
    /// nothing to depth-test against otherwise.
    fn render(&mut self, depth: &Rc<RefCell<Option<wgpu::TextureView>>>) -> bool {
        let Some(update) = self.updates.latest() else {
            return false;
        };
        if !self.redraw && update.generation == self.drawn_update {
            return false;
        }
        let _scope = BackendScope::new(&*self.backend, ScopeType::Implicit);
        let Some(draw) = self.target.begin_frame() else {
            return false;
        };
        *depth.borrow_mut() = as_webgpu(&*draw)
            .map(|webgpu| webgpu.resource())
            .and_then(TargetResource::get_depth_stencil_texture_view);
        self.redraw = false;
        self.drawn_update = update.generation;
        self.renderer.render(&update.parameters, &*self.target);
        true
    }
}

/// What the overlay needs to draw deck into the map's frame.
struct Layers {
    deck: Option<Deck>,
    viewport: Option<Viewport>,
    interleave: bool,
}

/// A continuous map in a canvas, with deck layers drawn into its frames.
#[wasm_bindgen]
pub struct WebDemo {
    frontend: Frontend,
    map: Map,
    run_loop: RunLoop,
    scheduler: Arc<CooperativeScheduler>,
    pixel_ratio: f64,
    size: Size,
    layers: Rc<RefCell<Layers>>,
    depth: Rc<RefCell<Option<wgpu::TextureView>>>,
    format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

#[wasm_bindgen]
impl WebDemo {
    /// Creates the WebGPU device, a surface on `canvas`, and the map that draws on it.
    ///
    /// deck shares all of it: the same device and queue, and the same frame.
    pub async fn create(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        pixel_ratio: f64,
    ) -> Result<WebDemo, JsValue> {
        let size = Size::new(width.max(1), height.max(1));
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(js_error)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|e| js_error(format!("WebGPU found no adapter: {e}")))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("visgl-rs + mln-rs"),
                required_limits: wgpu::Limits::default().or_worse_values_from(&adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|e| js_error(format!("WebGPU device: {e}")))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .first()
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
        let options = mln_webgpu::SurfaceOptions {
            cap_frame_rate: true,
            usages: Some(capabilities.usages | wgpu::TextureUsages::COPY_SRC),
        };

        let mln_device =
            mln_webgpu::Device::from_wgpu(Some(instance), device.clone(), queue.clone(), Some(format));
        let webgpu_backend = Rc::new(mln_webgpu::RendererBackend::new(&mln_device));
        let target = Rc::new(
            Target::new(webgpu_backend.clone(), surface, physical(size, pixel_ratio), options)
                .map_err(|e| js_error(format!("WebGPU surface: {e}")))?,
        );
        // The format the surface target picks for its depth buffer, which deck's
        // pipelines have to match to share it.
        let depth_format = if device.features().contains(wgpu::Features::DEPTH32FLOAT_STENCIL8) {
            wgpu::TextureFormat::Depth32FloatStencil8
        } else {
            wgpu::TextureFormat::Depth24PlusStencil8
        };
        let backend: Rc<dyn RendererBackend> = webgpu_backend;

        let run_loop = RunLoop::get();
        // No tile worker transport: the renderer's own in-process one runs the tiles on
        // the cooperative scheduler, a slice at a time in `tick`.
        let mut renderer = Renderer::new(backend.clone(), pixel_ratio as f32, None);
        let mut map = Map::new(MapOptions::new().with_size(size).with_pixel_ratio(pixel_ratio as f32))
            .map_err(js_error)?;
        renderer.set_event_queue(Some(map.renderer_events()));

        let layers = Rc::new(RefCell::new(Layers { deck: None, viewport: None, interleave: true }));
        let depth: Rc<RefCell<Option<wgpu::TextureView>>> = Rc::new(RefCell::new(None));

        // deck draws here: after the map has drawn into the frame, before it is presented.
        {
            let layers = layers.clone();
            let depth = depth.clone();
            target.set_overlay(Some(Box::new(move |frame| {
                let mut layers = layers.borrow_mut();
                let Layers { deck: Some(deck), viewport: Some(viewport), interleave } = &mut *layers else {
                    return;
                };
                deck.set_viewport(viewport.clone());
                let depth = depth.borrow();
                let depth_view = if *interleave { depth.as_ref() } else { None };
                let mut encoder = frame.device.create_command_encoder(&Default::default());
                // No clear: the map's frame stays underneath.
                if let Err(error) = deck.render(&mut encoder, frame.view, depth_view, None) {
                    mln_util::logging::Log::error(
                        mln_util::logging::Event::Render,
                        format!("deck: {error}"),
                    );
                    return;
                }
                frame.queue.submit([encoder.finish()]);
            })));
        }

        let frontend = Frontend {
            renderer,
            target,
            backend,
            updates: map.update_reader(),
            drawn_update: 0,
            redraw: true,
        };
        Ok(WebDemo {
            frontend,
            map,
            run_loop,
            scheduler: scheduler::cooperative().expect("installed at start").clone(),
            pixel_ratio,
            size,
            layers,
            depth,
            format,
            depth_format,
            device,
            queue,
        })
    }
}

#[wasm_bindgen]
impl WebDemo {
    /// Loads a style from a URL, e.g. OpenFreeMap Liberty.
    pub fn set_style_url(&mut self, url: &str) {
        self.map.style_mut().load_url(url);
        self.frontend.redraw = true;
    }

    /// Loads a style from inline JSON, for a demo that needs no network.
    pub fn set_style_json(&mut self, json: &str) {
        self.map.style_mut().load_json(json);
        self.frontend.redraw = true;
    }

    /// Puts the camera where the scene expects it.
    pub fn jump_to(&mut self, longitude: f64, latitude: f64, zoom: f64, pitch: f64, bearing: f64) {
        let Ok(centre) = LatLng::new(latitude, longitude) else {
            return;
        };
        self.map.jump_to(
            &CameraOptions::default()
                .with_center(Some(centre))
                .with_zoom(Some(zoom))
                .with_pitch(Some(pitch))
                .with_bearing(Some(bearing)),
        );
        self.frontend.redraw = true;
    }

    /// Replaces the deck layers with the ones in `json`, a deck.gl JSON description.
    pub fn set_scene(&mut self, json: &str) -> Result<(), JsValue> {
        let scene = JsonConverter::new().parse(json).map_err(js_error)?;
        let size = physical(self.size, self.pixel_ratio);
        let target = RenderTarget {
            color_format: self.format,
            depth_format: Some(self.depth_format),
            sample_count: 1,
        };
        let deck = Deck::new(
            &self.device,
            &self.queue,
            target,
            DeckProps {
                width: size.width,
                height: size.height,
                layers: scene.layers,
                // The map's opaque fills write the ground plane into the shared depth
                // buffer; bias deck above it so ground layers do not z-fight.
                depth_bias_base: -100,
                // mln-rs inherits maplibre's OpenGL style clip depth, as maplibre-native
                // does: `mat4::perspective` writes `(far + near) / (near - far)`.
                clip_depth_range: ClipDepthRange::NegativeOneToOne,
                ..Default::default()
            },
        )
        .map_err(js_error)?;
        self.layers.borrow_mut().deck = Some(deck);
        self.frontend.redraw = true;
        Ok(())
    }

    /// Draws deck against the map's depth buffer, or on top of it.
    pub fn set_interleave(&mut self, interleave: bool) {
        self.layers.borrow_mut().interleave = interleave;
        self.frontend.redraw = true;
    }

    /// Runs the map's work between frames: fetches that finished and tile parsing, for up
    /// to `budget_ms`. Returns whether work remains, for another `tick`.
    pub fn tick(&mut self, budget_ms: f64) -> bool {
        self.run_loop.run_once();
        self.drain_events();
        let deadline = mln_util::time::Instant::now()
            + std::time::Duration::from_secs_f64(budget_ms.max(0.0) / 1000.0);
        let more = self.scheduler.run_until(Some(deadline));
        self.run_loop.run_once();
        self.drain_events();
        more
    }

    /// The map's events have to be drained, and a style that fails to load says so here.
    fn drain_events(&mut self) {
        for event in self.map.drain_events() {
            match event {
                MapEvent::LoadingFailed(error) => {
                    web_sys::console::error_1(&JsValue::from_str(&format!(
                        "map failed to load: {error:?}"
                    )));
                }
                MapEvent::LoadingFinished => {
                    web_sys::console::log_1(&JsValue::from_str("map: style loaded"));
                    self.frontend.redraw = true;
                }
                _ => {}
            }
        }
    }

    /// Draws a frame, if the map has a new one. Returns whether it drew.
    pub fn frame(&mut self) -> bool {
        self.update_viewport();
        self.frontend.render(&self.depth)
    }

    /// Resizes the map and deck to `width` x `height` CSS pixels.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.size = Size::new(width.max(1), height.max(1));
        let size = physical(self.size, self.pixel_ratio);
        let _ = self.map.set_size(self.size);
        self.frontend.target.resize(size);
        if let Some(deck) = self.layers.borrow_mut().deck.as_mut() {
            deck.set_size(size.width, size.height);
        }
        self.frontend.redraw = true;
    }

    /// deck's viewport, from the camera the map is about to draw with.
    ///
    /// The near and far planes are maplibre's, not deck's defaults: the two have to agree
    /// on the frustum or the depth buffer they share means nothing.
    fn update_viewport(&mut self) {
        let camera = self.map.get_camera_options(None);
        let state = self.map.get_transform_state();
        let size = physical(self.size, self.pixel_ratio);
        let height = f64::from(size.height.max(1));
        let fov_degrees = f64::from(state.get_field_of_view()).to_degrees();
        let pitch_degrees = f64::from(state.get_pitch()).to_degrees();
        let centre = camera.center.unwrap_or_else(|| LatLng::new(0.0, 0.0).expect("origin"));
        let (near, far) = near_far_pixels(fov_degrees, pitch_degrees, height);
        let mut opts = WebMercatorViewportOptions {
            width: f64::from(size.width.max(1)),
            height,
            longitude: ((centre.longitude() + 540.0) % 360.0) - 180.0,
            latitude: centre.latitude(),
            zoom: camera.zoom.unwrap_or(0.0),
            bearing: camera.bearing.unwrap_or(0.0),
            pitch: pitch_degrees,
            ..Default::default()
        };
        if fov_degrees > 0.0 {
            opts.fovy = Some(fov_degrees);
        }
        opts.near_z = Some(near / height);
        opts.far_z = Some(far / height);
        self.layers.borrow_mut().viewport = Some(Viewport::web_mercator(&opts));
    }
}
