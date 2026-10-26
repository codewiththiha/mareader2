//! The compact player: steps, play, a seek track with time left, and stop.

use egui::containers::menu::MenuButton;
use egui::{
    Color32, CornerRadius, Id, LayerId, Order, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui,
    UiBuilder, pos2, vec2,
};
use mareader_audio::RATES;
use mareader_settings::{Highlight, ReadAloudSettings};

use super::rate_label;
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::motion;
use crate::theme::{self, Tokens};
use crate::widgets;

/// The pill's height, and its buttons'.
pub(super) const HEIGHT: f32 = 44.0;
const BUTTON: f32 = 32.0;
const PLAY: f32 = 36.0;
const ICON: f32 = 16.0;
const PAD: f32 = 6.0;
const TRACK_MOST: f32 = 168.0;
const TRACK_LEAST: f32 = 56.0;
/// The track shrinks to this before speed goes.
const TRACK_EASY: f32 = 110.0;
/// Chapter gaps closer than this would make a comb.
const TICK_ROOM: f32 = 12.0;
const TIME_W: f32 = 78.0;
const SPEED_W: f32 = 44.0;

/// What the player shows this frame.
pub(super) struct View<'a> {
    pub playing: bool,
    /// The voice is there to play with.
    pub ready: bool,
    pub loading: bool,
    pub back: bool,
    pub next: bool,
    /// What a step moves by: "sentence" or "phrase".
    pub unit: &'static str,
    /// Inside a table a narrator can leave.
    pub escape: bool,
    /// How much is read, 0 to 1, and seconds likely left and in all.
    pub at: Option<f32>,
    pub left: Option<f64>,
    pub total: Option<f64>,
    pub seek: bool,
    /// Where chapters start, as shares of the track.
    pub ticks: Vec<f32>,
    /// A fault, or the end; shown in place of the track.
    pub note: Option<(String, bool)>,
    /// Where a share of the track lands, as a page and chapter.
    pub hint: &'a dyn Fn(f32) -> Option<String>,
    /// Narrated books skip their notes.
    pub narrated: bool,
}

/// What the player was asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Act {
    Back,
    Toggle,
    Next,
    Escape,
    /// To this share of the track.
    Seek(f32),
    Stop,
}

/// Which parts fit: the track always, the time and speed when room allows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Fit {
    pub track: f32,
    pub time: bool,
    pub speed: bool,
}

impl Fit {
    /// The parts that fit `room`, widest first.
    pub fn new(room: f32, escape: bool) -> Self {
        // Speed goes before the track is short; the time, only after.
        let tries = [(true, TRACK_EASY), (false, TRACK_LEAST)];
        let mut fit = Self {
            track: TRACK_LEAST,
            time: false,
            speed: false,
        };
        for (speed, least) in tries {
            fit = Self {
                track: 0.0,
                time: true,
                speed,
            };
            let track = room - fit.width(escape);
            fit.track = track.clamp(least, TRACK_MOST);
            if track >= least {
                return fit;
            }
        }
        fit.time = false;
        fit.track =
            (room - Self { track: 0.0, ..fit }.width(escape)).clamp(TRACK_LEAST, TRACK_MOST);
        fit
    }

    fn extra(&self) -> f32 {
        let time = if self.time { TIME_W + 8.0 } else { 0.0 };
        time + if self.speed { SPEED_W } else { 0.0 }
    }

    pub fn width(&self, escape: bool) -> f32 {
        // Pads, steps, play, more and stop, the rule and spacing.
        let fixed = 2.0 * PAD + 2.0 * BUTTON + PLAY + 2.0 * BUTTON + 22.0;
        fixed + self.track + self.extra() + if escape { BUTTON } else { 0.0 }
    }
}

/// The player in `rect`; what it was asked, if anything.
pub(super) fn show(
    ui: &mut Ui,
    rect: Rect,
    (view, fit): (&View<'_>, Fit),
    settings: &mut ReadAloudSettings,
    out: &mut Commands,
) -> Option<Act> {
    let t = Tokens::get(ui.ctx());
    let round = CornerRadius::same((HEIGHT / 2.0) as u8);
    let shadow = ui.ctx().global_style().visuals.window_shadow;
    ui.painter().add(shadow.as_shape(rect, round));
    ui.painter().rect_filled(rect, round, t.surface);
    let edge = Stroke::new(1.0, t.line);
    ui.painter()
        .rect_stroke(rect, round, edge, egui::StrokeKind::Inside);
    let mut act = None;
    let mut x = rect.left() + PAD;
    let mut slot = |w: f32| {
        let r = Rect::from_min_size(pos2(x, rect.top()), vec2(w, HEIGHT));
        x += w;
        r
    };
    let unit = view.unit;
    let back = slot(BUTTON);
    if icon_button(
        ui,
        back,
        Icon::SkipBack,
        &format!("Previous {unit}"),
        view.back,
    ) {
        act = Some(Act::Back);
    }
    if play_button(ui, slot(PLAY), view) {
        act = Some(Act::Toggle);
    }
    let next = slot(BUTTON);
    if icon_button(
        ui,
        next,
        Icon::SkipForward,
        &format!("Next {unit}"),
        view.next,
    ) {
        act = Some(Act::Next);
    }
    if view.escape {
        let r = slot(BUTTON);
        if icon_button(ui, r, Icon::LeaveTable, "Skip the rest of this table", true) {
            act = Some(Act::Escape);
        }
    }
    slot(10.0);
    let time_w = if fit.time { TIME_W + 8.0 } else { 0.0 };
    let middle = slot(fit.track + time_w);
    match &view.note {
        Some((text, bad)) => note(ui, middle, text, *bad),
        None => {
            let (track, time) = middle.split_left_right_at_x(middle.left() + fit.track);
            if let Some(share) = seek_track(ui, track, view) {
                act = Some(Act::Seek(share));
            }
            if fit.time {
                time_label(ui, time.with_min_x(time.left() + 8.0), view);
            }
        }
    }
    slot(4.0);
    if fit.speed {
        speed(ui, slot(SPEED_W), settings);
    }
    options(ui, slot(BUTTON), settings, view.narrated, out);
    let rule = slot(8.0);
    let half = vec2(0.0, BUTTON * 0.28);
    let stroke = Stroke::new(1.0, t.line);
    (ui.painter()).line_segment([rule.center() - half, rule.center() + half], stroke);
    if icon_button(ui, slot(BUTTON), Icon::Close, "Stop reading aloud", true) {
        act = Some(Act::Stop);
    }
    act
}

/// A round, frameless icon button; true when clicked.
fn icon_button(ui: &mut Ui, slot: Rect, icon: Icon, label: &str, enabled: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let rect = Rect::from_center_size(slot.center(), egui::Vec2::splat(BUTTON));
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let r = ui.interact(rect, ui.id().with(("read-aloud", label)), sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let time = motion::secs(ui.ctx(), 0.12);
    let hot = ui
        .ctx()
        .animate_bool_with_time(r.id, enabled && r.hovered(), time);
    let fill = if r.is_pointer_button_down_on() {
        t.pressed
    } else {
        t.hover
    };
    (ui.painter()).circle_filled(rect.center(), BUTTON / 2.0, fill.gamma_multiply(hot));
    let ink = match enabled {
        true => t.ink,
        false => t.muted.gamma_multiply(0.5),
    };
    let mark = Rect::from_center_size(rect.center(), egui::Vec2::splat(ICON));
    icon.paint(ui, mark, ink);
    widgets::tip(&r, label, None);
    enabled && r.clicked()
}

/// The accent circle whose triangle folds into two bars; true when clicked.
fn play_button(ui: &mut Ui, slot: Rect, view: &View<'_>) -> bool {
    let t = Tokens::get(ui.ctx());
    let label = if view.playing { "Pause" } else { "Play" };
    let rect = Rect::from_center_size(slot.center(), egui::Vec2::splat(PLAY));
    let sense = if view.ready {
        Sense::click()
    } else {
        Sense::hover()
    };
    let r = ui.interact(rect, ui.id().with("read-aloud-play"), sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, view.ready, label));
    let ctx = ui.ctx().clone();
    let quick = motion::secs(&ctx, 0.1);
    let hot = ctx.animate_bool_with_time(r.id.with("hot"), r.hovered(), quick);
    let down = ctx.animate_bool_with_time(r.id.with("down"), r.is_pointer_button_down_on(), quick);
    let morph =
        ctx.animate_bool_with_time(r.id.with("morph"), view.playing, motion::secs(&ctx, 0.18));
    let scale = 1.0 + 0.06 * hot - 0.08 * down;
    let radius = PLAY / 2.0 * scale;
    let fill = if view.ready {
        t.accent
    } else {
        t.accent.gamma_multiply(0.45)
    };
    ui.painter().circle_filled(rect.center(), radius, fill);
    let ink = theme::ink_on(t.accent);
    for quad in morph_quads(rect.center(), 13.0 * scale, ease(morph)) {
        ui.painter()
            .add(Shape::convex_polygon(quad.to_vec(), ink, Stroke::NONE));
    }
    if view.loading {
        spinner(ui, rect.center(), radius + 3.5, t.accent);
    }
    widgets::tip(&r, label, None);
    view.ready && r.clicked()
}

/// Play's triangle in two halves at 0, pause's two bars at 1.
fn morph_quads(c: Pos2, size: f32, k: f32) -> [[Pos2; 4]; 2] {
    let h = size;
    let (top, bottom) = (c.y - h / 2.0, c.y + h / 2.0);
    // The triangle sits a little right, so it looks centred.
    let (l, r) = (c.x - h * 0.42, c.x + h * 0.58);
    let m = (l + r) / 2.0;
    let play = [
        [
            pos2(l, top),
            pos2(m, top + h / 4.0),
            pos2(m, bottom - h / 4.0),
            pos2(l, bottom),
        ],
        [
            pos2(m, top + h / 4.0),
            pos2(r, c.y),
            pos2(r, c.y),
            pos2(m, bottom - h / 4.0),
        ],
    ];
    let (bl, br, bar) = (c.x - h * 0.4, c.x + h * 0.4, h * 0.3);
    let pause = [
        [
            pos2(bl, top),
            pos2(bl + bar, top),
            pos2(bl + bar, bottom),
            pos2(bl, bottom),
        ],
        [
            pos2(br - bar, top),
            pos2(br, top),
            pos2(br, bottom),
            pos2(br - bar, bottom),
        ],
    ];
    let mut out = play;
    for (o, p) in out.iter_mut().flatten().zip(pause.iter().flatten()) {
        *o = o.lerp(*p, k);
    }
    out
}

/// Ease in and out, so the fold starts and lands softly.
fn ease(k: f32) -> f32 {
    k * k * (3.0 - 2.0 * k)
}

/// A quarter arc running round `c`, while the voice loads.
fn spinner(ui: &Ui, c: Pos2, radius: f32, colour: Color32) {
    let time = ui.input(|i| i.time) as f32;
    let start = time * 5.0;
    let points: Vec<Pos2> = (0..=20)
        .map(|i| start + i as f32 / 20.0 * std::f32::consts::FRAC_PI_2 * 1.4)
        .map(|a| c + radius * vec2(a.cos(), a.sin()))
        .collect();
    ui.painter()
        .add(Shape::line(points, Stroke::new(2.0, colour)));
    ui.ctx().request_repaint();
}

/// The track read along; the share to read from once let go or clicked.
fn seek_track(ui: &mut Ui, rect: Rect, view: &View<'_>) -> Option<f32> {
    let t = Tokens::get(ui.ctx());
    let ctx = ui.ctx().clone();
    let id = ui.id().with("read-aloud-seek");
    let sense = if view.seek {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let r = ui.interact(rect, id, sense);
    let rail = rect.shrink2(vec2(6.0, 0.0));
    let share_at = |x: f32| ((x - rail.left()) / rail.width().max(1.0)).clamp(0.0, 1.0);
    let pointer = r
        .hover_pos()
        .or(r.interact_pointer_pos())
        .map(|p| share_at(p.x));
    let dragging = r.dragged() && view.seek;
    let read = view.at.unwrap_or(0.0);
    let shown = match (dragging, pointer) {
        (true, Some(p)) => p,
        _ => ctx.animate_value_with_time(id.with("at"), read, motion::secs(&ctx, 0.25)),
    };
    let value = f64::from(shown) * 100.0;
    r.widget_info(|| {
        let mut info =
            egui::WidgetInfo::labeled(egui::WidgetType::Slider, view.seek, "Reading position");
        info.value = Some(value);
        info
    });
    let quick = motion::secs(&ctx, 0.12);
    let hot = ctx.animate_bool_with_time(
        id.with("hot"),
        view.seek && (r.hovered() || dragging),
        quick,
    );
    let thick = 4.0 + 2.0 * hot;
    let bar = Rect::from_center_size(rail.center(), vec2(rail.width(), thick));
    let round = CornerRadius::same(3);
    ui.painter().rect_filled(bar, round, t.track);
    // Hovered ahead or behind, the stretch to the pointer shows softly.
    if let (false, true, Some(p)) = (dragging, view.seek && r.hovered(), pointer) {
        let (a, b) = (shown.min(p), shown.max(p));
        let ghost = bar.with_min_x(bar.left() + bar.width() * a);
        let ghost = ghost.with_max_x(bar.left() + bar.width() * b);
        ui.painter()
            .rect_filled(ghost, round, t.accent.gamma_multiply(0.35));
    }
    let filled = bar.with_max_x(bar.left() + bar.width() * shown);
    ui.painter().rect_filled(filled, round, t.accent);
    // Chapters part the track, as gaps in it, while they stand apart.
    let roomy = bar.width() / (view.ticks.len() + 1) as f32 >= TICK_ROOM;
    let gap = Stroke::new(1.5, t.surface);
    for tick in (view.ticks.iter()).filter(|k| roomy && **k > 0.01 && **k < 0.99) {
        let x = bar.left() + bar.width() * tick;
        ui.painter()
            .line_segment([pos2(x, bar.top()), pos2(x, bar.bottom())], gap);
    }
    let knob = pos2(bar.left() + bar.width() * shown, bar.center().y);
    let size = 2.5 + 3.5 * hot;
    ui.painter()
        .circle(knob, size, t.accent, Stroke::new(2.0, t.surface));
    if let (true, Some(share)) = (view.seek && (r.hovered() || dragging), pointer) {
        bubble(ui, rect, share, view);
    }
    let let_go = r.drag_stopped() || r.clicked();
    let share = r.interact_pointer_pos().map(|p| share_at(p.x));
    share.filter(|_| let_go && view.seek)
}

/// Above the track at `share`: where it lands and the time left from there.
fn bubble(ui: &Ui, track: Rect, share: f32, view: &View<'_>) {
    let t = Tokens::get(ui.ctx());
    let mut text = (view.hint)(share).unwrap_or_default();
    if let Some(left) = view.total.map(|all| all * f64::from(1.0 - share)) {
        let sep = if text.is_empty() { "" } else { " · " };
        text = format!("{text}{sep}{}", left_text(left));
    }
    if text.is_empty() {
        return;
    }
    let painter = ui
        .ctx()
        .layer_painter(LayerId::new(Order::Tooltip, Id::new("read-aloud-bubble")));
    let galley = painter.layout_no_wrap(text, theme::medium(t.text_small), t.ink);
    let size = galley.size() + vec2(16.0, 8.0);
    let x = track.left() + 6.0 + (track.width() - 12.0) * share;
    let mut at = Rect::from_center_size(pos2(x, track.top() - 8.0 - size.y / 2.0), size);
    let screen = ui.ctx().content_rect().shrink(4.0);
    at = at.translate(vec2(
        (screen.left() - at.left()).max(0.0) + (screen.right() - at.right()).min(0.0),
        0.0,
    ));
    let round = CornerRadius::same(t.radius as u8);
    painter.rect_filled(at, round, t.surface);
    (painter).rect_stroke(
        at,
        round,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    painter.galley(at.center() - galley.size() / 2.0, galley, t.ink);
}

/// The time left, or the share read; a click swaps them.
fn time_label(ui: &mut Ui, rect: Rect, view: &View<'_>) {
    let t = Tokens::get(ui.ctx());
    let id = Id::new("read-aloud-time-shows-share");
    let share = ui
        .data_mut(|d| d.get_persisted::<bool>(id))
        .unwrap_or(false);
    let text = match (share, view.at, view.left) {
        _ if view.loading => "Loading voice…".to_owned(),
        (true, Some(at), _) => format!("{:.0}% read", at * 100.0),
        (false, _, Some(left)) => left_text(left),
        _ => "—".to_owned(),
    };
    let label = egui::Label::new(RichText::new(text).size(t.text_small).color(t.muted))
        .sense(Sense::click())
        .selectable(false);
    let r = ui.put(rect, label);
    let tip = match share {
        true => "Show the time left",
        false => "Show how much is read",
    };
    widgets::tip(&r, tip, None);
    if r.clicked() {
        ui.data_mut(|d| d.insert_persisted(id, !share));
    }
}

/// A fault or the end, where the track was.
fn note(ui: &mut Ui, rect: Rect, text: &str, bad: bool) {
    let t = Tokens::get(ui.ctx());
    let colour = if bad { t.danger } else { t.muted };
    let label = egui::Label::new(RichText::new(text).size(t.text_small).color(colour)).truncate();
    let r = ui.put(rect.shrink2(vec2(4.0, 0.0)), label);
    widgets::tip(&r, text, None);
}

/// "12 min left", rounded so it doesn't flicker.
pub(super) fn left_text(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match minutes {
        0 if seconds < 1.0 => "No time left".to_owned(),
        0 => "Under a minute left".to_owned(),
        1..=59 => format!("{minutes} min left"),
        _ if minutes.is_multiple_of(60) => format!("{} h left", minutes / 60),
        _ => format!("{} h {} min left", minutes / 60, minutes % 60),
    }
}

/// The speed picker: the rates offered, the one in use shown.
fn speed(ui: &mut Ui, slot: Rect, settings: &mut ReadAloudSettings) {
    let t = Tokens::get(ui.ctx());
    let text = RichText::new(rate_label(settings.rate)).size(t.text_small);
    let button = egui::Button::new(text)
        .frame_when_inactive(false)
        .corner_radius(CornerRadius::same(255))
        .min_size(vec2(SPEED_W - 4.0, 26.0));
    let place = UiBuilder::new().max_rect(slot.shrink2(vec2(2.0, (HEIGHT - 26.0) / 2.0)));
    ui.scope_builder(place, |ui| {
        let (r, _) = MenuButton::from_button(button).ui(ui, |ui| {
            for rate in RATES {
                let on = (settings.rate - rate).abs() < 0.01;
                if ui.selectable_label(on, rate_label(rate)).clicked() {
                    settings.rate = rate;
                    ui.close();
                }
            }
        });
        widgets::tip(&r, "Speed", None);
    });
}

/// The options behind More: what is skipped and lit, then voice settings.
fn options(ui: &mut Ui, slot: Rect, s: &mut ReadAloudSettings, narrated: bool, out: &mut Commands) {
    let more = egui::Button::image(Icon::More.image(ICON))
        .image_tint_follows_text_color(true)
        .frame_when_inactive(false)
        .corner_radius(CornerRadius::same(255))
        .min_size(egui::Vec2::splat(BUTTON));
    let place = UiBuilder::new().max_rect(Rect::from_center_size(
        slot.center(),
        egui::Vec2::splat(BUTTON),
    ));
    ui.scope_builder(place, |ui| {
        let (r, _) = MenuButton::from_button(more).ui(ui, |ui| {
            // Menu rows fill what they're given, so the menu sets its width.
            ui.set_width(220.0);
            if narrated {
                ui.checkbox(&mut s.skip_notes, "Skip notes");
            }
            ui.checkbox(&mut s.skip_page_numbers, "Skip page numbers");
            ui.checkbox(&mut s.follow, "Turn pages to follow");
            if !narrated {
                ui.separator();
                ui.label(RichText::new("Highlight").size(Tokens::get(ui.ctx()).text_small));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut s.highlight, Highlight::Sentence, "Sentence");
                    ui.selectable_value(&mut s.highlight, Highlight::Phrases, "Phrases");
                    ui.selectable_value(&mut s.highlight, Highlight::Words, "Words");
                });
            }
            ui.separator();
            let row = widgets::menu_row(ui, Icon::Settings, "Voice settings…", None, false);
            if row.clicked() {
                out.push(Command::OpenSettings(
                    crate::settings_window::Tab::ReadAloud,
                ));
                ui.close();
            }
        });
        widgets::tip(&r, "Read-aloud options", None);
    });
}

/// Where it pops up: centred over the foot of `pane`, rising as it shows.
pub(super) fn place(pane: Rect, width: f32, shown: f32) -> Rect {
    let bottom = pane.bottom() - crate::page_bar::BAR_H - 12.0 + (1.0 - shown) * 10.0;
    let width = width.min(pane.width() - 16.0).max(0.0);
    Rect::from_min_size(
        pos2(pane.center().x - width / 2.0, bottom - HEIGHT),
        vec2(width, HEIGHT),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_left_reads_plainly_and_rounds() {
        assert_eq!(left_text(0.2), "No time left");
        assert_eq!(left_text(20.0), "Under a minute left");
        assert_eq!(left_text(100.0), "2 min left");
        assert_eq!(left_text(3600.0), "1 h left");
        assert_eq!(left_text(3600.0 + 65.0 * 60.0), "2 h 5 min left");
    }

    #[test]
    fn a_narrow_pane_drops_speed_then_time_then_shortens_the_track() {
        let wide = Fit::new(800.0, false);
        assert_eq!(
            (wide.track, wide.time, wide.speed),
            (TRACK_MOST, true, true)
        );
        let roomy = wide.width(false);
        let squeezed = Fit::new(roomy - 40.0, false);
        assert!(squeezed.speed && squeezed.time && squeezed.track < TRACK_MOST);
        // Speed goes before the track is short.
        let narrow = Fit::new(roomy - (TRACK_MOST - TRACK_EASY) - 10.0, false);
        assert!(!narrow.speed && narrow.time && narrow.track > TRACK_EASY);
        // Without the time, the track takes its room back.
        let none = Fit {
            track: 0.0,
            time: false,
            speed: false,
        };
        let bare = Fit::new(none.width(false) + TRACK_LEAST + TIME_W + 7.0, false);
        assert!(!bare.time && bare.track > TRACK_LEAST + TIME_W);
        let tiny = Fit::new(200.0, true);
        assert_eq!(
            (tiny.time, tiny.speed, tiny.track),
            (false, false, TRACK_LEAST)
        );
        assert!(Fit::new(roomy, true).width(true) <= roomy + 1.0);
    }

    #[test]
    fn the_play_triangle_folds_into_two_bars() {
        let c = pos2(0.0, 0.0);
        let play = morph_quads(c, 10.0, 0.0);
        let pause = morph_quads(c, 10.0, 1.0);
        // Play's tip meets at one point; pause's bars stand apart.
        assert_eq!(play[1][1], play[1][2]);
        assert!(pause[0][1].x < pause[1][0].x);
        assert!(ease(0.5) == 0.5 && ease(0.0) == 0.0 && ease(1.0) == 1.0);
    }
}
