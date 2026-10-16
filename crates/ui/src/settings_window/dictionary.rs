//! The Dictionary tab: language packs to download, and the card's
//! language.

use egui::{ProgressBar, RichText, Ui};
use mareader_dict::{HUB, PACKS, Pack, State, language, languages};
use mareader_settings::DictionarySettings;

use super::{divider, group, note, row, section};
use crate::command::{Command, Commands};
use crate::dict::{Action, Dict, count, megabytes};
use crate::icons::Icon;
use crate::theme::Tokens;
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
    section(ui, "Lookup card");
    group(ui, |ui| default_language(ui, s, &dict.ready()));
    note(
        ui,
        "The card answers in this language. Only a language whose pack is downloaded is offered.",
    );
}

fn pack_row(ui: &mut Ui, pack: &'static Pack, dict: &Dict, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let hint = format!("{} words", count(pack.entries));
    let label = pack.label();
    let id = pack.id;
    row(ui, &label, &hint, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !dict.attached() {
            muted(ui, &t, &megabytes(pack.bytes));
            return;
        }
        let mut press = |ui: &mut Ui, icon, name: &str, action| {
            if widgets::icon_button(ui, icon, &format!("{name} {label}"), false).clicked() {
                out.push(Command::Dict(action));
            }
        };
        match dict.state(pack) {
            State::Absent => {
                press(ui, Icon::Download, "Download", Action::Download(id));
                muted(ui, &t, &megabytes(pack.bytes));
            }
            State::Fetching { got, total } => {
                press(ui, Icon::Close, "Cancel", Action::Cancel(id));
                press(ui, Icon::Pause, "Pause", Action::Pause(id));
                let total = total.unwrap_or(pack.bytes).max(1);
                let done = (*got as f32 / total as f32).clamp(0.0, 1.0);
                let text = format!("{} of {}", megabytes(*got), megabytes(total));
                ui.add(ProgressBar::new(done).desired_width(150.0).text(text));
            }
            State::Paused { got, .. } => {
                press(ui, Icon::Close, "Cancel", Action::Cancel(id));
                press(ui, Icon::Resume, "Resume", Action::Download(id));
                muted(ui, &t, &format!("Paused at {}", megabytes(*got)));
            }
            State::Converting => {
                ui.spinner();
                muted(ui, &t, "Preparing…");
            }
            State::Ready => {
                ui.visuals_mut().override_text_color = Some(t.danger);
                press(ui, Icon::Remove, "Remove", Action::Remove(id));
                ui.visuals_mut().override_text_color = None;
                muted(ui, &t, "Downloaded");
            }
            State::Failed(why) => {
                press(ui, Icon::Rescan, "Try again", Action::Download(id));
                let r = ui.label(RichText::new("Couldn't download").color(t.danger));
                widgets::tip(&r, why, None);
            }
        }
    });
}

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
                egui::ComboBox::from_id_salt("dict-default-lang")
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

fn muted(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).size(t.text_small).color(t.muted));
}
