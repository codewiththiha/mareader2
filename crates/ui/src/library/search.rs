//! Search: the bar's field, its suggestions, and the card a choice lights.

use std::ops::Range;
use std::rc::Rc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Color32, Context, EventFilter, FontId, Id, Key, Modifiers, Order, Rect, Sense, Stroke,
    StrokeKind, TextFormat, Ui, pos2, vec2,
};
use mareader_library::{
    Book, BookId, Home, Item, Library, Query, SUGGEST_LIMIT, ShelfId, SortKey, Suggestion, Target,
    View,
};

use super::{Action, LibraryState, Pick, PickAction, Scope};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::motion;
use crate::theme::{self, Tokens};

/// The field's id: the app asks it whether it holds the keys.
pub const FIELD: &str = "library-search";
pub const LABEL: &str = "Search the library";
/// Set to focus the field once it's next drawn.
const FOCUS_ASK: &str = "library-search-focus";
const PANEL: &str = "library-suggestions";
const ROW_H: f32 = 48.0;
/// The ring holds, then fades out by [`RING_END`].
const RING_HOLD: f64 = 4.0;
const RING_END: f64 = 4.6;
/// Each halo grows [`HALO`] points out over a pulse.
const PULSE: f64 = 1.6;
const HALO: f32 = 16.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchAction {
    /// Focuses the field with its text selected; Ctrl+F.
    Focus,
    Text(String),
    /// Down opens the suggestions or lights the next; Up goes back.
    Move(i32),
    /// Shows the book or shelf where it sits on the page.
    Choose(Target),
    /// Enter: chooses the lit suggestion, else closes them.
    ChooseLit,
    Close,
    Clear,
    /// Leaves a shelf or filter that hides every match.
    Widen,
}

#[derive(Default)]
pub(super) struct Search {
    text: String,
    query: Query,
    /// Suggestions list under the field while it's focused.
    open: bool,
    lit: Option<usize>,
    reveal: Option<Reveal>,
    listing: Option<(Shape, Rc<Listing>)>,
    suggestions: Option<(u64, Query, Rc<[Suggestion]>)>,
}

impl Search {
    fn set_text(&mut self, text: String) {
        self.query = Query::new(&text);
        self.text = text;
        self.open = !self.query.is_empty();
        self.lit = None;
    }

    fn close(&mut self) {
        self.open = false;
        self.lit = None;
    }

    /// Empties the field; true if it held anything.
    pub(super) fn clear(&mut self) -> bool {
        let had = !self.text.is_empty();
        self.set_text(String::new());
        had
    }
}

/// A card shown by choosing it: ringed for a while, scrolled to once.
#[derive(Clone, Copy, Debug)]
struct Reveal {
    pick: Pick,
    since: f64,
    scroll: bool,
}

/// What the page lists, kept until the library or the page changes.
#[derive(Debug, Default)]
pub struct Listing {
    /// Shelves and books in the order shown.
    pub items: Vec<Item>,
    /// Books before the search: the 12 in "3 of 12 books".
    pub total: usize,
    /// Matches hidden by the shelf or filter, when none show.
    pub elsewhere: usize,
}

/// Everything a listing depends on.
#[derive(Clone, Debug, PartialEq)]
struct Shape {
    revision: u64,
    at: Option<ShelfId>,
    scope: Scope,
    sort: SortKey,
    ascending: bool,
    home: Home,
    query: Query,
}

/// What the bar's field shows; the bar edits a copy.
#[derive(Clone, Debug, Default)]
pub struct Field {
    pub text: String,
    pub placeholder: String,
    pub open: bool,
    pub lit: Option<usize>,
}

/// The card being revealed: what it is, and how long ago it was chosen.
#[derive(Clone, Copy, Debug)]
pub struct Lit {
    pub pick: Pick,
    pub age: f64,
}

impl LibraryState {
    /// The shelves and books the page shows, in order.
    pub fn listing(&mut self, view: &View) -> Rc<Listing> {
        let key = Shape {
            revision: self.model.revision(),
            at: self.at,
            scope: self.scope,
            sort: view.sort,
            ascending: view.ascending,
            home: view.home,
            query: self.search.query.clone(),
        };
        if let Some((held, listing)) = &self.search.listing
            && *held == key
        {
            return Rc::clone(listing);
        }
        let listing = Rc::new(self.list(view));
        self.search.listing = Some((key, Rc::clone(&listing)));
        listing
    }

    fn list(&self, view: &View) -> Listing {
        let (model, query) = (&self.model, &self.search.query);
        // Unsearched, the reader's own order mixes shelves among books.
        if self.arranging(view) {
            let items = model.arranged(self.at, view.home);
            let total = items.iter().filter(|i| matches!(i, Item::Book(_))).count();
            return Listing {
                items,
                total,
                elsewhere: 0,
            };
        }
        let pool = self.pool(view);
        let books: Vec<BookId> = (pool.iter())
            .filter(|b| query.keeps(model, b))
            .map(|b| b.id)
            .collect();
        // A folder or Missing is a flat list of what matches.
        let shelves = match (self.scope, self.at) {
            // As with books, a search at the top reaches every shelf.
            (Scope::All, None) if !query.is_empty() => model.every_shelf(),
            (Scope::All, at) => model.children(at),
            _ => Vec::new(),
        };
        let shelves: Vec<Item> = (shelves.into_iter())
            .filter(|s| query.keeps_name(&s.name.value))
            .map(|s| Item::Shelf(s.id))
            .collect();
        let narrowed = self.at.is_some() || self.scope != Scope::All;
        let none = books.is_empty() && shelves.is_empty();
        let elsewhere = match narrowed && none && !query.is_empty() {
            true => model.books().filter(|b| query.keeps(model, b)).count(),
            false => 0,
        };
        Listing {
            items: shelves
                .into_iter()
                .chain(books.into_iter().map(Item::Book))
                .collect(),
            total: pool.len(),
            elsewhere,
        }
    }

    /// Whether the page shows a level in the reader's own order.
    pub fn arranging(&self, view: &View) -> bool {
        let plain = self.scope == Scope::All && self.search.query.is_empty();
        plain && view.sort == SortKey::Arranged
    }

    /// The books the page would list unsearched, sorted.
    fn pool(&self, view: &View) -> Vec<&Book> {
        let model = &self.model;
        if let Some(shelf) = self.at {
            let own = model.shelf_books(shelf);
            return match view.sort {
                SortKey::Arranged => own,
                _ => view.sort(own.into_iter()),
            };
        }
        let scope = self.scope;
        // In a folder, its subfolder's own books; searching, all below it.
        if scope == Scope::Missing {
            return view.sort(model.books().filter(|b| b.missing()));
        }
        // A search at the top looks through every book, shelved or not.
        if view.sort == SortKey::Arranged {
            let top = model.arranged(None, Home::Everything).into_iter();
            let book = |i| match i {
                Item::Book(id) => model.book(id),
                Item::Shelf(_) => None,
            };
            return top.filter_map(book).collect();
        }
        match (view.home, self.search.query.is_empty()) {
            (Home::Unshelved, true) => {
                let shelved = model.shelved();
                view.sort(model.books().filter(|b| !shelved.contains(&b.id)))
            }
            _ => view.sort(model.books()),
        }
    }

    /// The best few books and shelves for what's typed.
    pub fn suggestions(&mut self) -> Rc<[Suggestion]> {
        let revision = self.model.revision();
        if let Some((held, query, found)) = &self.search.suggestions
            && *held == revision
            && *query == self.search.query
        {
            return Rc::clone(found);
        }
        let found: Rc<[Suggestion]> = self.model.suggest(&self.search.query, SUGGEST_LIMIT).into();
        let query = self.search.query.clone();
        self.search.suggestions = Some((revision, query, Rc::clone(&found)));
        found
    }

    /// What's typed in the bar.
    pub fn search_text(&self) -> &str {
        &self.search.text
    }

    pub fn searching(&self) -> bool {
        !self.search.query.is_empty()
    }

    /// What the bar's field shows this frame.
    pub fn field(&self) -> Field {
        let s = &self.search;
        Field {
            text: s.text.clone(),
            placeholder: self.placeholder(),
            open: s.open,
            lit: s.lit,
        }
    }

    /// "Search", or what the search is limited to.
    fn placeholder(&self) -> String {
        let shelf = self.at.and_then(|id| self.model.shelf(id));
        if let Some(shelf) = shelf {
            return format!("Search “{}”", shelf.name.value);
        }
        match self.scope {
            Scope::Missing => "Search missing books".into(),
            Scope::All => "Search".into(),
        }
    }

    /// The card last chosen from the suggestions, while it's ringed.
    pub fn revealed(&self) -> Option<Pick> {
        self.search.reveal.map(|r| r.pick)
    }

    /// The card a choice lights, if still lit; true once to scroll to it.
    pub(super) fn revealing(&mut self, now: f64) -> Option<(Lit, bool)> {
        let reveal = self.search.reveal.as_mut()?;
        let age = now - reveal.since;
        if !(0.0..RING_END).contains(&age) {
            self.search.reveal = None;
            return None;
        }
        let scroll = std::mem::take(&mut reveal.scroll);
        let pick = reveal.pick;
        Some((Lit { pick, age }, scroll))
    }

    pub(super) fn search_act(&mut self, action: SearchAction, now: f64) {
        let s = &mut self.search;
        match action {
            // The app focuses the field; it holds the context.
            SearchAction::Focus => {}
            SearchAction::Text(text) => s.set_text(text),
            SearchAction::Move(by) => self.move_lit(by),
            SearchAction::Choose(target) => self.choose(target, now),
            SearchAction::ChooseLit => {
                let lit = s.lit.filter(|_| s.open);
                let found = self.suggestions();
                match lit.and_then(|i| found.get(i)) {
                    Some(chosen) => self.choose(chosen.target, now),
                    None => self.search.close(),
                }
            }
            SearchAction::Close => s.close(),
            SearchAction::Clear => {
                s.clear();
            }
            SearchAction::Widen => {
                self.at = None;
                self.scope = Scope::All;
                self.renaming = None;
                self.picks.apply(PickAction::Clear);
            }
        }
    }

    /// Down opens the list on its first row; Up off the top closes it.
    fn move_lit(&mut self, by: i32) {
        let rows = self.suggestions().len();
        let s = &mut self.search;
        if s.query.is_empty() || rows == 0 {
            return;
        }
        if !s.open {
            s.open = by > 0;
            s.lit = s.open.then_some(0);
            return;
        }
        let next = match s.lit {
            None if by > 0 => Some(0),
            None => None,
            Some(at) => at.checked_add_signed(by as isize),
        };
        match next {
            Some(at) => s.lit = Some(at.min(rows - 1)),
            None => s.close(),
        }
    }

    /// Lights the target where it shows, going there if it's elsewhere.
    fn choose(&mut self, target: Target, now: f64) {
        let model = &self.model;
        let (pick, there) = match target {
            Target::Book(id) => {
                let Some(book) = model.book(id) else {
                    return;
                };
                // A search at the top lists every match.
                let here = match (self.at, self.scope) {
                    (Some(shelf), _) => model.shelves_of(id).contains(&shelf),
                    (None, Scope::All) => true,
                    (None, Scope::Missing) => book.missing(),
                };
                (Pick::Book(id), (!here).then_some(None))
            }
            Target::Shelf(id) => {
                let Some(shelf) = model.shelf(id) else {
                    return;
                };
                let parent = model.parent_of(shelf);
                // A search at the top lists every matching shelf.
                let top = self.at.is_none() && !self.search.query.is_empty();
                let here = self.scope == Scope::All && (top || self.at == parent);
                (Pick::Shelf(id), (!here).then_some(parent))
            }
        };
        if let Some(at) = there {
            self.at = at;
            self.scope = Scope::All;
            self.renaming = None;
        }
        self.picks.apply(PickAction::Clear);
        self.search.close();
        let scroll = true;
        self.search.reveal = Some(Reveal {
            pick,
            since: now,
            scroll,
        });
    }

    /// Goes where a row stands, then lights it; a book's first shelf.
    pub(super) fn reveal(&mut self, pick: Pick, now: f64) {
        let model = &self.model;
        let at = match pick {
            Pick::Book(id) => {
                let shelves = model.shelves_of(id);
                match self.at.filter(|s| shelves.contains(s)) {
                    Some(here) => Some(here),
                    None => shelves.first().copied(),
                }
            }
            Pick::Shelf(id) => model.shelf(id).and_then(|s| model.parent_of(s)),
        };
        if at != self.at {
            self.renaming = None;
        }
        self.at = at;
        self.scope = Scope::All;
        self.search.clear();
        self.picks.apply(PickAction::Clear);
        let scroll = true;
        self.search.reveal = Some(Reveal {
            pick,
            since: now,
            scroll,
        });
    }
}

/// Focuses the field when it's next drawn, all of it selected.
pub fn focus(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(Id::new(FOCUS_ASK), true));
}

/// Lets go of the keys: a choice was made.
pub fn blur(ctx: &Context) {
    ctx.memory_mut(|m| m.surrender_focus(Id::new(FIELD)));
}

/// Whether the field holds the keys, as of last frame.
pub fn focused(ctx: &Context) -> bool {
    ctx.memory(|m| m.has_focus(Id::new(FIELD)))
}

fn act(out: &mut Commands, action: SearchAction) {
    out.push(Command::Library(Action::Search(action)));
}

/// The bar's pill in `rect`; arrows, Enter and Escape act before typing.
pub fn field(ui: &mut Ui, rect: Rect, field: &Field, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let id = Id::new(FIELD);
    let focused = ui.memory(|m| m.has_focus(id));
    if focused {
        keys(ui, field, out);
    }
    let ring = match focused {
        true => Stroke::new(1.5, t.accent),
        false => Stroke::new(1.0, t.line),
    };
    let round = rect.height() / 2.0;
    let fill = if focused { t.surface } else { t.hover };
    ui.painter()
        .rect(rect, round, fill, ring, StrokeKind::Inside);
    let glass = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(15.0, 15.0));
    Icon::Search.paint(ui, glass, t.muted);
    let clear = !field.text.is_empty();
    let right = rect.right() - if clear { 34.0 } else { 12.0 };
    let room = Rect::from_min_max(
        pos2(glass.right() + 8.0, rect.top()),
        pos2(right, rect.bottom()),
    );
    let mut text = field.text.clone();
    let hint = egui::RichText::new(&field.placeholder).color(t.muted);
    // Escape and the arrows are the field's own, not focus moves.
    let filter = EventFilter {
        horizontal_arrows: true,
        vertical_arrows: true,
        escape: true,
        ..EventFilter::default()
    };
    let edit = egui::TextEdit::singleline(&mut text)
        .id(id)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .font(FontId::proportional(13.0))
        .hint_text(hint)
        .event_filter(filter)
        .vertical_align(egui::Align::Center)
        .min_size(room.size())
        .desired_width(room.width());
    let edited = ui.put(room, edit);
    // Named for screen readers, and for tests to tell it from a name field.
    let name = |b: &mut egui::accesskit::Node| b.set_label(LABEL);
    ui.ctx().accesskit_node_builder(id, name);
    if ui.data_mut(|d| d.remove_temp::<bool>(Id::new(FOCUS_ASK))) == Some(true) {
        super::crumbs::select_all(&edited, id, &text);
    }
    if edited.changed() {
        act(out, SearchAction::Text(text));
    }
    if clear {
        let x =
            Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(22.0, 22.0));
        let r = ui.interact(x, id.with("clear"), Sense::click());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Clear search"));
        if r.hovered() {
            ui.painter().circle_filled(x.center(), 11.0, t.pressed);
        }
        let mark = Rect::from_center_size(x.center(), vec2(13.0, 13.0));
        Icon::Close.paint(ui, mark, t.muted);
        if r.clicked() {
            act(out, SearchAction::Clear);
        }
    }
    let pass = ui.ctx().cumulative_pass_nr();
    ui.data_mut(|d| d.insert_temp(Id::new(PANEL).with("anchor"), (rect, pass)));
}

/// Taken before the field sees them, so they move through suggestions.
fn keys(ui: &mut Ui, field: &Field, out: &mut Commands) {
    let shown = field.open && !field.text.trim().is_empty();
    let mut asked = Vec::new();
    let mut leave = false;
    ui.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            asked.push(SearchAction::Move(1));
        }
        if shown && i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            asked.push(SearchAction::Move(-1));
        }
        if shown && field.lit.is_some() && i.consume_key(Modifiers::NONE, Key::Enter) {
            asked.push(SearchAction::ChooseLit);
        } else if shown && i.key_pressed(Key::Enter) {
            // The field lets go of Enter itself; the filter stays.
            asked.push(SearchAction::Close);
        }
        // Escape peels: the list, then the text, then the focus.
        if i.consume_key(Modifiers::NONE, Key::Escape) {
            match () {
                _ if shown => asked.push(SearchAction::Close),
                _ if !field.text.is_empty() => asked.push(SearchAction::Clear),
                _ => leave = true,
            }
        }
    });
    if leave {
        ui.memory_mut(|m| m.surrender_focus(Id::new(FIELD)));
    }
    for a in asked {
        act(out, a);
    }
}

/// The list under the field, while it's focused or the pointer is on it.
pub fn suggestions(ctx: &Context, library: &mut LibraryState, out: &mut Commands) {
    let rect_id = Id::new(PANEL).with("rect");
    let pass = ctx.cumulative_pass_nr();
    let anchor = ctx.data(|d| d.get_temp::<(Rect, u64)>(Id::new(PANEL).with("anchor")));
    let anchor = anchor.filter(|(_, at)| *at == pass).map(|(rect, _)| rect);
    let last = ctx.data(|d| d.get_temp::<Rect>(rect_id));
    let pointer = ctx.pointer_hover_pos();
    let hovered = last.zip(pointer).is_some_and(|(r, p)| r.contains(p));
    let showing = library.search.open && library.searching() && (focused(ctx) || hovered);
    let Some(anchor) = anchor.filter(|_| showing) else {
        ctx.data_mut(|d| d.remove::<Rect>(rect_id));
        return;
    };
    let found = library.suggestions();
    let lit = library.search.lit;
    let t = Tokens::get(ctx);
    let width = anchor.width().max(340.0);
    let left = (anchor.center().x - width / 2.0).max(ctx.content_rect().left() + 8.0);
    let area = egui::Area::new(Id::new(PANEL))
        .order(Order::Foreground)
        .fixed_pos(pos2(left, anchor.bottom() + 6.0))
        .constrain(true);
    let shown = area.show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(width - 12.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            if found.is_empty() {
                let none = egui::RichText::new("No books or shelves match.").color(t.muted);
                ui.add_space(8.0);
                ui.label(none);
                ui.add_space(8.0);
                return;
            }
            let (model, covers) = library.parts();
            for (n, suggestion) in found.iter().enumerate() {
                let row = Row {
                    model,
                    suggestion,
                    lit: lit == Some(n),
                    t: &t,
                };
                if row.show(ui, covers).clicked() {
                    act(out, SearchAction::Choose(suggestion.target));
                }
            }
            ui.add_space(4.0);
            let hint = "↑↓ to move · Enter to show · Esc to close";
            let hint = egui::RichText::new(hint).size(11.0).color(t.muted);
            ui.add(egui::Label::new(hint).selectable(false));
        });
    });
    ctx.move_to_top(shown.response.layer_id);
    ctx.data_mut(|d| d.insert_temp(rect_id, shown.response.rect));
}

/// One suggestion: a thumb or the shelf mark, its name and where it is.
struct Row<'a> {
    model: &'a Library,
    suggestion: &'a Suggestion,
    lit: bool,
    t: &'a Tokens,
}

impl Row<'_> {
    fn show(&self, ui: &mut Ui, covers: &mut super::Covers) -> egui::Response {
        let t = self.t;
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        let hit = &self.suggestion.hit;
        let (name, sub, sub_lit) = self.texts();
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, self.lit, &name)
        });
        if self.lit || resp.hovered() {
            let fill = if self.lit { t.accent_soft } else { t.hover };
            ui.painter().rect_filled(rect, t.radius, fill);
        }
        let thumb =
            Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(26.0, 36.0));
        match self.suggestion.target {
            Target::Book(id) => {
                if let Some(book) = self.model.book(id) {
                    super::view::thumb(ui, thumb, book, covers, t);
                }
            }
            Target::Shelf(_) => {
                let mark = Rect::from_center_size(thumb.center(), vec2(20.0, 20.0));
                Icon::Shelf.paint(ui, mark, t.muted);
            }
        }
        let p = ui.painter();
        let x = thumb.right() + 12.0;
        let wide = (rect.right() - 10.0 - x).max(40.0);
        let title = marked(&name, &hit.title, theme::medium(13.0), t.ink, t, wide);
        let g = p.layout_job(title);
        p.galley(pos2(x, rect.center().y - g.size().y - 1.0), g, t.ink);
        let small = FontId::proportional(12.0);
        let g = p.layout_job(marked(&sub, sub_lit, small, t.muted, t, wide));
        p.galley(pos2(x, rect.center().y + 2.0), g, t.muted);
        resp
    }

    /// The name, the line under it, and what's lit in that line.
    fn texts(&self) -> (String, String, &[Range<usize>]) {
        let hit = &self.suggestion.hit;
        let model = self.model;
        match self.suggestion.target {
            Target::Book(id) => {
                let Some(book) = model.book(id) else {
                    return Default::default();
                };
                let author = book.author.value.as_deref().map(str::trim);
                let author = author.filter(|a| !a.is_empty());
                // The address, when that's what the search found.
                let (sub, lit) = match author {
                    Some(a) if hit.address.is_empty() || !hit.author.is_empty() => {
                        (a.to_owned(), hit.author.as_slice())
                    }
                    _ => (model.address(book), hit.address.as_slice()),
                };
                (book.title.value.clone(), sub, lit)
            }
            Target::Shelf(id) => {
                let path = model.path_to(id);
                let name = path.last().map(|s| s.name.value.clone());
                let above: Vec<&str> = (path.iter().rev().skip(1).rev())
                    .map(|s| s.name.value.as_str())
                    .collect();
                let sub = match above.is_empty() {
                    true => "Shelf".to_owned(),
                    false => format!("Shelf in {}", above.join(" › ")),
                };
                (name.unwrap_or_default(), sub, &[])
            }
        }
    }
}

/// `text` truncated to `wide`, its `lit` byte ranges picked out.
fn marked(
    text: &str,
    lit: &[Range<usize>],
    font: FontId,
    color: Color32,
    t: &Tokens,
    wide: f32,
) -> LayoutJob {
    let plain = TextFormat::simple(font.clone(), color);
    let strong = TextFormat {
        color: t.ink,
        background: t.accent.gamma_multiply(if t.dark { 0.4 } else { 0.22 }),
        ..TextFormat::simple(font, t.ink)
    };
    let mut job = LayoutJob::default();
    let mut from = 0;
    for span in lit {
        let (Some(before), Some(hit)) = (text.get(from..span.start), text.get(span.clone())) else {
            continue;
        };
        job.append(before, 0.0, plain.clone());
        job.append(hit, 0.0, strong.clone());
        from = span.end;
    }
    job.append(text.get(from..).unwrap_or_default(), 0.0, plain);
    job.wrap = TextWrapping::truncate_at_width(wide);
    job
}

/// The ring around a revealed card: a steady line and a fading halo.
pub fn ring(ui: &Ui, rect: Rect, radius: f32, age: f64, t: &Tokens) {
    let painter = ui.painter();
    let fade = match age {
        a if a < RING_HOLD => 1.0,
        a => (1.0 - (a - RING_HOLD) / (RING_END - RING_HOLD)).clamp(0.0, 1.0) as f32,
    };
    let line = Stroke::new(2.0, t.accent.gamma_multiply(fade));
    painter.rect_stroke(rect, radius, line, StrokeKind::Outside);
    let pulsing = motion::enabled(ui.ctx()) && age < 2.0 * PULSE;
    if pulsing {
        let swell = ((age % PULSE) / PULSE) as f32;
        let halo = t.accent.gamma_multiply(0.5 * (1.0 - swell) * fade);
        let out = rect.expand(2.0 + swell * HALO);
        let width = 4.0 * (1.0 - swell) + 1.0;
        painter.rect_stroke(
            out,
            radius + swell * HALO,
            Stroke::new(width, halo),
            StrokeKind::Middle,
        );
    }
    // Held still, it sleeps until the fade.
    match pulsing || age >= RING_HOLD {
        true => ui.ctx().request_repaint(),
        false => ui
            .ctx()
            .request_repaint_after_secs((RING_HOLD - age) as f32),
    }
}
