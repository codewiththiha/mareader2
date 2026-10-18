//! The numbers the library shows as words: sizes, ages and counts.

use std::path::Path;

/// Binary units, decimal spelling, one decimal below ten.
pub fn human_size(bytes: u64) -> String {
    const STEP: f64 = 1024.0;
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= STEP && unit + 1 < UNITS.len() {
        value /= STEP;
        unit += 1;
    }
    let name = UNITS.get(unit).copied().unwrap_or("TB");
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.1} {name}")
    } else {
        format!("{value:.0} {name}")
    }
}

/// Coarse past a day; a stamp in the future reads as "just now".
pub fn human_age(then_ms: u64, now_ms: u64) -> String {
    const MINUTE: u64 = 60_000;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    let elapsed = now_ms.saturating_sub(then_ms);
    let (count, one, many) = match elapsed {
        e if e < MINUTE => return "just now".to_string(),
        e if e < HOUR => (e / MINUTE, "minute", "minutes"),
        e if e < DAY => (e / HOUR, "hour", "hours"),
        e if e < WEEK => (e / DAY, "day", "days"),
        e => (e / WEEK, "week", "weeks"),
    };
    format!("{} ago", plural(count as usize, one, many))
}

/// The irregular plural is the caller's: `plural(2, "shelf", "shelves")`.
pub fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// A file's name without its folder or extension.
pub fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// A name, or the file's stem when the name is blank.
pub fn name_or_stem(name: &str, path: &Path) -> String {
    if name.trim().is_empty() {
        stem(path)
    } else {
        name.to_string()
    }
}

/// Names compare trimmed and case-blind, as a file manager does.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// `name`, or its next free `name_N` among the names in use.
pub fn free_name<'a>(name: &str, in_use: impl IntoIterator<Item = &'a str> + Clone) -> String {
    let trimmed = name.trim();
    let taken = |candidate: &str| in_use.clone().into_iter().any(|n| same_name(n, candidate));
    if !trimmed.is_empty() && !taken(trimmed) {
        return trimmed.to_string();
    }
    let base = strip_counter(if trimmed.is_empty() { "Book" } else { trimmed });
    (1u32..)
        .map(|n| format!("{base}_{n}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| format!("{base}_{}", u32::MAX))
}

/// `Dune_2` is a copy of `Dune`; `_` with nothing after stays.
fn strip_counter(name: &str) -> &str {
    match name.rsplit_once('_') {
        Some((base, n))
            if !base.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) =>
        {
            base
        }
        _ => name,
    }
}
