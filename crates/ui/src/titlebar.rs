//! The auto-hiding title bar, kept open by the pin or its popups.

use egui::text::{LayoutJob, TextWrapping};
use egui::{Align, Context, Id, Layout, Order, Rect, Sense, UiBuilder, pos2, vec2};
use mareader_appearance::{Appearance, Preset};
use mareader_workspace::SplitAxis;

use crate::backdrop::Backdrop;
use crate::command::{Command, Commands, Independence, WindowCommand};
use crate::gpu::{Gpu, Look, Reach};
use crate::icons::Icon;
use crate::keymap;
use crate::layout::ViewMode;
use crate::motion;
use crate::platform::Platform;
use crate::theme::{self, TITLE_BAR_H, Tokens};
use crate::view_menu::{self, MenuContext, Tool};
use crate::widgets::{self, CaptionStyle};
use crate::zoom::ZoomState;

/// How long the bar stays after the pointer leaves the band.
pub const HIDE_GRACE: f64 = 0.4;
pub const FADE: f32 = 0.2;
pub const APPEARANCE_POPOVER: &str = "appearance-popover";
/// A toolbar button and the gap after it.
const TOOL_W: f32 = 40.0;
/// Width the title keeps before tools fold into the menu.
const TITLE_ROOM: f32 = 160.0;

/// What the bar needs from the app this frame.
pub struct BarContext<'a> {
    pub title: &'a str,
    /// The library page is up: its button shows pressed.
    pub library: bool,
    pub platform: Platform,
    /// The sidebar's width: the band starts after it.
    pub left_inset: f32,
    /// The bar's sidebar toggle; the rail's own header closes it.
    pub sidebar_toggle: bool,
    pub maximized: bool,
    /// No captions or traffic lights in full screen.
    pub fullscreen: bool,
    /// The row starts after the platform's leading room (macOS's lights).
    pub gutter: bool,
    /// The active pane's look, which the popover edits.
    pub appearance: &'a Appearance,
    /// The per-pane modes; offered only while the workspace is split.
    pub independence: Option<Independence>,
    /// Every preset, built-ins first, and the user's own among them.
    pub presets: &'a [Preset],
    pub user_presets: &'a [Preset],
    /// Lets the popover draw preset cards as real pages.
    pub gpu: Option<&'a Gpu>,
    /// The active document's zoom, for the menu's zoom row.
    pub zoom: Option<ZoomState>,
    pub mode: Option<ViewMode>,
    /// What shows behind the bar: glass, a tint or a solid fill.
    pub backdrop: &'a Backdrop,
    /// The ink dial, offered while the active pane shows a book.
    pub ink: Option<f32>,
}

/// The tools that may fold, rightmost first: the leftmost folds first.
fn tools() -> [Tool; 4] {
    let tool = |icon, label, command| Tool {
        icon,
        label,
        command,
    };
    [
        tool(Icon::Settings, "Settings", Command::ShowSettings),
        tool(Icon::Search, "Search", Command::ShowSearch),
        tool(
            Icon::SplitDown,
            "Split down",
            Command::Split(SplitAxis::Vertical),
        ),
        tool(
            Icon::SplitRight,
            "Split right",
            Command::Split(SplitAxis::Horizontal),
        ),
    ]
}

/// How many folding tools fit beside the fixed controls and the title.
fn fitting(bar: &BarContext<'_>, row_width: f32) -> usize {
    let lead = if bar.sidebar_toggle { 2.0 } else { 1.0 } * TOOL_W;
    let captions = match captions(bar) {
        Some(CaptionStyle::Windows) => 3.0 * 46.0 + 4.0,
        Some(CaptionStyle::Gnome) => 3.0 * 24.0 + 2.0 * 8.0 + 4.0,
        None => 0.0,
    };
    // Pin, Appearance and the menu never fold.
    let fixed = 3.0 * TOOL_W + 8.0;
    let room = row_width - lead - captions - fixed - TITLE_ROOM;
    ((room / TOOL_W).floor().max(0.0) as usize).min(tools().len())
}

fn captions(bar: &BarContext<'_>) -> Option<CaptionStyle> {
    bar.platform.captions().filter(|_| !bar.fullscreen)
}

#[derive(Clone, Debug, Default)]
pub struct TitleBar {
    pub pinned: bool,
    /// Never shows, hover or pin (distraction-free reading).
    pub chromeless: bool,
    /// Kept shown while set, as the find bar does, once shown.
    pub hold: bool,
    shown: bool,
    left_at: Option<f64>,
    opacity: f32,
}

impl TitleBar {
    pub fn new(pinned: bool) -> Self {
        Self {
            pinned,
            ..Self::default()
        }
    }

    /// Shows the bar as if hovered; it hides after the usual grace.
    pub fn reveal(&mut self) {
        self.shown = true;
    }

    /// How much of the bar showed last frame, 0 to 1; native buttons follow it.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    pub fn visible(&self) -> bool {
        !self.chromeless && (self.pinned || self.shown)
    }

    /// Where the hover band ends; nothing interactive should sit above it.
    pub fn band_bottom(&self, ctx: &Context) -> f32 {
        let top = ctx.content_rect().top();
        if self.chromeless {
            top
        } else {
            top + TITLE_BAR_H
        }
    }

    fn update_hover(&mut self, ctx: &Context, band: Rect) {
        let now = ctx.input(|i| i.time);
        let inside = ctx.pointer_hover_pos().is_some_and(|p| band.contains(p));
        let held = self.hold
            || egui::Popup::is_any_open(ctx)
            || (self.shown && ctx.input(|i| i.pointer.any_down()));
        if inside {
            self.shown = true;
            self.left_at = None;
        } else if self.shown && !held {
            let waited = now - *self.left_at.get_or_insert(now);
            if waited >= HIDE_GRACE {
                self.shown = false;
                self.left_at = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(HIDE_GRACE - waited));
            }
        }
    }

    pub fn show(&mut self, ctx: &Context, bar: &BarContext<'_>, out: &mut Commands) {
        self.opacity = 0.0;
        if self.chromeless {
            return;
        }
        let screen = ctx.content_rect();
        let band = Rect::from_min_max(
            pos2(screen.left() + bar.left_inset, screen.top()),
            pos2(screen.right(), screen.top() + TITLE_BAR_H),
        );
        self.update_hover(ctx, band);
        let visible = self.visible();
        let fade =
            ctx.animate_bool_with_time(Id::new("titlebar"), visible, motion::secs(ctx, FADE));
        self.opacity = fade;
        if fade <= 0.0 {
            return;
        }
        // Our own fade below; egui's would stack on it.
        egui::Area::new(Id::new("titlebar"))
            .fade_in(false)
            .order(Order::Foreground)
            .fixed_pos(band.min)
            .constrain(false)
            .interactable(visible)
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                ui.set_min_size(band.size());
                let look = Look {
                    slot: 0,
                    reach: Reach::Below,
                    opacity: fade,
                };
                bar.backdrop.paint(ui, band, look);
                // Registered before the buttons so they win the pointer.
                let drag = ui.interact(band, Id::new("titlebar-drag"), Sense::click_and_drag());
                if visible && drag.double_clicked() {
                    out.push(Command::Window(WindowCommand::ToggleMaximize));
                } else if visible && drag.drag_started() {
                    out.push(Command::Window(WindowCommand::StartDrag));
                }
                let live = visible;
                let mut row = band;
                // Over a rail or in overlay mode, the bar hosts no lights.
                row.min.x += if bar.gutter {
                    bar.platform.leading_inset()
                } else {
                    8.0
                };
                if captions(bar) != Some(CaptionStyle::Windows) {
                    row.max.x -= 8.0;
                }
                let builder = UiBuilder::new()
                    .max_rect(row)
                    .layout(Layout::left_to_right(Align::Center));
                ui.scope_builder(builder, |ui| self.contents(ui, bar, live, out));
            });
    }

    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        bar: &BarContext<'_>,
        live: bool,
        out: &mut Commands,
    ) {
        let pinned = self.pinned;
        let fit = fitting(bar, ui.max_rect().width());
        let ((lead_end, lead), (trail_start, trail)) =
            egui::Sides::new().height(TITLE_BAR_H).spacing(8.0).show(
                ui,
                |ui| leading(ui, bar.sidebar_toggle, bar.library),
                |ui| trailing(ui, bar, pinned, fit),
            );
        for c in lead.into_iter().chain(trail).filter(|_| live) {
            if let Command::SetTitlebarPinned(p) = c {
                self.pinned = p;
            }
            out.push(c);
        }
        title(ui, bar.title, lead_end, trail_start);
    }
}

fn clicked(r: egui::Response, c: Command) -> Option<Command> {
    r.clicked().then_some(c)
}

/// Returns its right edge and what was clicked.
fn leading(ui: &mut egui::Ui, sidebar_toggle: bool, library: bool) -> (f32, Commands) {
    ui.spacing_mut().item_spacing.x = 4.0;
    let mut out = Commands::new();
    if sidebar_toggle {
        let r = widgets::icon_button(ui, Icon::Sidebar, "Toggle sidebar", false);
        out.extend(clicked(r, Command::ToggleSidebar));
    }
    let r = widgets::icon_button(ui, Icon::Library, "Library", library);
    out.extend(clicked(r, Command::ShowLibrary));
    (ui.min_rect().right(), out)
}

/// Laid out right to left. Returns its left edge and what was clicked.
fn trailing(ui: &mut egui::Ui, bar: &BarContext<'_>, pinned: bool, fit: usize) -> (f32, Commands) {
    ui.spacing_mut().item_spacing.x = 4.0;
    let mut out = Commands::new();
    if let Some(style) = captions(bar) {
        let (w, gap) = match style {
            CaptionStyle::Windows => (3.0 * 46.0, 0.0),
            CaptionStyle::Gnome => (3.0 * 24.0 + 2.0 * 8.0, 8.0),
        };
        let size = vec2(w, ui.available_height());
        let caption =
            ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = gap;
                widgets::captions(ui, style, bar.maximized)
            });
        out.extend(caption.inner.map(Command::Window));
        ui.add_space(4.0);
    }
    let tools = tools();
    let (shown, folded) = tools.split_at(fit);
    let menu = MenuContext {
        folded,
        fullscreen: bar.fullscreen,
        platform: bar.platform,
        zoom: bar.zoom,
        mode: bar.mode,
    };
    view_menu::button(ui, &menu, &mut out);
    let r = widgets::icon_button(ui, Icon::Pin, "Pin title bar", pinned);
    out.extend(clicked(r, Command::SetTitlebarPinned(!pinned)));
    appearance_button(ui, bar, &mut out);
    for tool in shown {
        let keys = keymap::hint(bar.platform, &tool.command);
        let r = widgets::icon_button_keyed(ui, tool.icon, tool.label, false, keys.as_deref());
        out.extend(clicked(r, tool.command.clone()));
    }
    (ui.min_rect().left(), out)
}

/// Toggles the Appearance popover, which edits the look live.
fn appearance_button(ui: &mut egui::Ui, bar: &BarContext<'_>, out: &mut Commands) {
    let id = Id::new(APPEARANCE_POPOVER);
    let open = egui::Popup::is_id_open(ui.ctx(), id);
    let r = widgets::icon_button(ui, Icon::Appearance, "Appearance", open);
    egui::Popup::from_toggle_button_response(&r)
        .id(id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .show(|ui| {
            let cx = crate::appearance_menu::MenuContext {
                current: bar.appearance,
                independence: bar.independence,
                presets: bar.presets,
                user: bar.user_presets,
                gpu: bar.gpu,
                ink: bar.ink,
            };
            crate::appearance_menu::show(ui, &cx, out);
        });
    if !open && !egui::Popup::is_id_open(ui.ctx(), id) {
        crate::appearance_menu::forget_draft(ui.ctx());
    }
}

/// Centred on the window when it fits, else in the gap, truncated.
fn title(ui: &egui::Ui, text: &str, lead_end: f32, trail_start: f32) {
    const PAD: f32 = 12.0;
    let gap = (trail_start - lead_end - 2.0 * PAD).max(0.0);
    if gap < 48.0 {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let mut job = LayoutJob::simple_singleline(text.to_owned(), theme::medium(13.0), t.ink);
    job.wrap = TextWrapping::truncate_at_width(gap);
    let galley = ui.painter().layout_job(job);
    let rect = ui.max_rect();
    let w = galley.size().x;
    let centred = ui.ctx().content_rect().center().x - w / 2.0;
    let x = centred.clamp(lead_end + PAD, (trail_start - PAD - w).max(lead_end + PAD));
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(pos2(x, y), galley, t.ink);
}
