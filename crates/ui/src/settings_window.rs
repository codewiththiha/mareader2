//! The settings window: a modal with a sidebar of tabs, grouped.

use mareader_appearance::Rgb;
use mareader_settings::{
    Corners, FloatingLabelStyle, LayoutSettings, LibraryClick, PageIndicatorStyle, PaperArea,
    Settings, StartupFit, Swatch,
};

use egui::{
    Align, Button, Color32, CornerRadius, Frame, Id, Layout, Margin, Modal, Rect, RichText,
    ScrollArea, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets;

#[cfg(not(target_arch = "wasm32"))]
mod ai;
mod dictionary;
mod fonts;
mod highlights;
mod scope;
mod vocabulary;

pub use scope::{Applies, Scope, Shown};

/// Its size at most; below that, it takes [`SHARE`] of the window.
pub const MAX_SIZE: Vec2 = vec2(760.0, 960.0);
pub const SHARE: Vec2 = vec2(0.92, 0.86);

/// The window's size in an app window of `screen` points.
pub fn window_size(screen: Vec2) -> Vec2 {
    (screen * SHARE).min(MAX_SIZE)
}
const ROW_H: f32 = 44.0;
/// The sidebar of tabs, and each tab's row in it.
const NAV_W: f32 = 184.0;
const NAV_ROW_H: f32 = 32.0;
/// The shown tab's name and the close button.
const HEADER_H: f32 = 56.0;
/// Clear space under the page, below the scroll's fade.
const FOOT_H: f32 = 12.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    /// Colours, a book's text and a PDF's paper.
    #[default]
    Appearance,
    Layout,
    /// The pens, the selection menu and the AI's reserved look.
    Highlights,
    Vocabulary,
    Dictionary,
    /// Who explains words; desktop builds only.
    Ai,
    Workspace,
    /// The master switch, then each motion.
    Motion,
}

impl Tab {
    pub const ALL: [Tab; 8] = [
        Tab::Appearance,
        Tab::Layout,
        Tab::Highlights,
        Tab::Vocabulary,
        Tab::Dictionary,
        Tab::Ai,
        Tab::Workspace,
        Tab::Motion,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Appearance => "Appearance",
            Tab::Layout => "Layout",
            Tab::Highlights => "Highlights",
            Tab::Vocabulary => "Vocabulary",
            Tab::Dictionary => "Dictionary",
            Tab::Ai => "AI",
            Tab::Workspace => "Workspace",
            Tab::Motion => "Motion",
        }
    }

    /// A tab by its name; the old Theme, Fonts and Animations still work.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.to_ascii_lowercase();
        match s.as_str() {
            "theme" | "fonts" => Some(Tab::Appearance),
            "animations" => Some(Tab::Motion),
            _ => Self::ALL
                .into_iter()
                .find(|t| t.label().eq_ignore_ascii_case(&s)),
        }
    }

    fn icon(self) -> Icon {
        match self {
            Tab::Appearance => Icon::Appearance,
            Tab::Layout => Icon::Layout,
            Tab::Highlights => Icon::Highlight,
            Tab::Vocabulary => Icon::Levels,
            Tab::Dictionary => Icon::Dictionary,
            Tab::Ai => Icon::Explain,
            Tab::Workspace => Icon::SplitRight,
            Tab::Motion => Icon::Motion,
        }
    }

    /// The heading the sidebar groups it under.
    fn group(self) -> &'static str {
        match self {
            Tab::Appearance | Tab::Layout => "Reading",
            Tab::Highlights | Tab::Vocabulary | Tab::Dictionary | Tab::Ai => "Study",
            Tab::Workspace | Tab::Motion => "App",
        }
    }

    /// Every tab always shows, so the list never shifts under the pointer.
    fn available(self) -> bool {
        self != Tab::Ai || cfg!(not(target_arch = "wasm32"))
    }
}

#[derive(Clone, Debug, Default)]
pub struct SettingsWindow {
    open: bool,
    tab: Tab,
}

impl SettingsWindow {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens on `tab`, or where it was left.
    pub fn open(&mut self, tab: Option<Tab>) {
        self.open = true;
        if let Some(t) = tab {
            self.tab = t;
        }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// The tab on screen: one this build lacks falls back to the first.
    pub fn tab(&self) -> Tab {
        if self.tab.available() {
            self.tab
        } else {
            Tab::Appearance
        }
    }

    /// Edits a copy of `current`; any change goes out as one command.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        current: &Settings,
        cx: &Scope,
        (ai, dict, vocab): (&crate::ai::Ai, &crate::dict::Dict, &crate::vocab::Vocab),
        out: &mut Commands,
    ) {
        if !self.open {
            return;
        }
        let t = Tokens::get(ctx);
        let size = window_size(ctx.content_rect().size());
        let frame = Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(1.0, t.line))
            .corner_radius(t.radius_lg)
            .shadow(ctx.global_style().visuals.window_shadow);
        let backdrop = Color32::from_black_alpha(if t.dark { 110 } else { 70 });
        let mut draft = current.clone();
        let mut close = false;
        let modal = Modal::new(Id::new("settings-window"))
            .frame(frame)
            .backdrop_color(backdrop)
            .show(ctx, |ui| {
                ui.set_width(size.x);
                ui.set_height(size.y);
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.horizontal_top(|ui| {
                    self.nav(ui, size.y);
                    let (line, _) = ui.allocate_exact_size(vec2(1.0, size.y), Sense::hover());
                    let edge = Stroke::new(1.0, t.line);
                    ui.painter().vline(line.center().x, line.y_range(), edge);
                    ui.vertical(|ui| {
                        ui.set_width(size.x - NAV_W - 1.0);
                        let tab = self.tab();
                        close = header(ui, tab);
                        // Short of the rounded corner, fade and all.
                        let room = ui.available_height() - FOOT_H;
                        let body = ScrollArea::vertical()
                            .id_salt(("settings-body", tab.label()))
                            .max_height(room)
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                let margin = Margin {
                                    left: 24,
                                    right: 24,
                                    top: 0,
                                    bottom: 24,
                                };
                                Frame::new().inner_margin(margin).show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    tab_body(ui, tab, &mut draft, (cx, ai, dict, vocab), out);
                                });
                            });
                        widgets::scroll_fades(ui, &body, t.surface);
                    });
                });
            });
        if close || modal.should_close() {
            out.push(Command::CloseSettings);
        }
        if draft != *current {
            out.push(Command::SetSettings(Box::new(draft)));
        }
    }

    /// The sidebar: every tab named, under its group's heading.
    fn nav(&mut self, ui: &mut Ui, height: f32) {
        let t = Tokens::get(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(vec2(NAV_W, height), Sense::hover());
        let r = t.radius_lg as u8;
        let corners = CornerRadius {
            nw: r,
            sw: r,
            ne: 0,
            se: 0,
        };
        ui.painter()
            .rect_filled(rect, corners, t.hover.gamma_multiply(0.5));
        let inner = rect.shrink2(vec2(10.0, 12.0));
        let layout = Layout::top_down(Align::Min);
        let mut ui = ui.new_child(UiBuilder::new().max_rect(inner).layout(layout));
        ui.spacing_mut().item_spacing.y = 2.0;
        let shown = self.tab();
        let mut group = None;
        for tab in Tab::ALL.into_iter().filter(|tab| tab.available()) {
            if group != Some(tab.group()) {
                ui.add_space(if group.is_some() { 14.0 } else { 6.0 });
                nav_heading(&mut ui, tab.group());
                group = Some(tab.group());
            }
            if nav_row(&mut ui, tab, tab == shown).clicked() {
                self.tab = tab;
            }
        }
    }
}

/// A small muted heading over a group of tabs.
fn nav_heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        let text = RichText::new(text)
            .font(theme::medium(t.text_small))
            .color(t.muted);
        ui.label(text);
    });
    ui.add_space(4.0);
}

/// A tab in the sidebar: its icon and name, filled while shown.
fn nav_row(ui: &mut Ui, tab: Tab, on: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), NAV_ROW_H);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let label = tab.label();
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
    let fill = if on {
        t.accent_soft
    } else if r.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, t.radius, fill);
    let icon = Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(16.0));
    let tint = if on { t.accent } else { t.muted };
    tab.icon().image(16.0).tint(tint).paint_at(ui, icon);
    let font = if on {
        theme::medium(t.text)
    } else {
        egui::FontId::proportional(t.text)
    };
    let at = pos2(rect.left() + 38.0, rect.center().y);
    painter.text(at, egui::Align2::LEFT_CENTER, label, font, t.ink);
    r
}

/// The shown tab's name over its page, and the close button; true on close.
fn header(ui: &mut Ui, tab: Tab) -> bool {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), HEADER_H);
    ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
        ui.set_min_height(HEADER_H);
        ui.add_space(12.0);
        let close = widgets::icon_button(ui, Icon::Close, "Close settings", false).clicked();
        // Painted, not a label: the sidebar already names the tab.
        let at = pos2(ui.max_rect().left() + 24.0, ui.max_rect().center().y);
        let font = theme::medium(18.0);
        ui.painter()
            .text(at, egui::Align2::LEFT_CENTER, tab.label(), font, t.ink);
        close
    })
    .inner
}

fn tab_body(
    ui: &mut Ui,
    tab: Tab,
    s: &mut Settings,
    (cx, ai, dict, vocab): (
        &Scope,
        &crate::ai::Ai,
        &crate::dict::Dict,
        &crate::vocab::Vocab,
    ),
    out: &mut Commands,
) {
    match tab {
        Tab::Appearance => appearance(ui, s, cx, out),
        Tab::Layout => layout(ui, s, cx),
        Tab::Motion => motion(ui, s, cx),
        Tab::Highlights => {
            let study = &mut s.study;
            highlights::tab(ui, &mut study.highlights, &mut study.gloss);
        }
        Tab::Vocabulary => {
            let (study, dictionary) = (&mut s.study, &mut s.dictionary);
            vocabulary::tab(ui, study, dictionary, vocab, out);
        }
        Tab::Dictionary => dictionary::tab(ui, &mut s.dictionary, dict, out),
        #[cfg(not(target_arch = "wasm32"))]
        Tab::Ai => ai::tab(ui, &mut s.ai, ai.board(), out),
        #[cfg(target_arch = "wasm32")]
        Tab::Ai => pending(ui, "AI answers need the desktop app."),
        Tab::Workspace => workspace(ui, s, cx),
    }
    let _ = ai;
}

fn layout(ui: &mut Ui, s: &mut Settings, cx: &Scope) {
    rows(ui, cx, Applies::Always, "Reader chrome", |r| {
        let c = &mut s.chrome;
        r.always(|ui| {
            let hint = "Pin the title bar so it never hides";
            switch_row(ui, "Keep title bar shown", hint, &mut c.titlebar_pinned);
        });
        r.always(|ui| {
            let hint = "The sidebar floats over pages and hides when the pointer leaves";
            switch_row(ui, "Overlay sidebar", hint, &mut c.sidebar_overlay);
        });
        r.always(|ui| {
            let hint = "Blur the page behind the title bar and page bar; off makes both solid";
            switch_row(ui, "Frosted glass", hint, &mut c.glass);
        });
    });
    rows(ui, cx, Applies::Always, "Page position", |r| {
        let l = &mut s.layout;
        r.always(|ui| {
            let hint = "A page counter in the corner of each pane";
            switch_row(ui, "Page indicator", hint, &mut l.page_indicator);
        });
        if l.page_indicator {
            r.always(|ui| {
                row(ui, "Indicator shows", "", |ui| {
                    let style = &mut l.page_indicator_style;
                    ui.selectable_value(style, PageIndicatorStyle::Percentage, "Percent");
                    ui.selectable_value(style, PageIndicatorStyle::PageNumber, "Pages");
                });
            });
        }
        r.always(|ui| {
            let hint = "A thin line along each pane's bottom edge";
            switch_row(ui, "Progress bar", hint, &mut l.progress_bar);
        });
    });
    rows(ui, cx, Applies::Always, "Page label", |r| {
        label_rows(r, &mut s.layout)
    });
    rows(ui, cx, Applies::Always, "Pages", |r| {
        page_rows(r, &mut s.layout)
    });
}

/// The floating document label; its options show once it's on.
fn label_rows(r: &mut Rows<'_>, l: &mut LayoutSettings) {
    r.always(|ui| {
        let hint = "The document's name in each pane's corner while the title bar hides";
        switch_row(ui, "Floating label", hint, &mut l.floating_label);
    });
    if !l.floating_label {
        return;
    }
    r.always(|ui| {
        row(ui, "Label shows", "", |ui| {
            let style = &mut l.floating_label_style;
            ui.selectable_value(style, FloatingLabelStyle::Chapter, "Chapter");
            ui.selectable_value(style, FloatingLabelStyle::FileName, "File name");
        });
    });
    r.always(|ui| {
        let hint = "Keep it while the title bar shows, however narrow the side";
        switch_row(ui, "Always show label", hint, &mut l.floating_label_persist);
    });
    if l.floating_label_persist {
        return;
    }
    r.always(|ui| {
        let hint = "How much of the room beside the page the label may take";
        row(ui, "Label width limit", hint, |ui| {
            let pct = &mut l.floating_label_max_pct;
            let show = |v: f32| format!("{v:.0}%");
            widgets::stepper(ui, pct, 10.0..=100.0, 10.0, show, "label width");
        });
    });
}

/// How pages sit: the fit they open at, their spacing and shadow.
fn page_rows(r: &mut Rows<'_>, l: &mut LayoutSettings) {
    let sideways = r.cx.sideways;
    r.always(|ui| {
        row(ui, "Default fit", "How a document opens", |ui| {
            ui.selectable_value(&mut l.startup_fit, StartupFit::Width, "Width");
            ui.selectable_value(&mut l.startup_fit, StartupFit::Page, "Page");
        });
    });
    // A book's vertical stream has neither gaps nor shadows.
    r.add(Applies::Pages, |ui| {
        let hint = "Pages in the scrolling views touch";
        switch_row(ui, "No gap", hint, &mut l.no_gap);
    });
    r.always(|ui| {
        let hint = if sideways {
            "Pages scrolling sideways have no margin"
        } else {
            "Space beside the pages"
        };
        row(ui, "Page margin", hint, |ui| {
            ui.add_enabled_ui(!sideways, |ui| {
                let show = |v: f32| {
                    if v <= 0.0 {
                        "Off".to_owned()
                    } else {
                        format!("{v:.0} px")
                    }
                };
                let margin = &mut l.page_margin;
                widgets::stepper(ui, margin, 0.0..=64.0, 4.0, show, "page margin");
            });
        });
    });
    r.always(|ui| {
        let hint = "Fit the width on switching to single or two pages";
        switch_row(ui, "Auto scale", hint, &mut l.auto_scale);
    });
    // A book's pages are all one size.
    r.add(Applies::NotBook, |ui| {
        let hint = "Refit when a page of another size is read";
        switch_row(ui, "Auto resize", hint, &mut l.auto_resize);
    });
    r.add(Applies::Pages, |ui| {
        switch_row(ui, "Page shadow", "", &mut l.page_shadow);
    });
}

fn motion(ui: &mut Ui, s: &mut Settings, cx: &Scope) {
    let m = &mut s.motion;
    rows(ui, cx, Applies::Always, "All motion", |r| {
        r.always(|ui| {
            let hint = "Everything that moves. Off, a change lands as its end frame.";
            switch_row(ui, "Animations", hint, &mut m.enabled);
        });
    });
    if !m.enabled {
        note(ui, "Turn Animations on to choose each motion.");
        return;
    }
    rows(ui, cx, Applies::Always, "Each motion", |r| {
        r.always(|ui| {
            let hint = "The sidebar slides in and out, docked or floating";
            switch_row(ui, "Sidebar animation", hint, &mut m.sidebar_slide);
        });
        r.always(|ui| {
            let hint = "Re-fit the page while the window is dragged; off, once the drag ends";
            switch_row(ui, "Canvas follows window", hint, &mut m.canvas_resize);
        });
        r.always(|ui| {
            let hint = "Ease a zoom to its new scale; off, it lands on the first frame";
            switch_row(ui, "Zoom in and out", hint, &mut m.zoom);
        });
        r.always(|ui| {
            let hint = "Glide to a page, and with the arrow and page keys; off, land there";
            switch_row(ui, "Scroll to page", hint, &mut m.scroll_jumps);
        });
    });
    note(ui, "Anything switched off still changes, in one frame.");
}

/// Colours stay in the popover, where the page shows each change.
fn appearance(ui: &mut Ui, s: &mut Settings, cx: &Scope, out: &mut Commands) {
    rows(ui, cx, Applies::Always, "Colours", |r| {
        r.always(|ui| {
            let hint = "Opens over the page, so each change shows as you make it";
            row(ui, "Mode, tint, texture and presets", hint, |ui| {
                let t = Tokens::get(ui.ctx());
                let text = RichText::new("Open Appearance").color(t.accent);
                if ui.add(Button::new(text).fill(t.accent_soft)).clicked() {
                    out.push(Command::CloseSettings);
                    out.push(Command::ShowAppearance);
                }
            });
        });
    });
    if cx.applies(Applies::Book) {
        fonts::tab(ui, &mut s.text);
    } else {
        section(ui, "Text");
        note(ui, "Open an EPUB, Markdown or text file to set its type.");
    }
    let title = "Split pane appearance";
    rows(ui, cx, Applies::Split, title, |r| pane_look(r, s));
    // Read from page rasters, so only PDFs have it.
    rows(ui, cx, Applies::NotBook, "Paper", |r| paper(r, s));
}

fn paper(r: &mut Rows<'_>, s: &mut Settings) {
    let p = &mut s.paper;
    r.always(|ui| {
        let hint = "The background takes the page's own paper colour";
        switch_row(ui, "Blend mode", hint, &mut p.blend);
    });
    r.always(|ui| {
        row(ui, "Detection", "Where the paper colour is read", |ui| {
            ui.add_enabled_ui(p.blend, |ui| {
                // Right to left: the last option is drawn first.
                let options = [
                    (PaperArea::Edges, "Edges"),
                    (PaperArea::WholePage, "Whole page"),
                ];
                for (area, label) in options {
                    ui.selectable_value(&mut p.area, area, label);
                }
            });
        });
    });
}

fn pane_look(r: &mut Rows<'_>, s: &mut Settings) {
    let w = &mut s.workspace.look;
    r.always(|ui| {
        row(ui, "Outline thickness", "Zero hides the outline", |ui| {
            ui.spacing_mut().slider_width = 140.0;
            ui.add(
                egui::Slider::new(&mut w.outline_width, 0.0..=8.0)
                    .step_by(1.0)
                    .fixed_decimals(0)
                    .suffix(" px"),
            );
        });
    });
    r.always(|ui| {
        row(ui, "Outline colour", "The focused pane's outline", |ui| {
            outline_colours(ui, &mut w.outline_color, &mut w.outline_custom);
        });
    });
    r.always(|ui| {
        row(ui, "Space around and between panes", "", |ui| {
            ui.spacing_mut().slider_width = 140.0;
            ui.add(
                egui::Slider::new(&mut w.gap, 0.0..=24.0)
                    .step_by(1.0)
                    .fixed_decimals(0)
                    .suffix(" px"),
            );
        });
    });
    r.always(|ui| {
        row(ui, "Corners", "", |ui| {
            // Right to left: the last option is drawn first.
            for (corners, label) in [(Corners::Rounded, "Rounded"), (Corners::Square, "Square")] {
                if ui
                    .add(Button::selectable(w.corners == corners, label))
                    .clicked()
                {
                    w.corners = corners;
                }
            }
        });
    });
    r.always(|ui| switch_row(ui, "Pane drop shadow", "", &mut w.shadow));
}

/// Laid out right to left: the custom picker, then the palette.
fn outline_colours(ui: &mut Ui, choice: &mut Swatch, custom: &mut Rgb) {
    let t = Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.x = 6.0;
    let mut srgb = [custom.0, custom.1, custom.2];
    ui.spacing_mut().interact_size = Vec2::splat(20.0);
    let picker = egui::color_picker::color_edit_button_srgb(ui, &mut srgb);
    if picker.clicked() || srgb != [custom.0, custom.1, custom.2] {
        *custom = Rgb(srgb[0], srgb[1], srgb[2]);
        *choice = Swatch::Custom;
    }
    widgets::tip(&picker, "Custom", None);
    for c in Swatch::PALETTE.into_iter().rev() {
        let (rect, r) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
        let fill = c.rgb(*custom).map_or(t.accent, theme::color);
        let on = *choice == c;
        ui.painter().circle_filled(rect.center(), 9.0, fill);
        if on {
            ui.painter()
                .circle_stroke(rect.center(), 10.5, Stroke::new(1.5, t.ink));
        }
        let label = c.label();
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
        widgets::tip(&r, label, None);
        if r.clicked() {
            *choice = c;
        }
    }
}

/// The Library panel's click choices, in the tab's order.
const LIBRARY_CLICKS: [(LibraryClick, &str, &str); 3] = [
    (
        LibraryClick::Replace,
        "Open in focused pane",
        "The book replaces what the focused pane shows.",
    ),
    (
        LibraryClick::Split,
        "Open as a new split",
        "The book opens in a new pane beside the focused one.",
    ),
    (
        LibraryClick::DragOnly,
        "Do nothing",
        "Books open by dragging them onto a pane, or with Enter.",
    ),
];

fn workspace(ui: &mut Ui, s: &mut Settings, cx: &Scope) {
    let w = &mut s.workspace;
    section(ui, "Library panel");
    group(ui, |ui| {
        for (i, (choice, title, detail)) in LIBRARY_CLICKS.into_iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            if choice_row(ui, title, detail, w.library_click == choice) {
                w.library_click = choice;
            }
        }
    });
    let page = "The Library page follows this too; with Do nothing it opens in the focused pane.";
    note(ui, page);
    rows(ui, cx, Applies::Always, "Pane themes", |r| {
        r.always(|ui| {
            let hint = "While split, each pane keeps its own colour and texture";
            let label = "Independent theme for each pane";
            switch_row(ui, label, hint, &mut w.independent_themes);
        });
        // Only means something once each pane has its own theme.
        if w.independent_themes {
            r.always(|ui| {
                let hint = "Off: each pane keeps its own mode too";
                let label = "Light, Dark and Dim change every pane";
                switch_row(ui, label, hint, &mut w.shared_base_mode);
            });
        }
        r.always(|ui| {
            let hint = "While split, each pane keeps its own texture and dials";
            let label = "Independent page texture for each pane";
            switch_row(ui, label, hint, &mut w.independent_textures);
        });
    });
    // Text settings only reach books, so PDFs alone don't need it.
    rows(ui, cx, Applies::SplitWithBook, "Pane text", |r| {
        r.always(|ui| {
            let hint = "While split, the Fonts tab changes only the focused pane";
            let label = "Independent text for each pane";
            switch_row(ui, label, hint, &mut w.independent_text);
        });
    });
}

/// A titled group where `applies`, its rows each where they apply.
fn rows(ui: &mut Ui, cx: &Scope, applies: Applies, title: &str, add: impl FnOnce(&mut Rows<'_>)) {
    if !cx.applies(applies) {
        return;
    }
    section(ui, title);
    group(ui, |ui| {
        add(&mut Rows {
            ui,
            cx: *cx,
            drawn: 0,
        });
    });
}

/// A group's rows; dividers fall only between rows drawn.
struct Rows<'a> {
    ui: &'a mut Ui,
    cx: Scope,
    drawn: usize,
}

impl Rows<'_> {
    fn add(&mut self, applies: Applies, row: impl FnOnce(&mut Ui)) {
        if !self.cx.applies(applies) {
            return;
        }
        if self.drawn > 0 {
            divider(self.ui);
        }
        row(self.ui);
        self.drawn += 1;
    }

    fn always(&mut self, row: impl FnOnce(&mut Ui)) {
        self.add(Applies::Always, row);
    }
}

fn section(ui: &mut Ui, title: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(14.0);
    ui.label(
        RichText::new(title)
            .font(theme::medium(t.text_small))
            .color(t.muted),
    );
    ui.add_space(6.0);
}

/// A bordered block of rows.
fn group(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    Frame::new()
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius_lg)
        .inner_margin(Margin::symmetric(14, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
}

fn divider(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, t.line));
}

/// Label on the left (its hint as a tooltip), control on the right.
fn row(ui: &mut Ui, label: &str, hint: &str, control: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_height(ROW_H);
            let r = ui.label(label);
            if !hint.is_empty() {
                widgets::tip(&r, hint, None);
            }
            widgets::labelled(ui, r.id, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), control);
            });
        },
    );
}

fn switch_row(ui: &mut Ui, label: &str, hint: &str, on: &mut bool) {
    row(ui, label, hint, |ui| {
        widgets::switch(ui, on, label);
    });
}

/// A row that shows the rows under it; true while open.
fn disclosure(ui: &mut Ui, label: &str, hint: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let id = ui.id().with(("disclosure", label));
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let size = vec2(ui.available_width(), ROW_H);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, open, label));
    if !hint.is_empty() {
        widgets::tip(&r, hint, None);
    }
    if r.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    let ink = if r.hovered() { t.ink } else { t.muted };
    let font = egui::FontId::proportional(t.text);
    let left = pos2(rect.left(), rect.center().y);
    ui.painter()
        .text(left, egui::Align2::LEFT_CENTER, label, font, ink);
    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let mark = Rect::from_center_size(pos2(rect.right() - 9.0, rect.center().y), Vec2::splat(16.0));
    chevron.image(16.0).tint(ink).paint_at(ui, mark);
    open
}

/// A titled choice with its detail and a radio mark; true when picked.
fn choice_row(ui: &mut Ui, title: &str, detail: &str, on: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), 52.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, title));
    let painter = ui.painter();
    let left = rect.left();
    let font = egui::FontId::proportional(t.text);
    let top = pos2(left, rect.center().y - 9.0);
    painter.text(top, egui::Align2::LEFT_CENTER, title, font, t.ink);
    let small = egui::FontId::proportional(t.text_small);
    let below = pos2(left, rect.center().y + 9.0);
    painter.text(below, egui::Align2::LEFT_CENTER, detail, small, t.muted);
    let mark = pos2(rect.right() - 9.0, rect.center().y);
    let ring = if on || resp.hovered() {
        t.accent
    } else {
        t.line
    };
    painter.circle_stroke(mark, 8.0, Stroke::new(1.5, ring));
    if on {
        painter.circle_filled(mark, 4.5, t.accent);
    }
    resp.clicked() && !on
}

/// Honest copy for settings whose feature has not landed yet.
#[cfg(target_arch = "wasm32")]
fn pending(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.add(Icon::Clock.image(13.0).tint(t.muted));
        ui.label(RichText::new(text).size(t.text_small).color(t.muted));
    });
}

fn note(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(8.0);
    ui.label(RichText::new(text).size(t.text_small).color(t.muted));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_grows_with_the_app_past_mareader_s_640() {
        let tall = window_size(vec2(1600.0, 1100.0));
        assert!(tall.y > 900.0 && tall.x > 700.0, "{tall:?}");
        assert_eq!(window_size(vec2(5000.0, 5000.0)), MAX_SIZE);
        let small = window_size(vec2(500.0, 400.0));
        assert!(small.x < 500.0 && small.y < 400.0, "{small:?}: it fits");
    }
}
