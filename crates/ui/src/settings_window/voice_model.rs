//! A downloaded model's tab, and the download controls both tabs share.

use egui::{ProgressBar, RichText, Ui};
use mareader_settings::ReadAloudSettings;
use mareader_speech::{Item, Model, ModelVoice, Shelf, Stage};

use super::{detail_row, divider, group, note, row, section};
use crate::command::{Command, Commands};
use crate::dict::megabytes;
use crate::icons::Icon;
use crate::read_aloud::ModelAction;
use crate::theme::Tokens;
use crate::widgets;

pub fn tab(
    ui: &mut Ui,
    s: &mut ReadAloudSettings,
    model: &'static Model,
    shelf: &Shelf,
    out: &mut Commands,
) {
    section(ui, "Voices");
    // A voice picked, then removed, falls back to the first.
    let picked = s.voices.get(model.id).and_then(|v| model.voice(v));
    let picked = picked
        .filter(|v| shelf.has(model, v.id))
        .map_or(model.first_voice, |v| v.id);
    group(ui, |ui| {
        every_voice(ui, model, shelf, out);
        for voice in model.voices {
            divider(ui);
            if let Some(id) = voice_row(ui, (model, voice), picked == voice.id, shelf, out) {
                s.voices.insert(model.id.to_owned(), id.to_owned());
            }
        }
    });
    note(
        ui,
        "Reading with this model uses the voice picked here. Each voice works offline once downloaded.",
    );
    section(ui, "Model");
    group(ui, |ui| {
        let t = Tokens::get(ui.ctx());
        detail_row(ui, model.name, model.about, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            remove(ui, &t, (Item::Model(model.id), model.name), out);
            muted(ui, &t, &megabytes(shelf.on_disk(model)));
        });
        divider(ui);
        row(ui, "License", "", |ui| muted(ui, &t, model.license));
    });
    note(
        ui,
        "Removing the model deletes its voices too; it can be downloaded again.",
    );
}

/// Downloads every voice still missing at once.
fn every_voice(ui: &mut Ui, model: &'static Model, shelf: &Shelf, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let missing: Vec<&ModelVoice> = (model.voices.iter())
        .filter(|v| !shelf.has(model, v.id))
        .collect();
    let detail = format!("{} voices", model.voices.len());
    detail_row(ui, "All voices", &detail, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if missing.is_empty() {
            muted(ui, &t, "All downloaded");
            return;
        }
        let label = "Download every voice";
        if widgets::icon_button(ui, Icon::Download, label, false).clicked() {
            for v in &missing {
                let item = Item::Voice(model.id, v.id);
                out.push(Command::VoiceModel(ModelAction::Download(item)));
            }
        }
        let bytes = missing.iter().map(|v| v.file.bytes).sum();
        muted(ui, &t, &format!("{} to go", megabytes(bytes)));
    });
}

/// One voice: picked, or its download; the id when picked now.
fn voice_row(
    ui: &mut Ui,
    (model, voice): (&'static Model, &'static ModelVoice),
    on: bool,
    shelf: &Shelf,
    out: &mut Commands,
) -> Option<&'static str> {
    let t = Tokens::get(ui.ctx());
    let kind = if voice.feminine { "female" } else { "male" };
    let detail = format!("{}, {kind}", accent(voice.language));
    let item = Item::Voice(model.id, voice.id);
    let mut pick = None;
    detail_row(ui, voice.name, &detail, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !shelf.has(model, voice.id) {
            controls(
                ui,
                shelf.stage(item),
                (item, voice.name, voice.file.bytes),
                out,
            );
            return;
        }
        let radio = ui.radio(on, "");
        let name = format!("Use {}", voice.name);
        radio.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, &name)
        });
        if radio.clicked() && !on {
            pick = Some(voice.id);
        }
        // The first voice came with the model and goes with it.
        if voice.id != model.first_voice {
            remove(ui, &t, (item, voice.name), out);
        }
    });
    pick
}

/// A download's buttons and progress, right to left.
pub(super) fn controls(
    ui: &mut Ui,
    stage: &Stage,
    (item, name, bytes): (Item, &str, u64),
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let mut press = |ui: &mut Ui, icon, verb: &str, action| {
        let label = format!("{verb} {name}");
        if widgets::icon_button(ui, icon, &label, false).clicked() {
            out.push(Command::VoiceModel(action));
        }
    };
    match stage {
        Stage::Absent => {
            press(ui, Icon::Download, "Download", ModelAction::Download(item));
            muted(ui, &t, &megabytes(bytes));
        }
        Stage::Ready => muted(ui, &t, "Downloaded"),
        Stage::Waiting => {
            press(ui, Icon::Close, "Cancel", ModelAction::Cancel(item));
            muted(ui, &t, "Waiting…");
        }
        Stage::Fetching { got, total } => {
            press(ui, Icon::Close, "Cancel", ModelAction::Cancel(item));
            press(ui, Icon::Pause, "Pause", ModelAction::Pause(item));
            let done = (*got as f32 / (*total).max(1) as f32).clamp(0.0, 1.0);
            let text = format!("{} of {}", megabytes(*got), megabytes(*total));
            ui.add(ProgressBar::new(done).desired_width(150.0).text(text));
        }
        Stage::Paused { got, .. } => {
            press(ui, Icon::Close, "Cancel", ModelAction::Cancel(item));
            press(ui, Icon::Resume, "Resume", ModelAction::Download(item));
            muted(ui, &t, &format!("Paused at {}", megabytes(*got)));
        }
        Stage::Checking => {
            ui.spinner();
            muted(ui, &t, "Checking…");
        }
        Stage::Failed(why) => {
            press(ui, Icon::Rescan, "Try again", ModelAction::Download(item));
            let r = ui.label(RichText::new("Couldn't download").color(t.danger));
            widgets::tip(&r, why, None);
        }
    }
}

fn remove(ui: &mut Ui, t: &Tokens, (item, name): (Item, &str), out: &mut Commands) {
    ui.visuals_mut().override_text_color = Some(t.danger);
    let label = format!("Remove {name}");
    if widgets::icon_button(ui, Icon::Remove, &label, false).clicked() {
        out.push(Command::VoiceModel(ModelAction::Remove(item)));
    }
    ui.visuals_mut().override_text_color = None;
}

fn muted(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).size(t.text_small).color(t.muted));
}

/// A voice's language as a listener names it: `en-GB` is British English.
fn accent(tag: &str) -> &str {
    match tag {
        "en-US" => "American English",
        "en-GB" => "British English",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_voices_are_named_by_their_accent() {
        assert_eq!(accent("en-US"), "American English");
        assert_eq!(accent("en-GB"), "British English");
        assert_eq!(accent("fr-FR"), "fr-FR");
    }
}
