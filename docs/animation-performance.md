# Animation performance

> **Status:** source audit and implementation plan · **Last updated:** 2026-11-06

This document answers two separate questions:

1. Does Mareader's current animated-icon provider use a lot of CPU?
2. How should icon and ordinary UI animation be implemented efficiently in a
   native egui application?

The answer comes from the current Mareader implementation and egui 0.36.2.
It is an architectural assessment, not a measured CPU result. A number such
as “10% CPU” or “massive”
would require the release-build measurements specified below.

## Short answer

Mareader's animated icons do **avoidable CPU work**, but the source alone does
not prove they use “a lot” of CPU on a particular machine.

Each active icon frame currently:

1. evaluates the pack's timeline;
2. builds a new SVG string;
3. parses it into a new `usvg::Tree`;
4. CPU-rasterises it through resvg/tiny-skia;
5. constructs an RGBA `ColorImage`; and
6. updates a GPU texture.

A normal 15–18 point icon uses a small canvas, usually 26–62 device pixels at
1–2× scale. A run lasts 0.42–0.84 seconds and asks for its next pass after
`1/60 s`. That is normally about 25–51 parse/raster/upload operations for one
click or hover. Several live icons multiply the raster work, although their
repaint requests coalesce into the same egui passes.

This is much heavier than interpolating a number and painting a few egui
shapes. It is still short-lived, small, and finite. Whether it is noticeable
must be profiled in a release build. The immediate improvement is not to leave
egui; it is to stop producing the same animation pixels repeatedly on the UI
thread.

## Mareader's current paths

### Animated icons

`crates/ui/src/icons/anim.rs::advance` owns one finite run per icon and widget.
While the run is live it calls `frame_svg`, `mareader_reflow::raster_svg`, and
`TextureHandle::set`, then schedules another pass after `1/60 s`.

`crates/reflow/src/svg.rs::raster_svg` calls `usvg::Tree::from_data`, renders a
fresh tiny-skia pixmap with resvg, demultiplies every pixel, and returns a new
RGBA image. The texture **handle** is reused; parsed SVGs and raster frames are
not.

The stock resting path is already cached independently:

- Lucide goes through `egui-lucide`;
- Mareader's static SVGs use stable `bytes://` URIs;
- egui's image and texture loaders cache a stable URI and size hint.

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
reports the previous frame's CPU time, including app logic, egui layout,
tessellation and rendering but excluding the vsync wait.

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

## Recommended icon architecture

Keep the current public provider, trigger policy, stable layout, stock fallback
and finite run semantics. Change only how an active frame is supplied.

### Phase 0: measure the current path

Add an opt-in native diagnostic, disabled in normal builds, that records:

- total app CPU frame time from `eframe::Frame::info().cpu_usage`;
- time in timeline evaluation, SVG construction, usvg parse, resvg raster and
  texture update separately;
- active icon count and raster/upload count per pass;
- `Context::repaint_causes()` when the app does not settle;
- cache hits, misses, bytes and evictions after a cache exists.

Measure a release build in CI/native jobs and on at least one hardware-GPU
machine. Workloads:

1. idle for ten seconds;
2. one click-owned run;
3. a title-bar hover sweep with the maximum expected overlap;
4. View-menu footer and Zoom runs;
5. Animations off;
6. software rendering or a low-end adapter where available.

Record median, p95 and maximum CPU frame time, process CPU, frames produced,
SVG rasters, uploads and cache bytes. Do not call an optimization successful
from FPS alone: a 60 Hz app can still waste most of a core.

### Phase 1: quantised, context-local frame cache

Use a fixed frame index rather than arbitrary floating time:

```text
FrameKey = (pack id, device-pixel side bucket, frame index)
```

At 60 samples per second, the current timelines need about 25–51 frames. The
index's timestamp is the timestamp passed to `frame_svg`, so cache identity and
rendered content cannot disagree. Frames stay neutral white and are tinted when
painted, so theme, hover and disabled colours do not multiply cache entries.

On a miss, the first implementation may use the existing synchronous raster
path. On a hit, it only selects a cached `TextureHandle`. Replaying a warmed
icon then performs no SVG parse, CPU raster or texture upload.

The cache must:

- be owned by the UI/egui context, never global across renderers;
- have an explicit byte cap and entry cap;
- account for RGBA bytes and texture lifetime;
- evict least-recently-drawn entries first;
- never evict an entry used in the current or previous pass;
- remove a whole obsolete size bucket after DPI/zoom changes settle;
- expose deterministic hit/miss/eviction counters for tests; and
- be explicitly cleared with the UI owner or context.

This is the smallest low-risk change and preserves pixel output. Its limit is
that the first run at a new size still pays every miss on the UI thread.

### Phase 2: one lazy atlas per pack and size

Once quantisation is stable, pack the frames of one `(pack, side)` into one
texture and draw the selected frame by UV rectangle. Even the longest current
run fits in a small grid: 51 cells fit in an 8×7 atlas. At a common 62-pixel
cell that is about 496×434 pixels, under 1 MiB RGBA.

Build the atlas away from the UI pass:

1. enqueue only the pack and size that is actually requested;
2. rasterise frames on a bounded worker;
3. publish results with pack, size and cache generation;
4. wake egui when pixels are ready;
5. upload with a per-pass count/byte budget; and
6. reject a stale result after eviction, DPI change or cache teardown.

The first run can use the current per-frame path for missing cells while the
worker warms the atlas, ensuring interaction never waits. Later runs change
only UVs. Frequently visible title-bar packs may be warmed during an already
scheduled idle pass, but all 98 packs must not be generated eagerly.

A single atlas handle also avoids a large number of tiny texture objects. The
cache's byte admission is checked before building; if an unusually large
192-pixel atlas cannot fit, the provider keeps the stock glyph or uses the
bounded current-frame path instead of evicting visible atlases back and forth.

### Phase 3: classify transform-only packs only if profiling justifies it

For a pack whose parts only translate, rotate, scale and fade, each part can be
rasterised once and transformed as an egui mesh/quad every frame. This is cheaper than changing atlas UVs only in
memory terms.

Do not apply it blindly. Path length, path offset and stroke-width animation
change pixel coverage and cannot be reproduced by transforming one cached
whole-icon image. Supporting those properties as native epaint paths would
require generated path geometry, partial-stroke tessellation and visual parity
tests. That is a larger renderer, not the first optimization.

### Why not a custom GPU renderer first

A wgpu atlas shader could use alpha-only textures and part transforms, but it
adds backend integration, device-loss handling, multiple viewport ownership,
software-renderer behavior and more test surface. Standard egui images and UVs
already remove parse/raster/upload work from warmed runs. Use a custom pass only
if measurements show texture memory or egui image submission is then the
bottleneck.

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

- one provider still owns stock fallback and animated frames;
- no-run, unmapped, disabled and reduced-motion states draw the stock glyph;
- one initiating event produces one finite run;
- a mid-run event queues at most one replay;
- the final frame is the stock resting image;
- fixed glyph geometry does not clip or shift layout;
- closing a popup stops that widget from scheduling frames.

Cache and work:

- the first quantised frame has one miss and its replay is a hit;
- two widgets asking for the same pack, side and frame share cached pixels;
- a warmed full run performs zero SVG parses, rasters and uploads;
- cache bytes never exceed the configured admission bound;
- current/previous-pass entries are not evicted into repaint thrash;
- DPI change makes the correct size without retaining unbounded old sizes;
- worker results after cache generation/owner teardown are dropped;
- an idle settled window has no icon-animation repaint cause.

Visual parity:

- compare current and cached frames at start, end, extrema and segment
  boundaries for every pack;
- inspect 1×, 1.25× and 2× output plus the 192-pixel cap;
- keep the existing nearest-announcement, footer-slot and unclipped-frame
  regressions; and
- use remote native screenshots, not the browser preview, for final evidence.

Performance evidence:

- publish current versus optimized release measurements for all Phase 0
  workloads;
- report UI-thread frame time and raster/upload counts, not only FPS;
- report cache peak bytes and evictions;
- identify renderer, scale, operating system and CPU; and
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
- Do not add a second static icon provider; the existing stock fallback remains
  the one resting path.

## Primary references

- [egui 0.36.2 `Context`](https://docs.rs/egui/0.36.2/egui/struct.Context.html)
  for repaint coalescing, deadlines and repaint causes.
- [egui cache module](https://docs.rs/egui/0.36.2/egui/cache/index.html) for
  immediate-mode computation caching and generation eviction.

## Decision

Mareader should remain a reactive native egui application. Ordinary UI motion
should continue to use egui interpolation and painter geometry. Animated icons
should keep their current interaction and layout contract but migrate from
per-pass SVG production to a byte-capped, context-local cache, then to lazy
per-pack frame atlases if the measured first phase warrants it.

The implementation order is **measure → quantise/cache → worker/atlas → only
then consider custom vector/GPU paths**. This removes the known redundant CPU
work without trading it for unbounded memory, first-run stalls, or a second icon
system.
