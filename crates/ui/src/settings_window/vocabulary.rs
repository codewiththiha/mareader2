//! The Vocabulary tab: shading words above the reader's level, its look
//! and the word dataset.

use egui::{ProgressBar, RichText, Slider, Ui};
use mareader_cefr::{DATASET, Part, State, TAGGER};
use mareader_settings::{CefrLevel, DictionarySettings, Pen, StudySettings};

use super::highlights::{palette, preview};
use super::{divider, group, note, row, section, switch_row};
use crate::command::{Command, Commands};
use crate::dict::{count, megabytes};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::vocab::{Action, Vocab};
use crate::widgets;

pub fn tab(
    ui: &mut Ui,
    study: &mut StudySettings,
    dictionary: &mut DictionarySettings,
    vocab: &Vocab,
    out: &mut Commands,
) {
    section(ui, "Vocabulary highlighter");
    group(ui, |ui| {
        let c = &mut study.cefr;
        let hint = "Words harder than your level, shaded in PDFs and books";
        switch_row(ui, "Shade words above my level", hint, &mut c.enabled);
        divider(ui);
        level_row(ui, &mut c.level);
        divider(ui);
        let hint = "Opens the AI card for the word, marking it";
        switch_row(
            ui,
            "Click a shaded word to explain it",
            hint,
            &mut c.click_explain,
        );
        divider(ui);
        let hint = "Its dictionary card, when a language pack is downloaded";
        switch_row(
            ui,
            "Rest on a shaded word to look it up",
            hint,
            &mut dictionary.hover,
        );
    });
    section(ui, "Shading look");
    group(ui, |ui| look_rows(ui, study));
    note(
        ui,
        "Highlights can't take this look, so none passes for shading.",
    );
    section(ui, "Word dataset");
    group(ui, |ui| {
        dataset_row(ui, vocab, out);
        divider(ui);
        roles_row(ui, vocab, out);
    });
    if vocab.attached() {
        note(
            ui,
            "Both work offline once downloaded. A paused download resumes where it stopped.",
        );
    } else {
        note(ui, "Downloads need the desktop app.");
    }
}

/// A slider over the five levels; it applies as it moves.
fn level_row(ui: &mut Ui, level: &mut CefrLevel) {
    let all = CefrLevel::ALL;
    let mut at = all.iter().position(|l| l == level).unwrap_or(2);
    row(ui, "My level", level.detail(), |ui| {
        let t = Tokens::get(ui.ctx());
        ui.label(RichText::new(level.label()).strong().color(t.ink));
        let max = all.len().saturating_sub(1);
        let slider = Slider::new(&mut at, 0..=max).show_value(false);
        let r = ui.add_sized([150.0, 20.0], slider);
        r.widget_info(|| {
            let mut info =
                egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Reading level");
            info.value = Some(at as f64);
            info
        });
    });
    *level = all.get(at).copied().unwrap_or_default();
}

/// The shading's pen and colour, which highlights then can't take.
fn look_rows(ui: &mut Ui, study: &mut StudySettings) {
    let h = &mut study.highlights;
    let mut cefr = h.cefr;
    row(ui, "Mark", "How hard words are shaded", |ui| {
        for pen in Pen::ALL.into_iter().rev() {
            let on = cefr.pen == pen;
            if ui.add(egui::Button::selectable(on, pen.label())).clicked() {
                cefr.pen = pen;
            }
        }
        preview(ui, cefr.pen, cefr.color, h.opacity);
    });
    divider(ui);
    let (taken, ai) = (h.color(cefr.pen), h.ai);
    row(
        ui,
        "Colour",
        "A pen's own colour and the AI's look can't be used",
        |ui| {
            let kept = "Kept for a pen or for AI marks";
            let free = |c| c != taken && !(ai.pen == cefr.pen && ai.color == c);
            palette(ui, ("Shading", kept), &mut cefr.color, free);
        },
    );
    h.cefr = cefr;
}

fn press(ui: &mut Ui, out: &mut Commands, (icon, name): (Icon, &str), action: Action) {
    let label = format!("{name} word levels");
    if widgets::icon_button(ui, icon, &label, false).clicked() {
        out.push(Command::Vocab(action));
    }
}

fn progress(ui: &mut Ui, got: u64, total: Option<u64>, guess: u64) {
    let total = total.unwrap_or(guess).max(1);
    let done = (got as f32 / total as f32).clamp(0.0, 1.0);
    let text = format!("{} of {}", megabytes(got), megabytes(total));
    ui.add(ProgressBar::new(done).desired_width(150.0).text(text));
}

fn dataset_row(ui: &mut Ui, vocab: &Vocab, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let hint = "Each word's CEFR level, one small file";
    row(ui, "Word levels", hint, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !vocab.attached() {
            muted(ui, &t, &megabytes(DATASET.bytes));
            return;
        }
        match vocab.state(Part::Dataset) {
            State::Absent => {
                press(ui, out, (Icon::Download, "Download"), Action::Download);
                muted(ui, &t, &megabytes(DATASET.bytes + TAGGER.bytes));
            }
            State::Fetching { got, total } => {
                press(ui, out, (Icon::Close, "Cancel"), Action::Cancel);
                press(ui, out, (Icon::Pause, "Pause"), Action::Pause);
                progress(ui, *got, *total, DATASET.bytes);
            }
            State::Paused { got, .. } => {
                press(ui, out, (Icon::Close, "Cancel"), Action::Cancel);
                press(ui, out, (Icon::Resume, "Resume"), Action::Download);
                muted(ui, &t, &format!("Paused at {}", megabytes(*got)));
            }
            State::Converting => {
                ui.spinner();
                muted(ui, &t, "Preparing…");
            }
            State::Ready => {
                ui.visuals_mut().override_text_color = Some(t.danger);
                press(ui, out, (Icon::Remove, "Remove"), Action::Remove);
                ui.visuals_mut().override_text_color = None;
                let words = vocab.words().map(|n| format!("{} words", count(n)));
                muted(ui, &t, words.as_deref().unwrap_or("Downloaded"));
            }
            State::Failed(why) => failed(ui, &t, why, out),
        }
    });
}

/// The tagger comes with the dataset; it only shows how it stands.
fn roles_row(ui: &mut Ui, vocab: &Vocab, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let hint = "A clicked word's role in its sentence, and that sense's level";
    row(ui, "Word roles", hint, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !vocab.attached() {
            muted(ui, &t, &megabytes(TAGGER.bytes));
            return;
        }
        match vocab.state(Part::Tagger) {
            State::Absent => muted(ui, &t, "Comes with the word levels"),
            State::Fetching { got, total } => progress(ui, *got, *total, TAGGER.bytes),
            State::Paused { got, .. } => {
                muted(ui, &t, &format!("Paused at {}", megabytes(*got)));
            }
            State::Converting => {
                ui.spinner();
            }
            State::Ready => muted(ui, &t, "Downloaded"),
            State::Failed(why) => failed(ui, &t, why, out),
        }
    });
}

fn failed(ui: &mut Ui, t: &Tokens, why: &str, out: &mut Commands) {
    press(ui, out, (Icon::Rescan, "Try again"), Action::Download);
    let r = ui.label(RichText::new("Couldn't download").color(t.danger));
    widgets::tip(&r, why, None);
}

fn muted(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).size(t.text_small).color(t.muted));
}
