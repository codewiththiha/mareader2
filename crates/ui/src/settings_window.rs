//! The settings window: a modal with one tab per area of the app.

use mareader_appearance::Rgb;
use mareader_settings::{
    Corners, FloatingLabelStyle, PageIndicatorStyle, Settings, StartupFit, Swatch,
};

use egui::{
    Align, Button, Color32, Frame, Id, Layout, Margin, Modal, RichText, ScrollArea, Sense, Stroke,
    Ui, Vec2, vec2,
};

use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets;

pub const MAX_SIZE: Vec2 = vec2(620.0, 640.0);
const ROW_H: f32 = 44.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Layout,
    Theme,
    /// Shown only while the Animations master switch is on.
    Animations,
    /// Shown only while an EPUB is focused.
    Fonts,
    Vocabulary,
    Dictionary,
    Workspace,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Layout,
        Tab::Theme,
        Tab::Animations,
        Tab::Fonts,
        Tab::Vocabulary,
        Tab::Dictionary,
        Tab::Workspace,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Layout => "Layout",
            Tab::Theme => "Theme",
            Tab::Animations => "Animations",
            Tab::Fonts => "Fonts",
            Tab::Vocabulary => "Vocabulary",
            Tab::Dictionary => "Dictionary",
            Tab::Workspace => "Workspace",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.label().eq_ignore_ascii_case(s))
    }

    fn icon(self) -> Icon {
        match self {
            Tab::Layout => Icon::Layout,
            Tab::Theme => Icon::Appearance,
            Tab::Animations => Icon::Motion,
            Tab::Fonts => Icon::Type,
            Tab::Vocabulary => Icon::Levels,
            Tab::Dictionary => Icon::Dictionary,
            Tab::Workspace => Icon::SplitRight,
        }
    }

    fn available(self, s: &Settings, cx: &WindowContext) -> bool {
        match self {
            Tab::Animations => s.motion.enabled,
            Tab::Fonts => cx.reflowable,
            _ => true,
        }
    }
}

/// What the window needs to know about the workspace this frame.
pub struct WindowContext {
    pub split: bool,
    pub reflowable: bool,
    /// The focused pane scrolls sideways, where pages have no margin.
    pub sideways: bool,
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

    /// The tab on screen: a hidden tab falls back to Layout.
    pub fn tab(&self, s: &Settings, cx: &WindowContext) -> Tab {
        if self.tab.available(s, cx) {
            self.tab
        } else {
            Tab::Layout
        }
    }

    /// Edits a copy of `current`; any change goes out as one command.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        current: &Settings,
        cx: &WindowContext,
        out: &mut Commands,
    ) {
        if !self.open {
            return;
        }
        let t = Tokens::get(ctx);
        let screen = ctx.content_rect().size();
        let size = vec2(
            (screen.x * 0.92).min(MAX_SIZE.x),
            (screen.y * 0.76).min(MAX_SIZE.y),
        );
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
                ui.spacing_mut().item_spacing.y = 0.0;
                let header = Margin {
                    left: 16,
                    right: 16,
                    top: 16,
                    bottom: 8,
                };
                Frame::new().inner_margin(header).show(ui, |ui| {
                    close = self.tabs(ui, &draft, cx);
                });
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    let body = Margin {
                        left: 16,
                        right: 16,
                        top: 4,
                        bottom: 20,
                    };
                    Frame::new().inner_margin(body).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let tab = self.tab(&draft, cx);
                        tab_body(ui, tab, &mut draft, cx, out);
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

    /// The tab strip and the close button; true when close was clicked.
    fn tabs(&mut self, ui: &mut Ui, s: &Settings, cx: &WindowContext) -> bool {
        let shown = self.tab(s, cx);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for tab in Tab::ALL.into_iter().filter(|tab| tab.available(s, cx)) {
                let on = tab == shown;
                let image = tab.icon().image(17.0);
                // The active tab names itself; the others are icons.
                let button = if on {
                    Button::selectable(true, (image, tab.label()))
                } else {
                    Button::image(image).frame_when_inactive(false)
                };
                let r = ui.add(
                    button
                        .image_tint_follows_text_color(true)
                        .min_size(Vec2::splat(36.0)),
                );
                let label = tab.label();
                r.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label)
                });
                widgets::tip(&r, label, None);
                if r.clicked() {
                    self.tab = tab;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                widgets::icon_button(ui, Icon::Close, "Close settings", false).clicked()
            })
            .inner
        })
        .inner
    }
}

fn tab_body(ui: &mut Ui, tab: Tab, s: &mut Settings, cx: &WindowContext, out: &mut Commands) {
    match tab {
        Tab::Layout => layout(ui, s, cx),
        Tab::Theme => theme_tab(ui, s, cx, out),
        Tab::Animations => animations(ui, s),
        Tab::Fonts => pending(ui, "Typeface, size, spacing and margins for EPUBs (M4)."),
        Tab::Vocabulary => pending(
            ui,
            "CEFR level highlighting and its word lists arrive with the study tools (M6).",
        ),
        Tab::Dictionary => pending(
            ui,
            "Hover lookup, language packs and languages arrive with the study tools (M6).",
        ),
        Tab::Workspace => workspace(ui),
    }
}

fn layout(ui: &mut Ui, s: &mut Settings, cx: &WindowContext) {
    section(ui, "Reader chrome");
    group(ui, |ui| {
        switch_row(
            ui,
            "Keep title bar shown",
            "Pin the title bar so it never hides",
            &mut s.chrome.titlebar_pinned,
        );
        divider(ui);
        switch_row(
            ui,
            "Overlay sidebar",
            "The sidebar floats over pages and hides when the pointer leaves",
            &mut s.chrome.sidebar_overlay,
        );
        divider(ui);
        switch_row(
            ui,
            "Frosted glass",
            "Blur the page behind the title bar and page bar; off makes both solid",
            &mut s.chrome.glass,
        );
    });
    section(ui, "Page position");
    group(ui, |ui| {
        switch_row(
            ui,
            "Page indicator",
            "A page counter in the corner of each pane",
            &mut s.layout.page_indicator,
        );
        divider(ui);
        let on = s.layout.page_indicator;
        row(ui, "Indicator shows", "", |ui| {
            ui.add_enabled_ui(on, |ui| {
                let style = &mut s.layout.page_indicator_style;
                ui.selectable_value(style, PageIndicatorStyle::Percentage, "Percent");
                ui.selectable_value(style, PageIndicatorStyle::PageNumber, "Pages");
            });
        });
        divider(ui);
        label_rows(ui, s);
        divider(ui);
        switch_row(
            ui,
            "Progress bar",
            "A thin line along each pane's bottom edge",
            &mut s.layout.progress_bar,
        );
    });
    section(ui, "Pages");
    group(ui, |ui| page_rows(ui, s, cx));
    section(ui, "Motion");
    group(ui, |ui| {
        switch_row(
            ui,
            "Animations",
            "Everything that moves. Off, a change lands as its end frame.",
            &mut s.motion.enabled,
        );
    });
}

/// The floating document label and how it behaves.
fn label_rows(ui: &mut Ui, s: &mut Settings) {
    let l = &mut s.layout;
    switch_row(
        ui,
        "Floating label",
        "The document's name in each pane's corner while the title bar hides",
        &mut l.floating_label,
    );
    let on = l.floating_label;
    divider(ui);
    row(ui, "Label shows", "", |ui| {
        ui.add_enabled_ui(on, |ui| {
            let style = &mut l.floating_label_style;
            ui.selectable_value(style, FloatingLabelStyle::Chapter, "Chapter");
            ui.selectable_value(style, FloatingLabelStyle::FileName, "File name");
        });
    });
    divider(ui);
    row(
        ui,
        "Always show label",
        "Keep it while the title bar shows, however narrow the side",
        |ui| {
            ui.add_enabled_ui(on, |ui| {
                widgets::switch(ui, &mut l.floating_label_persist, "Always show label");
            });
        },
    );
    divider(ui);
    row(
        ui,
        "Label width limit",
        "How much of the room beside the page the label may take",
        |ui| {
            ui.add_enabled_ui(on && !l.floating_label_persist, |ui| {
                let pct = &mut l.floating_label_max_pct;
                let show = |v: f32| format!("{v:.0}%");
                widgets::stepper(ui, pct, 10.0..=100.0, 10.0, show, "label width");
            });
        },
    );
}

/// How pages sit: the fit they open at, their spacing and shadow.
fn page_rows(ui: &mut Ui, s: &mut Settings, cx: &WindowContext) {
    let l = &mut s.layout;
    row(ui, "Default fit", "How a document opens", |ui| {
        ui.selectable_value(&mut l.startup_fit, StartupFit::Width, "Width");
        ui.selectable_value(&mut l.startup_fit, StartupFit::Page, "Page");
    });
    divider(ui);
    switch_row(
        ui,
        "No gap",
        "Pages in the scrolling views touch",
        &mut l.no_gap,
    );
    divider(ui);
    let hint = if cx.sideways {
        "Pages scrolling sideways have no margin"
    } else {
        "Space beside the pages"
    };
    row(ui, "Page margin", hint, |ui| {
        ui.add_enabled_ui(!cx.sideways, |ui| {
            let show = |v: f32| {
                if v <= 0.0 {
                    "Off".to_owned()
                } else {
                    format!("{v:.0} px")
                }
            };
            widgets::stepper(ui, &mut l.page_margin, 0.0..=64.0, 4.0, show, "page margin");
        });
    });
    divider(ui);
    switch_row(
        ui,
        "Auto scale",
        "Fit the width on switching to single or two pages",
        &mut l.auto_scale,
    );
    divider(ui);
    switch_row(
        ui,
        "Auto resize",
        "Refit when a page of another size is read",
        &mut l.auto_resize,
    );
    divider(ui);
    switch_row(ui, "Page shadow", "", &mut l.page_shadow);
}

fn animations(ui: &mut Ui, s: &mut Settings) {
    section(ui, "Reader motion");
    group(ui, |ui| {
        switch_row(
            ui,
            "Sidebar animation",
            "The sidebar slides in and out, docked or floating",
            &mut s.motion.sidebar_slide,
        );
        divider(ui);
        switch_row(
            ui,
            "Canvas follows window",
            "Re-fit the page while the window is dragged; off, once the drag ends",
            &mut s.motion.canvas_resize,
        );
        divider(ui);
        switch_row(
            ui,
            "Zoom in and out",
            "Ease a zoom to its new scale; off, it lands on the first frame",
            &mut s.motion.zoom,
        );
        divider(ui);
        switch_row(
            ui,
            "Scroll to page",
            "Glide to a page, and with the arrow and page keys; off, land there",
            &mut s.motion.scroll_jumps,
        );
    });
    note(
        ui,
        "Anything switched off still changes, in one frame. The master switch is in Layout.",
    );
}

fn theme_tab(ui: &mut Ui, s: &mut Settings, cx: &WindowContext, out: &mut Commands) {
    section(ui, "Colours");
    group(ui, |ui| {
        row(ui, "Mode, tint, texture and presets", "", |ui| {
            let t = Tokens::get(ui.ctx());
            let text = RichText::new("Open Appearance").color(t.accent);
            if ui.add(Button::new(text).fill(t.accent_soft)).clicked() {
                out.push(Command::CloseSettings);
                out.push(Command::ShowAppearance);
            }
        });
    });
    section(ui, "Split pane appearance");
    if cx.split {
        pane_look(ui, s);
    } else {
        note(ui, "Shown while the workspace is split.");
    }
    pending(
        ui,
        "Paper blend, where the backdrop takes the page's colour, arrives with \
         page rendering (M3).",
    );
}

fn pane_look(ui: &mut Ui, s: &mut Settings) {
    let w = &mut s.workspace.look;
    group(ui, |ui| {
        row(ui, "Outline thickness", "Zero hides the outline", |ui| {
            ui.spacing_mut().slider_width = 140.0;
            ui.add(
                egui::Slider::new(&mut w.outline_width, 0.0..=8.0)
                    .step_by(1.0)
                    .fixed_decimals(0)
                    .suffix(" px"),
            );
        });
        divider(ui);
        row(ui, "Outline colour", "The focused pane's outline", |ui| {
            outline_colours(ui, &mut w.outline_color, &mut w.outline_custom);
        });
        divider(ui);
        row(ui, "Space around and between panes", "", |ui| {
            ui.spacing_mut().slider_width = 140.0;
            ui.add(
                egui::Slider::new(&mut w.gap, 0.0..=24.0)
                    .step_by(1.0)
                    .fixed_decimals(0)
                    .suffix(" px"),
            );
        });
        divider(ui);
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
        divider(ui);
        switch_row(ui, "Pane drop shadow", "", &mut w.shadow);
    });
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

fn workspace(ui: &mut Ui) {
    section(ui, "Library panel");
    pending(
        ui,
        "What a click on a file in the Library panel does arrives with the library (M5).",
    );
    section(ui, "Pane themes");
    pending(
        ui,
        "An independent theme and texture for each pane arrive with page rendering (M3).",
    );
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
            ui.with_layout(Layout::right_to_left(Align::Center), control);
        },
    );
}

fn switch_row(ui: &mut Ui, label: &str, hint: &str, on: &mut bool) {
    row(ui, label, hint, |ui| {
        widgets::switch(ui, on, label);
    });
}

/// Honest copy for settings whose feature has not landed yet.
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
