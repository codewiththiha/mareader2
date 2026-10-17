//! The rail's Dictionary panel: search packs or between two languages.

use egui::{Align, Layout, RichText, ScrollArea, TextEdit, Ui};
use mareader_dict::{Dictionaries, HUB, PACKS, Pair, Query, Reply, Ticket, language, languages};
use mareader_settings::DictionarySettings;

use crate::command::{Command, Commands};
use crate::settings_window::Tab;
use crate::theme::Tokens;

/// The search field's name, for screen readers and tests.
pub const LABEL: &str = "Search the dictionaries";

/// Typing settles this long before the packs are asked.
const DEBOUNCE: f64 = 0.18;

#[derive(Default)]
pub struct Panel {
    text: String,
    typed_at: f64,
    /// The ask sent last, to know what its reply answers.
    asked: Option<(Ticket, Query)>,
    reply: Option<Reply>,
}

impl Panel {
    pub fn receive(&mut self, reply: Reply) {
        if self.asked.as_ref().is_some_and(|(t, _)| *t == reply.ticket) {
            self.reply = Some(reply);
        }
    }

    /// The packs changed: ask again.
    pub fn stale(&mut self) {
        self.asked = None;
    }

    /// The ask the panel's text and choices make now.
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

    pub fn show(
        &mut self,
        ui: &mut Ui,
        service: &mut Dictionaries,
        s: &DictionarySettings,
        out: &mut Commands,
    ) {
        let t = Tokens::get(ui.ctx());
        ui.spacing_mut().item_spacing.y = 8.0;
        if !service.attached() {
            muted(ui, &t, "Dictionaries need the desktop app.");
            return;
        }
        let ready = service.ready();
        if ready.is_empty() {
            muted(ui, &t, "No dictionary is downloaded.");
            if ui.button("Get dictionaries…").clicked() {
                out.push(Command::OpenSettings(Tab::Dictionary));
            }
            return;
        }
        let search = TextEdit::singleline(&mut self.text)
            .hint_text("Search any word…")
            .desired_width(f32::INFINITY);
        let now = ui.input(|i| i.time);
        let field = ui.add(search);
        let name = |b: &mut egui::accesskit::Node| b.set_label(LABEL);
        ui.ctx().accesskit_node_builder(field.id, name);
        if field.changed() {
            self.typed_at = now;
        }
        let mut draft = s.clone();
        choices(ui, &mut draft, &ready);
        if draft != *s {
            out.push(Command::SetDictionary(Box::new(draft.clone())));
        }
        let query = self.query(&draft);
        let settled = now - self.typed_at >= DEBOUNCE;
        let changed = self.asked.as_ref().is_none_or(|(_, q)| *q != query);
        if changed && !settled {
            ui.ctx()
                .request_repaint_after_secs((DEBOUNCE - (now - self.typed_at)) as f32);
        } else if changed {
            let ticket = service.ask(query.clone());
            self.asked = Some((ticket, query));
        }
        if self.text.trim().is_empty() {
            muted(ui, &t, "Type a word to look it up.");
            return;
        }
        let Some(reply) = &self.reply else {
            muted(ui, &t, "Searching…");
            return;
        };
        if reply.entries.is_empty() {
            muted(ui, &t, "Nothing found.");
            if draft.pair {
                search_as(ui, &mut draft, &ready, reply, out, s);
            }
            return;
        }
        let asked = self.text.trim().to_owned();
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            super::card::entries(ui, &reply.entries, &asked, &t, |ui, entry| {
                if !draft.pair && ready.len() > 1 {
                    let from = PACKS.iter().find(|p| p.id == entry.pack);
                    if let Some(p) = from {
                        ui.label(RichText::new(p.label()).size(t.text_small).color(t.muted));
                    }
                }
            });
        });
    }
}

/// Packs or a pair, and which.
fn choices(ui: &mut Ui, s: &mut DictionarySettings, ready: &[&'static mareader_dict::Pack]) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut s.pair, false, "Dictionaries");
        ui.selectable_value(&mut s.pair, true, "Languages");
    });
    if !s.pair {
        let chosen = s
            .langs
            .first()
            .and_then(|id| ready.iter().find(|p| p.id == id));
        let shown = chosen.map_or("Every dictionary".to_owned(), |p| p.label());
        let pick = egui::ComboBox::from_id_salt("dict-pack")
            .width(ui.available_width())
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
        crate::widgets::name(&pick.response, "Dictionary to search");
        return;
    }
    let langs = languages(ready);
    ui.horizontal(|ui| {
        let half = (ui.available_width() - 36.0) / 2.0;
        language_box(
            ui,
            ("dict-from", "Translate from"),
            half,
            &mut s.from,
            &langs,
        );
        let swap =
            crate::widgets::small_icon_button(ui, crate::icons::Icon::Swap, "Swap languages");
        if swap.clicked() {
            std::mem::swap(&mut s.from, &mut s.to);
        }
        language_box(ui, ("dict-to", "Translate to"), half, &mut s.to, &langs);
    });
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
    let pick = egui::ComboBox::from_id_salt(id.0)
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
    crate::widgets::name(&pick.response, id.1);
}

/// After nothing was found between a pair: the other source languages.
fn search_as(
    ui: &mut Ui,
    draft: &mut DictionarySettings,
    ready: &[&'static mareader_dict::Pack],
    reply: &Reply,
    out: &mut Commands,
    s: &DictionarySettings,
) {
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
    if draft != s {
        out.push(Command::SetDictionary(Box::new(draft.clone())));
    }
}

fn muted(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).color(t.muted));
}
