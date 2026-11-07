//! Opt-in, context-local timing and work counters for native icon benchmarks.

use std::collections::HashSet;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use egui::{Context, Id};

const KEY: &str = "mareader-icon-diagnostics";
const DEFAULT_WINDOW: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct CacheStats {
    pub bytes: usize,
    pub evictions: u64,
    pub evicted_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum AtlasLookup {
    Hit,
    Miss,
    Pending,
    Failed,
    Rejected,
}

#[derive(Clone, Default)]
struct Pass {
    active: HashSet<Id>,
    atlas_hits: u64,
    atlas_misses: u64,
    atlas_pending: u64,
    atlas_failed: u64,
    atlas_rejected: u64,
    resting_hits: u64,
    resting_misses: u64,
    rasters: u64,
    raster_time: Duration,
    uploads: u64,
    upload_bytes: u64,
    upload_time: Duration,
    mesh_vertices: u64,
}

#[derive(Clone, Default)]
struct Window {
    started: Option<Instant>,
    frames: u64,
    cpu_ms: Vec<f64>,
    harness_ms: Vec<f64>,
    active_total: u64,
    active_max: usize,
    atlas_hits: u64,
    atlas_misses: u64,
    atlas_pending: u64,
    atlas_failed: u64,
    atlas_rejected: u64,
    resting_hits: u64,
    resting_misses: u64,
    rasters: u64,
    raster_time: Duration,
    uploads: u64,
    upload_bytes: u64,
    upload_time: Duration,
    mesh_vertices: u64,
    cache_current: usize,
    cache_peak: usize,
    evictions: u64,
    evicted_bytes: u64,
}

#[derive(Clone, Default)]
struct State {
    pass: Pass,
    window: Window,
    previous_evictions: u64,
    previous_evicted_bytes: u64,
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED
        .get_or_init(|| std::env::var("MAREADER_ICON_DIAGNOSTICS").is_ok_and(|value| value != "0"))
}

#[cfg(target_arch = "wasm32")]
pub(super) const fn enabled() -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn report_window() -> Duration {
    static WINDOW: OnceLock<Duration> = OnceLock::new();
    *WINDOW.get_or_init(|| {
        std::env::var("MAREADER_ICON_DIAGNOSTICS_WINDOW_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(|millis| Duration::from_millis(millis.clamp(100, 60_000)))
            .unwrap_or(DEFAULT_WINDOW)
    })
}

#[cfg(target_arch = "wasm32")]
fn report_window() -> Duration {
    DEFAULT_WINDOW
}

fn update(ctx: &Context, change: impl FnOnce(&mut State)) {
    if !enabled() {
        return;
    }
    ctx.data_mut(|data| change(data.get_temp_mut_or_default::<State>(Id::new(KEY))));
}

pub(super) fn active(ctx: &Context, id: Id) {
    update(ctx, |state| {
        state.pass.active.insert(id);
    });
}

pub(super) fn atlas_lookup(ctx: &Context, result: AtlasLookup) {
    update(ctx, |state| match result {
        AtlasLookup::Hit => state.pass.atlas_hits += 1,
        AtlasLookup::Miss => state.pass.atlas_misses += 1,
        AtlasLookup::Pending => state.pass.atlas_pending += 1,
        AtlasLookup::Failed => state.pass.atlas_failed += 1,
        AtlasLookup::Rejected => state.pass.atlas_rejected += 1,
    });
}

pub(super) fn resting_lookup(ctx: &Context, hit: bool) {
    update(ctx, |state| {
        if hit {
            state.pass.resting_hits += 1;
        } else {
            state.pass.resting_misses += 1;
        }
    });
}

pub(super) fn raster(ctx: &Context, elapsed: Duration) {
    update(ctx, |state| {
        state.pass.rasters += 1;
        state.pass.raster_time += elapsed;
    });
}

pub(super) fn upload(ctx: &Context, bytes: usize, elapsed: Duration) {
    update(ctx, |state| {
        state.pass.uploads += 1;
        state.pass.upload_bytes = state.pass.upload_bytes.saturating_add(bytes as u64);
        state.pass.upload_time += elapsed;
    });
}

pub(super) fn mesh(ctx: &Context, vertices: usize) {
    update(ctx, |state| {
        state.pass.mesh_vertices = state.pass.mesh_vertices.saturating_add(vertices as u64);
    });
}

pub(super) fn finish_frame(
    ctx: &Context,
    cpu_usage: Option<Duration>,
    harness_elapsed: Option<Duration>,
    cache: CacheStats,
) {
    if !enabled() {
        return;
    }
    let now = Instant::now();
    let report = ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(Id::new(KEY));
        let pass = std::mem::take(&mut state.pass);
        let window = &mut state.window;
        let started = *window.started.get_or_insert(now);
        window.frames += 1;
        if let Some(cpu) = cpu_usage {
            window.cpu_ms.push(cpu.as_secs_f64() * 1000.0);
        }
        if let Some(elapsed) = harness_elapsed {
            window.harness_ms.push(elapsed.as_secs_f64() * 1000.0);
        }
        window.active_total += pass.active.len() as u64;
        window.active_max = window.active_max.max(pass.active.len());
        window.atlas_hits += pass.atlas_hits;
        window.atlas_misses += pass.atlas_misses;
        window.atlas_pending += pass.atlas_pending;
        window.atlas_failed += pass.atlas_failed;
        window.atlas_rejected += pass.atlas_rejected;
        window.resting_hits += pass.resting_hits;
        window.resting_misses += pass.resting_misses;
        window.rasters += pass.rasters;
        window.raster_time += pass.raster_time;
        window.uploads += pass.uploads;
        window.upload_bytes = window.upload_bytes.saturating_add(pass.upload_bytes);
        window.upload_time += pass.upload_time;
        window.mesh_vertices = window.mesh_vertices.saturating_add(pass.mesh_vertices);
        window.cache_current = cache.bytes;
        window.cache_peak = window.cache_peak.max(cache.bytes);
        window.evictions += cache.evictions.saturating_sub(state.previous_evictions);
        window.evicted_bytes += cache
            .evicted_bytes
            .saturating_sub(state.previous_evicted_bytes);
        state.previous_evictions = cache.evictions;
        state.previous_evicted_bytes = cache.evicted_bytes;

        if now.duration_since(started) < report_window() {
            return None;
        }
        let report = format_report(window, now.duration_since(started));
        state.window = Window {
            started: Some(now),
            ..Window::default()
        };
        Some(report)
    });
    if let Some(report) = report {
        eprintln!("{report}");
    }
}

fn format_report(window: &Window, elapsed: Duration) -> String {
    let frames = window.frames.max(1);
    format!(
        "icon diagnostics ({:.1}s): frames={} app_cpu_ms[median/p95/max]={} harness_ms[median/p95/max]={} active_icons[mean/max]={:.2}/{} atlas[hit/miss/pending/failed/rejected]={}/{}/{}/{}/{} resting[hit/miss]={}/{} raster[count/ms]={}/{:.2} upload[calls/bytes/ms]={}/{}/{:.2} mesh_vertices={} cache_bytes[current/peak]={}/{} evictions={}/{}B",
        elapsed.as_secs_f64(),
        window.frames,
        percentile_summary(&window.cpu_ms),
        percentile_summary(&window.harness_ms),
        window.active_total as f64 / frames as f64,
        window.active_max,
        window.atlas_hits,
        window.atlas_misses,
        window.atlas_pending,
        window.atlas_failed,
        window.atlas_rejected,
        window.resting_hits,
        window.resting_misses,
        window.rasters,
        window.raster_time.as_secs_f64() * 1000.0,
        window.uploads,
        window.upload_bytes,
        window.upload_time.as_secs_f64() * 1000.0,
        window.mesh_vertices,
        window.cache_current,
        window.cache_peak,
        window.evictions,
        window.evicted_bytes,
    )
}

#[cfg(test)]
pub(super) fn flush(ctx: &Context) {
    if !enabled() {
        return;
    }
    let now = Instant::now();
    let report = ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(Id::new(KEY));
        if state.window.frames == 0 {
            return None;
        }
        let elapsed = state
            .window
            .started
            .map_or(Duration::ZERO, |started| now.duration_since(started));
        let report = format_report(&state.window, elapsed);
        state.window = Window::default();
        Some(report)
    });
    if let Some(report) = report {
        eprintln!("{report}");
    }
}

fn percentile_summary(samples: &[f64]) -> String {
    if samples.is_empty() {
        return "n/a".to_owned();
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    let p95 = sorted[((sorted.len() - 1) * 95).div_ceil(100)];
    let max = sorted[sorted.len() - 1];
    format!("{median:.3}/{p95:.3}/{max:.3}")
}
