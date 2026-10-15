//! The library page: shelves and books as a grid of covers or a list.

use egui::epaint::Shadow;
use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, DragAndDrop, FontId, Id, Key, KeyboardShortcut,
    LayerId, Modifiers, Order, Painter, Rect, Sense, Stroke, StrokeKind, TextFormat, Ui, pos2,
    vec2,
};
use std::collections::BTreeSet;

use mareader_library::{
    Book, BookId, COLUMNS_MAX, COLUMNS_MIN, CoverFit, Home, Layout, Library, Shelf, ShelfId,
    SortKey, SourceId, View,
};

use super::covers::{self, Cover, Covers};
use super::folders::{self, Shown};
use super::menus;
use super::search::{self, Lit, SearchAction};
use super::shelving::{Carry, ShelfAction};
use super::{Action, BookAction, LibraryState, Pick, PickAction, Picks, Scope};
use crate::command::{Command, Commands};
use crate::documents::Placement;
use crate::icons::Icon;
use crate::motion;
use crate::theme::{self, Tokens};

const ROW_H: f32 = 60.0;
/// How far a list row's content sits in from its edges.
const ROW_INSET: f32 = 12.0;
/// Keeps the rings of picked rows apart.
const ROW_GAP: egui::Vec2 = egui::vec2(0.0, 2.0);
/// The list reads best narrow; the grid takes the window.
const LIST_WIDTH: f32 = 880.0;
const GRID_WIDTH: f32 = 1_600.0;
const MARGIN: f32 = 24.0;
const GAP: f32 = 20.0;
/// The narrowest card an automatic grid makes.
const CARD_MIN: f32 = 150.0;
/// A cover's box: A4, height over width.
const A4: f32 = 297.0 / 210.0;
/// Covers stranger than these are trimmed to them.
const SHAPES: std::ops::RangeInclusive<f32> = 0.55..=1.8;
/// The empty page's two buttons, side by side.
const EMPTY_ROW: f32 = 236.0;
/// Title, detail and progress under a card's cover.
const CARD_TEXT: f32 = 54.0;

/// A card or row: shelves come first, then books.
#[derive(Clone, Copy)]
enum Item<'a> {
    Shelf(&'a Shelf),
    Book(&'a Book),
}

impl Item<'_> {
    fn pick(self) -> Pick {
        match self {
            Self::Shelf(s) => Pick::Shelf(s.id),
            Self::Book(b) => Pick::Book(b.id),
        }
    }
}

/// The page over `ui`'s whole rect; `top` clears the title bar.
pub fn show(ui: &mut Ui, library: &mut LibraryState, view: &View, top: f32, out: &mut Commands) {
    library.settle();
    let t = Tokens::get(ui.ctx());
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, t.paper);
    let loading = library.loading();
    let offline = library.offline().clone();
    let (scope, scanning, at) = (library.scope, library.scanning(), library.at());
    let picks = library.picks().clone();
    let listing = library.listing(view);
    let searched = library
        .searching()
        .then(|| library.search_text().trim().to_owned());
    let now = ui.input(|i| i.time);
    let (lit, scroll) = library
        .revealing(now)
        .map_or((None, false), |(l, s)| (Some(l), s));
    let (model, covers) = library.parts();
    let shown = Shown {
        model,
        offline: &offline,
        scanning,
        scope,
    };
    if model.is_empty() && model.shelves().next().is_none() {
        if !loading {
            empty_state(ui, full, &shown, &t, out);
        }
        return;
    }
    let cap = match view.layout {
        Layout::Grid => GRID_WIDTH,
        Layout::List => LIST_WIDTH,
    };
    let width = (full.width() - 2.0 * MARGIN).min(cap);
    let left = full.center().x - width / 2.0;
    let body = Rect::from_min_max(
        pos2(left, top.max(full.top()) + 20.0),
        pos2(left + width, full.bottom()),
    );
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
    let books: Vec<&Book> = (listing.books.iter())
        .filter_map(|&id| model.book(id))
        .collect();
    let shelves: Vec<&Shelf> = (listing.shelves.iter())
        .filter_map(|&id| model.shelf(id))
        .collect();
    // Searching, the count says how many of how many.
    let of = searched.is_some().then_some(listing.total);
    header(
        &mut ui,
        at,
        (books.len(), shelves.len(), of),
        view,
        &shown,
        &t,
        out,
    );
    ui.add_space(12.0);
    if at.is_none() {
        folders::chips(&mut ui, &shown, out);
    }
    let items: Vec<Item<'_>> = (shelves.iter().map(|&s| Item::Shelf(s)))
        .chain(books.iter().map(|&b| Item::Book(b)))
        .collect();
    let sequence: Vec<Pick> = items.iter().map(|i| i.pick()).collect();
    carried(&ui, model, &t);
    // Empty space: a click lets go of the selection; a right-click offers more.
    let back = ui.available_rect_before_wrap();
    let back = ui.interact(back, Id::new("library-page"), Sense::click());
    if back.clicked() && !picks.is_empty() {
        out.push(Command::Library(Action::Pick(PickAction::Clear)));
    }
    back.context_menu(|ui| menus::page_menu(ui, &sequence, out));
    let typing = ui.ctx().egui_wants_keyboard_input();
    let all = KeyboardShortcut::new(Modifiers::COMMAND, Key::A);
    if !typing && ui.input_mut(|i| i.consume_shortcut(&all)) {
        out.push(Command::Library(Action::Pick(PickAction::Only(
            sequence.clone(),
        ))));
    }
    let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    if items.is_empty() {
        if let Some(text) = &searched {
            no_match(&mut ui, text, listing.elsewhere, &t, out);
            library.file_target = None;
            return;
        }
        let note = match (scope, at) {
            (Scope::Missing, _) => "Nothing is missing.",
            (_, Some(_)) => "This shelf is empty. Drag books onto it from the library.",
            _ if !model.is_empty() => "Every book is on a shelf.",
            _ => "No books here yet.",
        };
        ui.label(egui::RichText::new(note).color(t.muted));
        if hovering {
            let onto = menus::shelf_name(model, at);
            menus::file_hint(&ui, body, onto.as_deref(), &t);
        }
        library.file_target = None;
        return;
    }
    // A shelf in its own order takes drops between its books.
    let order = match (at, view.sort) {
        (Some(shelf), SortKey::Manual) => Some(Placing {
            shelf,
            ascending: view.ascending,
        }),
        _ => None,
    };
    let mut cx = Cx {
        model,
        covers,
        offline: &offline,
        fit: view.cover,
        at,
        order,
        books: &books,
        shelves: shelves.len(),
        picks: &picks,
        sequence: &sequence,
        files: hovering.then_some(None),
        lit,
        scroll_to: lit
            .filter(|_| scroll)
            .and_then(|l| sequence.iter().position(|p| *p == l.pick)),
        t: &t,
        out,
    };
    // Rows are spaced by their own height; egui's gap would add to it.
    ui.spacing_mut().item_spacing.y = 0.0;
    match view.layout {
        Layout::Grid => grid(
            &mut ui,
            &items,
            view.columns.unwrap_or(auto_columns(width)),
            &mut cx,
        ),
        Layout::List => list(&mut ui, &items, &mut cx),
    }
    menus::selection_bar(&mut ui, body, &picks, at, &t, cx.out);
    let target = cx.files.flatten();
    if hovering {
        let onto = menus::shelf_name(model, target.or(at));
        menus::file_hint(&ui, body, onto.as_deref(), &t);
    }
    library.file_target = target;
}

/// A shelf shown in its own order, which drops can change.
#[derive(Clone, Copy)]
struct Placing {
    shelf: ShelfId,
    ascending: bool,
}

/// What every card and row draws with.
struct Cx<'a> {
    model: &'a Library,
    covers: &'a mut Covers,
    /// Folders whose drive wasn't there at the last look.
    offline: &'a BTreeSet<SourceId>,
    fit: CoverFit,
    /// The shelf open on the page; dragged books leave it.
    at: Option<ShelfId>,
    order: Option<Placing>,
    /// The books in the order shown, after `shelves` shelf cards.
    books: &'a [&'a Book],
    shelves: usize,
    picks: &'a Picks,
    /// Every card in the order shown, for Shift-click ranges.
    sequence: &'a [Pick],
    /// While files from outside hover: the shelf under them, if any.
    files: Option<Option<ShelfId>>,
    /// The card a search chose, ringed for a while.
    lit: Option<Lit>,
    /// Where that card sits in `sequence`, the frame it's chosen.
    scroll_to: Option<usize>,
    t: &'a Tokens,
    out: &'a mut Commands,
}

/// What Auto shows across a page `page_width` wide, for the View menu.
pub fn page_columns(page_width: f32) -> u8 {
    auto_columns((page_width - 2.0 * MARGIN).min(GRID_WIDTH))
}

/// As many cards as fit at least [`CARD_MIN`] wide.
fn auto_columns(width: f32) -> u8 {
    let n = ((width + GAP) / (CARD_MIN + GAP)).floor();
    (n.clamp(0.0, 255.0) as u8).clamp(COLUMNS_MIN, COLUMNS_MAX)
}

/// "3 books", "1 shelf": a count and its noun.
fn counted(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

fn header(
    ui: &mut Ui,
    at: Option<ShelfId>,
    (books, shelves, of): (usize, usize, Option<usize>),
    view: &View,
    shown: &Shown<'_>,
    t: &Tokens,
    out: &mut Commands,
) {
    // Lined up with the list rows' text, which sits in from their edges.
    let inset = match view.layout {
        Layout::Grid => 0.0,
        Layout::List => ROW_INSET,
    };
    let shelf = at.and_then(|id| shown.model.shelf(id));
    let name = shelf.map_or("Library", |s| s.name.value.as_str());
    // Unshelved, the top level counts only what it shows.
    let unshelved = shelf.is_none() && shown.scope == Scope::All && view.home == Home::Unshelved;
    let mut count = match (of, unshelved) {
        (Some(of), _) => format!("{books} of {}", counted(of, "book", "books")),
        (None, true) => counted(books, "unshelved book", "unshelved books"),
        (None, false) => counted(books, "book", "books"),
    };
    if shelf.is_some() && shelves > 0 {
        count = format!("{count} · {}", counted(shelves, "shelf", "shelves"));
    }
    ui.horizontal(|ui| {
        ui.add_space(inset);
        let title = egui::RichText::new(name).font(theme::medium(22.0));
        ui.add(egui::Label::new(title.color(t.ink)).truncate());
        ui.label(egui::RichText::new(count).color(t.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(inset);
            // Adding books is the shelf's own; the bar keeps the View menu.
            if shelf.is_none() && header_button(ui, Icon::Open, "Open…", t).clicked() {
                out.push(Command::ShowOpenDialog(Placement::Into));
            }
            // Copies kept by the library, onto the shelf shown.
            if header_button(ui, Icon::Import, "Import…", t).clicked() {
                out.push(Command::Library(Action::Book(BookAction::PickImport)));
            }
            if shelf.is_none() {
                folders::button(ui, shown, t, out);
            }
            if header_button(ui, Icon::Plus, "New shelf", t).clicked() {
                out.push(Command::Library(Action::Shelf(ShelfAction::New)));
            }
        });
    });
}

fn header_button(ui: &mut Ui, icon: Icon, label: &str, t: &Tokens) -> egui::Response {
    let button = egui::Button::image_and_text(icon.image(16.0), label)
        .image_tint_follows_text_color(true)
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius)
        .min_size(vec2(96.0, 32.0));
    ui.add(button)
}

fn grid(ui: &mut Ui, items: &[Item<'_>], columns: u8, cx: &mut Cx<'_>) {
    let cols = usize::from(columns.max(1));
    let width = ui.available_width();
    let card = ((width - GAP * (cols - 1) as f32) / cols as f32).max(1.0);
    let height = card * A4 + CARD_TEXT;
    let rows = items.len().div_ceil(cols);
    let mut area = egui::ScrollArea::vertical()
        .id_salt("library-grid")
        .auto_shrink(false)
        .content_margin(margin(cx));
    if let Some(at) = cx.scroll_to {
        area = area.vertical_scroll_offset(centred(ui, at / cols, height + GAP));
    }
    area.show_rows(ui, height + GAP, rows, |ui, range| {
        for row in range.clone() {
            let (line, _) = ui.allocate_exact_size(vec2(width, height + GAP), Sense::hover());
            for col in 0..cols {
                let at = row * cols + col;
                let Some(&item) = items.get(at) else {
                    break;
                };
                let x = line.left() + col as f32 * (card + GAP);
                let rect = Rect::from_min_size(pos2(x, line.top()), vec2(card, height));
                match item {
                    Item::Shelf(shelf) => shelf_card(ui, rect, shelf, cx),
                    Item::Book(book) => grid_card(ui, rect, book, at, cx),
                }
            }
        }
        prefetch(cx.covers, items, range, cols);
    });
}

/// Room for the cards' rings above, and for the selection bar below.
fn margin(cx: &Cx<'_>) -> egui::Margin {
    let foot = if cx.picks.is_empty() { 8 } else { 76 };
    egui::Margin {
        top: 8,
        bottom: foot,
        ..egui::Margin::ZERO
    }
}

/// The scroll that puts row `row` mid-page.
fn centred(ui: &Ui, row: usize, height: f32) -> f32 {
    let mid = row as f32 * height + height / 2.0;
    (mid - ui.available_height() / 2.0).max(0.0)
}

fn list(ui: &mut Ui, items: &[Item<'_>], cx: &mut Cx<'_>) {
    let mut area = egui::ScrollArea::vertical()
        .id_salt("library-list")
        .auto_shrink(false)
        .content_margin(margin(cx));
    if let Some(at) = cx.scroll_to {
        area = area.vertical_scroll_offset(centred(ui, at, ROW_H));
    }
    area.show_rows(ui, ROW_H, items.len(), |ui, range| {
        for at in range.clone() {
            match items.get(at) {
                Some(Item::Shelf(shelf)) => shelf_row(ui, shelf, cx),
                Some(Item::Book(book)) => list_row(ui, book, at, cx),
                None => {}
            }
        }
        prefetch(cx.covers, items, range, 1);
    });
}

/// Asks for the covers a screen below, then above, what shows.
fn prefetch(covers: &mut Covers, items: &[Item<'_>], rows: std::ops::Range<usize>, cols: usize) {
    let screen = rows.len().max(1);
    let below = rows.end..rows.end + screen;
    let above = (rows.start.saturating_sub(screen)..rows.start).rev();
    for row in below.chain(above) {
        for item in items.iter().skip(row * cols).take(cols) {
            if let Item::Book(book) = item {
                covers.prefetch(book);
            }
        }
    }
}

/// A card or row that opens its book, and carries it when dragged.
fn hit(ui: &mut Ui, rect: Rect, book: &Book, cx: &mut Cx<'_>) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(book.id), Sense::click_and_drag());
    let title = book.title.value.as_str();
    let picked = cx.picks.contains(Pick::Book(book.id));
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, picked, title));
    if resp.clicked() && !picking(ui, Pick::Book(book.id), cx) {
        cx.out.push(Command::OpenBook(book.id));
    }
    if resp.drag_started() {
        let from = cx.at;
        resp.dnd_set_drag_payload(carry(Pick::Book(book.id), from, cx));
    }
    resp.context_menu(|ui| match picked {
        true => menus::picks_menu(ui, cx.picks, cx.at, cx.t, cx.out),
        false => menus::book_menu(ui, book, cx.at, cx.t, cx.out),
    });
    resp
}

/// Picks on a click if asked or already picking; false to open instead.
fn picking(ui: &Ui, pick: Pick, cx: &mut Cx<'_>) -> bool {
    let m = ui.input(|i| i.modifiers);
    let action = match () {
        _ if m.shift && cx.picks.has_anchor() => PickAction::Add(cx.picks.range(cx.sequence, pick)),
        _ if m.command || m.shift || !cx.picks.is_empty() => PickAction::Toggle(pick),
        _ => return false,
    };
    cx.out.push(Command::Library(Action::Pick(action)));
    true
}

/// What dragging `pick` carries: the whole selection, if it's in it.
fn carry(pick: Pick, from: Option<ShelfId>, cx: &Cx<'_>) -> Carry {
    let many = cx.picks.len() > 1 && cx.picks.contains(pick);
    match pick {
        _ if many => {
            // In the order shown, for drops between books.
            let picked = cx.sequence.iter().filter(|p| cx.picks.contains(**p));
            let books = picked.filter_map(|p| match p {
                Pick::Book(id) => Some(*id),
                Pick::Shelf(_) => None,
            });
            Carry::Many {
                books: books.collect(),
                shelves: cx.picks.shelves(),
                from,
            }
        }
        Pick::Book(id) => Carry::Book { id, from },
        Pick::Shelf(id) => Carry::Shelf(id),
    }
}

/// On a shelf in its own order, a book dropped beside this one moves there.
fn reorder_drop(ui: &Ui, resp: &egui::Response, at: usize, across: bool, cx: &mut Cx<'_>) {
    let Some(order) = cx.order else {
        return;
    };
    let carried = resp.dnd_hover_payload::<Carry>();
    let Some(moving) = carried.and_then(|c| c.books_from(order.shelf)) else {
        return;
    };
    let Some(pointer) = ui.ctx().pointer_interact_pos() else {
        return;
    };
    let rect = resp.rect;
    let after = match across {
        true => pointer.x > rect.center().x,
        false => pointer.y > rect.center().y,
    };
    let shown = at.saturating_sub(cx.shelves);
    let gap = shown + usize::from(after);
    // Shown last to first, the gap's key order is mirrored.
    let after_gap: Vec<BookId> = match order.ascending {
        true => cx.books.iter().skip(gap).map(|b| b.id).collect(),
        false => cx.books.iter().take(gap).rev().map(|b| b.id).collect(),
    };
    // The first book after the gap that isn't itself moving.
    let before = after_gap.into_iter().find(|b| !moving.contains(b));
    let bar = match (across, after) {
        (true, false) => Rect::from_center_size(
            rect.left_center() - vec2(GAP / 2.0, 0.0),
            vec2(3.0, rect.height()),
        ),
        (true, true) => Rect::from_center_size(
            rect.right_center() + vec2(GAP / 2.0, 0.0),
            vec2(3.0, rect.height()),
        ),
        (false, false) => Rect::from_center_size(rect.center_top(), vec2(rect.width(), 3.0)),
        (false, true) => Rect::from_center_size(rect.center_bottom(), vec2(rect.width(), 3.0)),
    };
    ui.painter().rect_filled(bar, 1.5, cx.t.accent);
    if resp.dnd_release_payload::<Carry>().is_some() {
        let shelf = order.shelf;
        let action = ShelfAction::Reorder {
            books: moving,
            shelf,
            before,
        };
        cx.out.push(Command::Library(Action::Shelf(action)));
    }
}

/// What's carried, beside the pointer; Alt adds a copy instead.
fn carried(ui: &Ui, model: &Library, t: &Tokens) {
    let Some(carry) = DragAndDrop::payload::<Carry>(ui.ctx()) else {
        return;
    };
    let ctx = ui.ctx();
    ctx.set_cursor_icon(CursorIcon::Grabbing);
    let Some(pointer) = ctx.pointer_latest_pos() else {
        return;
    };
    let keep = ctx.input(|i| i.modifiers.alt);
    let mut label = carry.label(model);
    if keep && carry.off_shelf() {
        label = format!("+ {label}");
    }
    let p = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("library-carried")));
    let mut job = LayoutJob::single_section(label, TextFormat::simple(theme::medium(13.0), t.ink));
    job.wrap = TextWrapping::truncate_at_width(240.0);
    let g = p.layout_job(job);
    let chip = Rect::from_min_size(pointer + vec2(14.0, 12.0), g.size() + vec2(20.0, 12.0));
    let shadow = Shadow {
        offset: [0, 3],
        blur: 12,
        spread: 0,
        color: t.shadow,
    };
    p.add(shadow.as_shape(chip, CornerRadius::from(t.radius)));
    p.rect(
        chip,
        t.radius,
        t.surface,
        Stroke::new(1.0, t.line),
        StrokeKind::Inside,
    );
    p.galley(chip.min + vec2(10.0, 6.0), g, t.ink);
}

/// A shelf's card or row: opens on click, carried by drag, takes drops.
fn shelf_hit(ui: &mut Ui, rect: Rect, shelf: &Shelf, cx: &mut Cx<'_>) -> (egui::Response, bool) {
    let id = shelf.id;
    let resp = ui.interact(rect, ui.id().with(("shelf", id)), Sense::click_and_drag());
    let name = shelf.name.value.clone();
    let picked = cx.picks.contains(Pick::Shelf(id));
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, picked, &name));
    if resp.clicked() && !picking(ui, Pick::Shelf(id), cx) {
        let go = ShelfAction::GoTo(Some(id));
        cx.out.push(Command::Library(Action::Shelf(go)));
    }
    if resp.drag_started() {
        resp.dnd_set_drag_payload(carry(Pick::Shelf(id), cx.at, cx));
    }
    let keep = ui.input(|i| i.modifiers.alt);
    let hovered = resp.dnd_hover_payload::<Carry>();
    let mut target = hovered.is_some_and(|c| c.onto(Some(id), keep).is_some());
    if let Some(carry) = resp.dnd_release_payload::<Carry>() {
        let action = carry.onto(Some(id), keep).map(Command::Library);
        cx.out.extend(action);
    }
    // Files from outside, held over it, go onto it.
    let pointer = ui.ctx().pointer_latest_pos();
    if let Some(files) = &mut cx.files
        && pointer.is_some_and(|p| rect.contains(p) && ui.clip_rect().contains(p))
    {
        *files = Some(id);
        target = true;
    }
    resp.context_menu(|ui| match picked {
        true => menus::picks_menu(ui, cx.picks, cx.at, cx.t, cx.out),
        false => menus::shelf_menu(ui, shelf, cx.t, cx.out),
    });
    (resp, target)
}

/// A card's ground: hovered, picked, or a drop target ringed.
fn backing(
    ui: &Ui,
    rect: Rect,
    radius: f32,
    (hovered, picked, target): (bool, bool, bool),
    t: &Tokens,
) {
    let p = ui.painter();
    if picked {
        p.rect_filled(rect, radius, t.accent.gamma_multiply(0.14));
    } else if hovered || target {
        p.rect_filled(rect, radius, t.hover);
    }
    if picked || target {
        let ring = Stroke::new(2.0, t.accent);
        p.rect_stroke(rect, radius, ring, StrokeKind::Inside);
    }
}

/// Rings the card a search chose.
fn lit_ring(ui: &Ui, rect: Rect, radius: f32, pick: Pick, cx: &Cx<'_>) {
    if let Some(lit) = cx.lit.filter(|l| l.pick == pick) {
        search::ring(ui, rect, radius, lit.age, cx.t);
    }
}

/// "4 books · 1 shelf", or "Empty".
fn shelf_detail(model: &Library, id: ShelfId) -> String {
    let (books, shelves) = model.counts(id);
    let mut parts = Vec::new();
    if books > 0 {
        parts.push(counted(books, "book", "books"));
    }
    if shelves > 0 {
        parts.push(counted(shelves, "shelf", "shelves"));
    }
    match parts.is_empty() {
        true => "Empty".into(),
        false => parts.join(" · "),
    }
}

fn shelf_card(ui: &mut Ui, rect: Rect, shelf: &Shelf, cx: &mut Cx<'_>) {
    let (resp, target) = shelf_hit(ui, rect, shelf, cx);
    let t = cx.t;
    let pad = rect.expand2(vec2(8.0, 8.0));
    let picked = cx.picks.contains(Pick::Shelf(shelf.id));
    backing(ui, pad, t.radius_lg, (resp.hovered(), picked, target), t);
    lit_ring(ui, pad, t.radius_lg, Pick::Shelf(shelf.id), cx);
    let bounds = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width() * A4));
    plate(ui, bounds, shelf.id, cx);
    if !cx.picks.is_empty() {
        menus::mark(ui, bounds.min + vec2(16.0, 16.0), picked, t);
    }
    let p = ui.painter();
    let width = rect.width();
    let mut y = bounds.bottom() + 8.0;
    y += line(
        p,
        &shelf.name.value,
        theme::medium(13.0),
        t.ink,
        (rect.left(), y),
        width,
    );
    let detail = shelf_detail(cx.model, shelf.id);
    let small = FontId::proportional(12.0);
    line(p, &detail, small, t.muted, (rect.left(), y + 1.0), width);
}

/// A shelf's face: its first four shelves and books, two by two.
fn plate(ui: &Ui, bounds: Rect, id: ShelfId, cx: &mut Cx<'_>) {
    let t = cx.t;
    let radius = t.radius.min(6.0);
    ui.painter().rect_filled(bounds, radius, t.hover);
    let edge = Stroke::new(1.0, t.line);
    ui.painter()
        .rect_stroke(bounds, radius, edge, StrokeKind::Inside);
    let pad = (bounds.width() * 0.07).max(4.0);
    let cell = vec2(
        (bounds.width() - 3.0 * pad) / 2.0,
        (bounds.height() - 3.0 * pad) / 2.0,
    );
    let model = cx.model;
    let inner = model.children(Some(id)).into_iter().map(Item::Shelf);
    let books = model.shelf_books(id).into_iter().map(Item::Book);
    let mut items = inner.chain(books).take(4).peekable();
    if items.peek().is_none() {
        let icon = (bounds.width() * 0.22).clamp(16.0, 40.0);
        let mark = Rect::from_center_size(bounds.center(), vec2(icon, icon));
        Icon::Shelf.paint(ui, mark, t.muted.gamma_multiply(0.6));
    }
    for (n, item) in items.enumerate() {
        let (col, row) = ((n % 2) as f32, (n / 2) as f32);
        let min = bounds.min + vec2(pad + col * (cell.x + pad), pad + row * (cell.y + pad));
        let at = Rect::from_min_size(min, cell);
        match item {
            Item::Book(book) => cover(ui, at, book, cx, 1.0, true),
            Item::Shelf(_) => {
                ui.painter().rect_filled(at, 3.0, t.surface);
                ui.painter().rect_stroke(at, 3.0, edge, StrokeKind::Inside);
                let icon = (cell.x * 0.4).clamp(10.0, 28.0);
                let mark = Rect::from_center_size(at.center(), vec2(icon, icon));
                Icon::Shelf.paint(ui, mark, t.muted);
            }
        }
    }
}

fn shelf_row(ui: &mut Ui, shelf: &Shelf, cx: &mut Cx<'_>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let (resp, target) = shelf_hit(ui, rect, shelf, cx);
    let t = cx.t;
    let picked = cx.picks.contains(Pick::Shelf(shelf.id));
    backing(
        ui,
        rect.shrink2(ROW_GAP),
        t.radius,
        (resp.hovered(), picked, target),
        t,
    );
    lit_ring(
        ui,
        rect.shrink2(ROW_GAP),
        t.radius,
        Pick::Shelf(shelf.id),
        cx,
    );
    let thumb = Rect::from_center_size(
        pos2(rect.left() + ROW_INSET + 17.0, rect.center().y),
        vec2(34.0, 34.0 * A4),
    );
    // While picking, the mark takes the icon's place.
    if cx.picks.is_empty() {
        let icon = Rect::from_center_size(thumb.center(), vec2(24.0, 24.0));
        Icon::Shelf.paint(ui, icon, t.muted);
    } else {
        menus::mark(ui, thumb.center(), picked, t);
    }
    let p = ui.painter();
    let x = thumb.right() + 14.0;
    let wide = (rect.right() - ROW_INSET - x - 24.0).max(40.0);
    let name = &shelf.name.value;
    line(
        p,
        name,
        theme::medium(14.0),
        t.ink,
        (x, rect.center().y - 18.0),
        wide,
    );
    let detail = shelf_detail(cx.model, shelf.id);
    let small = FontId::proportional(12.0);
    line(p, &detail, small, t.muted, (x, rect.center().y + 2.0), wide);
    let chevron = Rect::from_center_size(
        pos2(rect.right() - ROW_INSET - 8.0, rect.center().y),
        vec2(16.0, 16.0),
    );
    Icon::ChevronRight.paint(ui, chevron, t.muted);
    p.hline(
        rect.x_range().shrink(ROW_INSET),
        rect.bottom(),
        Stroke::new(1.0, t.line),
    );
}

fn grid_card(ui: &mut Ui, rect: Rect, book: &Book, at: usize, cx: &mut Cx<'_>) {
    let resp = hit(ui, rect, book, cx);
    reorder_drop(ui, &resp, at, true, cx);
    let t = cx.t;
    // A missing book stays shown, faded, until found or removed.
    let missing = book.missing();
    let offline = folders::offline(book, cx.offline);
    let fade = if missing { 0.5 } else { 1.0 };
    let picked = cx.picks.contains(Pick::Book(book.id));
    let pad = rect.expand2(vec2(8.0, 8.0));
    backing(ui, pad, t.radius_lg, (resp.hovered(), picked, false), t);
    lit_ring(ui, pad, t.radius_lg, Pick::Book(book.id), cx);
    let bounds = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width() * A4));
    cover(ui, bounds, book, cx, fade, false);
    if !cx.picks.is_empty() {
        menus::mark(ui, bounds.min + vec2(16.0, 16.0), picked, t);
    }
    let p = ui.painter();
    if missing {
        let tag = if offline { "Offline" } else { "Missing" };
        let tag = p.layout_no_wrap(tag.into(), theme::medium(11.0), t.ink);
        let pill = Rect::from_min_size(
            pos2(bounds.right() - tag.size().x - 20.0, bounds.top() + 8.0),
            tag.size() + vec2(12.0, 6.0),
        );
        p.rect_filled(pill, t.radius, t.surface);
        p.galley(pill.min + vec2(6.0, 3.0), tag, t.ink);
    }
    let width = rect.width();
    let mut y = bounds.bottom() + 8.0;
    let title = book.title.value.as_str();
    y += line(
        p,
        title,
        theme::medium(13.0),
        faded(t.ink, fade),
        (rect.left(), y),
        width,
    );
    let detail = card_detail(cx.model, book, offline);
    let small = FontId::proportional(12.0);
    y += line(
        p,
        &detail,
        small,
        faded(t.muted, fade),
        (rect.left(), y + 1.0),
        width,
    );
    if let Some(done) = cx.model.position(book.id).and_then(|p| p.fraction()) {
        let track = Rect::from_min_size(pos2(rect.left(), y + 7.0), vec2(width, 3.0));
        p.rect_filled(track, 1.5, faded(t.line, fade));
        let read = Rect::from_min_size(track.min, vec2(width * done, 3.0));
        p.rect_filled(read, 1.5, faded(t.accent, fade));
    }
}

fn list_row(ui: &mut Ui, book: &Book, at: usize, cx: &mut Cx<'_>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let resp = hit(ui, rect, book, cx);
    reorder_drop(ui, &resp, at, false, cx);
    let t = cx.t;
    let missing = book.missing();
    let fade = if missing { 0.5 } else { 1.0 };
    let picked = cx.picks.contains(Pick::Book(book.id));
    backing(
        ui,
        rect.shrink2(ROW_GAP),
        t.radius,
        (resp.hovered(), picked, false),
        t,
    );
    lit_ring(ui, rect.shrink2(ROW_GAP), t.radius, Pick::Book(book.id), cx);
    let thumb = Rect::from_center_size(
        pos2(rect.left() + ROW_INSET + 17.0, rect.center().y),
        vec2(34.0, 34.0 * A4),
    );
    cover(ui, thumb, book, cx, fade, true);
    if !cx.picks.is_empty() {
        menus::mark(ui, thumb.center(), picked, t);
    }
    let p = ui.painter();
    // Books found in a folder may never have been opened.
    let when = match book.opened.value {
        0 => "Not opened".to_owned(),
        at => ago(super::wall_ms().saturating_sub(at)),
    };
    let when = p.layout_no_wrap(when, FontId::proportional(12.0), faded(t.muted, fade));
    let right = rect.right() - ROW_INSET;
    let at = pos2(right - when.size().x, rect.center().y - when.size().y / 2.0);
    p.galley(at, when, t.muted);
    let x = thumb.right() + 14.0;
    let wide = (right - x - 96.0).max(40.0);
    let title = book.title.value.as_str();
    let ink = faded(t.ink, fade);
    let center = |y: f32, h: f32| y - h / 2.0;
    let job = |text: &str, font: FontId, color: Color32| {
        let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
        job.wrap = TextWrapping::truncate_at_width(wide);
        p.layout_job(job)
    };
    let g = job(title, theme::medium(14.0), ink);
    p.galley(pos2(x, center(rect.center().y - 9.0, g.size().y)), g, ink);
    let offline = folders::offline(book, cx.offline);
    let detail = format!(
        "{} · {}",
        book.format.label(),
        detail(cx.model, book, offline)
    );
    let muted = faded(t.muted, fade);
    let g = job(&detail, FontId::proportional(12.0), muted);
    p.galley(
        pos2(x, center(rect.center().y + 10.0, g.size().y)),
        g,
        muted,
    );
    p.hline(
        rect.x_range().shrink(ROW_INSET),
        rect.bottom(),
        Stroke::new(1.0, t.line),
    );
}

/// One truncated line at `at`; returns its height.
fn line(p: &Painter, text: &str, font: FontId, color: Color32, at: (f32, f32), w: f32) -> f32 {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    job.wrap = TextWrapping::truncate_at_width(w.max(8.0));
    let g = p.layout_job(job);
    let h = g.size().y;
    p.galley(pos2(at.0, at.1), g, color);
    h
}

fn faded(c: Color32, fade: f32) -> Color32 {
    c.gamma_multiply(fade)
}

fn cover(ui: &Ui, bounds: Rect, book: &Book, cx: &mut Cx<'_>, fade: f32, small: bool) {
    let fit = (!small).then_some(cx.fit);
    paint_cover(ui, bounds, book, (cx.covers, cx.t), fit, fade);
}

/// A small cover, as a suggestion shows it.
pub(super) fn thumb(ui: &Ui, bounds: Rect, book: &Book, covers: &mut Covers, t: &Tokens) {
    paint_cover(ui, bounds, book, (covers, t), None, 1.0);
}

/// Art if read, else a placeholder or a drawn cover; no fit is a thumb.
fn paint_cover(
    ui: &Ui,
    bounds: Rect,
    book: &Book,
    (covers, t): (&mut Covers, &Tokens),
    fit: Option<CoverFit>,
    fade: f32,
) {
    let small = fit.is_none();
    let radius = if small { 3.0 } else { t.radius.min(6.0) };
    let p = ui.painter();
    match covers.get(ui.ctx(), book, motion::enabled(ui.ctx())) {
        Cover::Art { texture, alpha } => {
            let shape = texture.size.y / texture.size.x;
            let size = vec2(1.0, shape.clamp(*SHAPES.start(), *SHAPES.end()));
            let rect = match fit {
                Some(CoverFit::Fit) => covers::fit(bounds, size),
                _ => bounds,
            };
            if alpha < 1.0 {
                p.rect_filled(rect, radius, faded(t.hover, fade));
            }
            let shadow = Shadow {
                offset: [0, 2],
                blur: if small { 3 } else { 10 },
                spread: 0,
                color: faded(t.shadow, fade * alpha),
            };
            p.add(shadow.as_shape(rect, CornerRadius::from(radius)));
            egui::Image::from_texture(texture)
                .uv(covers::crop(rect, texture.size))
                .corner_radius(radius)
                .tint(Color32::WHITE.gamma_multiply(fade * alpha))
                .paint_at(ui, rect);
            let edge = Stroke::new(1.0, faded(t.line, 0.6 * fade * alpha));
            p.rect_stroke(rect, radius, edge, StrokeKind::Inside);
        }
        Cover::Waiting => {
            p.rect_filled(bounds, radius, faded(t.hover, fade));
        }
        Cover::Drawn => drawn(p, bounds, book, t, fade, small),
    }
}

/// A cover for a book without art: a tinted card with its names.
fn drawn(p: &Painter, rect: Rect, book: &Book, t: &Tokens, fade: f32, small: bool) {
    let radius = if small { 3.0 } else { t.radius.min(6.0) };
    // A steady hue per book, softened toward the theme's surface.
    let mixed = book
        .content
        .0
        .wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835);
    let hue = ((mixed >> 96) % 360) as f32 / 360.0;
    let (s, v) = if t.dark { (0.35, 0.36) } else { (0.24, 0.9) };
    let tone = Color32::from(egui::ecolor::Hsva::new(hue, s, v, 1.0));
    let paper = tone.lerp_to_gamma(t.surface, 0.3);
    p.rect_filled(rect, radius, faded(paper, fade));
    let spine = Rect::from_min_size(rect.min, vec2(rect.width() * 0.06, rect.height()));
    let corners = CornerRadius {
        nw: radius as u8,
        sw: radius as u8,
        ne: 0,
        se: 0,
    };
    p.rect_filled(spine, corners, faded(t.ink, 0.08 * fade));
    p.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, faded(t.line, fade)),
        StrokeKind::Inside,
    );
    if small {
        return;
    }
    let pad = rect.width() * 0.12;
    let left = spine.right() + pad * 0.8;
    let width = rect.right() - pad - left;
    let size = (rect.width() / 9.0).clamp(11.0, 22.0);
    let block = |text: &str, font: FontId, color: Color32, rows: usize| {
        let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
        job.wrap = TextWrapping {
            max_width: width.max(8.0),
            max_rows: rows,
            break_anywhere: false,
            overflow_character: Some('…'),
        };
        p.layout_job(job)
    };
    let ink = faded(t.ink, fade);
    let title = block(&book.title.value, theme::medium(size), ink, 5);
    let mut y = rect.top() + pad * 1.2;
    let title_h = title.size().y;
    p.galley(pos2(left, y), title, ink);
    y += title_h + size * 0.6;
    let rule = Rect::from_min_size(pos2(left, y), vec2(width.min(size * 2.0), 2.0));
    p.rect_filled(rule, 1.0, faded(t.accent, fade));
    y += size * 0.6;
    if let Some(author) = book.author.value.as_deref() {
        let muted = faded(t.muted, fade);
        let g = block(author, FontId::proportional(size * 0.72), muted, 2);
        p.galley(pos2(left, y), g, muted);
    }
    p.text(
        pos2(left, rect.bottom() - pad),
        Align2::LEFT_BOTTOM,
        book.format.label(),
        theme::medium((size * 0.55).max(9.0)),
        faded(t.muted, fade),
    );
}

/// Under a card's title: the author, else how far through.
fn card_detail(model: &Library, book: &Book, offline: bool) -> String {
    if offline {
        return "Drive not connected".into();
    }
    if book.missing() {
        return "Moved or deleted".into();
    }
    let author = book.author.value.as_deref().map(str::trim);
    if let Some(author) = author.filter(|a| !a.is_empty()) {
        return author.to_owned();
    }
    match model.position(book.id) {
        Some(p) if p.pages > 0 => format!("Page {} of {}", p.page + 1, p.pages),
        _ => "Not started".into(),
    }
}

/// "Page 12 of 300 · 4%", or why it can't open.
fn detail(model: &Library, book: &Book, offline: bool) -> String {
    if offline {
        return "Missing: its drive isn't connected".into();
    }
    if book.missing() {
        return "Missing: moved or deleted".into();
    }
    let author = book.author.value.as_deref();
    let place = model.position(book.id).and_then(|p| {
        let pct = (p.fraction()? * 100.0).round();
        Some(format!("Page {} of {} · {pct}%", p.page + 1, p.pages))
    });
    let parts: Vec<&str> = [author, place.as_deref()].into_iter().flatten().collect();
    match parts.is_empty() {
        true => "Not started".into(),
        false => parts.join(" · "),
    }
}

/// "5 min ago": how long since, in the largest whole unit.
fn ago(ms: u64) -> String {
    let min = ms / 60_000;
    let (h, d) = (min / 60, min / 1_440);
    match () {
        _ if min < 1 => "Just now".into(),
        _ if h < 1 => format!("{min} min ago"),
        _ if d < 1 => format!("{h} h ago"),
        _ if d < 2 => "Yesterday".into(),
        _ if d < 14 => format!("{d} days ago"),
        _ if d < 60 => format!("{} weeks ago", d / 7),
        _ if d < 730 => format!("{} months ago", d / 30),
        _ => format!("{} years ago", d / 365),
    }
}

/// Nothing here matches: say so, and offer the wider library.
fn no_match(ui: &mut Ui, text: &str, elsewhere: usize, t: &Tokens, out: &mut Commands) {
    let note = format!("No books match “{text}”.");
    ui.label(egui::RichText::new(note).color(t.muted));
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let act = |a| Command::Library(Action::Search(a));
        if elsewhere > 0 {
            let all = match elsewhere {
                1 => "Show the 1 match elsewhere".to_owned(),
                n => format!("Show the {n} matches elsewhere"),
            };
            if header_button(ui, Icon::Search, &all, t).clicked() {
                out.push(act(SearchAction::Widen));
            }
        }
        if header_button(ui, Icon::Close, "Clear the search", t).clicked() {
            out.push(act(SearchAction::Clear));
        }
    });
}

fn empty_state(ui: &mut Ui, rect: Rect, shown: &Shown<'_>, t: &Tokens, out: &mut Commands) {
    let at = Rect::from_center_size(rect.center(), vec2(rect.width(), 200.0));
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(at));
    ui.vertical_centered(|ui| {
        ui.add(Icon::Library.image(40.0).tint(t.muted));
        ui.add_space(10.0);
        let title = egui::RichText::new("Your library is empty").font(theme::medium(16.0));
        ui.label(title.color(t.ink));
        let hint = "Books you open appear here, ready to resume. \
                    Or add a folder to read its books where they are.";
        ui.label(egui::RichText::new(hint).size(13.0).color(t.muted));
        ui.add_space(14.0);
        // A fixed-width row, which the centred layout centres.
        let row = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_ui_with_layout(vec2(EMPTY_ROW, 32.0), row, |ui| {
            if header_button(ui, Icon::Open, "Open…", t).clicked() {
                out.push(Command::ShowOpenDialog(Placement::Into));
            }
            // An empty folder already added stays reachable to remove.
            match shown.model.sources().next() {
                Some(_) => folders::button(ui, shown, t, out),
                None => {
                    let add =
                        egui::Button::image_and_text(Icon::FolderPlus.image(16.0), "Add folder…")
                            .image_tint_follows_text_color(true)
                            .fill(t.surface)
                            .stroke(Stroke::new(1.0, t.line))
                            .corner_radius(t.radius)
                            .min_size(vec2(96.0, 32.0));
                    if ui.add(add).clicked() {
                        out.push(Command::Library(Action::PickFolder));
                    }
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::{CARD_MIN, GAP, ago, auto_columns};

    #[test]
    fn times_read_in_the_largest_whole_unit() {
        let min = 60_000;
        assert_eq!(ago(10_000), "Just now");
        assert_eq!(ago(5 * min), "5 min ago");
        assert_eq!(ago(3 * 60 * min), "3 h ago");
        assert_eq!(ago(30 * 60 * min), "Yesterday");
        assert_eq!(ago(4 * 1_440 * min), "4 days ago");
        assert_eq!(ago(800 * 1_440 * min), "2 years ago");
    }

    #[test]
    fn automatic_columns_keep_cards_wide_within_the_limits() {
        assert_eq!(auto_columns(100.0), 2);
        assert_eq!(auto_columns(4.0 * CARD_MIN + 3.0 * GAP), 4);
        assert_eq!(auto_columns(4.0 * CARD_MIN + 3.0 * GAP - 1.0), 3);
        assert_eq!(auto_columns(10_000.0), 10);
        assert_eq!(auto_columns(f32::NAN), 2);
    }
}
