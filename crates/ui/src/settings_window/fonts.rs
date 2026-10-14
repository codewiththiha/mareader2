//! The Fonts tab: how EPUBs are set; open books re-set as it changes.
use std::sync::Arc;

use egui::{Button, ComboBox, RichText, Ui};
use mareader_session::{FACES, Kind, bundled, has_family};
use mareader_settings::{ColumnAlign, FontChoice, FontFamily, SystemFont, TextSettings};

use super::{divider, group, pending, row, section, switch_row};
use crate::theme::Tokens;
use crate::widgets;

const PICKER_WIDTH: f32 = 180.0;

pub(super) fn tab(ui: &mut Ui, t: &mut TextSettings) {
    section(ui, "Typeface");
    group(ui, |ui| typeface(ui, t));
    section(ui, "Spacing");
    group(ui, |ui| spacing(ui, t));
    section(ui, "Layout");
    group(ui, |ui| {
        switch_row(
            ui,
            "Full justification",
            "Stretch every line but a paragraph's last to both margins",
            &mut t.justify,
        );
        divider(ui);
        switch_row(
            ui,
            "Hyphenation",
            "Break long words at line ends where the book's language allows",
            &mut t.hyphenation,
        );
        divider(ui);
        switch_row(
            ui,
            "Book layout",
            "A wider margin toward the spine, as in a bound book",
            &mut t.book_layout,
        );
        divider(ui);
        let hint = "Where the vertical stream's text sits";
        row(ui, "Column alignment", hint, |ui| {
            // Added right to left, so they read left to right.
            let align = &mut t.column_align;
            ui.selectable_value(align, ColumnAlign::Right, "Right");
            ui.selectable_value(align, ColumnAlign::Center, "Center");
            ui.selectable_value(align, ColumnAlign::Left, "Left");
        });
    });
    pending(ui, "Ink contrast arrives with selection (M4).");
}

fn typeface(ui: &mut Ui, t: &mut TextSettings) {
    let body = "The body text; Default follows the serif font";
    picker(ui, "Font", body, &mut t.default_font, None);
    divider(ui);
    let serif = Some(FontFamily::Serif);
    picker(ui, "Serif font", "", &mut t.serif_font, serif);
    divider(ui);
    let sans = Some(FontFamily::Sans);
    picker(ui, "Sans-serif font", "Headings", &mut t.sans_font, sans);
    divider(ui);
    let mono = Some(FontFamily::Mono);
    picker(ui, "Monospace font", "Code", &mut t.mono_font, mono);
    divider(ui);
    let size = TextSettings::FONT_SIZE;
    let px = |v: f32| format!("{v:.0} px");
    stepped(ui, "Font size", "", &mut t.font_size, size, 1.0, px);
    divider(ui);
    let hint = "Headings keep their own";
    let weight = TextSettings::FONT_WEIGHT;
    let plain = |v: f32| format!("{v:.0}");
    stepped(
        ui,
        "Font weight",
        hint,
        &mut t.font_weight,
        weight,
        100.0,
        plain,
    );
}

fn spacing(ui: &mut Ui, t: &mut TextSettings) {
    let em2 = |v: f32| format!("{v:.2} em");
    let line = TextSettings::LINE_HEIGHT;
    let times = |v: f32| format!("{v:.1}×");
    stepped(ui, "Line spacing", "", &mut t.line_height, line, 0.1, times);
    divider(ui);
    let hint = "Space between paragraphs";
    let gap = TextSettings::PARAGRAPH;
    stepped(
        ui,
        "Paragraph margin",
        hint,
        &mut t.paragraph_margin,
        gap,
        0.25,
        em2,
    );
    divider(ui);
    let words = TextSettings::WORD_SPACING;
    let px = |v: f32| format!("{v:.1} px");
    stepped(ui, "Word spacing", "", &mut t.word_spacing, words, 0.5, px);
    divider(ui);
    let letters = TextSettings::LETTER_SPACING;
    stepped(
        ui,
        "Letter spacing",
        "",
        &mut t.letter_spacing,
        letters,
        0.01,
        em2,
    );
    divider(ui);
    let hint = "A paragraph's first line";
    let indent = TextSettings::INDENT;
    let em1 = |v: f32| format!("{v:.1} em");
    stepped(
        ui,
        "Text indent",
        hint,
        &mut t.text_indent,
        indent,
        0.5,
        em1,
    );
}

/// A row with a stepper over `lo..=hi`.
fn stepped(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: &mut f32,
    (lo, hi): (f32, f32),
    step: f32,
    show: impl Fn(f32) -> String,
) {
    row(ui, label, hint, |ui| {
        let name = label.to_lowercase();
        widgets::stepper(ui, value, lo..=hi, step, show, &name);
    });
}

/// What a pick is called in the picker.
fn label(choice: &FontChoice) -> &'static str {
    match choice {
        FontChoice::Default => "Default",
        FontChoice::System(f) => f.label(),
        FontChoice::Bundled(id) => bundled(id).map_or("Default", |f| f.name),
    }
}

fn kind(family: FontFamily) -> Kind {
    match family {
        FontFamily::Serif => Kind::Serif,
        FontFamily::Sans => Kind::Sans,
        FontFamily::Mono => Kind::Mono,
    }
}

/// One slot's picker; `only` keeps a family's slot to its own faces.
fn picker(ui: &mut Ui, name: &str, hint: &str, choice: &mut FontChoice, only: Option<FontFamily>) {
    row(ui, name, hint, |ui| {
        ComboBox::from_id_salt(name)
            .width(PICKER_WIDTH)
            .selected_text(label(choice))
            .show_ui(ui, |ui| {
                ui.selectable_value(choice, FontChoice::Default, "Default");
                heading(ui, "Bundled");
                let ours = FACES
                    .iter()
                    .filter(|f| only.is_none_or(|o| kind(o) == f.kind));
                for face in ours {
                    let pick = FontChoice::Bundled(face.id.to_owned());
                    ui.selectable_value(choice, pick, face.name);
                }
                heading(ui, "On this computer");
                let installed = installed(ui);
                let theirs = SystemFont::ALL
                    .iter()
                    .filter(|f| only.is_none_or(|o| o == f.family()));
                for &font in theirs {
                    let pick = FontChoice::System(font);
                    let on = *choice == pick;
                    let button = Button::selectable(on, font.label());
                    let r = ui.add_enabled(installed.contains(&font), button);
                    let r = r.on_disabled_hover_text("Not installed");
                    if r.clicked() {
                        *choice = pick;
                    }
                }
            });
    });
}

fn heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(4.0);
    ui.label(RichText::new(text).size(t.text_small).color(t.muted));
}

/// The system faces this machine has, looked up once per run.
fn installed(ui: &Ui) -> Arc<[SystemFont]> {
    let id = egui::Id::new("installed-system-fonts");
    if let Some(known) = ui.data(|d| d.get_temp::<Arc<[SystemFont]>>(id)) {
        return known;
    }
    // A generic name always finds the system's own face.
    let generic = |n: &str| matches!(n, "serif" | "sans-serif" | "monospace");
    let found: Arc<[SystemFont]> = SystemFont::ALL
        .iter()
        .copied()
        .filter(|f| f.names().iter().any(|n| generic(n) || has_family(n)))
        .collect();
    ui.data_mut(|d| d.insert_temp(id, Arc::clone(&found)));
    found
}
