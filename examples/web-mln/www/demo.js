// Runs one scene live: an mln-rs basemap and deck layers on one WebGPU device.
import init, { WebDemo } from './pkg/visgl_web_mln.js'

/** The basemap. Its 3D buildings are what deck's layers are occluded by. */
export const STYLE = 'https://tiles.openfreemap.org/styles/liberty'

let ready
/** wasm-bindgen's module is global, so initialise it once however many demos run. */
function started() {
  ready ??= init()
  return ready
}

export function supported() {
  return typeof navigator !== 'undefined' && 'gpu' in navigator
}

/**
 * Draws `sceneUrl` over the basemap in `canvas`, and keeps drawing.
 * Returns the demo, with `stop()` to end its loop.
 */
export async function run(canvas, sceneUrl, { interleave = true, style = STYLE } = {}) {
  if (!supported()) throw new Error('This browser has no WebGPU.')
  await started()

  const ratio = Math.min(window.devicePixelRatio || 1, 2)
  const box = canvas.getBoundingClientRect()
  const width = Math.max(1, Math.round(box.width))
  const height = Math.max(1, Math.round(box.height))
  canvas.width = Math.round(width * ratio)
  canvas.height = Math.round(height * ratio)

  const demo = await WebDemo.create(canvas, width, height, ratio)
  demo.set_style_url(style)

  const scene = await (await fetch(sceneUrl)).json()
  const view = scene.initialViewState ?? {}
  demo.jump_to(
    view.longitude ?? 0,
    view.latitude ?? 0,
    view.zoom ?? 0,
    view.pitch ?? 0,
    view.bearing ?? 0,
  )
  demo.set_scene(JSON.stringify(scene))
  demo.set_interleave(interleave)

  let running = true
  const frame = () => {
    if (!running) return
    // A slice of the map's own work (fetches that landed, tiles being parsed), then
    // the frame: the map draws, then deck draws into it.
    demo.tick(8)
    demo.frame()
    requestAnimationFrame(frame)
  }
  requestAnimationFrame(frame)

  const observer = new ResizeObserver(() => {
    const box = canvas.getBoundingClientRect()
    if (box.width >= 1 && box.height >= 1) {
      demo.resize(Math.round(box.width), Math.round(box.height))
    }
  })
  observer.observe(canvas)

  return Object.assign(demo, {
    stop() {
      running = false
      observer.disconnect()
    },
  })
}
