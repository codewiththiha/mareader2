//! The rail's Library panel: a shelf tree to click or drag onto panes.

use std::collections::BTreeSet;

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, DragAndDrop, FontId, FontSelection, Id, Key,
    LayerId, Order, Rect, RichText, ScrollArea, Sense, Stroke, TextWrapMode, Ui, WidgetText, pos2,
    vec2,
};
use mareader_library::{Book, BookId, Format, Home, Library, ShelfId, natural};
use mareader_settings::LibraryClick;
use mareader_workspace::{Bounds, DropTarget, PaneId};

use crate::command::{Command, Commands};
use crate::documents::Placement;
use crate::icons::Icon;
use crate::theme::{self, Tokens};

const ROW_H: f32 = 28.0;
const PAD_X: f32 = 8.0;
const INDENT: f32 = 12.0;
const INDENT_MAX: f32 = 96.0;
/// Chevron and gap: a book's icon lines up under its shelf's name.
const CHEVRON_W: f32 = 18.0;
const SHELF_DEPTH_MAX: usize = 32;

/// A library book being dragged from the rail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carried {
    pub book: BookId,
    pub title: String,
}

/// One visible row of the tree, in drawing order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Shelf {
        id: ShelfId,
        name: String,
        depth: usize,
        open: bool,
        /// Books on it and under it.
        count: usize,
    },
    Book {
        id: BookId,
        title: String,
        format: Format,
        depth: usize,
        missing: bool,
    },
}

/// Rows with `open` shelves expanded: shelves, then books, by name.
pub fn lines(model: &Library, open: &BTreeSet<ShelfId>, home: Home) -> Vec<Line> {
    let mut out = Vec::new();
    let shelved = model.shelved();
    let top: Vec<&Book> = match home {
        Home::Everything => model.books().collect(),
        Home::Unshelved => model.books().filter(|b| !shelved.contains(&b.id)).collect(),
    };
    push_level(model, None, top, 0, open, &mut out);
    out
}

fn push_level(
    model: &Library,
    shelf: Option<ShelfId>,
    books: Vec<&Book>,
    depth: usize,
    open: &BTreeSet<ShelfId>,
    out: &mut Vec<Line>,
) {
    if depth > SHELF_DEPTH_MAX {
        return;
    }
    for child in model.children(shelf) {
        let is_open = open.contains(&child.id);
        out.push(Line::Shelf {
            id: child.id,
            name: child.name.value.clone(),
            depth,
            open: is_open,
            count: count_under(model, child.id, depth),
        });
        if is_open {
            let books = model.shelf_books(child.id);
            push_level(model, Some(child.id), books, depth + 1, open, out);
        }
    }
    let mut books = books;
    books.sort_by(|a, b| natural(&a.title.value, &b.title.value).then(a.id.cmp(&b.id)));
    out.extend(books.into_iter().map(|b| Line::Book {
        id: b.id,
        title: b.title.value.clone(),
        format: b.format,
        depth,
        missing: b.missing(),
    }));
}

/// Books on `shelf` and its shelves, each counted once.
fn count_under(model: &Library, shelf: ShelfId, depth: usize) -> usize {
    let mut seen = BTreeSet::new();
    let mut stack = vec![(shelf, depth)];
    while let Some((at, d)) = stack.pop() {
        seen.extend(model.shelf_books(at).into_iter().map(|b| b.id));
        if d < SHELF_DEPTH_MAX {
            stack.extend(model.children(Some(at)).into_iter().map(|c| (c.id, d + 1)));
        }
    }
    seen.len()
}

/// One open pane, as the strip lists it.
#[derive(Clone, Debug)]
pub struct PaneTab {
    pub pane: PaneId,
    pub name: String,
    pub book: Option<BookId>,
}

/// What the panel reads from the app this frame.
pub struct PanelContext<'a> {
    pub model: &'a Library,
    /// Every pane, in the tree's order.
    pub tabs: &'a [PaneTab],
    pub active: PaneId,
    pub click: LibraryClick,
    pub home: Home,
}

/// What the panel keeps between frames.
#[derive(Debug, Default)]
pub struct PanelState {
    open: BTreeSet<ShelfId>,
    /// The rows, kept until the library, the open set or Home change.
    cache: Option<((u64, Home), Vec<Line>)>,
}

impl PanelState {
    fn lines(&mut self, model: &Library, home: Home) -> &[Line] {
        let key = (model.revision(), home);
        let open = &self.open;
        let stale = self.cache.as_ref().is_none_or(|(k, _)| *k != key);
        if stale {
            self.cache = Some((key, lines(model, open, home)));
        }
        self.cache.as_ref().map_or(&[], |(_, l)| l.as_slice())
    }

    pub fn set_open(&mut self, shelf: ShelfId, open: bool) {
        let changed = match open {
            true => self.open.insert(shelf),
            false => self.open.remove(&shelf),
        };
        if changed {
            self.cache = None;
        }
    }

    pub fn is_open(&self, shelf: ShelfId) -> bool {
        self.open.contains(&shelf)
    }
}

/// Draws the panel; true, as it always has something to say.
pub fn show(
    ui: &mut Ui,
    cx: &PanelContext<'_>,
    state: &mut PanelState,
    out: &mut Commands,
) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.y = 0.0;
    if cx.tabs.len() > 1 {
        heading(ui, "Open", cx.tabs.len(), &t);
        for tab in cx.tabs {
            tab_row(ui, tab, tab.pane == cx.active, &t, out);
        }
        ui.add_space(6.0);
        let y = ui.cursor().top();
        let x = ui.max_rect().x_range();
        ui.painter().hline(x, y, Stroke::new(1.0, t.line));
    }
    heading(ui, "Library", cx.model.len(), &t);
    if cx.model.is_empty() {
        empty(ui, &t);
        return true;
    }
    let reading: Option<BookId> = cx
        .tabs
        .iter()
        .find(|tab| tab.pane == cx.active)
        .and_then(|tab| tab.book);
    let open: BTreeSet<BookId> = cx.tabs.iter().filter_map(|tab| tab.book).collect();
    let dragged = DragAndDrop::payload::<Carried>(ui.ctx()).map(|c| c.book);
    let lines = state.lines(cx.model, cx.home).to_vec();
    let width = ui.available_width();
    let mut toggles = Vec::new();
    ScrollArea::vertical()
        .id_salt("library-panel")
        .auto_shrink(false)
        .show_rows(ui, ROW_H, lines.len(), |ui, range| {
            for line in lines.get(range).unwrap_or_default() {
                let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
                match line {
                    Line::Shelf { id, open, .. } => {
                        if let Some(want) = shelf_row(ui, rect, line, &t) {
                            toggles.push((*id, want.unwrap_or(!*open)));
                        }
                    }
                    Line::Book { id, .. } => {
                        let look = BookLook {
                            active: reading == Some(*id),
                            open: open.contains(id),
                            dragged: dragged == Some(*id),
                        };
                        book_row(ui, rect, line, look, cx.click, &t, out);
                    }
                }
            }
        });
    for (shelf, want) in toggles {
        state.set_open(shelf, want);
    }
    true
}

/// A small caps heading with a count at the right.
fn heading(ui: &mut Ui, title: &str, count: usize, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let painter = ui.painter();
    let font = theme::medium(11.0);
    let y = rect.bottom() - 9.0;
    let left = pos2(rect.left() + PAD_X + 4.0, y);
    painter.text(
        left,
        Align2::LEFT_CENTER,
        title.to_uppercase(),
        font,
        t.muted,
    );
    let right = pos2(rect.right() - PAD_X - 4.0, y);
    let small = FontId::proportional(11.0);
    painter.text(
        right,
        Align2::RIGHT_CENTER,
        count.to_string(),
        small,
        t.muted,
    );
}

fn tab_row(ui: &mut Ui, tab: &PaneTab, active: bool, t: &Tokens, out: &mut Commands) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let row = rect.shrink2(vec2(6.0, 1.0));
    let id = Id::new(("library-panel-tab", tab.pane));
    let resp = ui.interact(row, id, Sense::click());
    let name = &tab.name;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, name));
    let painter = ui.painter();
    if active {
        painter.rect_filled(row, CornerRadius::same(6), t.hover);
    } else if resp.hovered() {
        painter.rect_filled(row, CornerRadius::same(6), t.hover.gamma_multiply(0.6));
    }
    let close = Rect::from_center_size(pos2(row.right() - 14.0, row.center().y), vec2(20.0, 20.0));
    let room = row.width() - 46.0;
    let colour = if active { t.ink } else { t.muted };
    let font = if active {
        theme::medium(13.0)
    } else {
        FontId::proportional(13.0)
    };
    let at = pos2(row.left() + 10.0, row.center().y);
    text(ui, at, name, font, colour, room);
    let x = ui.interact(close, id.with("close"), Sense::click());
    let label = format!("Close {name}");
    x.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if x.hovered() {
        ui.painter()
            .rect_filled(close, CornerRadius::same(5), t.pressed);
    }
    if x.hovered() || resp.hovered() || active {
        Icon::Close.paint(ui, close.shrink(5.0), t.muted);
    }
    if x.clicked() {
        out.push(Command::ClosePane(tab.pane));
    } else if resp.clicked() {
        out.push(Command::FocusPane(tab.pane));
    }
}

/// A shelf row; `Some(None)` toggles, `Some(Some(open))` sets.
fn shelf_row(ui: &Ui, rect: Rect, line: &Line, t: &Tokens) -> Option<Option<bool>> {
    let Line::Shelf {
        id,
        name,
        depth,
        open,
        count,
    } = line
    else {
        return None;
    };
    let row = rect.shrink2(vec2(6.0, 1.0));
    let resp = ui.interact(row, Id::new(("library-panel-shelf", id)), Sense::click());
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name);
        info.selected = Some(*open);
        info
    });
    if resp.hovered() || resp.has_focus() {
        ui.painter()
            .rect_filled(row, CornerRadius::same(6), t.hover.gamma_multiply(0.6));
    }
    let mut x = row.left() + indent(*depth);
    let chevron = if *open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let icon = |x: f32, size: f32| {
        Rect::from_center_size(pos2(x + size / 2.0, row.center().y), vec2(size, size))
    };
    chevron.paint(ui, icon(x, 12.0), t.muted);
    x += CHEVRON_W;
    Icon::Shelf.paint(ui, icon(x, 14.0), t.muted);
    x += 20.0;
    let number = count.to_string();
    let small = FontId::proportional(11.0);
    let right = pos2(row.right() - 8.0, row.center().y);
    ui.painter()
        .text(right, Align2::RIGHT_CENTER, number, small, t.muted);
    let room = row.right() - 36.0 - x;
    text(
        ui,
        pos2(x, row.center().y),
        name,
        FontId::proportional(13.0),
        t.ink,
        room,
    );
    if resp.has_focus() {
        // Left and Right fold the shelf instead of moving focus away.
        let filter = egui::EventFilter {
            horizontal_arrows: true,
            ..Default::default()
        };
        ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, filter));
        let keys = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::ArrowLeft),
            )
        });
        match keys {
            (true, _) => return Some(Some(true)),
            (_, true) => return Some(Some(false)),
            _ => {}
        }
    }
    resp.clicked().then_some(None)
}

#[derive(Clone, Copy)]
struct BookLook {
    /// The focused pane reads it.
    active: bool,
    /// Some pane reads it.
    open: bool,
    dragged: bool,
}

fn book_row(
    ui: &Ui,
    rect: Rect,
    line: &Line,
    look: BookLook,
    click: LibraryClick,
    t: &Tokens,
    out: &mut Commands,
) {
    let Line::Book {
        id,
        title,
        format,
        depth,
        missing,
    } = line
    else {
        return;
    };
    let row = rect.shrink2(vec2(6.0, 1.0));
    let sense = match missing {
        true => Sense::click(),
        false => Sense::click_and_drag(),
    };
    let resp = ui.interact(row, Id::new(("library-panel-book", id)), sense);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, look.active, title)
    });
    let fade = if *missing || look.dragged { 0.4 } else { 1.0 };
    let painter = ui.painter();
    if look.active {
        painter.rect_filled(row, CornerRadius::same(6), t.hover);
    } else if resp.hovered() || resp.has_focus() {
        painter.rect_filled(row, CornerRadius::same(6), t.hover.gamma_multiply(0.6));
    }
    let ink = |c: Color32| c.gamma_multiply(fade);
    let mut x = row.left() + indent(*depth) + CHEVRON_W;
    let icon = Rect::from_center_size(pos2(x + 6.5, row.center().y), vec2(13.0, 13.0));
    Icon::Book.paint(ui, icon, ink(t.muted).gamma_multiply(0.8));
    x += 19.0;
    let badge = badge_rect(ui, row, format.label(), t, fade);
    let dot = look.open && !look.active;
    let room = badge.left() - x - if dot { 14.0 } else { 6.0 };
    let (font, colour) = match (look.active, look.open || resp.hovered()) {
        (true, _) => (theme::medium(13.0), t.ink),
        (false, true) => (FontId::proportional(13.0), t.ink),
        (false, false) => (FontId::proportional(13.0), t.muted),
    };
    let elided = text(ui, pos2(x, row.center().y), title, font, ink(colour), room);
    if dot {
        let c = pos2(badge.left() - 9.0, row.center().y);
        ui.painter()
            .circle_filled(c, 3.0, t.accent.gamma_multiply(0.7));
    }
    if resp.drag_started() {
        let carried = Carried {
            book: *id,
            title: title.clone(),
        };
        resp.dnd_set_drag_payload(carried);
    }
    let tip = match missing {
        true => Some(format!("{title} isn't where it was; open it to find it")),
        false => elided.then(|| title.clone()),
    };
    let clicked = resp.clicked();
    if let Some(tip) = tip {
        resp.on_hover_text(tip);
    }
    if !clicked {
        return;
    }
    // A key press has no pointer click; Do nothing still lets keys open.
    let keyboard = !ui.input(|i| i.pointer.any_click());
    let placement = match click {
        LibraryClick::Replace => Placement::Into,
        LibraryClick::Split => Placement::Beside,
        LibraryClick::DragOnly if keyboard || *missing => Placement::Beside,
        LibraryClick::DragOnly => return,
    };
    out.push(Command::PlaceBook {
        book: *id,
        pane: None,
        placement,
    });
}

/// The format tag at the row's right, painted; its rect.
fn badge_rect(ui: &Ui, row: Rect, label: &str, t: &Tokens, fade: f32) -> Rect {
    let font = theme::medium(9.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, t.muted.gamma_multiply(fade));
    let size = vec2(galley.size().x + 8.0, 16.0);
    let rect = Rect::from_min_size(pos2(row.right() - 6.0 - size.x, row.center().y - 8.0), size);
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    let at = rect.center() - galley.size() / 2.0;
    painter.galley(at, galley, t.muted);
    rect
}

/// One line of text, cut to `room`; true if it was cut.
fn text(ui: &Ui, left: egui::Pos2, s: &str, font: FontId, colour: Color32, room: f32) -> bool {
    let text = WidgetText::from(RichText::new(s).font(font).color(colour));
    let galley = text.into_galley(
        ui,
        Some(TextWrapMode::Truncate),
        room.max(16.0),
        FontSelection::Default,
    );
    let elided = galley.elided;
    let at = pos2(left.x, left.y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, colour);
    elided
}

fn indent(depth: usize) -> f32 {
    PAD_X + (depth as f32 * INDENT).min(INDENT_MAX)
}

fn empty(ui: &mut Ui, t: &Tokens) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        let size = vec2(20.0, 20.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        Icon::Library.paint(ui, rect, t.muted);
        ui.add_space(6.0);
        ui.label(
            RichText::new("Your library is empty")
                .size(13.0)
                .color(t.ink),
        );
        let hint = "Books you add on the Library page appear here.";
        ui.label(RichText::new(hint).size(t.text_small).color(t.muted));
    });
}

/// Lights the pane a carried book would land in; `rail` is excluded.
pub fn drop_overlay(
    ui: &Ui,
    panes: &[(PaneId, Rect)],
    rail: Option<f32>,
    empty: impl Fn(PaneId) -> bool,
    name: impl Fn(PaneId) -> String,
    out: &mut Commands,
) {
    let ctx = ui.ctx();
    let Some(carried) = DragAndDrop::payload::<Carried>(ctx) else {
        return;
    };
    let Some(pointer) = ctx.pointer_latest_pos() else {
        return;
    };
    ctx.set_cursor_icon(CursorIcon::Grabbing);
    let t = Tokens::get(ctx);
    let boxes: Vec<(PaneId, Bounds)> = panes.iter().map(|(id, r)| (*id, bounds(*r))).collect();
    let over_rail = rail.is_some_and(|edge| pointer.x < edge);
    let target = match over_rail {
        true => None,
        false => DropTarget::at(&boxes, (pointer.x, pointer.y), &empty),
    };
    let layer = LayerId::new(Order::Foreground, Id::new("library-panel-drop"));
    let painter = ctx.layer_painter(layer);
    if let Some(target) = target
        && let Some((_, rect)) = panes.iter().find(|(id, _)| *id == target.pane())
    {
        let b = target.predicted(bounds(*rect));
        let lit = Rect::from_min_size(pos2(b.x, b.y), vec2(b.w, b.h)).shrink(6.0);
        painter.rect_filled(lit, 10.0, t.accent.gamma_multiply(0.14));
        let stroke = Stroke::new(2.0, t.accent);
        painter.rect_stroke(lit, 10.0, stroke, egui::StrokeKind::Inside);
        let words = target.describe(&name(target.pane()));
        chip(&painter, lit.center(), &words, &t, true);
    }
    let label = pointer + vec2(14.0, 16.0);
    chip(&painter, label, &carried.title, &t, false);
    let released = ctx.input(|i| i.pointer.any_released());
    if !released {
        return;
    }
    DragAndDrop::clear_payload(ctx);
    if let Some(target) = target {
        let placement = match target {
            DropTarget::Here { .. } => Placement::Into,
            DropTarget::Split { edge, .. } => Placement::Edge(edge),
        };
        out.push(Command::PlaceBook {
            book: carried.book,
            pane: Some(target.pane()),
            placement,
        });
    }
}

fn bounds(r: Rect) -> Bounds {
    Bounds::new(r.left(), r.top(), r.width(), r.height())
}

/// A pill of words; centred on `at`, else hanging from it.
fn chip(painter: &egui::Painter, at: egui::Pos2, words: &str, t: &Tokens, centred: bool) {
    let font = theme::medium(12.5);
    let galley = painter.layout_no_wrap(words.to_owned(), font, t.ink);
    let size = galley.size() + vec2(20.0, 10.0);
    let rect = match centred {
        true => Rect::from_center_size(at, size),
        false => Rect::from_min_size(at, size),
    };
    painter.rect_filled(rect, size.y / 2.0, t.surface);
    painter.rect_stroke(
        rect,
        size.y / 2.0,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, t.ink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_library::{ContentId, DeviceId, FileStamp, Measure, Opened};
    use std::path::PathBuf;

    fn library() -> (Library, Vec<BookId>, Vec<ShelfId>) {
        let mut lib = Library::new(DeviceId(1));
        let mut ids = Vec::new();
        for (n, title) in ["zeta", "Alpha notes", "plain", "Book"].iter().enumerate() {
            let opened = Opened {
                path: PathBuf::from(format!("/lib/{n}.epub")),
                measure: Measure {
                    content: ContentId(n as u128 + 1),
                    stamp: FileStamp::default(),
                },
                format: Format::Epub,
                title: (*title).to_owned(),
                author: None,
            };
            ids.push(lib.record_open(opened, 1));
        }
        let work = lib.create_shelf("Work", None, 2).unwrap();
        let archive = lib.create_shelf("Archive", Some(work), 3).unwrap();
        let empty = lib.create_shelf("empty", None, 4).unwrap();
        lib.file(ids[2], work, 5);
        lib.file(ids[3], archive, 6);
        (lib, ids, vec![work, archive, empty])
    }

    fn names(lines: &[Line]) -> Vec<String> {
        let name = |line: &Line| match line {
            Line::Shelf {
                name, depth, count, ..
            } => format!("{depth}:[{name} {count}]"),
            Line::Book { title, depth, .. } => format!("{depth}:{title}"),
        };
        lines.iter().map(name).collect()
    }

    #[test]
    fn the_top_lists_shelves_then_what_home_shows() {
        let (lib, _, _) = library();
        let none = BTreeSet::new();
        let unshelved = lines(&lib, &none, Home::Unshelved);
        let expect = ["0:[empty 0]", "0:[Work 2]", "0:Alpha notes", "0:zeta"];
        assert_eq!(names(&unshelved), expect);
        let every = lines(&lib, &none, Home::Everything);
        assert_eq!(every.len(), 6, "{:?}", names(&every));
    }

    #[test]
    fn an_open_shelf_shows_its_shelves_and_books_one_deeper() {
        let (lib, _, shelves) = library();
        let open: BTreeSet<ShelfId> = shelves.iter().take(2).copied().collect();
        let shown = lines(&lib, &open, Home::Unshelved);
        let expect = [
            "0:[empty 0]",
            "0:[Work 2]",
            "1:[Archive 1]",
            "2:Book",
            "1:plain",
            "0:Alpha notes",
            "0:zeta",
        ];
        assert_eq!(names(&shown), expect);
    }

    #[test]
    fn the_rows_follow_the_open_set_and_the_library() {
        let (mut lib, ids, shelves) = library();
        let mut state = PanelState::default();
        assert_eq!(state.lines(&lib, Home::Unshelved).len(), 4);
        state.set_open(shelves[0], true);
        assert!(state.is_open(shelves[0]));
        // Work opens on Archive, still shut, and plain.
        assert_eq!(state.lines(&lib, Home::Unshelved).len(), 6);
        lib.unfile(ids[2], shelves[0], 9);
        let shown = names(state.lines(&lib, Home::Unshelved));
        assert!(shown.contains(&"0:plain".to_owned()), "{shown:?}");
    }
}
