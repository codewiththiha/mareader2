//! The window's keys that reach the engine: a file to open, how panes share.
//!
//! What stays in a front end names its own widgets — the platform skin, the
//! sidebar's panel, the bar's query, a toast, the workers' inline flag, which
//! pane is in front, and how many panes the tree holds.

use mareader_settings::LibraryClick;

use super::{field, flag, named};
use crate::command::Command;
use crate::documents::OpenRequest;

/// The panes' keys; `None` for a key that is not one of them.
pub(super) fn set(key: &str, value: &str) -> Option<Result<Vec<Command>, String>> {
    let parsed = match key {
        "pdf" | "epub" => open(key, value),
        "independent" => field("workspace.independent_themes", flag(value)),
        "independent_text" => field("workspace.independent_text", flag(value)),
        "independent_textures" => field("workspace.independent_textures", flag(value)),
        "shared_base" => field("workspace.shared_base_mode", flag(value)),
        "library_click" => click(value),
        _ => return None,
    };
    Some(parsed)
}

/// A sample built here, or the file the value names.
///
/// The pane is left unset, so the open lands in the one in front, which is how
/// `focus=` aims each file.
fn open(key: &str, value: &str) -> Result<Vec<Command>, String> {
    let (kind, pages) = value.split_once(':').unwrap_or((value, ""));
    let count = pages.parse().ok();
    let made = match (key, kind) {
        ("epub", "sample") => OpenRequest::bytes(
            mareader_epub::sample::epub(count.unwrap_or(3), "A Mareader Book"),
            "sample.epub",
        ),
        (_, "sample") => OpenRequest::bytes(
            mareader_pdf::sample::pdf(count.unwrap_or(120), "A Mareader Sample"),
            "sample.pdf",
        ),
        (_, "figures") => OpenRequest::bytes(
            mareader_pdf::sample::figures(count.unwrap_or(120), "A Mareader Sample"),
            "figures.pdf",
        ),
        (_, "scan") => OpenRequest::bytes(
            mareader_pdf::sample::scan(count.unwrap_or(120), "A Mareader Scan"),
            "scan.pdf",
        ),
        _ => OpenRequest::path(value.into()),
    };
    Ok(vec![Command::Open(made)])
}

fn click(value: &str) -> Result<Vec<Command>, String> {
    let hint = "library_click: replace, split or drag_only";
    named(
        value,
        &[
            ("replace", LibraryClick::Replace),
            ("split", LibraryClick::Split),
            ("drag_only", LibraryClick::DragOnly),
        ],
        hint,
    )
    .and_then(|some| field("workspace.library_click", some))
}
