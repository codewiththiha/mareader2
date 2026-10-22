//! A book's type, and a PDF's AI pages', in the Appearance tab.
use std::sync::Arc;

use egui::{Button, ComboBox, RichText, Ui};
use mareader_session::{FACES, Kind, bundled, has_family};
use mareader_settings::{
    BookDesign, ColumnAlign, FontChoice, FontFamily, SystemFont, TextSettings,
};

use mareader_library::{BookId, Design};

use super::{Scope, disclosure, divider, group, note, row, section, switch_row};
use crate::command::{Command, Commands};
use crate::library::Action;
use crate::theme::Tokens;
use crate::widgets;

const PICKER_WIDTH: f32 = 180.0;

/// The focused book's own design choice, over the setting above.
fn this_book(ui: &mut Ui, (book, own): (BookId, Option<Design>), out: &mut Commands) {
    let hint = "This book only, over the setting above; saved with the book";
    row(ui, "This book", hint, |ui| {
        let mut pick = own;
        // Added right to left, so they read left to right.
        ui.selectable_value(&mut pick, Some(Design::Off), "Off");
        ui.selectable_value(&mut pick, Some(Design::On), "On");
        ui.selectable_value(&mut pick, Some(Design::Auto), "Auto");
        ui.selectable_value(&mut pick, None, "As set");
        if pick != own {
            out.push(Command::Library(Action::Design(book, pick)));
        }
    });
}

pub(super) fn tab(ui: &mut Ui, t: &mut TextSettings, cx: &Scope, out: &mut Commands) {
    section(ui, "Book design");
    group(ui, |ui| {
        let hint = "A book's own fonts, sizes and spacing; Auto, where it designs pages";
        row(ui, "Use the book's design", hint, |ui| {
            // Added right to left, so they read left to right.
            let design = &mut t.book_design;
            ui.selectable_value(design, BookDesign::Off, "Off");
            ui.selectable_value(design, BookDesign::On, "On");
            ui.selectable_value(design, BookDesign::Auto, "Auto");
        });
        if let Some(chosen) = cx.book_design {
            divider(ui);
            this_book(ui, chosen, out);
        }
    });
    section(ui, "Typeface");
    group(ui, |ui| typeface(ui, t));
    section(ui, "Spacing");
    group(ui, |ui| spacing(ui, t));
    section(ui, "Column");
    group(ui, |ui| {
        lines(ui, t);
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
        divider(ui);
        page(ui, t);
        divider(ui);
        // The stream's measure; paged books take the pane's.
        row(
            ui,
            "Column width",
            "How wide the vertical stream's text runs",
            |ui| {
                ui.spacing_mut().slider_width = 140.0;
                let (lo, hi) = TextSettings::COLUMN_PCT;
                ui.add(
                    egui::Slider::new(&mut t.column_width_pct, lo..=hi)
                        .step_by(5.0)
                        .fixed_decimals(0)
                        .suffix(" %"),
                );
            },
        );
    });
    section(ui, "Ink and pictures");
    group(ui, |ui| {
        let hint = "How dark a book's text stays; its pictures keep their colours";
        let percent = |v: f32| format!("{v:.0} %");
        let ink = &mut t.ink_contrast;
        stepped(ui, "Text ink", hint, ink, TextSettings::INK, 5.0, percent);
        divider(ui);
        switch_row(
            ui,
            "Theme pictures",
            "Tint a book's pictures with the page; off, they keep their colours",
            &mut t.theme_pictures,
        );
        divider(ui);
        switch_row(
            ui,
            "Theme emoji",
            "Tint emoji with the page; off, they keep their colours",
            &mut t.theme_emojis,
        );
    });
}

/// A paged book's page: it fills the pane, inside these margins.
fn page(ui: &mut Ui, t: &mut TextSettings) {
    let pt = |v: f32| format!("{v:.0} pt");
    let margin = TextSettings::PAGE_MARGIN;
    let hint = "Above and below a paged book's text";
    stepped(
        ui,
        "Top and bottom margins",
        hint,
        &mut t.page_margin_y,
        margin,
        4.0,
        pt,
    );
    divider(ui);
    let hint = "Beside a paged book's text";
    stepped(
        ui,
        "Side margins",
        hint,
        &mut t.page_margin_x,
        margin,
        4.0,
        pt,
    );
    divider(ui);
    let hint = "Past this, a wide page centres its text";
    let line = TextSettings::MAX_LINE;
    stepped(
        ui,
        "Longest line",
        hint,
        &mut t.max_line_width,
        line,
        20.0,
        pt,
    );
}

/// A PDF's AI pages: type and spacing; its page keeps their measure.
pub(super) fn ai_tab(ui: &mut Ui, t: &mut TextSettings) {
    section(ui, "AI pages");
    note(
        ui,
        "This PDF's AI pages are set in this type. A result never outgrows \
        its page: past the room, its text shrinks to fit.",
    );
    section(ui, "Typeface");
    group(ui, |ui| typeface(ui, t));
    section(ui, "Spacing");
    group(ui, |ui| spacing(ui, t));
    section(ui, "Lines");
    group(ui, |ui| lines(ui, t));
}

fn lines(ui: &mut Ui, t: &mut TextSettings) {
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
}

fn typeface(ui: &mut Ui, t: &mut TextSettings) {
    let body = "The body text; Default follows the serif font";
    picker(ui, "Body font", body, &mut t.default_font, None);
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
    divider(ui);
    // Rarely changed: they stand in for faces the book itself names.
    let hint = "Faces used where a book asks for serif, sans-serif or code";
    if !disclosure(ui, "Book's own families", hint) {
        return;
    }
    divider(ui);
    let serif = Some(FontFamily::Serif);
    picker(ui, "Serif font", "", &mut t.serif_font, serif);
    divider(ui);
    let sans = Some(FontFamily::Sans);
    picker(ui, "Sans-serif font", "Headings", &mut t.sans_font, sans);
    divider(ui);
    let mono = Some(FontFamily::Mono);
    picker(ui, "Monospace font", "Code", &mut t.mono_font, mono);
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
            .icon(widgets::combo_chevron)
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
