//! Toasts: a stack in the bottom-right corner, newest in front.

mod card;
mod stack;

use egui::{Context, Rect, pos2};

use crate::command::{Command, Commands};
use crate::motion;

/// How long a note or a result stays up.
pub const SHOW_FOR: f64 = 3.5;
/// A failure stays longer: it may need reading twice.
pub const ERROR_FOR: f64 = 6.0;
/// At least this long is left once a held stack is let go.
pub const HOVER_GRACE: f64 = 1.5;
/// How often a running time line is redrawn.
const LINE_STEP: f32 = 1.0 / 30.0;
pub const FADE: f32 = 0.2;
/// At most this many are kept; the oldest goes without a fade.
const KEEP: usize = 12;
/// Clear of the window's edges.
const INSET: f32 = 16.0;
const WIDTH: f32 = 360.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// A result or a note: a check mark.
    Info,
    /// Something failed: an alert in the danger colour.
    Error,
    /// It worked, but not wholly: an alert in the accent colour.
    Warning,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub message: String,
    pub tone: Tone,
    /// A button, Undo unless said, and what it sends; it lasts longer.
    pub undo: Option<Box<Command>>,
    pub label: &'static str,
}

impl Toast {
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(message, Tone::Info)
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::new(message, Tone::Error)
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(message, Tone::Warning)
    }

    fn new(message: impl Into<String>, tone: Tone) -> Self {
        Self {
            message: message.into(),
            tone,
            undo: None,
            label: "Undo",
        }
    }

    /// With an Undo button that sends `command`.
    pub fn undo(self, command: Command) -> Self {
        Self {
            undo: Some(Box::new(command)),
            ..self
        }
    }

    /// With a button `label` that sends `command`, as Resume.
    pub fn action(self, label: &'static str, command: Command) -> Self {
        Self {
            undo: Some(Box::new(command)),
            label,
            ..self
        }
    }

    fn lasts(&self) -> f64 {
        match (&self.undo, self.tone) {
            (Some(_), _) => crate::study::UNDO_FOR,
            (None, Tone::Error) => ERROR_FOR,
            (None, _) => SHOW_FOR,
        }
    }
}

/// How a toast leaves.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Exit {
    Fade,
    /// Thrown sideways from where it was let go.
    Swipe(f32),
}

/// A toast on screen; its clock starts on its first frame.
struct Live {
    id: u64,
    toast: Toast,
    /// How often the same words came while it was up.
    times: u32,
    born: Option<f64>,
    left: Option<f64>,
    gone: Option<(f64, Exit)>,
    /// How far it has been dragged sideways.
    drag: f32,
    dragging: bool,
    /// Where the drag left it last frame, so its hit area moves with it.
    offset: f32,
    /// Drawn in the opened list, where the clocks stand still.
    listed: bool,
}

impl Live {
    fn going(&self) -> bool {
        self.gone.is_some()
    }

    /// The share of its time still to run, for its line.
    fn remaining(&self) -> f32 {
        let left = self.left.unwrap_or_else(|| self.toast.lasts());
        (left / self.toast.lasts()).clamp(0.0, 1.0) as f32
    }
}

/// Where open menus sit; modals are not menus.
fn open_menus(ctx: &Context) -> Vec<Rect> {
    if !egui::Popup::is_any_open(ctx) {
        return Vec::new();
    }
    ctx.memory(|m| {
        let modal = m.top_modal_layer();
        let shown = |l: &egui::LayerId| {
            l.order == egui::Order::Foreground && Some(*l) != modal && m.areas().is_visible(l)
        };
        let layers = m.layer_ids().filter(shown);
        layers.filter_map(|l| m.area_rect(l.id)).collect()
    })
}

/// The stack, oldest first.
#[derive(Default)]
pub struct Toasts {
    live: Vec<Live>,
    next: u64,
    /// Opened into a list by a click.
    open: bool,
    /// The pointer was over the stack last frame.
    hovered: bool,
    last: Option<f64>,
}

impl Toasts {
    /// Puts `toast` in front; the same words again count instead of stacking.
    pub fn push(&mut self, toast: Toast) {
        let same = |l: &Live| {
            !l.going()
                && l.toast.undo.is_none()
                && toast.undo.is_none()
                && l.toast.tone == toast.tone
                && l.toast.message == toast.message
        };
        if let Some(at) = self.live.iter().position(same) {
            let mut again = self.live.remove(at);
            again.times += 1;
            again.left = None;
            self.live.push(again);
            return;
        }
        self.next += 1;
        self.live.push(Live {
            id: self.next,
            toast,
            times: 1,
            born: None,
            left: None,
            gone: None,
            drag: 0.0,
            dragging: false,
            offset: 0.0,
            listed: false,
        });
        let over = self.live.len().saturating_sub(KEEP);
        self.live.drain(..over);
    }

    /// Closes the toast `id`.
    pub fn close(&mut self, id: u64, now: f64) {
        let found = self.live.iter_mut().find(|l| l.id == id && !l.going());
        if let Some(live) = found {
            live.gone = Some((now, Exit::Fade));
        }
    }

    /// Closes them all.
    pub fn clear(&mut self, now: f64) {
        for live in self.live.iter_mut().filter(|l| !l.going()) {
            live.gone = Some((now, Exit::Fade));
        }
        self.open = false;
    }

    /// The newest toast still up.
    pub fn current(&self) -> Option<&Toast> {
        self.shown().next()
    }

    /// The toasts still up, newest first.
    pub fn shown(&self) -> impl Iterator<Item = &Toast> {
        let up = self.live.iter().rev().filter(|l| !l.going());
        up.map(|l| &l.toast)
    }

    /// Opened into its list.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the stack into its list, when there is one.
    pub fn set_open(&mut self, open: bool) {
        self.open = open && self.live.iter().any(|l| !l.going());
    }

    /// Read, opened or dragged: the clocks stop.
    fn held(&self) -> bool {
        self.hovered || self.open || self.live.iter().any(|l| l.dragging)
    }

    /// Runs the clocks unless held; ended toasts fade out.
    fn tick(&mut self, now: f64, fade: f64) {
        let held = self.held();
        let step = self.last.map_or(0.0, |last| (now - last).max(0.0));
        self.last = Some(now);
        for live in self.live.iter_mut().filter(|l| !l.going()) {
            live.born.get_or_insert(now);
            let Some(left) = live.left.as_mut() else {
                live.left = Some(live.toast.lasts());
                continue;
            };
            match held {
                true => *left = left.max(HOVER_GRACE),
                false => *left -= step,
            }
            if *left <= 0.0 {
                live.gone = Some((now, Exit::Fade));
            }
        }
        self.live
            .retain(|l| l.gone.is_none_or(|(at, _)| fade > 0.0 && now - at < fade));
        if self.live.iter().all(Live::going) {
            self.open = false;
        }
    }

    /// Draws the stack in `area`'s bottom-right corner, `lift` above its foot.
    pub fn show(&mut self, ctx: &Context, area: Rect, lift: f32, out: &mut Commands) {
        let now = ctx.input(|i| i.time);
        let fade = f64::from(motion::secs(ctx, FADE));
        self.tick(now, fade);
        let opened = ctx.animate_bool_with_time(egui::Id::new("toasts-open"), self.open, FADE);
        if self.live.is_empty() {
            self.hovered = false;
            return;
        }
        let width = WIDTH.min(area.width() - 2.0 * INSET).max(200.0);
        let corner = pos2(area.right() - INSET, area.bottom() - INSET - lift);
        // A drag from the page passes over the toasts.
        let ours = self.live.iter().any(|l| l.dragging);
        let aside = ctx.dragged_id().is_some() && !ours;
        let place = stack::Place {
            corner,
            width,
            room: area.height() - 2.0 * INSET - lift,
            now,
            fade,
            aside,
            menus: open_menus(ctx),
        };
        self.hovered = match self.open {
            true => stack::open(ctx, self, &place, opened, out),
            false => stack::closed(ctx, self, &place, out),
        };
        self.repaint(ctx, now, fade);
    }

    /// Every frame while something moves; often enough for a running line.
    fn repaint(&self, ctx: &Context, now: f64, fade: f64) {
        let moving = self.live.iter().any(|l| {
            let entering = l.born.is_none_or(|b| now - b < fade);
            l.going() || l.dragging || l.drag != 0.0 || entering
        });
        if moving {
            ctx.request_repaint();
            return;
        }
        if !self.held() && self.live.iter().any(|l| l.left.is_some()) {
            ctx.request_repaint_after_secs(LINE_STEP);
        }
    }
}
