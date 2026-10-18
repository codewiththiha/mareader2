//! One dictionary entry's rows, shared by the card and the panel.

use egui::{Align, Layout, RichText, Ui};
use mareader_dict::{Entry, WordMatch, fold};

use crate::shaped;
use crate::theme::Tokens;

/// The answer reads a step larger than the interface, as on a page.
pub const ANSWER: f32 = 1.15;
/// Burmese and its kin run taller at a size, so read a step smaller.
pub const TALL: f32 = 1.05;

/// The answer's size: tall scripts take the smaller step.
pub fn answer_size(text: &str, t: &Tokens) -> f32 {
    t.text
        * if shaped::needs_shaping(text) {
            TALL
        } else {
            ANSWER
        }
}

/// The headword an entry stands for when it isn't the word asked.
pub fn near<'a>(e: &'a Entry, asked: &str) -> Option<&'a str> {
    // A bridge's asked side is the English between, not the ask.
    let other = e.via.is_none() && fold(e.asked()) != fold(asked);
    (e.fit != WordMatch::Exact || other).then(|| e.asked())
}

/// "≈ word": the nearest entry stands in for the one asked.
pub fn heading(ui: &mut Ui, word: &str, t: &Tokens) {
    let text = format!("≈ {word}");
    shaped::label(ui, &text, t.text_small, t.muted).on_hover_text("The nearest entry");
}

/// Entries under one heading per nearest word; `after` adds to each.
pub fn entries(
    ui: &mut Ui,
    list: &[Entry],
    asked: &str,
    t: &Tokens,
    mut after: impl FnMut(&mut Ui, &Entry),
) {
    let mut shown: Option<&str> = None;
    for (i, e) in list.iter().enumerate() {
        let near = near(e, asked);
        let new_group = near.is_some() && near != shown;
        if i > 0 {
            ui.separator();
        }
        if new_group && let Some(word) = near {
            heading(ui, word, t);
        }
        shown = near;
        rows(ui, e, t);
        after(ui, e);
    }
}

/// Tags and source, the answer, then its romanization and sense.
pub fn rows(ui: &mut Ui, e: &Entry, t: &Tokens) {
    meta(ui, e, t);
    shaped::label(ui, e.answer(), answer_size(e.answer(), t), t.ink);
    if let Some(r) = e.romanization.as_deref().filter(|r| !r.is_empty()) {
        shaped::label(ui, r, t.text_small, t.muted);
    }
    if let Some(s) = e.sense.as_deref().filter(|s| !s.is_empty()) {
        if shaped::needs_shaping(s) {
            shaped::label(ui, s, t.text_small, t.muted);
        } else {
            ui.label(RichText::new(s).italics().size(t.text_small).color(t.muted));
        }
    }
}

/// "noun · formal" on the left, "via English" on the right.
fn meta(ui: &mut Ui, e: &Entry, t: &Tokens) {
    let tags = e.tags();
    if tags.is_empty() && e.via.is_none() {
        return;
    }
    let small = |s: String| RichText::new(s).size(t.text_small).color(t.muted);
    ui.horizontal(|ui| {
        if !tags.is_empty() {
            ui.label(small(tags).italics());
        }
        if let Some(via) = &e.via {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(small(format!("via {via}")));
            });
        }
    });
}
