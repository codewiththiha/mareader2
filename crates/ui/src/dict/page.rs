//! The Dictionary page: a search, its results and one entry in full.

use egui::{
    Align, Button, FontId, Frame, Key, Layout, Margin, Modifiers, Rect, Response, RichText,
    ScrollArea, Sense, Shape, Stroke, TextEdit, Ui, UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_dict::{
    Dictionaries, Entry, HUB, PACKS, Pack, Pair, Query, Reply, Ticket, WordMatch, fold, language,
    languages,
};
use mareader_settings::DictionarySettings;

use super::entry;
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::settings_window::Tab;
use crate::shaped;
use crate::theme::{self, Tokens};
use crate::toast::Toast;
use crate::widgets;

/// The search field's name, for screen readers and tests.
pub const LABEL: &str = "Search the dictionaries";

/// Typing settles this long before the packs are asked.
const DEBOUNCE: f64 = 0.18;
/// From this width the results and the entry sit side by side.
const WIDE: f32 = 640.0;
const LIST_W: f32 = 300.0;
/// The page's column, centred on wide windows.
const MAX_W: f32 = 1080.0;
const MARGIN: f32 = 24.0;
/// A short answer can be looked up in turn.
const SHORT: usize = 32;

#[derive(Default)]
pub struct Page {
    text: String,
    typed_at: f64,
    /// The ask sent last, to know what its reply answers.
    asked: Option<(Ticket, Query)>,
    reply: Option<Reply>,
    /// The result shown in full.
    chosen: usize,
    /// Arriving or cleared: the field takes the keys.
    focus: bool,
    /// The chosen row moved by key, so it scrolls into view.
    reveal: bool,
    /// Drawn in the floating window: a smaller field.
    small: bool,
    /// Whether the field had the keys when last drawn.
    typing: bool,
}

/// What a click on the page asks of it.
enum Act {
    Choose(usize),
    Search(String),
}

impl Page {
    pub fn receive(&mut self, reply: Reply) {
        if self.asked.as_ref().is_some_and(|(t, _)| *t == reply.ticket) {
            self.reply = Some(reply);
            self.chosen = 0;
        }
    }

    /// The packs changed: ask again.
    pub fn stale(&mut self) {
        self.asked = None;
    }

    /// The page came up: the field takes the keys.
    pub fn arrive(&mut self) {
        self.focus = true;
    }

    /// Escape clears a search; false when there was none.
    pub fn escape(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }
        self.text.clear();
        self.focus = true;
        true
    }

    /// Searches `word` at once, as a recent or a reverse lookup does.
    pub fn search(&mut self, word: &str) {
        self.text = word.to_owned();
        self.typed_at = f64::NEG_INFINITY;
        self.focus = true;
    }

    /// Searches `word` at once, but the reader keeps the keys.
    pub fn feed(&mut self, word: &str) {
        self.text = word.to_owned();
        self.typed_at = f64::NEG_INFINITY;
    }

    /// The top answer to the search, for the folded window's glance.
    pub fn glance(&self) -> Option<&str> {
        let asked = self.text.trim();
        let fresh = self.asked.as_ref().is_some_and(|(_, q)| match q {
            Query::Search { text, .. } => text == asked,
            _ => false,
        });
        let e = self.reply.as_ref()?.entries.first()?;
        (fresh && !asked.is_empty()).then(|| e.answer())
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the field had the keys last frame; Escape drops them first.
    pub fn typing(&self) -> bool {
        self.typing
    }

    /// Not drawn this frame, so not typed into.
    pub fn hide(&mut self) {
        self.typing = false;
    }

    /// The page in whatever room `ui` has, as the floating window shows it.
    pub fn compact(
        &mut self,
        ui: &mut Ui,
        service: &mut Dictionaries,
        s: &DictionarySettings,
        out: &mut Commands,
    ) {
        let t = Tokens::get(ui.ctx());
        self.small = true;
        self.contents(ui, service, s, &t, out);
    }

    /// Asks again once typing settles and the ask has changed.
    pub fn ask(&mut self, ctx: &egui::Context, service: &mut Dictionaries, s: &DictionarySettings) {
        let now = ctx.input(|i| i.time);
        let query = self.query(s);
        let settled = now - self.typed_at >= DEBOUNCE;
        let changed = self.asked.as_ref().is_none_or(|(_, q)| *q != query);
        if changed && !settled {
            let wait = (DEBOUNCE - (now - self.typed_at)) as f32;
            ctx.request_repaint_after_secs(wait);
        } else if changed {
            let ticket = service.ask(query.clone());
            self.asked = Some((ticket, query));
        }
    }

    /// The ask the page's text and choices make now.
    fn query(&self, s: &DictionarySettings) -> Query {
        let pair = s.pair.then(|| Pair {
            from: s.from.clone(),
            to: s.to.clone(),
        });
        Query::Search {
            text: self.text.trim().to_owned(),
            pair,
            pack: s.langs.first().cloned().filter(|_| !s.pair),
        }
    }

    /// The whole page below `top`, its column centred.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        service: &mut Dictionaries,
        s: &DictionarySettings,
        top: f32,
        out: &mut Commands,
    ) {
        let t = Tokens::get(ui.ctx());
        self.small = false;
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, t.paper);
        let width = (full.width() - 2.0 * MARGIN).clamp(0.0, MAX_W);
        let left = full.center().x - width / 2.0;
        let bottom = (full.bottom() - MARGIN).max(top + 12.0);
        let col = Rect::from_min_max(pos2(left, top + 12.0), pos2(left + width, bottom));
        let builder = UiBuilder::new()
            .max_rect(col)
            .layout(Layout::top_down(Align::Min));
        ui.scope_builder(builder, |ui| self.contents(ui, service, s, &t, out));
    }

    /// Field, choices and results, in whatever room `ui` has.
    fn contents(
        &mut self,
        ui: &mut Ui,
        service: &mut Dictionaries,
        s: &DictionarySettings,
        t: &Tokens,
        out: &mut Commands,
    ) {
        ui.spacing_mut().item_spacing.y = 10.0;
        if !service.attached() {
            let line = "Dictionaries need the desktop app.";
            empty(ui, t, "No dictionaries here", line, None);
            return;
        }
        let ready = service.ready();
        if ready.is_empty() {
            let line = "No dictionary is downloaded.";
            if empty(
                ui,
                t,
                "No dictionaries yet",
                line,
                Some("Get dictionaries…"),
            ) {
                out.push(Command::OpenSettings(Tab::Dictionary));
            }
            return;
        }
        let mut draft = s.clone();
        let now = ui.input(|i| i.time);
        self.field(ui, t, now, &mut draft);
        let reply = self.reply.as_ref();
        match self.small {
            true => narrow_choices(ui, &mut draft, &ready, reply, t, out),
            false => choices(ui, &mut draft, &ready, reply, t, out),
        }
        self.ask(ui.ctx(), service, &draft);
        let asked = self.text.trim().to_owned();
        let act = if asked.is_empty() {
            home(ui, t, &ready, &mut draft)
        } else {
            match self.reply.take() {
                Some(reply) => {
                    let act = self.results(ui, t, &reply, &asked, &mut draft, &ready, out);
                    self.reply = Some(reply);
                    act
                }
                None => {
                    ui.label(RichText::new("Searching…").color(t.muted));
                    None
                }
            }
        };
        match act {
            Some(Act::Choose(i)) => {
                self.chosen = i;
                draft.remember(&asked);
            }
            Some(Act::Search(word)) => self.search(&word),
            None => {}
        }
        if draft != *s {
            out.push(Command::SetDictionary(Box::new(draft)));
        }
    }

    /// The big search field; arrows pick a result, Enter keeps the search.
    fn field(&mut self, ui: &mut Ui, t: &Tokens, now: f64, s: &mut DictionarySettings) {
        let id = ui.make_persistent_id("dict-search");
        let had = ui.memory(|m| m.has_focus(id));
        let count = self.reply.as_ref().map_or(0, |r| r.entries.len());
        if had && count > 0 {
            let (down, up) = ui.input_mut(|i| {
                let down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
                (down, i.consume_key(Modifiers::NONE, Key::ArrowUp))
            });
            if down || up {
                let at = self.chosen.min(count - 1);
                self.chosen = if down {
                    (at + 1).min(count - 1)
                } else {
                    at.saturating_sub(1)
                };
                self.reveal = true;
            }
        }
        let frame = Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(1.0, t.line))
            .corner_radius(t.radius_lg)
            .inner_margin(if self.small {
                Margin::symmetric(10, 5)
            } else {
                Margin::symmetric(12, 8)
            });
        let size = if self.small { 16.0 } else { 20.0 };
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(Icon::Search.image(size - 2.0).tint(t.muted));
                let room = if self.text.is_empty() { 0.0 } else { 40.0 };
                let edit = TextEdit::singleline(&mut self.text)
                    .id(id)
                    .hint_text("Search any word…")
                    .font(FontId::proportional(size))
                    .frame(Frame::NONE)
                    .return_key(None)
                    .desired_width((ui.available_width() - room).max(40.0));
                let field = ui.add(edit);
                let name = |b: &mut egui::accesskit::Node| b.set_label(LABEL);
                ui.ctx().accesskit_node_builder(field.id, name);
                if field.changed() {
                    self.typed_at = now;
                }
                if field.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    s.remember(&self.text);
                }
                if !self.text.is_empty()
                    && widgets::small_icon_button(ui, Icon::Close, "Clear search").clicked()
                {
                    self.text.clear();
                    self.focus = true;
                }
                if std::mem::take(&mut self.focus) {
                    field.request_focus();
                }
                self.typing = field.has_focus();
            });
        });
    }

    /// The reply: a list and the chosen entry, or one column when narrow.
    #[allow(clippy::too_many_arguments)]
    fn results(
        &mut self,
        ui: &mut Ui,
        t: &Tokens,
        reply: &Reply,
        asked: &str,
        draft: &mut DictionarySettings,
        ready: &[&'static Pack],
        out: &mut Commands,
    ) -> Option<Act> {
        if reply.entries.is_empty() {
            nothing(ui, t, asked, draft, ready, reply);
            return None;
        }
        let chosen = self.chosen.min(reply.entries.len() - 1);
        let reveal = std::mem::take(&mut self.reveal);
        let packs = !draft.pair && draft.langs.is_empty() && ready.len() > 1;
        let rest = ui.available_rect_before_wrap();
        if rest.width() < WIDE {
            // Compact: the entries as the card lists them.
            ScrollArea::vertical()
                .id_salt("dict-column")
                .auto_shrink(false)
                .show(ui, |ui| {
                    clear_of_bar(ui);
                    entry::entries(ui, &reply.entries, asked, t, |ui, e| {
                        if packs {
                            pack_note(ui, e, t);
                        }
                    });
                });
            return None;
        }
        let gap = 20.0;
        let list = Rect::from_min_size(rest.min, vec2(LIST_W, rest.height()));
        let detail = Rect::from_min_max(pos2(list.right() + gap, rest.top()), rest.max);
        let x = list.right() + gap / 2.0;
        let line = Stroke::new(1.0, t.line);
        ui.painter().vline(x, rest.y_range(), line);
        let mut act = None;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            act = results_list(ui, t, &reply.entries, asked, chosen, reveal);
        });
        ui.scope_builder(UiBuilder::new().max_rect(detail), |ui| {
            let more = full_entry(ui, t, &reply.entries, chosen, asked, packs, out);
            act = act.take().or(more);
        });
        ui.allocate_rect(rest, Sense::hover());
        act
    }
}

/// Mode, packs or languages, how the reply was reached, and settings.
fn choices(
    ui: &mut Ui,
    s: &mut DictionarySettings,
    ready: &[&'static Pack],
    reply: Option<&Reply>,
    t: &Tokens,
    out: &mut Commands,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.selectable_value(&mut s.pair, false, "Dictionaries");
        ui.selectable_value(&mut s.pair, true, "Languages");
        ui.add_space(6.0);
        if s.pair {
            languages_row(ui, s, ready, 150.0);
        } else {
            pack_box(ui, s, ready, 220.0);
        }
        if let Some((text, why)) = reply.and_then(|r| route_note(r, s)) {
            chip(ui, &text, t).on_hover_text(why);
        }
        manage(ui, out);
    });
}

/// The window's choices: the mode and settings, then the picker across.
fn narrow_choices(
    ui: &mut Ui,
    s: &mut DictionarySettings,
    ready: &[&'static Pack],
    reply: Option<&Reply>,
    t: &Tokens,
    out: &mut Commands,
) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut s.pair, false, "Dictionaries");
        ui.selectable_value(&mut s.pair, true, "Languages");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| manage(ui, out));
    });
    let width = ui.available_width();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        match s.pair {
            true => languages_row(ui, s, ready, ((width - 36.0) / 2.0).max(60.0)),
            false => pack_box(ui, s, ready, width),
        }
    });
    if let Some((text, why)) = reply.and_then(|r| route_note(r, s)) {
        chip(ui, &text, t).on_hover_text(why);
    }
}

fn manage(ui: &mut Ui, out: &mut Commands) {
    let manage = widgets::small_icon_button(ui, Icon::Settings, "Manage dictionaries…");
    if manage.clicked() {
        out.push(Command::OpenSettings(Tab::Dictionary));
    }
}

fn pack_box(ui: &mut Ui, s: &mut DictionarySettings, ready: &[&'static Pack], width: f32) {
    let chosen = s
        .langs
        .first()
        .and_then(|id| ready.iter().find(|p| p.id == id));
    let shown = chosen.map_or("Every dictionary".to_owned(), |p| p.label());
    let pick = widgets::combo("dict-pack")
        .width(width)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(chosen.is_none(), "Every dictionary")
                .clicked()
            {
                s.langs.clear();
            }
            for p in ready {
                let on = chosen.is_some_and(|c| c.id == p.id);
                if ui.selectable_label(on, p.label()).clicked() {
                    s.langs = vec![p.id.to_owned()];
                }
            }
        });
    widgets::name(&pick.response, "Dictionary to search");
}

fn languages_row(ui: &mut Ui, s: &mut DictionarySettings, ready: &[&'static Pack], width: f32) {
    let langs = languages(ready);
    let names = ("dict-from", "Translate from");
    language_box(ui, names, width, &mut s.from, &langs);
    let swap = widgets::small_icon_button(ui, Icon::Swap, "Swap languages");
    if swap.clicked() {
        std::mem::swap(&mut s.from, &mut s.to);
    }
    language_box(ui, ("dict-to", "Translate to"), width, &mut s.to, &langs);
}

/// A language picker; `id` is its salt and its spoken name.
fn language_box(
    ui: &mut Ui,
    id: (&str, &str),
    width: f32,
    value: &mut Option<String>,
    langs: &[&str],
) {
    let shown = value.as_deref().map_or("Automatic".to_owned(), language);
    let pick = widgets::combo(id.0)
        .width(width)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            if ui.selectable_label(value.is_none(), "Automatic").clicked() {
                *value = None;
            }
            for l in langs {
                let on = value.as_deref() == Some(*l);
                if ui.selectable_label(on, language(l)).clicked() {
                    *value = Some((*l).to_owned());
                }
            }
        });
    widgets::name(&pick.response, id.1);
}

/// "via English" when a bridge answered, or the language detected.
fn route_note(reply: &Reply, s: &DictionarySettings) -> Option<(String, String)> {
    // The rows name each English word between; this names the language.
    if reply.entries.iter().any(|e| e.via.is_some()) {
        let via = language(HUB);
        let why = format!("No dictionary joins these two; answers pass through {via}.");
        return Some((format!("via {via}"), why));
    }
    let from = reply
        .from
        .as_deref()
        .filter(|_| s.pair && s.from.is_none())?;
    let why = "Automatic chose this language for the word.";
    Some((format!("Detected {}", language(from)), why.to_owned()))
}

/// The rows down the left: headword, its tags, the answer cut short.
fn results_list(
    ui: &mut Ui,
    t: &Tokens,
    list: &[Entry],
    asked: &str,
    chosen: usize,
    reveal: bool,
) -> Option<Act> {
    let mut act = None;
    ScrollArea::vertical()
        .id_salt("dict-results")
        .auto_shrink(false)
        .show(ui, |ui| {
            clear_of_bar(ui);
            ui.spacing_mut().item_spacing.y = 2.0;
            let mut shown: Option<&str> = None;
            for (i, e) in list.iter().enumerate() {
                let near = entry::near(e, asked);
                if near.is_some()
                    && near != shown
                    && let Some(word) = near
                {
                    ui.add_space(4.0);
                    entry::heading(ui, word, t);
                }
                shown = near;
                let r = row(ui, e, i, i == chosen, t);
                if r.clicked() {
                    act = Some(Act::Choose(i));
                }
                if reveal && i == chosen {
                    r.scroll_to_me(None);
                }
            }
        });
    act
}

/// One result: a lit background when chosen, the whole row a button.
fn row(ui: &mut Ui, e: &Entry, i: usize, on: bool, t: &Tokens) -> Response {
    let bg = ui.painter().add(Shape::Noop);
    let inner = Frame::new()
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            // The headword leads; its part of speech sits at the end.
            ui.horizontal(|ui| {
                let pos = e.pos.as_deref().filter(|p| !p.is_empty());
                let tag = pos.map(|p| RichText::new(p).italics().size(t.text_small));
                let room = ui.available_width() - if tag.is_some() { 48.0 } else { 0.0 };
                let lead = Layout::left_to_right(Align::Center);
                ui.allocate_ui_with_layout(vec2(room.max(0.0), 0.0), lead, |ui| {
                    shaped::line(ui, RichText::new(e.asked()).strong(), t.text, t.ink);
                });
                if let Some(tag) = tag {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(tag.color(t.muted));
                    });
                }
            });
            let size = entry::answer_size(e.answer(), t) * 0.9;
            shaped::line(ui, e.answer(), size, t.muted);
        });
    let rect = inner.response.rect;
    let resp = ui.interact(rect, ui.id().with(("dict-row", i)), Sense::click());
    let fill = if on {
        t.selected
    } else if resp.hovered() {
        t.hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter()
        .set(bg, Shape::rect_filled(rect, t.radius, fill));
    let label = format!("{} — {}", e.asked(), e.answer());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, on, &label));
    resp
}

/// The chosen entry large, its actions, then the word's other senses.
fn full_entry(
    ui: &mut Ui,
    t: &Tokens,
    list: &[Entry],
    chosen: usize,
    asked: &str,
    packs: bool,
    out: &mut Commands,
) -> Option<Act> {
    let e = list.get(chosen)?;
    let mut act = None;
    ScrollArea::vertical()
        .id_salt("dict-entry")
        .auto_shrink(false)
        .show(ui, |ui| {
            clear_of_bar(ui);
            ui.spacing_mut().item_spacing.y = 6.0;
            if shaped::needs_shaping(e.asked()) {
                shaped::label(ui, e.asked(), 26.0, t.ink);
            } else {
                let head = RichText::new(e.asked()).font(theme::medium(28.0));
                ui.add(egui::Label::new(head.color(t.ink)).wrap());
            }
            if e.fit != WordMatch::Exact {
                let note = format!("The nearest entry to “{asked}”");
                ui.label(RichText::new(note).size(t.text_small).color(t.muted));
            }
            ui.horizontal_wrapped(|ui| {
                let tags = e.tags();
                if !tags.is_empty() {
                    let tags = RichText::new(tags).italics().size(t.text_small);
                    ui.label(tags.color(t.muted));
                }
                if let Some(p) = PACKS.iter().find(|p| p.id == e.pack) {
                    chip(ui, &p.label(), t);
                }
                if let Some(via) = &e.via {
                    chip(ui, &format!("via {via}"), t);
                }
            });
            ui.add_space(6.0);
            shaped::label(
                ui,
                e.answer(),
                entry::answer_size(e.answer(), t) * 1.5,
                t.ink,
            );
            if let Some(r) = e.romanization.as_deref().filter(|r| !r.is_empty()) {
                shaped::label(ui, r, t.text, t.muted);
            }
            if let Some(s) = e.sense.as_deref().filter(|s| !s.is_empty()) {
                if shaped::needs_shaping(s) {
                    shaped::label(ui, s, t.text, t.muted);
                } else {
                    ui.label(RichText::new(s).italics().color(t.muted));
                }
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let copy = Button::image_and_text(Icon::Copy.image(14.0), "Copy")
                    .image_tint_follows_text_color(true);
                if ui.add(copy).clicked() {
                    ui.ctx().copy_text(e.answer().to_owned());
                    out.push(Command::Notify(Toast::info("Copied")));
                }
                act = look_again(ui, e, t);
            });
            let word = fold(e.asked());
            let others: Vec<(usize, &Entry)> = list
                .iter()
                .enumerate()
                .filter(|(i, o)| *i != chosen && fold(o.asked()) == word)
                .collect();
            if others.is_empty() {
                return;
            }
            ui.add_space(14.0);
            let title = format!("More for “{}”", e.asked());
            ui.label(
                RichText::new(title)
                    .font(theme::medium(t.text_small))
                    .color(t.muted),
            );
            for (i, o) in others {
                if sense_card(ui, o, i, packs, t).clicked() {
                    act = Some(Act::Choose(i));
                }
            }
        });
    act
}

/// Another sense of the word, compact; a click shows it in full.
fn sense_card(ui: &mut Ui, e: &Entry, i: usize, packs: bool, t: &Tokens) -> Response {
    let bg = ui.painter().add(Shape::Noop);
    let inner = Frame::new()
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            entry::rows(ui, e, t, true);
            if packs {
                pack_note(ui, e, t);
            }
        });
    let rect = inner.response.rect;
    let resp = ui.interact(rect, ui.id().with(("dict-sense", i)), Sense::click());
    let fill = if resp.hovered() { t.hover } else { t.surface };
    ui.painter()
        .set(bg, Shape::rect_filled(rect, t.radius, fill));
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, e.answer()));
    resp
}

/// "Look up ⟨answer⟩", offered when the answer is a word or two.
fn look_again(ui: &mut Ui, e: &Entry, t: &Tokens) -> Option<Act> {
    let answer = e.answer().trim();
    let short = answer.chars().count() <= SHORT && answer.split_whitespace().count() <= 3;
    if !short || answer.is_empty() {
        return None;
    }
    let text = RichText::new(format!("Look up “{answer}”")).size(t.text_small);
    let button = Button::image_and_text(Icon::Search.image(14.0), text)
        .image_tint_follows_text_color(true)
        .frame_when_inactive(false);
    ui.add(button)
        .clicked()
        .then(|| Act::Search(answer.to_owned()))
}

/// Which pack answered, when several were asked.
fn pack_note(ui: &mut Ui, e: &Entry, t: &Tokens) {
    if let Some(p) = PACKS.iter().find(|p| p.id == e.pack) {
        ui.label(RichText::new(p.label()).size(t.text_small).color(t.muted));
    }
}

/// Keeps scrolled content off the floating bar, which paints over it.
pub fn clear_of_bar(ui: &mut Ui) {
    let bar = ui.spacing().scroll;
    let room = bar.bar_width + bar.bar_outer_margin + 2.0;
    ui.set_max_width((ui.available_width() - room).max(0.0));
}

/// A small rounded tag.
fn chip(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    let text = RichText::new(text).size(t.text_small).color(t.ink);
    Frame::new()
        .fill(t.accent_soft)
        .corner_radius(t.radius_lg)
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| ui.label(text))
        .response
}

/// Nothing typed: what is searched, how, and the recent searches.
fn home(
    ui: &mut Ui,
    t: &Tokens,
    ready: &[&'static Pack],
    s: &mut DictionarySettings,
) -> Option<Act> {
    let mut act = None;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.12).max(12.0));
        ui.add(Icon::Dictionary.image(36.0).tint(t.accent));
        let title = RichText::new("Look up any word").font(theme::medium(20.0));
        ui.label(title.color(t.ink));
        ui.label(RichText::new(scope(ready, s)).color(t.muted));
        let keys = "↑ ↓ pick a result · Enter keeps the search · Esc clears";
        ui.label(RichText::new(keys).size(t.text_small).color(t.muted));
        if s.recent.is_empty() {
            return;
        }
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.add(Icon::Clock.image(14.0).tint(t.muted));
            ui.label(RichText::new("Recent").size(t.text_small).color(t.muted));
            if ui.small_button("Clear").clicked() {
                s.recent.clear();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for word in &s.recent {
                let chip = Button::new(RichText::new(word).size(t.text_small))
                    .corner_radius(t.radius_lg)
                    .fill(t.surface);
                if ui.add(chip).clicked() {
                    act = Some(Act::Search(word.clone()));
                }
            }
        });
    });
    act
}

/// "Searching English → Burmese and English → French", or the pair.
fn scope(ready: &[&'static Pack], s: &DictionarySettings) -> String {
    if s.pair {
        let from = s
            .from
            .as_deref()
            .map_or("any language".to_owned(), language);
        let to = s.to.as_deref().map_or("your language".to_owned(), language);
        return format!("Translating from {from} to {to}");
    }
    let chosen = s
        .langs
        .first()
        .and_then(|id| ready.iter().find(|p| p.id == id));
    let labels: Vec<String> = match chosen {
        Some(p) => vec![p.label()],
        None => ready.iter().map(|p| p.label()).collect(),
    };
    format!("Searching {}", labels.join(", "))
}

/// Nothing found: other source languages, or every dictionary.
fn nothing(
    ui: &mut Ui,
    t: &Tokens,
    asked: &str,
    draft: &mut DictionarySettings,
    ready: &[&'static Pack],
    reply: &Reply,
) {
    let line = format!("Nothing found for “{asked}”.");
    ui.label(RichText::new(line).color(t.muted));
    if !draft.pair && !draft.langs.is_empty() && ui.link("Search every dictionary").clicked() {
        draft.langs.clear();
    }
    if !draft.pair {
        return;
    }
    let from = reply.from.as_deref().unwrap_or(HUB);
    let others: Vec<&str> = languages(ready)
        .into_iter()
        .filter(|l| *l != from)
        .collect();
    ui.with_layout(Layout::top_down(Align::Min), |ui| {
        for l in others {
            if ui.link(format!("Search as {}", language(l))).clicked() {
                draft.from = Some(l.to_owned());
                if draft.to.as_deref() == Some(l) {
                    draft.to = None;
                }
            }
        }
    });
}

/// An empty page: icon, title, a line of why, maybe an action.
fn empty(ui: &mut Ui, t: &Tokens, title: &str, line: &str, action: Option<&str>) -> bool {
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.3).max(24.0));
        ui.add(Icon::Dictionary.image(28.0).tint(t.muted));
        ui.add_space(4.0);
        let title = RichText::new(title).font(theme::medium(16.0));
        ui.label(title.color(t.ink));
        ui.label(RichText::new(line).color(t.muted));
        if let Some(action) = action {
            ui.add_space(6.0);
            clicked = ui.button(action).clicked();
        }
    });
    clicked
}
