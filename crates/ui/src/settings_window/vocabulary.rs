//! The Vocabulary tab: the word data, then shading once it is in.

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
    section(ui, "Word data");
    group(ui, |ui| data_row(ui, vocab, out));
    // Shading needs the levels: its options wait for them.
    let ready = *vocab.state(Part::Dataset) == State::Ready;
    if !vocab.attached() {
        note(ui, "Downloads need the desktop app.");
    } else if !ready {
        note(
            ui,
            "Download it to shade words above your level. It works offline, \
             and a paused download resumes where it stopped.",
        );
    }
    if !ready {
        return;
    }
    section(ui, "Vocabulary highlighter");
    group(ui, |ui| {
        let c = &mut study.cefr;
        let hint = "Words harder than your level, shaded in PDFs and books";
        switch_row(ui, "Shade words above my level", hint, &mut c.enabled);
        divider(ui);
        level_row(ui, &mut c.level);
        divider(ui);
        let hint = "Opens it in the AI Dictionary, marking it";
        switch_row(
            ui,
            "Click a shaded word to look it up",
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
}

/// Both files as the one download the reader sees.
#[derive(Clone, Debug, PartialEq)]
enum Whole<'a> {
    Absent,
    Fetching {
        got: u64,
        total: u64,
    },
    Paused {
        got: u64,
        total: u64,
    },
    Preparing,
    /// The levels are in; the roles model is still to come.
    Partial,
    Ready,
    Failed(&'a str),
}

/// How the levels and the roles model stand together.
fn whole<'a>(levels: &'a State, roles: &'a State) -> Whole<'a> {
    let parts = [(levels, DATASET.bytes), (roles, TAGGER.bytes)];
    let size = |s: &State, guess: u64| match s {
        State::Fetching { got, total } | State::Paused { got, total } => {
            (*got, total.unwrap_or(guess))
        }
        State::Ready | State::Converting => (guess, guess),
        State::Absent | State::Failed(_) => (0, guess),
    };
    let (got, total) = parts.iter().fold((0, 0), |(g, t), (s, guess)| {
        let (a, b) = size(s, *guess);
        (g + a, t + b)
    });
    let any = |f: fn(&State) -> bool| parts.iter().any(|(s, _)| f(s));
    let failed = parts.iter().find_map(|(s, _)| match s {
        State::Failed(why) => Some(why.as_str()),
        _ => None,
    });
    if let Some(why) = failed {
        Whole::Failed(why)
    } else if any(|s| matches!(s, State::Fetching { .. })) {
        Whole::Fetching { got, total }
    } else if any(|s| matches!(s, State::Paused { .. })) {
        Whole::Paused { got, total }
    } else if any(|s| *s == State::Converting) {
        Whole::Preparing
    } else {
        match (levels, roles) {
            (State::Ready, State::Ready) => Whole::Ready,
            (State::Ready, _) => Whole::Partial,
            _ => Whole::Absent,
        }
    }
}

/// A slider over the five levels; it applies as it moves.
fn level_row(ui: &mut Ui, level: &mut CefrLevel) {
    let all = CefrLevel::ALL;
    let mut at = all.iter().position(|l| l == level).unwrap_or(2);
    row(ui, "My level", level.detail(), |ui| {
        let t = Tokens::get(ui.ctx());
        ui.label(RichText::new(level.label()).strong().color(t.ink));
        ui.add_space(t.gap);
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
    let label = format!("{name} word data");
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

/// The levels and the roles model, downloaded, paused and removed as one.
fn data_row(ui: &mut Ui, vocab: &Vocab, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let hint = "Each word's CEFR level, and a model of its role in a sentence";
    let all = DATASET.bytes + TAGGER.bytes;
    row(ui, "Word data", hint, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !vocab.attached() {
            muted(ui, &t, &megabytes(all));
            return;
        }
        let levels = vocab.state(Part::Dataset);
        match whole(levels, vocab.state(Part::Tagger)) {
            Whole::Absent => {
                press(ui, out, (Icon::Download, "Download"), Action::Download);
                muted(ui, &t, &megabytes(all));
            }
            Whole::Fetching { got, total } => {
                press(ui, out, (Icon::Close, "Cancel"), Action::Cancel);
                press(ui, out, (Icon::Pause, "Pause"), Action::Pause);
                progress(ui, got, Some(total), all);
            }
            Whole::Paused { got, .. } => {
                press(ui, out, (Icon::Close, "Cancel"), Action::Cancel);
                press(ui, out, (Icon::Resume, "Resume"), Action::Download);
                muted(ui, &t, &format!("Paused at {}", megabytes(got)));
            }
            Whole::Preparing => {
                ui.spinner();
                muted(ui, &t, "Preparing…");
            }
            Whole::Partial => {
                remove(ui, &t, out);
                // The roles came later: shading works, a click's role waits.
                press(ui, out, (Icon::Download, "Download"), Action::Download);
                muted(ui, &t, "Word roles missing");
                words(ui, vocab, &t);
            }
            Whole::Ready => {
                remove(ui, &t, out);
                words(ui, vocab, &t);
            }
            Whole::Failed(why) => failed(ui, &t, why, out),
        }
    });
}

fn remove(ui: &mut Ui, t: &Tokens, out: &mut Commands) {
    ui.visuals_mut().override_text_color = Some(t.danger);
    press(ui, out, (Icon::Remove, "Remove"), Action::Remove);
    ui.visuals_mut().override_text_color = None;
}

fn words(ui: &mut Ui, vocab: &Vocab, t: &Tokens) {
    let words = vocab.words().map(|n| format!("{} words", count(n)));
    muted(ui, t, words.as_deref().unwrap_or("Downloaded"));
}

fn failed(ui: &mut Ui, t: &Tokens, why: &str, out: &mut Commands) {
    press(ui, out, (Icon::Rescan, "Try again"), Action::Download);
    let r = ui.label(RichText::new("Couldn't download").color(t.danger));
    widgets::tip(&r, why, None);
}

fn muted(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).size(t.text_small).color(t.muted));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_files_read_as_one_download() {
        let fetching = |got| State::Fetching {
            got,
            total: Some(1_000),
        };
        let (a, b) = (fetching(250), fetching(500));
        let both = whole(&a, &b);
        let (got, total) = (750, 2_000);
        assert_eq!(both, Whole::Fetching { got, total });
        let levels = State::Ready;
        let ready = whole(&levels, &b);
        let got = DATASET.bytes + 500;
        let total = DATASET.bytes + 1_000;
        assert_eq!(ready, Whole::Fetching { got, total });
        let why = State::Failed("offline".into());
        assert_eq!(whole(&State::Ready, &why), Whole::Failed("offline"));
        assert_eq!(whole(&State::Ready, &State::Absent), Whole::Partial);
        assert_eq!(whole(&State::Ready, &State::Ready), Whole::Ready);
        assert_eq!(whole(&State::Absent, &State::Absent), Whole::Absent);
    }
}
