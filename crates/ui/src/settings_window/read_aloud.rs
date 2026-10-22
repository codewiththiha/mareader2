//! Read aloud: the engine, its voice, how it sounds, and how reading goes.

use egui::{Id, Rect, RichText, ScrollArea, Sense, Ui, pos2, vec2};
use mareader_settings::{LANGUAGES, ReadAloudSettings};
use mareader_speech::Voice;

use super::{choice_row, divider, group, note, row, section, switch_row};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::keymap;
use crate::platform::Platform;
use crate::read_aloud::Booth;
use crate::theme::Tokens;
use crate::widgets;

/// What the tab shows besides settings: the engine's voices, the keys.
pub struct Voices<'a> {
    pub booth: &'a Booth,
    pub platform: Platform,
}

/// The voice list's height at most; it scrolls past this.
const LIST_H: f32 = 232.0;
const VOICE_H: f32 = 34.0;
/// The language filter's pick for every language.
const ALL: &str = "";

pub fn tab(ui: &mut Ui, s: &mut ReadAloudSettings, v: &Voices<'_>, out: &mut Commands) {
    engine(ui, s, v.booth);
    voice(ui, s, v.booth, out);
    sound(ui, s, v.booth);
    reading(ui, s);
    let keys = keymap::hint(v.platform, &Command::ToggleReadAloud).unwrap_or_default();
    note(
        ui,
        &format!("Start or pause reading with {keys}, or right-click a page to read from there."),
    );
}

fn engine(ui: &mut Ui, s: &mut ReadAloudSettings, booth: &Booth) {
    section(ui, "Engine");
    let mut engines: Vec<(String, &str)> = (mareader_speech::engines().into_iter())
        .map(|(id, name)| (id.to_owned(), name))
        .collect();
    // One unlisted still shows, so it can be left.
    if !engines.iter().any(|(id, _)| *id == s.engine) {
        let name = match s.engine.as_str() {
            mareader_speech::SILENT => "Silent voice",
            _ => "Unavailable engine",
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
                _ => "Not in this build",
            };
            if choice_row(ui, name, detail, s.engine == *id) {
                s.engine = id.clone();
            }
        }
    });
    if let Some(fault) = booth.fault() {
        let t = Tokens::get(ui.ctx());
        ui.add_space(8.0);
        ui.label(RichText::new(fault).size(t.text_small).color(t.danger));
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
    row(
        ui,
        "Hear a sample",
        "Says a line in this voice, at this speed",
        |ui| {
            let (icon, text, command) = match booth.trying() {
                true => (Icon::Pause, "Stop", Command::StopVoiceTry),
                false => (Icon::Resume, "Preview", Command::TryVoice),
            };
            let button = egui::Button::image_and_text(icon.image(14.0), text)
                .image_tint_follows_text_color(true);
            if ui.add_enabled(open, button).clicked() {
                out.push(command);
            }
        },
    );
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
    fn languages_are_named_by_their_first_part() {
        assert_eq!(primary("en-GB"), "en");
        assert_eq!(primary("pt_BR"), "pt");
        assert_eq!(language_name("en"), "English");
        assert_eq!(language_name("zh"), "Chinese");
        assert_eq!(language_name("my"), "Burmese");
        assert_eq!(language_name("tlh"), "tlh");
    }
}
