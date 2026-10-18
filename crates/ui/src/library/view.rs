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
    Book, BookId, COLUMNS_MAX, COLUMNS_MIN, CoverFit, FolderId, Home, Layout, Library, Link, Shelf,
    ShelfId, SortKey, View,
};

use super::add::{self, Here};
use super::covers::{self, Cover, Covers};
use super::folders;
use super::living::{Grab, Motion};
use super::menus;
use super::search::{self, Lit, SearchAction};
use super::shelving::{Carry, ShelfAction};
use super::{Action, LibraryState, Pick, PickAction, Picks, Scope};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::motion;
use crate::theme::{self, Tokens};

mod grid;
pub(super) mod links;
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
/// How long a still press takes to pick a card, as on a phone.
const LONG_PRESS: f64 = 0.45;
/// Title, detail and progress under a card's cover.
const CARD_TEXT: f32 = 54.0;

/// A card or row, a shelf, a book or a link, ready to draw.
#[derive(Clone, Copy)]
enum Item<'a> {
    Shelf(&'a Shelf),
    Book(&'a Book),
    Link(&'a Link),
}

impl<'a> Item<'a> {
    fn pick(self) -> Pick {
        match self {
            Self::Shelf(s) => Pick::Shelf(s.id),
            Self::Book(b) => Pick::Book(b.id),
            Self::Link(l) => Pick::Link(l.id),
        }
    }

    /// What a pick names, if it's still there.
    fn of(model: &'a Library, pick: Pick) -> Option<Self> {
        match pick {
            Pick::Shelf(id) => model.shelf(id).map(Self::Shelf),
            Pick::Book(id) => model.book(id).map(Self::Book),
            Pick::Link(id) => model.link(id).map(Self::Link),
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
    let (scope, at) = (library.scope, library.at());
    let picks = library.picks().clone();
    let listing = library.listing(view);
    let searched = library
        .searching()
        .then(|| library.search_text().trim().to_owned());
    let now = ui.input(|i| i.time);
    let (lit, scroll) = library
        .revealing(now)
        .map_or((None, false), |(l, s)| (Some(l), s));
    let (model, covers, motion) = library.page_parts();
    if model.is_empty() && model.shelves().next().is_none() {
        if !loading {
            empty_state(ui, full, model, &t, out);
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
    let items: Vec<Item<'_>> = (listing.items.iter())
        .filter_map(|&pick| Item::of(model, pick))
        .collect();
    let sequence: &[Pick] = &listing.items;
    let books = items.iter().filter(|i| matches!(i, Item::Book(_))).count();
    // Searching, the count says how many of how many.
    let of = searched.is_some().then_some(listing.total);
    let counts = (books, items.len() - books, of);
    header(&mut ui, (at, scope), counts, view, model, &t);
    ui.add_space(12.0);
    // The plus card ends the page, unless it shows found or lost books.
    let adding = (searched.is_none() && scope != Scope::Missing).then_some(Here { model, at });
    // Empty space: a click lets go of the selection; a right-click offers more.
    let back = ui.available_rect_before_wrap();
    let back = ui.interact(back, Id::new("library-page"), Sense::CLICK);
    if back.clicked() && picks.active() {
        out.push(Command::Library(Action::Pick(PickAction::Clear)));
    }
    back.context_menu(|ui| menus::page_menu(ui, picks.active(), sequence, out));
    let typing = ui.ctx().egui_wants_keyboard_input();
    let all = KeyboardShortcut::new(Modifiers::COMMAND, Key::A);
    if !typing && ui.input_mut(|i| i.consume_shortcut(&all)) {
        out.push(Command::Library(Action::Pick(PickAction::Only(
            sequence.to_vec(),
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
        ui.add_space(12.0);
        if adding.is_none() {
            if hovering {
                let onto = menus::shelf_name(model, at);
                menus::file_hint(&ui, body, onto.as_deref(), &t);
            }
            library.file_target = None;
            return;
        }
    }
    // Unsearched in the grid, cards move by dropping them between others.
    let plain = scope == Scope::All && searched.is_none();
    let arranging = (plain && view.layout == Layout::Grid).then(|| Arranging {
        level: at,
        switch: (view.sort != SortKey::Arranged).then_some(View {
            sort: SortKey::Arranged,
            ascending: true,
            ..*view
        }),
    });
    let mut cx = Cx {
        model,
        covers,
        offline: &offline,
        fit: view.cover,
        at,
        arranging,
        picks: &picks,
        sequence,
        motion,
        filing: None,
        stacking: None,
        files: hovering.then_some(None),
        lit,
        scroll_to: lit
            .filter(|_| scroll)
            .and_then(|l| sequence.iter().position(|p| *p == l.pick)),
        adding,
        t: &t,
        out,
    };
    // Rows are spaced by their own height; egui's gap would add to it.
    ui.spacing_mut().item_spacing.y = 0.0;
    match view.layout {
        Layout::Grid => grid::show(
            &mut ui,
            &items,
            view.columns.unwrap_or(auto_columns(width)),
            &mut cx,
        ),
        Layout::List => {
            cx.motion.forget();
            list(&mut ui, &items, &mut cx);
        }
    }
    // A grid carry shows the card itself; elsewhere a label says what.
    let lifted = view.layout == Layout::Grid && grid::ghost(&mut ui, &mut cx);
    if !lifted {
        carried(&ui, model, &t);
    }
    // While carried, the selection is in hand; the bar steps aside.
    if DragAndDrop::payload::<Carry>(ui.ctx()).is_none() {
        let bar = menus::Bar {
            model,
            picks: &picks,
            at,
            sequence,
        };
        menus::selection_bar(&mut ui, body, &bar, &t, cx.out);
    }
    let target = cx.files.flatten();
    if hovering {
        let onto = menus::shelf_name(model, target.or(at));
        menus::file_hint(&ui, body, onto.as_deref(), &t);
    }
    library.file_target = target;
}

/// A level whose cards drops can move; `switch` shows its own order.
#[derive(Clone, Copy)]
struct Arranging {
    level: Option<ShelfId>,
    switch: Option<View>,
}

/// What every card and row draws with.
struct Cx<'a> {
    model: &'a Library,
    covers: &'a mut Covers,
    /// Folders whose drive wasn't there at the last look.
    offline: &'a BTreeSet<FolderId>,
    fit: CoverFit,
    /// The shelf open on the page; dragged books leave it.
    at: Option<ShelfId>,
    arranging: Option<Arranging>,
    picks: &'a Picks,
    /// Every card in the order shown, for Shift-click ranges.
    sequence: &'a [Pick],
    motion: &'a mut Motion,
    /// While a card is carried in the grid: the shelf it would file onto.
    filing: Option<ShelfId>,
    /// The book a rest made ready to become a shelf with what's carried.
    stacking: Option<BookId>,
    /// While files from outside hover: the shelf under them, if any.
    files: Option<Option<ShelfId>>,
    /// The card a search chose, ringed for a while.
    lit: Option<Lit>,
    /// Where that card sits in `sequence`, the frame it's chosen.
    scroll_to: Option<usize>,
    /// Where the plus card adds, when it shows.
    adding: Option<Here<'a>>,
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

/// One left edge for the title, the chips and the covers or thumbnails.
fn inset(view: &View) -> f32 {
    match view.layout {
        Layout::Grid => 0.0,
        Layout::List => ROW_INSET,
    }
}

fn header(
    ui: &mut Ui,
    (at, scope): (Option<ShelfId>, Scope),
    (books, shelves, of): (usize, usize, Option<usize>),
    view: &View,
    model: &Library,
    t: &Tokens,
) {
    let shelf = at.and_then(|id| model.shelf(id));
    let name = shelf.map_or("Library", |s| s.name.value.as_str());
    // Unshelved, the top level counts only what it shows.
    let unshelved = shelf.is_none() && scope == Scope::All && view.home == Home::Unshelved;
    let mut count = match (of, unshelved) {
        (Some(of), _) => format!("{books} of {}", counted(of, "book", "books")),
        (None, true) => counted(books, "unshelved book", "unshelved books"),
        (None, false) => counted(books, "book", "books"),
    };
    if shelf.is_some() && shelves > 0 {
        count = format!("{count} · {}", counted(shelves, "shelf", "shelves"));
    }
    ui.horizontal(|ui| {
        ui.add_space(inset(view));
        let title = egui::RichText::new(name).font(theme::medium(22.0));
        ui.add(egui::Label::new(title.color(t.ink)).truncate());
        ui.label(egui::RichText::new(count).color(t.muted));
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

/// Room for the cards' rings above, and for the selection bar below.
fn margin(cx: &Cx<'_>) -> egui::Margin {
    let foot = if cx.picks.active() { 76 } else { 8 };
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
    let slots = items.len() + usize::from(cx.adding.is_some());
    area.show_rows(ui, ROW_H, slots, |ui, range| {
        for at in range.clone() {
            match items.get(at) {
                Some(Item::Shelf(shelf)) => shelf_row(ui, shelf, cx),
                Some(Item::Book(book)) => list_row(ui, book, cx),
                Some(Item::Link(link)) => links::row(ui, link, cx),
                None => {
                    let size = vec2(ui.available_width(), ROW_H);
                    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                    if let Some(here) = cx.adding {
                        add::row(ui, rect.shrink2(ROW_GAP), ROW_INSET, here, cx.t, cx.out);
                    }
                }
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
    let held = long_press(ui, &resp, Pick::Book(book.id), cx);
    if resp.clicked() && !held && !picking(ui, Pick::Book(book.id), cx) {
        cx.out.push(Command::OpenBook(book.id));
    }
    if resp.drag_started() {
        lift(&resp, Pick::Book(book.id), cx);
    }
    crate::widgets::context_menu(&resp, |ui| match picked {
        true => menus::picks_menu(ui, cx.picks, cx.t, cx.out),
        false => menus::book_menu(ui, book, cx.t, cx.out),
    });
    resp
}

/// A still press held on a card picks it; true on its release, no click.
fn long_press(ui: &Ui, resp: &egui::Response, pick: Pick, cx: &mut Cx<'_>) -> bool {
    let key = Id::new("library-long-press");
    let (now, start, pressed, released) = ui.input(|i| {
        let p = &i.pointer;
        (
            i.time,
            p.press_start_time(),
            p.any_pressed(),
            p.any_released(),
        )
    });
    // A new press forgets the last one.
    if pressed {
        ui.data_mut(|d| d.remove::<Pick>(key));
    }
    let fired = ui.data(|d| d.get_temp::<Pick>(key)) == Some(pick);
    if released {
        if fired {
            ui.data_mut(|d| d.remove::<Pick>(key));
        }
        return fired && resp.clicked();
    }
    let held = resp.is_pointer_button_down_on() && !resp.dragged();
    let Some(start) = start.filter(|_| held && !fired) else {
        return false;
    };
    let left = LONG_PRESS - (now - start);
    if left > 0.0 {
        ui.ctx().request_repaint_after_secs(left as f32);
        return false;
    }
    ui.data_mut(|d| d.insert_temp(key, pick));
    if !cx.picks.contains(pick) {
        let action = PickAction::Toggle(pick);
        cx.out.push(Command::Library(Action::Pick(action)));
    }
    false
}

/// Picks on a click if asked or already picking; false to open instead.
fn picking(ui: &Ui, pick: Pick, cx: &mut Cx<'_>) -> bool {
    let m = ui.input(|i| i.modifiers);
    let action = match () {
        _ if m.shift && cx.picks.has_anchor() => PickAction::Add(cx.picks.range(cx.sequence, pick)),
        _ if m.command || m.shift || cx.picks.active() => PickAction::Toggle(pick),
        _ => return false,
    };
    cx.out.push(Command::Library(Action::Pick(action)));
    true
}

/// What dragging `pick` carries: the whole selection, if it's in it.
fn carry(pick: Pick, from: Option<ShelfId>, cx: &Cx<'_>) -> Carry {
    let many = cx.picks.len() > 1 && cx.picks.contains(pick);
    // In the order shown, for drops between cards.
    let items = match many {
        true => (cx.sequence.iter())
            .filter(|p| cx.picks.contains(**p))
            .copied()
            .collect(),
        false => vec![pick],
    };
    Carry { items, from }
}

/// Starts carrying `pick`, noting where on its card it was pressed.
fn lift(resp: &egui::Response, pick: Pick, cx: &mut Cx<'_>) {
    resp.dnd_set_drag_payload(carry(pick, cx.at, cx));
    let pressed = resp.interact_pointer_pos().unwrap_or(resp.rect.min);
    let offset = pressed - resp.rect.min;
    cx.motion.grab = Some(Grab { pick, offset });
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
    let held = long_press(ui, &resp, Pick::Shelf(id), cx);
    if resp.clicked() && !held && !picking(ui, Pick::Shelf(id), cx) {
        let go = ShelfAction::GoTo(Some(id));
        cx.out.push(Command::Library(Action::Shelf(go)));
    }
    if resp.drag_started() {
        lift(&resp, Pick::Shelf(id), cx);
    }
    let keep = ui.input(|i| i.modifiers.alt);
    // Carried in the grid, only its middle takes the card.
    let edge = cx.motion.lift.is_some() && cx.filing != Some(id);
    let hovered = resp.dnd_hover_payload::<Carry>().filter(|_| !edge);
    let mut target = hovered.is_some_and(|c| c.onto(Some(id), keep).is_some());
    if !edge && let Some(carry) = resp.dnd_release_payload::<Carry>() {
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
    crate::widgets::context_menu(&resp, |ui| match picked {
        true => menus::picks_menu(ui, cx.picks, cx.t, cx.out),
        false => menus::shelf_menu(ui, cx.model, shelf, cx.t, cx.out),
    });
    (resp, target)
}

/// The focus ring follows a card's padded backing, not its cover.
fn card_ring(resp: &egui::Response, t: &Tokens) {
    let ring = crate::focus::Ring {
        radius: t.radius_lg,
        outside: false,
        grow: 8.0,
    };
    crate::focus::shape(resp, ring);
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
    card_ring(&resp, t);
    let bounds = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width() * A4));
    plate(ui, bounds, shelf.id, cx);
    if cx.picks.active() {
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
    let first = model.arranged(Some(id), Home::Everything).into_iter();
    let mut items = first.filter_map(|p| Item::of(model, p)).take(4).peekable();
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
            Item::Link(link) => links::face(ui, at, link, t),
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
    if !cx.picks.active() {
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

fn grid_card(ui: &mut Ui, rect: Rect, book: &Book, cx: &mut Cx<'_>) {
    let resp = hit(ui, rect, book, cx);
    let t = cx.t;
    // A missing book stays shown, faded, until found or removed.
    let missing = book.missing();
    let offline = folders::offline(book, cx.offline);
    let fade = if missing { 0.5 } else { 1.0 };
    let picked = cx.picks.contains(Pick::Book(book.id));
    let pad = rect.expand2(vec2(8.0, 8.0));
    backing(ui, pad, t.radius_lg, (resp.hovered(), picked, false), t);
    lit_ring(ui, pad, t.radius_lg, Pick::Book(book.id), cx);
    card_ring(&resp, t);
    let bounds = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width() * A4));
    let stack = stacked(ui, bounds, book.id, cx);
    cover(
        ui,
        bounds.shrink(bounds.width() * 0.12 * stack),
        book,
        cx,
        fade,
        false,
    );
    if cx.picks.active() {
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
        // The pill says what; hovering says why and what to do.
        let why = match offline {
            true => "Its drive isn't connected.",
            false => "Moved or deleted. Click to find it.",
        };
        resp.clone().on_hover_text(why);
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
    let detail = card_detail(cx.model, book);
    let small = FontId::proportional(12.0);
    y += line(
        p,
        &detail,
        small,
        faded(t.muted, fade),
        (rect.left(), y + 1.0),
        width,
    );
    let done = cx.model.position(book.id).and_then(|p| p.fraction());
    // An unopened book shows no empty track; its caption says so.
    if let Some(done) = done.filter(|d| *d > 0.0) {
        let track = Rect::from_min_size(pos2(rect.left(), y + 7.0), vec2(width, 3.0));
        p.rect_filled(track, 1.5, faded(t.line, fade));
        let read = Rect::from_min_size(track.min, vec2(width * done, 3.0));
        p.rect_filled(read, 1.5, faded(t.accent, fade));
    }
}

/// A book ready to stack sinks into a plate; returns how far, 0 to 1.
fn stacked(ui: &Ui, bounds: Rect, id: BookId, cx: &Cx<'_>) -> f32 {
    if cx.motion.lift.is_none() {
        return 0.0;
    }
    let on = cx.stacking == Some(id);
    let k = ui
        .ctx()
        .animate_bool_with_time(ui.id().with(("stack", id)), on, 0.12);
    if k > 0.0 {
        let t = cx.t;
        let plate = bounds.expand(bounds.width() * 0.04 * k);
        let radius = t.radius.min(6.0) + 2.0;
        ui.painter()
            .rect_filled(plate, radius, t.hover.gamma_multiply(k));
        let ring = Stroke::new(2.0, t.accent.gamma_multiply(k));
        ui.painter()
            .rect_stroke(plate, radius, ring, StrokeKind::Inside);
    }
    k
}

fn list_row(ui: &mut Ui, book: &Book, cx: &mut Cx<'_>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let resp = hit(ui, rect, book, cx);
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
    if cx.picks.active() {
        menus::mark(ui, thumb.center(), picked, t);
    }
    let p = ui.painter();
    // Books found in a folder may never have been opened.
    let when = match book.opened.value {
        0 => "Not opened".to_owned(),
        at => mareader_library::human_age(at, super::wall_ms()),
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

/// A steady hue per book, softened toward the theme's surface.
fn paper(book: &Book, t: &Tokens) -> Color32 {
    let mixed = book
        .content
        .0
        .wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835);
    let hue = ((mixed >> 96) % 360) as f32 / 360.0;
    let (s, v) = if t.dark { (0.35, 0.36) } else { (0.24, 0.9) };
    let tone = Color32::from(egui::ecolor::Hsva::new(hue, s, v, 1.0));
    tone.lerp_to_gamma(t.surface, 0.3)
}

/// A cover for a book without art: a tinted card with its initial.
fn drawn(p: &Painter, rect: Rect, book: &Book, t: &Tokens, fade: f32, small: bool) {
    let radius = if small { 3.0 } else { t.radius.min(6.0) };
    let paper = paper(book, t);
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
    // The caption under the card names the book, so the cover doesn't.
    let initial = book
        .title
        .value
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_default();
    let ink = faded(t.ink, 0.55 * fade);
    let middle = pos2(spine.right() + (rect.right() - spine.right()) / 2.0, 0.0);
    if small {
        let size = rect.width() * 0.5;
        let at = pos2(middle.x, rect.center().y);
        p.text(at, Align2::CENTER_CENTER, initial, theme::medium(size), ink);
        return;
    }
    let size = (rect.width() * 0.42).clamp(16.0, 96.0);
    let at = pos2(middle.x, rect.top() + rect.height() * 0.42);
    let glyph = p.text(at, Align2::CENTER_CENTER, initial, theme::medium(size), ink);
    let rule_w = (rect.width() * 0.16).max(12.0);
    let rule = Rect::from_center_size(
        pos2(middle.x, glyph.bottom() + size * 0.22),
        vec2(rule_w, 2.0),
    );
    p.rect_filled(rule, 1.0, faded(t.accent, fade));
    let pad = rect.width() * 0.1;
    p.text(
        pos2(middle.x, rect.bottom() - pad),
        Align2::CENTER_BOTTOM,
        book.format.label(),
        theme::medium((rect.width() / 16.0).clamp(9.0, 13.0)),
        faded(t.muted, fade),
    );
}

/// Under a card's title: the author, else how far through.
fn card_detail(model: &Library, book: &Book) -> String {
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

fn empty_state(ui: &mut Ui, rect: Rect, model: &Library, t: &Tokens, out: &mut Commands) {
    let at = Rect::from_center_size(rect.center(), vec2(rect.width(), 200.0));
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(at));
    ui.vertical_centered(|ui| {
        ui.add(Icon::Library.image(40.0).tint(t.muted));
        ui.add_space(10.0);
        let title = egui::RichText::new("Your library is empty").font(theme::medium(16.0));
        ui.label(title.color(t.ink));
        let hint = "Books you open appear here, ready to resume. \
                    Add files or a folder, or drop them on the window.";
        ui.label(egui::RichText::new(hint).size(13.0).color(t.muted));
        ui.add_space(14.0);
        let here = Here { model, at: None };
        add::button(ui, here, t, out);
    });
}

#[cfg(test)]
mod tests {
    use super::{CARD_MIN, GAP, auto_columns};

    #[test]
    fn automatic_columns_keep_cards_wide_within_the_limits() {
        assert_eq!(auto_columns(100.0), 2);
        assert_eq!(auto_columns(4.0 * CARD_MIN + 3.0 * GAP), 4);
        assert_eq!(auto_columns(4.0 * CARD_MIN + 3.0 * GAP - 1.0), 3);
        assert_eq!(auto_columns(10_000.0), 10);
        assert_eq!(auto_columns(f32::NAN), 2);
    }
}
