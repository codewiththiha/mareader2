//! The Dictionary tab: language packs to download, and the card's
//! language.

use egui::Ui;
use mareader_dict::{HUB, PACKS, Pack, State, language, languages};
use mareader_settings::{DictionaryOpens, DictionarySettings};

use super::downloads::{Buttons, muted, progress};
use super::{choice_row, divider, group, note, row, section, switch_row};
use crate::command::{Command, Commands, Download};
use crate::dict::{Action, Dict, count, megabytes};
use crate::icons::Icon;
use crate::widgets;

pub fn tab(ui: &mut Ui, s: &mut DictionarySettings, dict: &Dict, out: &mut Commands) {
    section(ui, "Language packs");
    group(ui, |ui| {
        for (i, pack) in PACKS.iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            pack_row(ui, pack, dict, out);
        }
    });
    if dict.attached() {
        note(
            ui,
            "Packs work offline once downloaded. A paused download resumes where it stopped.",
        );
    } else {
        note(ui, "Downloads need the desktop app.");
    }
    section(ui, "In the reader");
    group(ui, |ui| {
        for (i, (choice, title, detail)) in OPENS.into_iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            if choice_row(ui, title, detail, s.opens == choice) {
                s.opens = choice;
            }
        }
    });
    note(ui, "The Dictionary button and Cmd/Ctrl+Shift+D open it so.");
    section(ui, "Lookup card");
    group(ui, |ui| {
        default_language(ui, s, &dict.ready());
        divider(ui);
        let hint = "With no entry for a word, the closest ones in its place";
        switch_row(ui, "Show near matches", hint, &mut s.near);
    });
    note(
        ui,
        "The card answers in this language. Only a language whose pack is downloaded is offered.",
    );
}

fn pack_row(ui: &mut Ui, pack: &'static Pack, dict: &Dict, out: &mut Commands) {
    let hint = format!("{} words", count(pack.entries));
    let label = pack.label();
    let id = pack.id;
    let buttons = Buttons {
        name: &label,
        ask: |what| {
            Command::Dict(match what {
                Download::Start => Action::Download(id),
                Download::Pause => Action::Pause(id),
                Download::Cancel => Action::Cancel(id),
                Download::Remove => Action::Remove(id),
            })
        },
    };
    row(ui, &label, &hint, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !dict.attached() {
            muted(ui, &megabytes(pack.bytes));
            return;
        }
        match dict.state(pack) {
            State::Absent => {
                buttons.press(ui, (Icon::Download, "Download"), Download::Start, out);
                muted(ui, &megabytes(pack.bytes));
            }
            State::Fetching { got, total } => {
                buttons.press(ui, (Icon::Close, "Cancel"), Download::Cancel, out);
                buttons.press(ui, (Icon::Pause, "Pause"), Download::Pause, out);
                progress(ui, *got, total.unwrap_or(pack.bytes));
            }
            State::Paused { got, .. } => buttons.paused(ui, *got, out),
            State::Converting => {
                ui.spinner();
                muted(ui, "Preparing…");
            }
            State::Ready => {
                buttons.remove(ui, out);
                muted(ui, "Downloaded");
            }
            State::Failed(why) => buttons.failed(ui, why, out),
        }
    });
}

/// How the reader's Dictionary button opens it, and what that means.
const OPENS: [(DictionaryOpens, &str, &str); 2] = [
    (
        DictionaryOpens::Window,
        "Floating window",
        "Over the page; move, resize, fold or close it.",
    ),
    (
        DictionaryOpens::Page,
        "Full page",
        "In place of the panes, as the library shows.",
    ),
];

fn default_language(ui: &mut Ui, s: &mut DictionarySettings, ready: &[&'static Pack]) {
    // English is what packs translate from; the card answers in another.
    let offered: Vec<&str> = languages(ready).into_iter().filter(|l| *l != HUB).collect();
    let chosen: Option<&str> = s
        .default_lang
        .as_deref()
        .and_then(|l| offered.iter().copied().find(|o| *o == l));
    let shown = chosen.map_or("Automatic".to_owned(), language);
    row(
        ui,
        "Default language",
        "What the card translates into",
        |ui| {
            ui.add_enabled_ui(!offered.is_empty(), |ui| {
                widgets::combo("dict-default-lang")
                    .width(180.0)
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(chosen.is_none(), "Automatic").clicked() {
                            s.default_lang = None;
                        }
                        for l in &offered {
                            if ui
                                .selectable_label(chosen == Some(*l), language(l))
                                .clicked()
                            {
                                s.default_lang = Some((*l).to_owned());
                            }
                        }
                    });
            });
        },
    );
}
