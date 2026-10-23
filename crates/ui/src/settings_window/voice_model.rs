//! A downloaded model's tab, and the download controls both tabs share.

use egui::{ProgressBar, RichText, Ui};
use mareader_settings::ReadAloudSettings;
use mareader_speech::{Build, Item, Model, ModelVoice, Shelf, Stage};

use super::{detail_row, divider, group, note, radio, row, section};
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
    if model.builds.len() > 1 {
        builds(ui, s, model, shelf, out);
    }
    section(ui, "Voices");
    // Voices with no file of their own come inside every build.
    let inside = model.voices.iter().all(|v| v.file.is_none());
    // A voice picked, then removed, falls back to the first.
    let picked = s.voices.get(model.id).and_then(|v| model.voice(v));
    let picked = picked
        .filter(|v| shelf.has(model, v.id))
        .map_or(model.first_voice, |v| v.id);
    group(ui, |ui| {
        let shown = language_filter(ui, model);
        let shown: Vec<&'static ModelVoice> = (model.voices.iter())
            .filter(|v| shown.is_none_or(|l| v.language == l))
            .collect();
        if !inside {
            every_voice(ui, model, &shown, shelf, out);
        }
        for (i, voice) in shown.into_iter().enumerate() {
            if i > 0 || !inside {
                divider(ui);
            }
            if let Some(id) = voice_row(ui, (model, voice), picked == voice.id, shelf, out) {
                s.voices.insert(model.id.to_owned(), id.to_owned());
            }
        }
    });
    let offline = match inside {
        true => "Every voice comes with each build and works offline.",
        false => "Each voice works offline once downloaded.",
    };
    note(
        ui,
        &format!("Reading with this model uses the voice picked here. {offline}"),
    );
    if !model.knobs.is_empty() {
        sound(ui, s, model);
    }
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
        "Removing the model deletes its voices and builds too; it can be downloaded again.",
    );
}

/// The model's own settings, each a share of how a voice was made.
fn sound(ui: &mut Ui, s: &mut ReadAloudSettings, model: &'static Model) {
    section(ui, "Sound");
    let knobs = s.knobs.entry(model.id.to_owned()).or_default();
    group(ui, |ui| {
        for (i, knob) in model.knobs.iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            let mut value = knob.clamp(knobs.get(knob.id).copied());
            row(ui, knob.name, knob.about, |ui| {
                ui.spacing_mut().slider_width = 160.0;
                let (least, most) = knob.range;
                let slider = egui::Slider::new(&mut value, least..=most)
                    .step_by(0.05)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0));
                if ui.add(slider).changed() {
                    knobs.insert(knob.id.to_owned(), value);
                }
            });
        }
        divider(ui);
        let set = knobs.values().any(|v| *v != 1.0);
        row(ui, "As made", "Every setting back to 100%", |ui| {
            if ui.add_enabled(set, egui::Button::new("Reset")).clicked() {
                knobs.clear();
            }
        });
    });
    if knobs.is_empty() {
        s.knobs.remove(model.id);
    }
    note(
        ui,
        "100% is each voice as it was made. Speech already made is made again.",
    );
}

/// The builds on offer: each downloads alone, one is used.
fn builds(
    ui: &mut Ui,
    s: &mut ReadAloudSettings,
    model: &'static Model,
    shelf: &Shelf,
    out: &mut Commands,
) {
    section(ui, "Build");
    let wanted = s.builds.get(model.id).map(String::as_str);
    let used = shelf.build(model, wanted).map(|b| b.id);
    let here = shelf.builds(model).len();
    group(ui, |ui| {
        for (i, build) in model.builds.iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            if build_row(
                ui,
                (model, build),
                (used == Some(build.id), here),
                shelf,
                out,
            ) {
                s.builds.insert(model.id.to_owned(), build.id.to_owned());
            }
        }
    });
    // Builds that bring their own voices are models of their own sizes.
    let own = model.builds.iter().any(|b| b.extra.is_some());
    let about = match own {
        true => {
            "Each build is a model of its own size: a larger one sounds better, \
                 a smaller one speaks sooner and takes less memory."
        }
        false => {
            "Builds share the voices. A smaller build saves download and disk; \
                  each takes the same memory once loaded."
        }
    };
    note(ui, about);
}

/// One build: used, or its download; true when picked now.
fn build_row(
    ui: &mut Ui,
    (model, build): (&'static Model, &'static Build),
    (on, here): (bool, usize),
    shelf: &Shelf,
    out: &mut Commands,
) -> bool {
    let t = Tokens::get(ui.ctx());
    let item = Item::Build(model.id, build.id);
    let name = mareader_speech::label(model, build);
    let mut pick = false;
    detail_row(ui, build.name, build.about, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let stage = shelf.stage(item);
        if *stage != Stage::Ready {
            controls(ui, stage, (item, &name, build.bytes()), out);
            return;
        }
        pick = radio(ui, on, &format!("Use {}", build.name));
        // The last build goes with the model, below.
        if here > 1 {
            remove(ui, &t, (item, &name), out);
        }
        muted(ui, &t, &megabytes(build.bytes()));
    });
    pick
}

/// Picks which voices show, by language, where a model speaks more than one.
fn language_filter(ui: &mut Ui, model: &'static Model) -> Option<&'static str> {
    let mut languages: Vec<&'static str> = Vec::new();
    for v in model.voices {
        if !languages.contains(&v.language) {
            languages.push(v.language);
        }
    }
    if languages.len() < 2 {
        return None;
    }
    let id = egui::Id::new(("voice language", model.id));
    let kept = ui.data(|d| d.get_temp::<usize>(id)).unwrap_or_default();
    // 0 shows every language; 1 on, each in turn.
    let mut at = kept.min(languages.len());
    let name = |at: usize| match at.checked_sub(1).and_then(|i| languages.get(i)) {
        Some(l) => accent(l),
        None => "All languages",
    };
    row(ui, "Language", "", |ui| {
        widgets::combo(id)
            .selected_text(name(at))
            .show_ui(ui, |ui| {
                for i in 0..=languages.len() {
                    ui.selectable_value(&mut at, i, name(i));
                }
            });
    });
    divider(ui);
    if at != kept {
        ui.data_mut(|d| d.insert_temp(id, at));
    }
    at.checked_sub(1).and_then(|i| languages.get(i)).copied()
}

/// Downloads every voice shown still missing at once.
fn every_voice(
    ui: &mut Ui,
    model: &'static Model,
    shown: &[&'static ModelVoice],
    shelf: &Shelf,
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let missing: Vec<&ModelVoice> = (shown.iter().copied())
        .filter(|v| !shelf.has(model, v.id))
        .collect();
    let detail = format!("{} voices", shown.len());
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
        let bytes = shelf.to_fetch(model, &missing);
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
    let mut detail = format!("{}, {}", accent(voice.language), voice.gender.label());
    if !voice.note.is_empty() {
        detail = format!("{detail} · {}", voice.note);
    }
    let item = Item::Voice(model.id, voice.id);
    let mut pick = None;
    detail_row(ui, voice.name, &detail, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !shelf.has(model, voice.id) {
            controls(
                ui,
                shelf.stage(item),
                (item, voice.name, shelf.to_fetch(model, &[voice])),
                out,
            );
            return;
        }
        if radio(ui, on, &format!("Use {}", voice.name)) {
            pick = Some(voice.id);
        }
        // The first voice came with the model and goes with it.
        if voice.id != model.first_voice && voice.file.is_some() {
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
        "es-ES" => "Spanish",
        "it-IT" => "Italian",
        "pt-BR" => "Brazilian Portuguese",
        "fr-FR" => "French",
        "hi-IN" => "Hindi",
        "zh-CN" => "Mandarin Chinese",
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
        assert_eq!(accent("it-IT"), "Italian");
        assert_eq!(accent("pt-BR"), "Brazilian Portuguese");
        assert_eq!(accent("fr-FR"), "French");
        assert_eq!(accent("hi-IN"), "Hindi");
        assert_eq!(accent("zh-CN"), "Mandarin Chinese");
        assert_eq!(accent("ja-JP"), "ja-JP");
    }
}
