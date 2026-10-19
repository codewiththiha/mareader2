//! A corner card per long import or walk; quick runs only toast.

use std::f32::consts::TAU;

use egui::{
    Align, Align2, Area, Context, FontId, Frame, Id, Layout, Margin, Order, Pos2, Rect, RichText,
    Shape, Stroke, Ui, pos2, vec2,
};

use super::Action;
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets;

/// Seconds a run waits before its card shows.
pub const DELAY: f64 = 0.4;
const CARD_W: f32 = 248.0;
const RING: f32 = 30.0;
const INSET: f32 = 16.0;

pub type RunId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Walking a folder: no total yet, so the ring spins.
    Scanning,
    /// Measuring what a walk found.
    Reading,
    Copying,
    /// The AI planning shelves; the count runs across both its stages.
    Shelving,
    /// The AI filing books on them.
    Filing,
}

/// The worker's word on a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beat {
    pub run: RunId,
    pub phase: Phase,
    /// Files finished so far.
    pub done: usize,
    pub total: usize,
    /// The file or folder at hand.
    pub name: String,
}

impl Beat {
    /// Whether a beat at `done` of `total` moves the ring a whole percent.
    pub fn worth(done: usize, total: usize) -> bool {
        let percent = |n: usize| n * 100 / total.max(1);
        done == 0 || percent(done) != percent(done.saturating_sub(1))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub id: RunId,
    /// What was asked for: a file's name, "3 files", a folder's name.
    pub label: String,
    pub phase: Phase,
    pub done: usize,
    pub total: usize,
    pub name: String,
    /// When the card first could show, in egui seconds.
    started: Option<f64>,
}

impl Run {
    /// How far along, once there's a total.
    pub fn fraction(&self) -> Option<f32> {
        match (self.phase, self.total) {
            (Phase::Scanning, _) | (_, 0) => None,
            _ => Some((self.done as f32 / self.total as f32).clamp(0.0, 1.0)),
        }
    }

    pub fn headline(&self) -> String {
        let at = (self.done + 1).min(self.total);
        match (self.phase, self.total) {
            (Phase::Scanning, _) => "Scanning…".to_owned(),
            (Phase::Reading, 0) => "Reading…".to_owned(),
            (Phase::Copying, 0) => "Importing…".to_owned(),
            (Phase::Reading, n) => format!("Reading {at} of {n}"),
            (Phase::Copying, n) => format!("Importing {at} of {n}"),
            (Phase::Shelving, 0) => "Organizing…".to_owned(),
            (Phase::Shelving, _) => "Planning shelves…".to_owned(),
            (Phase::Filing, _) => "Filing books…".to_owned(),
        }
    }

    /// An AI run, which the card can stop.
    pub fn stoppable(&self) -> bool {
        matches!(self.phase, Phase::Shelving | Phase::Filing)
    }

    /// The file at hand, else what was asked for.
    pub fn detail(&self) -> &str {
        match self.name.is_empty() {
            true => &self.label,
            false => &self.name,
        }
    }
}

/// The runs in flight, oldest first.
#[derive(Debug, Default)]
pub struct Runs {
    list: Vec<Run>,
    next: RunId,
}

impl Runs {
    pub fn start(&mut self, label: impl Into<String>, phase: Phase) -> RunId {
        self.next += 1;
        self.list.push(Run {
            id: self.next,
            label: label.into(),
            phase,
            done: 0,
            total: 0,
            name: String::new(),
            started: None,
        });
        self.next
    }

    pub fn beat(&mut self, beat: Beat) {
        if let Some(run) = self.list.iter_mut().find(|r| r.id == beat.run) {
            run.phase = beat.phase;
            run.done = beat.done;
            run.total = beat.total;
            run.name = beat.name;
        }
    }

    /// The run moved on to `phase`; its count starts again.
    pub fn phase(&mut self, id: Option<RunId>, phase: Phase) {
        if let Some(run) = self.list.iter_mut().find(|r| Some(r.id) == id) {
            run.phase = phase;
            run.done = 0;
            run.total = 0;
        }
    }

    /// The run ended; its toast says how.
    pub fn finish(&mut self, id: Option<RunId>) {
        self.list.retain(|r| Some(r.id) != id);
    }

    pub fn list(&self) -> &[Run] {
        &self.list
    }

    /// A folder walk and an import, shown from the first frame, for shots.
    pub fn demo(&mut self) {
        self.start("Books", Phase::Scanning);
        let run = self.start("12 files", Phase::Copying);
        self.beat(Beat {
            run,
            phase: Phase::Copying,
            done: 4,
            total: 12,
            name: "Middlemarch.epub".into(),
        });
        self.show_now();
    }

    /// Every card shows from the first frame, for shots.
    pub fn show_now(&mut self) {
        for run in &mut self.list {
            run.started = Some(f64::NEG_INFINITY);
        }
    }
}

/// The cards, stacked up from `stage`'s bottom-left; `lift` clears a page bar.
pub fn show(ctx: &Context, runs: &mut Runs, stage: Rect, lift: f32, out: &mut Commands) {
    if runs.list.is_empty() {
        return;
    }
    let now = ctx.input(|i| i.time);
    let t = Tokens::get(ctx);
    let mut bottom = stage.bottom() - INSET - lift;
    let mut waiting = false;
    for run in &mut runs.list {
        let started = *run.started.get_or_insert(now);
        if now - started < DELAY {
            waiting = true;
            continue;
        }
        let pos = pos2(stage.left() + INSET, bottom);
        let area = Area::new(Id::new(("import-run", run.id)))
            .order(Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(pos)
            .interactable(run.stoppable());
        let shown = area.show(ctx, |ui| card(ui, run, now, &t, out));
        bottom = shown.response.rect.top() - 8.0;
    }
    // The ring turns, and a waiting card is due.
    let soon = std::time::Duration::from_secs_f64(DELAY / 2.0);
    match waiting {
        true => ctx.request_repaint_after(soon),
        false => ctx.request_repaint(),
    }
}

fn card(ui: &mut Ui, run: &Run, now: f64, t: &Tokens, out: &mut Commands) {
    let frame = Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius_lg)
        .shadow(egui::epaint::Shadow {
            offset: [0, 4],
            blur: 16,
            spread: 0,
            color: t.shadow,
        })
        .inner_margin(Margin::symmetric(12, 10));
    frame.show(ui, |ui| {
        ui.set_width(CARD_W);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(RING, RING), egui::Sense::hover());
            ring(ui, rect.center(), run.fraction(), now, t);
            ui.add_space(4.0);
            let lines = |ui: &mut Ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let head = RichText::new(run.headline()).font(theme::medium(t.text_small));
                    ui.add(egui::Label::new(head.color(t.ink)).truncate());
                    let detail = RichText::new(run.detail()).size(t.text_small - 1.0);
                    ui.add(egui::Label::new(detail.color(t.muted)).truncate());
                });
            };
            if !run.stoppable() {
                return lines(ui);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let stop = widgets::small_icon_button(ui, Icon::Close, "Stop organizing");
                if stop.clicked() {
                    out.push(Command::Library(Action::StopOrganizing));
                }
                lines(ui);
            });
        });
    });
}

/// A track and its arc: filled to `fraction`, else a turning quarter.
fn ring(ui: &Ui, centre: Pos2, fraction: Option<f32>, now: f64, t: &Tokens) {
    let painter = ui.painter();
    let r = RING / 2.0 - 2.0;
    painter.circle_stroke(centre, r, Stroke::new(3.0, t.line));
    let (from, sweep) = match fraction {
        Some(f) => (-TAU / 4.0, f * TAU),
        None => (((now * 1.4).fract() as f32) * TAU, TAU / 4.0),
    };
    if sweep > 0.0 {
        let steps = ((sweep / TAU) * 48.0).ceil().max(2.0) as usize;
        let points = (0..=steps)
            .map(|i| from + sweep * i as f32 / steps as f32)
            .map(|a| centre + vec2(a.cos(), a.sin()) * r)
            .collect();
        painter.add(Shape::line(points, Stroke::new(3.0, t.accent)));
    }
    if let Some(f) = fraction {
        let percent = format!("{}", (f * 100.0).round() as u32);
        let font = FontId::proportional(9.5);
        painter.text(centre, Align2::CENTER_CENTER, percent, font, t.ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(run: RunId, phase: Phase, done: usize, total: usize) -> Beat {
        Beat {
            run,
            phase,
            done,
            total,
            name: "ferns.epub".into(),
        }
    }

    #[test]
    fn a_scan_spins_then_reading_fills_the_ring() {
        let mut runs = Runs::default();
        let id = runs.start("Books", Phase::Scanning);
        let run = |r: &Runs| r.list()[0].clone();
        assert_eq!(run(&runs).fraction(), None);
        assert_eq!(run(&runs).headline(), "Scanning…");
        assert_eq!(run(&runs).detail(), "Books");
        runs.beat(beat(id, Phase::Reading, 3, 12));
        assert_eq!(run(&runs).fraction(), Some(0.25));
        assert_eq!(run(&runs).headline(), "Reading 4 of 12");
        assert_eq!(run(&runs).detail(), "ferns.epub");
        runs.beat(beat(id, Phase::Copying, 12, 12));
        assert_eq!(run(&runs).headline(), "Importing 12 of 12");
        runs.finish(Some(id));
        assert!(runs.list().is_empty());
    }

    #[test]
    fn beats_for_other_runs_and_unknown_ends_change_nothing() {
        let mut runs = Runs::default();
        let a = runs.start("a.pdf", Phase::Copying);
        let b = runs.start("2 files", Phase::Copying);
        runs.beat(beat(b + 1, Phase::Copying, 1, 2));
        runs.finish(None);
        assert_eq!(runs.list().len(), 2);
        assert_eq!(runs.list()[0].headline(), "Importing…");
        runs.finish(Some(a));
        assert_eq!(runs.list()[0].id, b);
    }

    #[test]
    fn an_ai_run_fills_one_ring_across_its_stages_and_can_stop() {
        let mut runs = Runs::default();
        let id = runs.start("5 books · Fast", Phase::Shelving);
        let run = |r: &Runs| r.list().first().cloned().expect("a run");
        assert_eq!(run(&runs).headline(), "Organizing…");
        assert!(run(&runs).stoppable());
        runs.beat(Beat {
            run: id,
            phase: Phase::Filing,
            done: 3,
            total: 4,
            name: String::new(),
        });
        assert_eq!(run(&runs).fraction(), Some(0.75));
        assert_eq!(run(&runs).headline(), "Filing books…");
        assert_eq!(run(&runs).detail(), "5 books · Fast");
        let import = runs.start("a.pdf", Phase::Copying);
        assert!(!runs.list().iter().any(|r| r.id == import && r.stoppable()));
    }

    #[test]
    fn beats_are_sent_once_a_percent() {
        let sent = (0..1000).filter(|&d| Beat::worth(d, 1000)).count();
        assert_eq!(sent, 100);
        assert!(
            (0..7).all(|d| Beat::worth(d, 7)),
            "small runs beat every file"
        );
    }
}
