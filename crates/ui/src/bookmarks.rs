//! Bookmarks: pages kept to come back to, shown as ribbons.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use mareader_library::{MarkId, Spot};
use mareader_pdf::active_entry;
use mareader_session::Document;

use crate::documents::Documents;
use crate::library::LibraryState;
use crate::notes_panel::page_of;
use crate::study::Study;
use mareader_workspace::PaneId;

/// The ribbon's size, hung from a page's top edge.
const RIBBON: egui::Vec2 = vec2(14.0, 22.0);

/// The pane's bookmarks, each with the page it falls on now.
pub fn on_pages(
    study: &Study,
    library: &LibraryState,
    documents: &Documents,
    pane: PaneId,
) -> Vec<(MarkId, usize)> {
    let Some(reading) = documents.reading(pane) else {
        return Vec::new();
    };
    let book = library.book_in(documents, pane);
    let name = documents.title(pane).unwrap_or_default();
    let kept = study.bookmarks_of(library.model(), book, name);
    let doc = &reading.document;
    kept.into_iter()
        .filter_map(|(id, spot)| Some((id, page_of(doc, spot)?)))
        .collect()
}

/// Where `page` starts: its first text when the text reflows.
pub fn spot(doc: &Document, page: usize) -> Spot {
    match doc.anchor(page) {
        Some(a) => Spot::Text {
            block: a.block,
            start: a.offset,
            end: a.offset,
        },
        None => Spot::Page {
            page,
            rect: [0.0; 4],
        },
    }
}

/// The chapter `page` falls in, from the outline.
pub fn chapter(doc: &Document, page: usize) -> Option<String> {
    let outline = doc.outline();
    let entry = active_entry(outline, page).and_then(|i| outline.get(i));
    let title = entry.map(|e| e.title.trim().to_owned());
    title.filter(|t| !t.is_empty())
}

/// A bookmark's name: its chapter, else its page.
pub fn label(doc: &Document, page: usize) -> String {
    chapter(doc, page).unwrap_or_else(|| format!("Page {}", page + 1))
}

/// Hangs a ribbon from `sheet`'s top edge, near its right corner.
pub fn ribbon(painter: &Painter, sheet: Rect, fill: Color32) {
    let right = sheet.right() - 18.0;
    let top = sheet.top();
    let (w, h) = (RIBBON.x, RIBBON.y);
    let at = |x: f32, y: f32| -> Pos2 { pos2(right - w + x, top + y) };
    // A notch cut into its tail, as a ribbon is.
    let points = vec![
        at(0.0, 0.0),
        at(w, 0.0),
        at(w, h),
        at(w / 2.0, h - 5.0),
        at(0.0, h),
    ];
    painter.add(Shape::convex_polygon(points, fill, Stroke::NONE));
}
