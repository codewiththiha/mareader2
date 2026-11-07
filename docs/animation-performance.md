# Animation performance

> **Status:** generated resting and part-atlas implementation; Remote release diagnostic added, first run pending · **Last updated:** 2026-11-07

This document answers two separate questions:

1. Does Mareader's current animated-icon provider use a lot of CPU?
2. How should icon and ordinary UI animation be implemented efficiently in a
   native egui application?

The answer comes from the current Mareader implementation and egui 0.36.2.
The source establishes which work is avoided, but not its CPU or memory impact
on a particular machine. The opt-in release diagnostic is now implemented;
publish its first Remote result before making quantitative claims. Real-GPU
and native-window measurements remain a separate, deferred step.

## Short answer

Mareader's previous animated-icon path did **avoidable CPU work**, but the
source alone does not prove it used “a lot” of CPU on a particular machine.

Before the generated-atlas replacement, each uncached icon sample:

1. evaluates the pack's timeline;
2. builds a new SVG string;
3. parses it into a new `usvg::Tree`;
4. CPU-rasterises it through resvg/tiny-skia;
5. constructs an RGBA `ColorImage`; and
6. uploads a GPU texture.

A normal 15–18 point icon uses a small canvas, usually 26–62 device pixels at
1–2× scale. A run lasts 0.42–0.84 seconds and targets 25–51 stable 60 Hz
samples. The first run for one pack and device size may therefore do 25–51
parse/raster/upload operations; a warmed run reuses those textures. Several
cold live icons multiply the raster work, although their repaint requests
coalesce into the same egui passes.

Cold SVG production was much heavier than interpolating a number and painting
a few egui shapes. The generated part atlas removes parsing from runtime,
performs bounded cold raster work on one native worker, uploads one texture and
then paints source-ordered mesh cells. Whether the change is noticeable on a
particular machine still requires a release profile.

## Implementation status

The runtime-SVG replacement is implemented across
`crates/animated-icons` and `crates/ui/src/icons/anim.rs`:

- the 98 SVG/timeline packs and six easing curves are declarative JSON in
  `crates/animated-icons/assets/packs.json`; `src/schema.rs` is shared by the
  build script and parity tests, and the `Prop` enum is shared with runtime;
- generated typed paths, ordered layers and animated-part ancestry are the
  runtime representation;
- time is quantised to stable 60 Hz samples and repaints target the exact next
  sample or landing deadline;
- tight, deduplicated layer-state variants are packed into one neutral-white
  texture per pack and device-pixel side;
- the UI emits those cells as source-ordered quads in one egui mesh, tinting at
  draw time;
- each authored top-level part is one atomic generated layer, so nested opacity
  is baked exactly without a whole-frame representation;
- every pack and DPI bucket must fit its generated part atlas below 8 MiB;
- native cold generation uses one bounded worker; wasm/threadless generation is
  synchronous;
- the context-local byte-accounted LRU remains capped at 8 MiB and 512 entries,
  with the generated resting glyph visible while an atlas is pending or unavailable; and
- `build.rs` still emits typed geometry into `OUT_DIR`; committing generated
  geometry to remove that generation step remains unfinished.

Default tests compare six representative packs at two sizes. The exhaustive
98-pack geometry/atlas and resting-glyph sweeps are marked ignored for ordinary
runs and execute on `main` CI. The Linux-only `Remote / icon-metrics` task
runs the ignored harness in release mode and saves its counters as an artifact.
That synthetic egui measurement is not a native-window or GPU result; real-GPU,
process-level and before/after measurements remain pending.

## Mareader's current paths

### Animated icons

`crates/ui/src/icons/anim.rs::advance` owns one finite run per icon and widget.
It selects a quantised sample and asks the context-local cache for the pack and
device-pixel side. A hit selects that frame's ordered atlas cells and paints
all of their quads through one egui mesh and texture. A miss queues one lazy
atlas build on native targets (or builds synchronously where threads are not
available), keeps the generated resting glyph visible, and requests a repaint when the
result is published.

`crates/animated-icons/build.rs` converts the build-only declarations into typed
Bézier geometry, exact nested scenes and flat source-ordered layers. Runtime
code rasterises transformed layer states directly with tiny-skia, deduplicates
identical states, crops transparent margins, and deterministically packs the
variants. Runtime SVG parsing and string construction are absent.

The resting path is cached independently:

- generated resting glyphs use `raster_resting` over generated typed geometry and a
  context-local 4 MiB/512-texture cache keyed by pack and physical side;
- each miss evaluates only the canonical first sample and uploads one tightly
  cropped neutral-white texture, never the complete animation atlas;
- Mareader's custom static SVGs use stable `bytes://` URIs; and
- `egui-lucide` is no longer a dependency.

The repeated work is confined to active pack frames.

### Ordinary UI animation

`crates/ui/src/motion.rs` turns the application's master motion setting into
`Style::animation_time` and `motion::secs`. Most fades, slides, lifts and morphs
use `Context::animate_bool_with_time` or `animate_value_with_time`, then paint
interpolated egui geometry.

In egui 0.36.2 these helpers keep only small numeric state in
`AnimationManager` and request another repaint while the result has not landed.
They do not parse SVGs or upload textures. They still cause the immediate-mode
UI to lay out and paint another pass, so expensive work elsewhere in the pass
must be cached.

Special cases have their own costs:

- `crossfade.rs` captures and uploads one viewport image, then fades that cached
  image;
- `loading_mark.rs` repaints continuously while visible;
- read-aloud meters and progress request bounded 20–30 Hz updates where that is
  enough;
- film grain schedules only the next grain step and uses the page's GPU pass.

## What egui itself provides

The relevant behavior was checked against tag `0.36.2`, the exact version in
Mareader's `Cargo.lock`.

### Reactive scheduling

`Context::request_repaint()` asks for one immediate follow-up pass. Multiple
calls coalesce. `request_repaint_after` keeps only the earliest deadline and can
be pre-empted by input or another earlier request. An idle reactive eframe app
sleeps.

This means a visible animation necessarily updates the whole immediate-mode
UI, but it does not require a permanent loop. CPU efficiency comes from making
each pass cheap and asking for passes only while a visible value can change.

`Context::repaint_causes()` records caller locations and is the first diagnostic
for an application that will not settle. `eframe::Frame::info().cpu_usage`
reports the previous frame's CPU time for `App::ui` and rendering, excluding the
vsync wait.

### Native numeric animation

`animate_bool*` and `animate_value_with_time` are the preferred path for
opacity, translation, scale, colour mixing, panel height and other numeric UI
motion. The value is frame-rate independent and the helper requests another
pass only while it is between endpoints.

The official toggle example allocates one stable rectangle, interpolates one
number, and paints its track and knob. This is the shape ordinary Mareader UI
motion should keep: stable layout plus cheap painter changes.

### Image and SVG caching

The 0.36.2 SVG loader caches `ColorImage`s by stable URI and size hint. The
default texture loader caches the corresponding `TextureHandle`s. If one URI is
seen at several sizes, variants not used in the current or previous pass are
removed.

This is excellent for static SVGs. Giving every animation frame a unique SVG
URI would not solve Mareader's problem: each frame is different, so it would
still be parsed and rasterised once, and an unbounded URI stream would become a
cache problem.

### Animated images

The built-in GIF/WebP widget decodes frames, stores their durations, selects an
already-loaded frame, and schedules the exact deadline at which the next frame
is due. It does not request a fixed high-frequency loop between two identical
frames. That is the right scheduling model for quantised icon frames too.

### FrameCache

`egui::cache::FrameCache` retains the result of an expensive computation while
it is requested in consecutive frames and evicts it after it goes unused. It is
useful for frame-local calculations such as syntax highlighting. A texture
cache with an explicit byte budget and lifecycle is a better fit for icon
frames because animations should survive gaps between runs and because
Mareader's resource rules require a hard bound.

## Release diagnostics and measurement

The generated-part-atlas implementation is already the active path. The old
SVG-template and whole-frame-cache proposals that informed its design have been
superseded; they are not future phases. Current performance claims must be
based on measurements of the generated-atlas path, not on earlier estimates.

### Opt-in native diagnostic

Set `MAREADER_ICON_DIAGNOSTICS=1` when launching the native app. Once rendered
frames span the ten-second reporting window, the next frame writes a
context-local summary to stderr. Set `MAREADER_ICON_DIAGNOSTICS_WINDOW_MS` to a
value from 100 to 60,000 to change the reporting window. The diagnostic adds no
idle repaint, so a fully settled app may emit its summary only after the next
input. With diagnostics disabled, the process does not take raster/upload
timers or collect context cache snapshots.

The summary includes:

- `app_cpu_ms`: median, p95 and maximum from `eframe::Frame::info().cpu_usage`
  (the previous frame's CPU time; it includes app UI and rendering but excludes
  vsync wait);
- unique active icon ids per frame (mean and maximum);
- animated-atlas cache hits, first misses, pending lookups, failures and
  rejected worker submissions, plus resting-glyph cache hits and misses;
- generated raster-call count and accumulated CPU raster time for the animated
  worker and synchronous resting-glyph path;
- `Context::load_texture` submissions, source pixel bytes and elapsed
  conversion/submission time; these are **not** measurements of GPU transfer or
  driver completion;
- animated mesh vertex count; and
- current/peak combined atlas and resting-glyph cache bytes, evictions and
  evicted bytes.

The report is intended to expose repeated work and cache behavior, not to
replace a profiler. `Context::repaint_causes()` remains the separate tool for
explaining why an otherwise idle app continues repainting.

### Remote release harness

The Linux-only `Remote / icon-metrics` task runs the ignored
`remote_release_icon_diagnostics_workloads` test in the release profile and
publishes its output as the `icon-metrics` artifact and in the GitHub step
summary. It is separate from `Remote / memory`, so dispatching it does not start
the existing macOS real-GPU memory job. The synthetic `egui::Context::run_ui`
harness drives these deterministic scenarios:

1. `idle`: twelve representative toolbar/viewer icons, no activation;
2. `click-run`: one click-owned finite animation;
3. `titlebar-sweep`: twelve distinct controls clicked on successive frames to
   create the maximum synthetic overlap; and
4. `animations-off`: the same sweep with icon motion disabled.

For this harness, `harness_ms` is wall time spent inside a synthetic egui pass;
`app_cpu_ms` is `n/a` because no eframe `Frame` or native window is involved.
The test uses a CPU-only context and texture manager. It **does not** measure a
native window, GPU uploads, vsync, or driver time. The separate `/usr/bin/time`
line records the release test command's wall/user/system time and peak RSS; it
is not a GPU measurement either.

### Evidence still required

The first Remote release run is pending. Publish its exact workflow run and
artifact numbers here before making claims about the current implementation.
The present branch does not provide a like-for-like result from the former
runtime-SVG implementation, so do not label the synthetic run a before/after
speedup. The user-selected scope is Remote-only: real-GPU results, native
window interaction (including the actual View footer and title-bar controls),
and a matched historical baseline are explicitly deferred.

For subsequent measurement, report median, p95 and maximum CPU frame time,
process CPU/RSS, produced frames, raster calls/time, texture submissions/bytes,
mesh vertices and cache peak/evictions. Identify the workflow/commit, build
profile, runner, workload and known limits. FPS alone is not enough: a 60 Hz
app can still waste CPU while producing the same frame rate.

## Efficient ordinary UI animation

Mareader's ordinary motion is already mostly on egui's efficient path. The
numeric interpolator is unlikely to be the bottleneck; the risk is uncached
work elsewhere in each full UI pass or a repaint request that never settles.
Optimize those call sites only after repaint-cause and scoped timing evidence.

### Keep layout stable

Allocate the final control rectangle once. Animate paint inside it—opacity,
position, scale, colour, underline width—not the surrounding layout unless the
layout change itself carries meaning. This prevents clipping, text movement and
extra reflow. The icon provider's padded fixed box and the formula-bar target
height are examples of explicit geometry.

### Use egui interpolation and painter primitives

Prefer `animate_bool_with_time`, `animate_value_with_time`, easing, `lerp`, and
`Painter` shapes for fades, lifts, tracks, knobs, glows and simple morphs. They
are native egui, carry reduced-motion behavior through `motion::secs`, and
avoid texture work.

### Schedule what can visibly change

- Smooth movement: `request_repaint()` while it is actually moving, allowing
  eframe/vsync to pace it.
- Deliberately capped motion: `request_repaint_after` at the chosen rate.
- Quantised image frames: request the exact next frame deadline.
- Timers and grace periods: request only the remaining deadline.
- Worker work: no polling loop; the worker calls `request_repaint()` when a
  result enters the channel.

A call from one subsystem repaints the whole egui viewport, so independent
requests coalesce. Increasing each subsystem's requested rate does not create
independent partial frames; it only shortens the global deadline.

### Cull and stop

An animation schedules nothing when its widget is not built or its rectangle is
not visible. Continuous indicators run only while their underlying operation is
active. Minimized, unfocused, occluded and software-rendered cases should
suppress nonessential looping motion or use a lower-power rate where the host
exposes that state. Finite runs remain based on elapsed time, not frame count,
so returning to a window cannot leave a control halfway through its motion. The
system reduced-motion setting and Mareader's Animations switch still land
directly on the end state.

### Cache every expensive dependency

During an active animation, assume the surrounding view may run at display
rate. Therefore:

- virtualise long rows with `ScrollArea::show_rows`;
- key and cache document layout, raster pages, syntax highlighting and galleys;
- send viewport commands only when the value changes;
- avoid file I/O, parsing, image decode and document scans in a view function;
- reuse texture handles when pixels change;
- use atlases for many small images;
- cap expensive misses and uploads per pass; and
- keep stale-but-valid output visible until its replacement arrives.

### Match quality to the visual

A progress waveform is readable at 30 Hz. A caret blink needs only its next
transition deadline. A short positional transition benefits from display-rate
updates. Icon frames may remain at 60 samples per second, but 30 samples should
be evaluated in visual CI as a low-power mode rather than assumed equivalent.

## Acceptance tests for an optimized provider

Behavior and layout:

- one provider still owns generated resting glyphs and animated frames;
- no-run, unmapped, disabled and reduced-motion states draw the generated
  resting glyph;
- one initiating event produces one finite run;
- a mid-run event queues at most one replay;
- the final frame is the generated resting image;
- fixed glyph geometry does not clip or shift layout;
- closing a popup stops that widget from scheduling frames.

Cache and work:

- one cold `(pack, device-pixel side)` request prepares one bounded atlas; a
  warmed replay hits the context-local cache and does no raster or upload;
- two widgets asking for the same pack and side share the atlas texture;
- a warmed active frame emits ordered mesh cells and no SVG construction or
  full-icon raster;
- animated and resting cache bytes stay under their respective admission
  bounds, and DPI buckets cannot grow memory without bound;
- the native worker queue and per-context unpublished-result mailbox remain
  bounded; and
- an idle settled window has no icon-animation repaint cause.

Visual parity:

- the ignored exhaustive parity sweep compares all quantised generated atlas
  frames and resting glyphs with the original SVG references;
- inspect 1×, 1.25× and 2× output plus the 192-pixel cap;
- keep the existing nearest-announcement, footer-slot and unclipped-frame
  regressions; and
- use remote native screenshots, not the browser preview, for final evidence.

Performance evidence:

- publish the Remote release harness result with its exact commit and workflow
  run, distinguishing synthetic `harness_ms` from native `app_cpu_ms`;
- report frame time, raster/upload counters, process CPU/RSS and cache peak /
  eviction counts, not only FPS;
- do not claim a before/after speedup without a matched historical baseline;
- identify runner, profile, workload and the lack of a GPU measurement; and
- record known limits in `docs/memory.md` when a cache budget is introduced.

## Approaches to avoid

- Do not move the same parse/raster/upload loop to a different icon API and call
  it caching.
- Do not create a unique egui image URI for every arbitrary timestamp.
- Do not build all packs, sizes and DPI variants eagerly.
- Do not use an unbounded global texture map.
- Do not let background prefetch occupy every worker or outrank visible work.
- Do not continuously request repaints for an idle timer or completed run.
- Do not replace cheap numeric UI motion with video, GIF or SVG frames.
- Do not assume a single GPU transform can express stroke drawing and
  independent-part animation.
- Do not add a second static icon provider; the generated resting renderer
  remains the one stock resting path.

## Primary references

- [egui 0.36.2 `Context`](https://docs.rs/egui/0.36.2/egui/struct.Context.html)
  for repaint coalescing, deadlines and repaint causes.
- [egui cache module](https://docs.rs/egui/0.36.2/egui/cache/index.html) for
  immediate-mode computation caching and generation eviction.
- [epaint 0.36.2 `PathShape`](https://docs.rs/epaint/0.36.2/epaint/struct.PathShape.html)
  and [`Mesh`](https://docs.rs/epaint/0.36.2/epaint/struct.Mesh.html) for the
  native vector/triangle capabilities and convex-fill limit.
- [tiny-skia 0.11.4 `Stroke`](https://docs.rs/tiny-skia/0.11.4/tiny_skia/struct.Stroke.html)
  for direct round-cap, round-join and dash rasterisation without an SVG tree.

## Decision and implementation

Mareader remains a reactive native egui application. Ordinary UI motion keeps
egui interpolation and painter geometry. Animated icons retain the same
interaction, timing, layout, headroom, tint and generated-resting fallback
contract, while the active path uses generated geometry plus ordered part-atlas
mesh composition.

The ownership boundary is deliberate:

- `mareader-animated-icons` is a toolkit-free L0 crate. Its build script alone
  reads the 98 declarative SVG/timeline packs and emits typed geometry, nested
  scene groups, source-ordered layers and timelines into `OUT_DIR`.
- Layer variants bake the quantised transform, path coverage, width and opacity
  state before tight alpha cropping. Equal states of a layer reuse one atlas
  cell. The deterministic packer admits no texture above 8 MiB.
- `mareader-ui` owns semantic `Icon` → pack-id mapping, trigger policy, lazy
  worker publication, the byte-accounted LRUs and egui mesh construction.
  `raster_resting` evaluates only time zero and supplies one cropped texture per
  requested stock pack and physical side under a separate 4 MiB bound.
  Animation cells are appended in source paint order and share one texture and
  one mesh.
- Each authored top-level part is an atomic generated scene. Nested opacity,
  transforms, path coverage and width changes are baked together in that part's
  cropped variants; no whole-frame atlas representation exists.
- Tests compare every quantised frame of all 98 packs with the original SVG
  renderer at 1×, 1.25×, 2× and the 192 px ceiling, then reconstruct every
  atlas frame from its ordered cells and compare it with the direct generated
  frame. Every one-frame resting crop is likewise reconstructed on its 24-unit
  canvas and compared with the canonical first SVG sample.

The first Remote release harness result is still required before quantifying
CPU or memory impact. It will measure the synthetic egui workload, not the
native application or GPU. The implementation avoids trading runtime parsing
for eager or unbounded storage and leaves retained-vector or custom-shader work
contingent on later measured need.
