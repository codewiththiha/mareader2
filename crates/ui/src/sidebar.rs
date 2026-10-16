//! One sidebar rail per window, docked or floating; it serves the focused pane.

use egui::emath::easing;
use egui::{
    Align, Area, Button, Context, CornerRadius, Id, Layout, Order, Rect, RichText, Sense, Stroke,
    Ui, UiBuilder, Vec2, pos2, vec2,
};
use serde::{Deserialize, Serialize};

use crate::command::{Command, Commands, WindowCommand};
use crate::icons::Icon;
use crate::motion;
use crate::platform::Platform;
use crate::theme::{self, TITLE_BAR_H, Tokens};
use crate::widgets;

pub const WIDTH: f32 = 288.0;
/// The rail's slide, docked or floating, in seconds.
pub const SLIDE: f32 = 0.3;
/// How long a floating rail waits for the pointer to come back.
pub const HOVER_GRACE: f64 = 0.25;
/// The floating rail's reveal strip at the window's left edge.
pub const EDGE: f32 = 6.0;
const SWITCHER_H: f32 = 48.0;

/// Draws a panel; false shows the placeholder. The flag: reveal the
/// reading place.
pub type PanelBody<'a> = dyn FnMut(&mut Ui, Panel, bool, &mut Commands) -> bool + 'a;

/// What the rail shows; the focused pane supplies the content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Panel {
    #[default]
    Thumbnails,
    Outline,
    Library,
    /// Joins the switcher once a language pack is built (M6).
    Dictionary,
}

impl Panel {
    /// The switcher's buttons, in order.
    pub const SWITCHER: [Panel; 3] = [Panel::Thumbnails, Panel::Outline, Panel::Library];

    pub fn label(self) -> &'static str {
        match self {
            Panel::Thumbnails => "Thumbnails",
            Panel::Outline => "Outline",
            Panel::Library => "Library",
            Panel::Dictionary => "Dictionary",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Panel::Thumbnails,
            Panel::Outline,
            Panel::Library,
            Panel::Dictionary,
        ]
        .into_iter()
        .find(|p| p.label().eq_ignore_ascii_case(s))
    }

    fn icon(self) -> Icon {
        match self {
            Panel::Thumbnails => Icon::Thumbnails,
            Panel::Outline => Icon::Outline,
            Panel::Library => Icon::Library,
            Panel::Dictionary => Icon::Dictionary,
        }
    }

    /// Placeholder copy until the panel's milestone lands.
    fn pending(self) -> &'static str {
        match self {
            Panel::Thumbnails => "Open a PDF to browse its pages here.",
            Panel::Outline => "Open a book to see its chapters here.",
            Panel::Library => "Your library as a tree, with the open tabs, arrives in M5.",
            Panel::Dictionary => "Look words up in your language packs (M6).",
        }
    }
}

/// What the rail needs from the app this frame.
pub struct RailContext {
    pub overlay: bool,
    /// Settings → Animations → Sidebar slide; off shows end frames only.
    pub slide: bool,
    pub platform: Platform,
}

#[derive(Clone, Debug, Default)]
pub struct Sidebar {
    open: bool,
    /// The panel shown, kept while closed so a reopen returns to it.
    panel: Panel,
    /// Floating only: the pointer has reached the rail since it opened.
    armed: bool,
    left_at: Option<f64>,
    /// The panel should bring the reading place into view.
    recentre: bool,
    /// Last frame's rail edge left of its open place; `None` when not drawn.
    offset: Option<f32>,
}

impl Sidebar {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The rail's shift last frame, `-WIDTH` to 0; `None` if not drawn.
    pub fn offset(&self) -> Option<f32> {
        self.offset
    }

    pub fn panel(&self) -> Panel {
        self.panel
    }

    /// Opens on `panel`, or on the last one shown.
    pub fn open(&mut self, panel: Option<Panel>) {
        if let Some(p) = panel {
            self.recentre |= p != self.panel;
            self.panel = p;
        }
        if !self.open {
            self.recentre = true;
            self.open = true;
            self.armed = false;
            self.left_at = None;
        }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open(None);
        }
    }

    /// How much of the rail shows, 0 to `WIDTH`, eased.
    fn slide(&self, ctx: &Context, cx: &RailContext, id: &str) -> f32 {
        let secs = if cx.slide {
            motion::secs(ctx, SLIDE)
        } else {
            0.0
        };
        let t = ctx.animate_bool_with_time_and_easing(
            Id::new(id),
            self.open,
            secs,
            easing::cubic_in_out,
        );
        (WIDTH * t).round()
    }

    /// Takes a sliding strip from the left of `ui`; returns its width.
    pub fn show_docked(
        &mut self,
        ui: &mut Ui,
        cx: &RailContext,
        body: &mut PanelBody<'_>,
        out: &mut Commands,
    ) -> f32 {
        let width = self.slide(ui.ctx(), cx, "sidebar-slide");
        self.offset = (width >= 1.0).then_some(width - WIDTH);
        if width < 1.0 {
            return 0.0;
        }
        egui::Panel::left("sidebar")
            .exact_size(width)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let strip = ui.max_rect();
                // The rail keeps its width and slides out under the strip.
                let rail = Rect::from_min_size(
                    pos2(strip.right() - WIDTH, strip.top()),
                    vec2(WIDTH, strip.height()),
                );
                let mut child = ui.new_child(UiBuilder::new().max_rect(rail).id_salt("rail"));
                child.shrink_clip_rect(strip);
                self.contents(&mut child, cx, body, out);
                let t = Tokens::get(ui.ctx());
                ui.painter().vline(
                    strip.right() - 0.5,
                    strip.y_range(),
                    Stroke::new(1.0, t.line),
                );
            });
        width
    }

    /// Slides over the panes from the left edge; returns the width it covers.
    pub fn show_floating(
        &mut self,
        ctx: &Context,
        cx: &RailContext,
        body: &mut PanelBody<'_>,
        out: &mut Commands,
    ) -> f32 {
        let screen = ctx.content_rect();
        let home = Rect::from_min_size(screen.min, vec2(WIDTH, screen.height()));
        self.track_hover(ctx, home, screen, out);
        let width = self.slide(ctx, cx, "sidebar-float-slide");
        self.offset = (width >= 1.0).then_some(width - WIDTH);
        if width < 1.0 {
            return 0.0;
        }
        let rail = home.translate(vec2(width - WIDTH, 0.0));
        // egui fades new areas in by default; this rail only slides.
        Area::new(Id::new("sidebar-float"))
            .fade_in(false)
            .order(Order::Foreground)
            .fixed_pos(rail.min)
            .constrain(false)
            .interactable(self.open)
            .show(ctx, |ui| {
                ui.set_min_size(rail.size());
                let t = Tokens::get(ctx);
                let shadow = egui::Shadow {
                    offset: [0, 8],
                    blur: 32,
                    spread: 0,
                    color: t.shadow,
                };
                ui.painter().add(shadow.as_shape(rail, CornerRadius::ZERO));
                let mut child = ui.new_child(UiBuilder::new().max_rect(rail).id_salt("rail"));
                child.set_clip_rect(rail);
                self.contents(&mut child, cx, body, out);
                ui.painter()
                    .vline(rail.right() - 0.5, rail.y_range(), Stroke::new(1.0, t.line));
            });
        width
    }

    /// The edge strip opens a closed rail; leaving an open one closes it.
    fn track_hover(&mut self, ctx: &Context, rail: Rect, screen: Rect, out: &mut Commands) {
        let pointer = ctx.pointer_hover_pos();
        let down = ctx.input(|i| i.pointer.any_down());
        if !self.open {
            let edge = Rect::from_min_size(screen.min, vec2(EDGE, screen.height()));
            if !down && pointer.is_some_and(|p| edge.contains(p)) {
                out.push(Command::OpenSidebar(None));
            }
            return;
        }
        // Keyboard focus in it holds it, as the pointer does.
        if crate::focus::within(ctx, rail) {
            self.left_at = None;
            return;
        }
        if pointer.is_some_and(|p| rail.contains(p)) {
            self.armed = true;
            self.left_at = None;
            return;
        }
        // Opened from a key or button: wait until the pointer has visited.
        if !self.armed || down || egui::Popup::is_any_open(ctx) {
            return;
        }
        let now = ctx.input(|i| i.time);
        let waited = now - *self.left_at.get_or_insert(now);
        if waited >= HOVER_GRACE {
            out.push(Command::CloseSidebar);
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(HOVER_GRACE - waited));
        }
    }

    /// Header, panel and switcher, laid out in `ui.max_rect()`.
    fn contents(
        &mut self,
        ui: &mut Ui,
        cx: &RailContext,
        body: &mut PanelBody<'_>,
        out: &mut Commands,
    ) {
        let t = Tokens::get(ui.ctx());
        let rail = ui.max_rect();
        ui.painter()
            .rect_filled(rail, CornerRadius::ZERO, t.surface);
        let (header, rest) = rail.split_top_bottom_at_y(rail.top() + TITLE_BAR_H);
        let (panel, switcher) = rest.split_top_bottom_at_y(rest.bottom() - SWITCHER_H);
        self.header(ui, header, cx, out);
        let recentre = std::mem::take(&mut self.recentre);
        let builder = UiBuilder::new()
            .max_rect(panel)
            .layout(Layout::top_down(Align::Min));
        let drew = ui
            .scope_builder(builder, |ui| {
                ui.shrink_clip_rect(panel);
                body(ui, self.panel, recentre, out)
            })
            .inner;
        if !drew {
            // Kept for the document still to come.
            self.recentre |= recentre;
            placeholder(ui, panel, self.panel);
        }
        ui.painter()
            .hline(switcher.x_range(), switcher.top(), Stroke::new(1.0, t.line));
        self.switcher(ui, switcher, out);
    }

    fn header(&self, ui: &mut Ui, rect: Rect, cx: &RailContext, out: &mut Commands) {
        // Registered before the buttons so they win the pointer.
        let drag = ui.interact(rect, Id::new("sidebar-drag"), Sense::CLICK | Sense::DRAG);
        if drag.double_clicked() {
            out.push(Command::Window(WindowCommand::ToggleMaximize));
        } else if drag.drag_started() {
            out.push(Command::Window(WindowCommand::StartDrag));
        }
        let mut row = rect;
        row.min.x += cx.platform.leading_inset();
        row.max.x -= 8.0;
        let builder = UiBuilder::new()
            .max_rect(row)
            .layout(Layout::left_to_right(Align::Center));
        ui.scope_builder(builder, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let buttons = [
                (Icon::SidebarClose, "Close sidebar", Command::CloseSidebar),
                (Icon::Search, "Search", Command::ShowSearch),
                (Icon::Settings, "Settings", Command::ShowSettings),
            ];
            for (icon, label, command) in buttons {
                if widgets::icon_button(ui, icon, label, false).clicked() {
                    out.push(command);
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let docked = !cx.overlay;
                if widgets::icon_button(ui, Icon::Pin, "Dock sidebar", docked).clicked() {
                    out.push(Command::SetSidebarOverlay(docked));
                }
            });
        });
    }

    fn switcher(&mut self, ui: &mut Ui, rect: Rect, out: &mut Commands) {
        let slot = rect.width() / Panel::SWITCHER.len() as f32;
        for (i, panel) in Panel::SWITCHER.into_iter().enumerate() {
            let centre = pos2(rect.left() + slot * (i as f32 + 0.5), rect.center().y);
            let at = Rect::from_center_size(centre, vec2(56.0, 36.0));
            let on = panel == self.panel;
            let button = Button::image(panel.icon().image(16.0))
                .image_tint_follows_text_color(true)
                .frame_when_inactive(false)
                .selected(on)
                .min_size(Vec2::new(56.0, 36.0));
            let r = ui.put(at, button);
            let label = panel.label();
            r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
            widgets::tip(&r, label, None);
            if r.clicked() && on {
                self.recentre = true;
            } else if r.clicked() {
                out.push(Command::OpenSidebar(Some(panel)));
            }
        }
    }
}

fn placeholder(ui: &mut Ui, rect: Rect, panel: Panel) {
    let t = Tokens::get(ui.ctx());
    let area = rect.shrink2(vec2(28.0, 0.0));
    let builder = UiBuilder::new()
        .max_rect(area)
        .layout(Layout::top_down(Align::Center));
    ui.scope_builder(builder, |ui| {
        ui.add_space(area.height() * 0.3);
        ui.add(panel.icon().image(24.0).tint(t.muted));
        ui.add_space(6.0);
        ui.label(
            RichText::new(panel.label())
                .font(theme::medium(14.0))
                .color(t.ink),
        );
        ui.label(
            RichText::new(panel.pending())
                .size(t.text_small)
                .color(t.muted),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::{Panel, Sidebar};

    #[test]
    fn a_reopen_returns_to_the_last_panel() {
        let mut s = Sidebar::default();
        s.open(Some(Panel::Outline));
        s.toggle();
        assert!(!s.is_open());
        s.toggle();
        assert!(s.is_open());
        assert_eq!(s.panel(), Panel::Outline);
    }

    #[test]
    fn opening_or_switching_panels_asks_for_the_reading_place() {
        let mut s = Sidebar::default();
        s.open(Some(Panel::Outline));
        assert!(std::mem::take(&mut s.recentre));
        s.open(Some(Panel::Outline));
        assert!(!s.recentre);
        s.open(Some(Panel::Thumbnails));
        assert!(s.recentre);
    }

    #[test]
    fn panels_parse_from_their_labels() {
        for p in Panel::SWITCHER {
            assert_eq!(Panel::parse(&p.label().to_lowercase()), Some(p));
        }
        assert_eq!(Panel::parse("nope"), None);
    }
}
