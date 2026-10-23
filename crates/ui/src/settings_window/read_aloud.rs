//! Read aloud: the engine, its voice, how it sounds, and how reading goes.

use egui::{Id, Rect, RichText, ScrollArea, Sense, Ui, pos2, vec2};
use mareader_settings::{LANGUAGES, ReadAloudSettings};
use mareader_speech::{Build, Item, Model, Shelf, Stage, Voice};

use super::voice_model::controls;
use super::{Tab, choice_row, detail_row, divider, group, note, row, section, switch_row};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::keymap;
use crate::platform::Platform;
use crate::read_aloud::Booth;
use crate::theme::Tokens;
use crate::widgets;

/// What the tab shows besides settings: voices, models, the keys.
pub struct Voices<'a> {
    pub booth: &'a Booth,
    /// Voice models on offer and their downloads.
    pub shelf: &'a Shelf,
    pub platform: Platform,
}

/// The voice list's height at most; it scrolls past this.
const LIST_H: f32 = 232.0;
const VOICE_H: f32 = 34.0;
/// The language filter's pick for every language.
const ALL: &str = "";

pub fn tab(ui: &mut Ui, s: &mut ReadAloudSettings, v: &Voices<'_>, out: &mut Commands) {
    engine(ui, s, v);
    models(ui, s, v.shelf, out);
    memory(ui, s, v.shelf);
    voice(ui, s, v.booth, out);
    sound(ui, s, v.booth);
    reading(ui, s);
    let keys = keymap::hint(v.platform, &Command::ToggleReadAloud).unwrap_or_default();
    note(
        ui,
        &format!("Start or pause reading with {keys}, or right-click a page to read from there."),
    );
}

fn engine(ui: &mut Ui, s: &mut ReadAloudSettings, v: &Voices<'_>) {
    section(ui, "Engine");
    let mut engines: Vec<(String, &str)> = (mareader_speech::engines(v.shelf).into_iter())
        .map(|(id, name)| (id.to_owned(), name))
        .collect();
    // One unlisted still shows, so it can be left.
    if !engines.iter().any(|(id, _)| *id == s.engine) {
        let name = match s.engine.as_str() {
            mareader_speech::SILENT => "Silent voice",
            id => v.shelf.model(id).map_or("Unavailable engine", |m| m.name),
        };
        engines.push((s.engine.clone(), name));
    }
    group(ui, |ui| {
        for (i, (id, name)) in engines.iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            let detail = match id.as_str() {
                mareader_speech::SYSTEM => "Built into your system: nothing to download",
                mareader_speech::SILENT => "Says nothing: for tests and screenshots",
                id if v.shelf.ready().iter().any(|m| m.id == id) => {
                    "Runs on this computer and works offline"
                }
                id if v.shelf.model(id).is_some() => "Not downloaded: get it under Voice models",
                _ => "Not in this build",
            };
            if choice_row(ui, name, detail, s.engine == *id) {
                s.engine = id.clone();
            }
        }
    });
    if let Some(fault) = v.booth.fault() {
        let t = Tokens::get(ui.ctx());
        ui.add_space(8.0);
        ui.label(RichText::new(fault).size(t.text_small).color(t.danger));
    }
}

/// Models to download; each ready one opens its own tab.
fn models(ui: &mut Ui, s: &mut ReadAloudSettings, shelf: &Shelf, out: &mut Commands) {
    section(ui, "Voice models");
    group(ui, |ui| {
        for (i, model) in shelf.catalogue().iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            model_row(ui, s, model, shelf, out);
        }
    });
    let text = match shelf.attached() {
        true => {
            "Models run on this computer and work offline once downloaded. \
             A paused download resumes where it stopped."
        }
        false => "Downloads need the desktop app.",
    };
    note(ui, text);
}

fn model_row(
    ui: &mut Ui,
    s: &mut ReadAloudSettings,
    model: &'static Model,
    shelf: &Shelf,
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let muted = |ui: &mut Ui, text: &str| {
        ui.label(RichText::new(text).size(t.text_small).color(t.muted));
    };
    let wanted = s.builds.get(model.id).and_then(|b| model.build(b));
    let picked = wanted.or(model.builds.first());
    detail_row(ui, model.name, model.about, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !shelf.attached() {
            let bytes = picked.map_or(model.bytes(), |b| model.bytes_with(b));
            muted(ui, &crate::dict::megabytes(bytes));
            return;
        }
        if shelf.ready().iter().any(|m| m.id == model.id) {
            let label = format!("{} voices", model.name);
            let open = egui::Button::new("Voices…");
            if ui.add(open).on_hover_text(label).clicked() {
                out.push(Command::OpenSettings(Tab::Model(model)));
            }
            muted(ui, &crate::dict::megabytes(shelf.on_disk(model)));
            return;
        }
        // Without builds, or with one here, only the shared files are owed.
        let Some(picked) = picked.filter(|_| shelf.builds(model).is_empty()) else {
            let item = Item::Model(model.id);
            controls(
                ui,
                shelf.stage(item),
                (item, model.name, model.bytes()),
                out,
            );
            return;
        };
        // A build begun, else the one picked, with the shared files.
        let begun = (model.builds.iter())
            .find(|b| *shelf.stage(Item::Build(model.id, b.id)) != Stage::Absent);
        let build = begun.unwrap_or(picked);
        let item = Item::Build(model.id, build.id);
        let stage = shelf.stage(item);
        controls(ui, stage, (item, model.name, model.bytes_with(build)), out);
        if *stage == Stage::Absent
            && model.builds.len() > 1
            && let Some(id) = build_pick(ui, model, build)
        {
            s.builds.insert(model.id.to_owned(), id.to_owned());
        }
    });
}

/// Which build a first download brings; the id when changed.
fn build_pick(ui: &mut Ui, model: &'static Model, on: &Build) -> Option<&'static str> {
    let mut pick = on.id;
    let combo = widgets::combo(("first build", model.id))
        .selected_text(on.name)
        .width(72.0);
    let shown = combo.show_ui(ui, |ui| {
        for b in model.builds {
            let size = crate::dict::megabytes(model.bytes_with(b));
            let r = ui.selectable_value(&mut pick, b.id, format!("{} · {size}", b.name));
            r.on_hover_text(b.about);
        }
    });
    let label = format!("{} build", model.name);
    shown
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, &label));
    shown.response.on_hover_text(on.about);
    (pick != on.id).then_some(pick)
}

/// What downloaded models keep in memory, once one is downloaded.
fn memory(ui: &mut Ui, s: &mut ReadAloudSettings, shelf: &Shelf) {
    if shelf.ready().is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    section(ui, "Memory");
    group(ui, |ui| {
        let (least, _, most) = ReadAloudSettings::MEMORY_MB;
        let hint = "Made ahead and heard lately, to play again at once";
        row(ui, "Speech kept", hint, |ui| {
            ui.spacing_mut().slider_width = 160.0;
            let slider = egui::Slider::new(&mut s.speech_memory_mb, least..=most)
                .logarithmic(true)
                .custom_formatter(|mb, _| memory_label(mb as u32));
            ui.add(slider);
        });
        divider(ui);
        let hint = "While nothing is read with it";
        row(ui, "Keep a model loaded", hint, |ui| {
            widgets::combo("model-idle")
                .width(150.0)
                .selected_text(idle_label(s.model_idle_min))
                .show_ui(ui, |ui| {
                    for minutes in ReadAloudSettings::IDLE_MIN {
                        let label = idle_label(minutes);
                        ui.selectable_value(&mut s.model_idle_min, minutes, label);
                    }
                });
        });
        divider(ui);
        row(ui, "In memory now", "", |ui| {
            let text = usage_label(&shelf.usage());
            ui.label(RichText::new(text).size(t.text_small).color(t.muted));
        });
    });
    note(
        ui,
        "A model loads when reading starts, which takes a moment, and leaves memory once unused.",
    );
}

/// Megabytes, and about how long that much speech plays.
fn memory_label(mb: u32) -> String {
    // Kept as 16-bit samples, 24,000 a second.
    let minutes = u64::from(mb) * (1 << 20) / 48_000 / 60;
    format!("{mb} MB · {minutes} min")
}

fn idle_label(minutes: u32) -> String {
    match minutes {
        0 => "While reading".to_owned(),
        1 => "For 1 minute".to_owned(),
        n => format!("For {n} minutes"),
    }
}

fn usage_label(usage: &mareader_speech::Usage) -> String {
    let speech = (usage.speech > 0).then(|| {
        let mb = crate::dict::megabytes(usage.speech as u64);
        format!("{mb} of speech")
    });
    let parts: Vec<String> = (usage.models.iter().map(|m| (*m).to_owned()))
        .chain(speech)
        .collect();
    match parts.is_empty() {
        true => "Nothing loaded".to_owned(),
        false => parts.join(" · "),
    }
}

fn voice(ui: &mut Ui, s: &mut ReadAloudSettings, booth: &Booth, out: &mut Commands) {
    section(ui, "Voice");
    let open = booth.engine() == s.engine && booth.ready();
    let failed = booth.engine() == s.engine && booth.fault().is_some() && !open;
    group(ui, |ui| {
        if failed {
            row(ui, "Voice", "", |ui| {
                let t = Tokens::get(ui.ctx());
                let text = "Voices show once the engine starts";
                ui.label(RichText::new(text).size(t.text_small).color(t.muted));
            });
            divider(ui);
        } else if open && booth.caps().voices {
            let picked = s.voice().map(str::to_owned);
            let language = language_filter(ui, booth.voices(), picked.as_deref());
            divider(ui);
            if let Some(pick) = voice_list(ui, booth.voices(), &language, picked.as_deref()) {
                match pick {
                    Some(id) => s.voices.insert(s.engine.clone(), id),
                    None => s.voices.remove(&s.engine),
                };
            }
            divider(ui);
        } else if open {
            row(ui, "Voice", "", |ui| {
                let t = Tokens::get(ui.ctx());
                let text = "This engine speaks in one voice";
                ui.label(RichText::new(text).size(t.text_small).color(t.muted));
            });
            divider(ui);
        }
        preview_row(ui, booth, open, out);
    });
}

/// The language picker over the voices; returns the primary tag picked.
fn language_filter(ui: &mut Ui, voices: &[Voice], picked: Option<&str>) -> String {
    let id = Id::new("read-aloud-language");
    let mut languages: Vec<String> = voices.iter().map(|v| primary(&v.language)).collect();
    languages.sort_by_key(|l| language_name(l));
    languages.dedup();
    let chosen = ui.data(|d| d.get_temp::<String>(id));
    // First shown: the picked voice's language, else English, else all.
    let mut language = chosen.unwrap_or_else(|| {
        let of_pick = picked.and_then(|p| voices.iter().find(|v| v.id == p));
        match of_pick {
            Some(v) => primary(&v.language),
            None if languages.iter().any(|l| l == "en") => "en".to_owned(),
            None => ALL.to_owned(),
        }
    });
    row(
        ui,
        "Language",
        "Shows only voices for this language",
        |ui| {
            let shown = match language.as_str() {
                ALL => "All languages".to_owned(),
                tag => language_name(tag),
            };
            widgets::combo("read-aloud-language")
                .selected_text(shown)
                .width(180.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut language, ALL.to_owned(), "All languages");
                    for tag in &languages {
                        ui.selectable_value(&mut language, tag.clone(), language_name(tag));
                    }
                });
        },
    );
    ui.data_mut(|d| d.insert_temp(id, language.clone()));
    language
}

/// The default voice, then each voice in `language`; a click picks one.
fn voice_list(
    ui: &mut Ui,
    voices: &[Voice],
    language: &str,
    picked: Option<&str>,
) -> Option<Option<String>> {
    let mut pick = None;
    let shown = voices
        .iter()
        .filter(|v| language == ALL || primary(&v.language) == language);
    ScrollArea::vertical()
        .id_salt("read-aloud-voices")
        .max_height(LIST_H)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.add_space(4.0);
            if voice_row(ui, "System default", "", picked.is_none()) {
                pick = Some(None);
            }
            for v in shown {
                let on = picked == Some(v.id.as_str());
                if voice_row(ui, &v.name, &v.language, on) {
                    pick = Some(Some(v.id.clone()));
                }
            }
            ui.add_space(4.0);
        });
    pick
}

/// One voice: its name, its language tag, a check while picked.
fn voice_row(ui: &mut Ui, name: &str, tag: &str, on: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), VOICE_H), Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, name));
    let fill = match (on, r.hovered()) {
        (true, _) => t.selected,
        (false, true) => t.hover,
        (false, false) => egui::Color32::TRANSPARENT,
    };
    let painter = ui.painter();
    painter.rect_filled(rect, t.radius, fill);
    let font = egui::FontId::proportional(t.text);
    let left = pos2(rect.left() + 10.0, rect.center().y);
    painter.text(left, egui::Align2::LEFT_CENTER, name, font, t.ink);
    let small = egui::FontId::proportional(t.text_small);
    let right = pos2(rect.right() - 34.0, rect.center().y);
    painter.text(right, egui::Align2::RIGHT_CENTER, tag, small, t.muted);
    if on {
        let mark =
            Rect::from_center_size(pos2(rect.right() - 17.0, rect.center().y), vec2(16.0, 16.0));
        Icon::Check.image(16.0).tint(t.accent).paint_at(ui, mark);
    }
    r.clicked() && !on
}

/// Tries the voice picked, or stops the try.
fn preview_row(ui: &mut Ui, booth: &Booth, open: bool, out: &mut Commands) {
    let hint = "Says a line in this voice, at this speed";
    row(ui, "Hear a sample", hint, |ui| {
        let (icon, text, command) = match booth.trying() {
            true => (Icon::Pause, "Stop", Command::StopVoiceTry),
            false => (Icon::Resume, "Preview", Command::TryVoice),
        };
        let button = egui::Button::image_and_text(icon.image(14.0), text)
            .image_tint_follows_text_color(true);
        if ui.add_enabled(open, button).clicked() {
            out.push(command);
        }
        if booth.loading() {
            let t = Tokens::get(ui.ctx());
            let text = RichText::new("Loading voice…").size(t.text_small);
            ui.label(text.color(t.muted));
        }
    });
}

fn sound(ui: &mut Ui, s: &mut ReadAloudSettings, booth: &Booth) {
    section(ui, "Sound");
    let caps = booth.caps();
    // Before the engine opens, nothing is greyed out on a guess.
    let known = booth.engine() == s.engine && booth.ready();
    group(ui, |ui| {
        row(ui, "Speed", "Narrated books play at this speed too", |ui| {
            ui.spacing_mut().slider_width = 160.0;
            let (lo, hi) = (ReadAloudSettings::SLOWEST, ReadAloudSettings::FASTEST);
            let slider = egui::Slider::new(&mut s.rate, lo..=hi)
                .step_by(0.05)
                .custom_formatter(|v, _| crate::read_aloud::rate_label(v as f32));
            ui.add(slider);
        });
        divider(ui);
        let hint = |can: bool, what: &'static str| match can || !known {
            true => "",
            false => what,
        };
        let can = caps.pitch || !known;
        row(
            ui,
            "Pitch",
            hint(caps.pitch, "This engine keeps its own pitch"),
            |ui| {
                ui.spacing_mut().slider_width = 160.0;
                let (lo, hi) = (ReadAloudSettings::LOWEST, ReadAloudSettings::HIGHEST);
                percent(ui, &mut s.pitch, (lo, hi), can);
            },
        );
        divider(ui);
        let can = caps.volume || !known;
        row(
            ui,
            "Volume",
            hint(caps.volume, "This engine keeps its own volume"),
            |ui| {
                ui.spacing_mut().slider_width = 160.0;
                percent(ui, &mut s.volume, (0.0, 1.0), can);
            },
        );
    });
}

/// A share as a percent slider, greyed out when the engine can't.
fn percent(ui: &mut Ui, share: &mut f32, (lo, hi): (f32, f32), can: bool) {
    let mut pct = *share * 100.0;
    let slider = egui::Slider::new(&mut pct, lo * 100.0..=hi * 100.0)
        .step_by(5.0)
        .fixed_decimals(0)
        .suffix("%");
    if ui.add_enabled(can, slider).changed() {
        *share = pct / 100.0;
    }
}

fn reading(ui: &mut Ui, s: &mut ReadAloudSettings) {
    section(ui, "While reading");
    group(ui, |ui| {
        let hint = "The page and scroll keep up with the words read";
        switch_row(ui, "Turn pages to follow", hint, &mut s.follow);
        divider(ui);
        let hint = "Numbers at the top or foot of a page are passed over";
        switch_row(ui, "Skip page numbers", hint, &mut s.skip_page_numbers);
        divider(ui);
        let hint = "Narrated books pass over their footnotes and asides";
        switch_row(ui, "Skip notes", hint, &mut s.skip_notes);
    });
}

/// A language tag's first part, lowercased: `en-GB` is `en`.
fn primary(tag: &str) -> String {
    let first = tag.split(['-', '_']).next().unwrap_or_default();
    first.to_ascii_lowercase()
}

/// A primary tag's English name, else the tag itself.
fn language_name(tag: &str) -> String {
    let named = LANGUAGES.iter().find(|(t, _)| primary(t) == tag);
    match named {
        // Both Chinese scripts share the one spoken name.
        Some(_) if tag == "zh" => "Chinese".to_owned(),
        Some((_, name)) => (*name).to_owned(),
        None => tag.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_rows_read_plainly() {
        assert_eq!(memory_label(16), "16 MB · 5 min");
        assert_eq!(memory_label(64), "64 MB · 23 min");
        assert_eq!(idle_label(0), "While reading");
        assert_eq!(idle_label(1), "For 1 minute");
        assert_eq!(idle_label(15), "For 15 minutes");
        let mut usage = mareader_speech::Usage::default();
        assert_eq!(usage_label(&usage), "Nothing loaded");
        usage.speech = 2_500_000;
        assert_eq!(usage_label(&usage), "2.5 MB of speech");
        usage.models.push("Kokoro (Small)".into());
        assert_eq!(usage_label(&usage), "Kokoro (Small) · 2.5 MB of speech");
    }

    #[test]
    fn languages_are_named_by_their_first_part() {
        assert_eq!(primary("en-GB"), "en");
        assert_eq!(primary("pt_BR"), "pt");
        assert_eq!(language_name("en"), "English");
        assert_eq!(language_name("zh"), "Chinese");
        assert_eq!(language_name("my"), "Burmese");
        assert_eq!(language_name("tlh"), "tlh");
    }
}
