//! Reads a narrated book aloud: its player, the phrase lit, the bar.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use egui::containers::menu::MenuButton;
use egui::{Align2, Color32, Context, CornerRadius, Frame, Id, Margin, Order, Rect, RichText, Ui};
use mareader_audio::{Clip, Fault, Fetch, Player, RATES, Segment};
use mareader_epub::{Files, Narration, Run, Skips};
use mareader_session::Anchor;
use mareader_settings::ReadAloudSettings;
use mareader_workspace::PaneId;

use crate::documents::{Documents, Reading};
use crate::icons::Icon;
use crate::selection::Pages;
use crate::theme::Tokens;
use crate::widgets;

/// How often the lit phrase is checked while it plays.
const TICK: Duration = Duration::from_millis(50);
/// Space between the bar and the page bar below it.
const LIFT: f32 = 12.0;
/// The bar's controls: one height, one icon size, even gaps.
const SIZE: f32 = 30.0;
const ICON: f32 = 16.0;
const GAP: f32 = 2.0;

/// The phrase being read, to paint in its pane.
#[derive(Clone, Debug, PartialEq)]
pub struct Lit {
    pub start: Anchor,
    pub end: Anchor,
    /// The book's own highlight, if it names one.
    pub fill: Option<[u8; 3]>,
}

/// One book being read aloud in one pane.
struct Session {
    pane: PaneId,
    narration: Arc<Narration>,
    player: Player,
    /// What was last asked to play, a run per segment tag.
    runs: Vec<Run>,
    /// The phrase heard last; where playing last started.
    heard: Option<usize>,
    started: usize,
    /// The page last turned to, so the reader may look away between.
    followed: Option<usize>,
    skip: Skips,
    rate: f32,
    paused: bool,
    finished: bool,
    fault: Option<Fault>,
}

impl Session {
    fn play_from(&mut self, from: usize) {
        self.runs = self.narration.runs(from, self.skip);
        let segments = (self.runs.iter().enumerate())
            .map(|(tag, run)| Segment {
                path: run.path.clone(),
                from: run.from,
                until: run.until,
                tag: tag as u64,
            })
            .collect();
        self.player.set_rate(self.rate);
        self.player.play(segments);
        self.heard = Some(from);
        self.started = from;
        self.followed = None;
        self.finished = false;
        self.paused = false;
        self.fault = None;
    }

    /// The first phrase read at or after `i`.
    fn read_from(&self, i: usize) -> Option<usize> {
        match self.narration.reads(i, self.skip) {
            true => Some(i),
            false => self.narration.next(i, self.skip),
        }
    }

    fn toggle(&mut self) {
        if self.finished {
            if let Some(first) = self.read_from(0) {
                self.play_from(first);
            }
        } else if self.paused {
            self.player.resume();
            self.paused = false;
        } else {
            self.player.pause();
            self.paused = true;
        }
    }

    fn step(&mut self, to: Option<usize>) {
        if let Some(to) = to {
            self.play_from(to);
        }
    }
}

/// Read-aloud for the window: at most one book reads at a time.
#[derive(Default)]
pub struct ReadAloud {
    session: Option<Session>,
}

impl ReadAloud {
    pub fn active(&self) -> bool {
        self.session.is_some()
    }

    /// Pauses the reading, as another sound starts.
    pub fn pause(&mut self) {
        if let Some(s) = self.session.as_mut().filter(|s| !s.paused && !s.finished) {
            s.player.pause();
            s.paused = true;
        }
    }

    /// Starts reading `reading`'s book from `from`, else its page.
    pub fn start(&mut self, reading: &Reading, from: Option<Anchor>, settings: &ReadAloudSettings) {
        let Some(narration) = reading.document.narration() else {
            return;
        };
        let skip = skips(settings);
        let from = from.or_else(|| reading.document.anchor(reading.current));
        let first = from.and_then(|a| narration.from_anchor(a, skip));
        let Some(first) = first.or_else(|| narration.from_anchor(Anchor::default(), skip)) else {
            return;
        };
        let same = (self.session.as_ref()).is_some_and(|s| Arc::ptr_eq(&s.narration, &narration));
        if !same {
            let Some(files) = reading.document.files() else {
                return;
            };
            self.session = Some(Session {
                pane: reading.pane,
                narration,
                player: Player::new(fetch(files)),
                runs: Vec::new(),
                heard: None,
                started: first,
                followed: None,
                skip,
                rate: settings.rate,
                paused: false,
                finished: false,
                fault: None,
            });
        }
        if let Some(s) = self.session.as_mut() {
            s.pane = reading.pane;
            s.skip = skip;
            s.rate = settings.rate;
            s.play_from(first);
        }
    }

    pub fn stop(&mut self) {
        self.session = None;
    }

    /// Each frame: follows what is heard and turns pages to it.
    pub fn tick(&mut self, ctx: &Context, documents: &mut Documents, settings: &ReadAloudSettings) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let reading = documents.reading(s.pane);
        let narration = reading.as_ref().and_then(|r| r.document.narration());
        // Another book in the pane, or none, ends the reading.
        let (Some(reading), true) = (
            reading,
            narration.is_some_and(|n| Arc::ptr_eq(&n, &s.narration)),
        ) else {
            self.session = None;
            return;
        };
        if (settings.rate - s.rate).abs() > f32::EPSILON {
            s.rate = settings.rate;
            s.player.set_rate(s.rate);
        }
        let status = s.player.status();
        let run = |tag: u64| usize::try_from(tag).ok().and_then(|t| s.runs.get(t));
        if let Some((run, time)) = status.at.and_then(|(tag, time)| Some((run(tag)?, time))) {
            s.heard = s.narration.phrase_at(run, time).or(s.heard);
        }
        s.paused = status.paused;
        s.finished = status.done && status.fault.is_none() && !s.runs.is_empty();
        s.fault = status.fault;
        let skip = skips(settings);
        if skip != s.skip {
            s.skip = skip;
            let from = s.heard.unwrap_or(s.started);
            if let Some(from) = s.read_from(from).filter(|_| !s.finished) {
                s.play_from(from);
            }
        }
        let shown = s.heard.and_then(|i| s.narration.phrases.get(i));
        let page = shown.and_then(|p| reading.document.locate(p.start));
        if settings.follow && page.is_some() && page != s.followed {
            s.followed = page;
            if let Some(page) = page.filter(|p| *p != reading.current) {
                documents.go_to(s.pane, page);
            }
        }
        if !s.paused && !s.finished {
            ctx.request_repaint_after(TICK);
        }
    }

    /// The phrase to light in `pane`, if it is reading.
    pub fn lit(&self, pane: PaneId) -> Option<Lit> {
        let s = self.session.as_ref().filter(|s| s.pane == pane)?;
        let phrase = s.narration.phrases.get(s.heard?)?;
        Some(Lit {
            start: phrase.start,
            end: phrase.end,
            fill: s.narration.highlight,
        })
    }

    /// The player, at the foot of its pane.
    pub fn bar(
        &mut self,
        ctx: &Context,
        panes: &[(PaneId, Rect)],
        settings: &mut ReadAloudSettings,
    ) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let Some(rect) = panes.iter().find(|(p, _)| *p == s.pane).map(|(_, r)| *r) else {
            return;
        };
        let t = Tokens::get(ctx);
        let at = egui::pos2(
            rect.center().x,
            rect.bottom() - crate::page_bar::BAR_H - LIFT,
        );
        let frame = Frame::new()
            .fill(t.surface)
            .stroke(egui::Stroke::new(1.0, t.line))
            .corner_radius(t.radius_lg)
            .shadow(ctx.global_style().visuals.window_shadow)
            .inner_margin(Margin::same(4));
        let mut close = false;
        egui::Area::new(Id::new("read-aloud-bar"))
            .fade_in(false)
            .order(Order::Foreground)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(at)
            .constrain_to(rect)
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(SIZE);
                        close = controls(ui, s, settings, &t);
                    });
                });
            });
        if close {
            self.session = None;
        }
    }
}

/// The bar's buttons; true when closed.
fn controls(ui: &mut Ui, s: &mut Session, settings: &mut ReadAloudSettings, t: &Tokens) -> bool {
    ui.spacing_mut().item_spacing = egui::vec2(GAP, 0.0);
    let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(SIZE), egui::Sense::hover());
    let mark = Rect::from_center_size(slot.center(), egui::Vec2::splat(ICON));
    Icon::ReadAloud.paint(ui, mark, t.muted);
    widgets::vrule(ui, SIZE * 0.5);
    let heard = s.heard.unwrap_or(s.started);
    let back = s.narration.previous(heard, s.skip);
    if button(ui, Icon::SkipBack, "Previous phrase", back.is_some()).clicked() {
        s.step(back);
    }
    let (icon, name) = match s.paused || s.finished {
        true => (Icon::Resume, "Play"),
        false => (Icon::Pause, "Pause"),
    };
    if button(ui, icon, name, true).clicked() {
        s.toggle();
    }
    let next = s.narration.next(heard, s.skip);
    if button(ui, Icon::SkipForward, "Next phrase", next.is_some()).clicked() {
        s.step(next);
    }
    if let Some(out) = s.narration.escape(heard, s.skip) {
        let leave = button(ui, Icon::LeaveTable, "Skip the rest of this table", true);
        if leave.clicked() {
            s.step(Some(out));
        }
    }
    widgets::vrule(ui, SIZE * 0.5);
    let speed = RichText::new(rate_label(settings.rate)).size(t.text_small);
    let speed = egui::Button::new(speed).frame_when_inactive(false);
    let speed = speed.min_size(egui::vec2(SIZE + 12.0, SIZE));
    let (resp, _) = MenuButton::from_button(speed).ui(ui, |ui| {
        for rate in RATES {
            let on = (settings.rate - rate).abs() < 0.01;
            if ui.selectable_label(on, rate_label(rate)).clicked() {
                settings.rate = rate;
                ui.close();
            }
        }
    });
    widgets::tip(&resp, "Speed", None);
    let more = egui::Button::image(Icon::More.image(ICON))
        .image_tint_follows_text_color(true)
        .frame_when_inactive(false)
        .min_size(egui::Vec2::splat(SIZE));
    let (resp, _) = MenuButton::from_button(more).ui(ui, |ui| {
        ui.checkbox(&mut settings.skip_notes, "Skip notes");
        ui.checkbox(&mut settings.skip_page_numbers, "Skip page numbers");
        ui.checkbox(&mut settings.follow, "Turn pages to follow");
    });
    widgets::tip(&resp, "Read-aloud options", None);
    let note = match (&s.fault, s.finished) {
        (Some(fault), _) => Some((fault_text(fault), t.danger)),
        (None, true) => Some(("The end".to_owned(), t.muted)),
        (None, false) => None,
    };
    if let Some((text, colour)) = note {
        ui.add_space(4.0);
        ui.label(RichText::new(text).size(t.text_small).color(colour));
        ui.add_space(4.0);
    }
    widgets::vrule(ui, SIZE * 0.5);
    button(ui, Icon::Close, "Stop reading aloud", true).clicked()
}

/// One of the bar's square buttons, all one size.
fn button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> egui::Response {
    let b = egui::Button::image(icon.image(ICON))
        .image_tint_follows_text_color(true)
        .frame_when_inactive(false)
        .min_size(egui::Vec2::splat(SIZE));
    let resp = ui.add_enabled(enabled, b);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    widgets::tip(&resp, label, None);
    resp
}

fn rate_label(rate: f32) -> String {
    let text = format!("{rate:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}×")
}

fn fault_text(fault: &Fault) -> String {
    match fault {
        Fault::NoDevice => "No sound output".to_owned(),
        Fault::Unreadable(_) => "A sound file won't play".to_owned(),
    }
}

/// What `settings` leaves out.
fn skips(settings: &ReadAloudSettings) -> Skips {
    Skips {
        notes: settings.skip_notes,
        page_numbers: settings.skip_page_numbers,
    }
}

/// Reads sound from `files`, keeping the last file for the next run.
pub(crate) fn fetch(files: Files) -> Fetch {
    let last: Mutex<Option<(String, Clip)>> = Mutex::new(None);
    Arc::new(move |path: &str| {
        let mut last = last.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, clip)) = last.as_ref().filter(|(p, _)| p == path) {
            return Some(clip.clone());
        }
        let media = files.media(path)?;
        let clip = Clip {
            data: Arc::new(media.data),
            extension: media.extension,
        };
        *last = Some((path.to_owned(), clip.clone()));
        Some(clip)
    })
}

/// The phrase's words on the pages shown, in the book's colour.
pub fn paint(ui: &Ui, pages: &Pages<'_>, lit: &Lit, t: &Tokens) {
    let fill = fill(lit.fill, t);
    let painter = ui.painter();
    let (start, end) = (lit.start, lit.end);
    let inside = |at: Option<(u32, u32)>| {
        at.is_some_and(|(b, i)| {
            let at = Anchor {
                block: b as usize,
                offset: i as usize,
            };
            start <= at && at < end
        })
    };
    for sheet in pages.sheets.iter().filter(|s| s.shown.is_positive()) {
        let Some(layer) = pages.texts.get(pages.doc, sheet.page) else {
            continue;
        };
        let mut from = None;
        let count = layer.places.len();
        for i in 0..=count {
            let on = i < count && inside(layer.places.get(i).copied().flatten());
            match (on, from) {
                (true, None) => from = Some(i),
                (false, Some(f)) => {
                    for b in layer.text.line_boxes(f..i) {
                        let r = sheet.to_screen(b).intersect(sheet.shown);
                        if r.is_positive() {
                            painter.rect_filled(r, CornerRadius::same(2), fill);
                        }
                    }
                    from = None;
                }
                _ => {}
            }
        }
    }
}

/// The book's highlight, else the accent, toned to the page's paper.
fn fill(book: Option<[u8; 3]>, t: &Tokens) -> Color32 {
    let paper = t.book.paper.map(|c| c.clamp(0.0, 1.0) * 255.0);
    let light = paper.iter().sum::<f32>() > 3.0 * 128.0;
    let accent = [t.accent.r(), t.accent.g(), t.accent.b()];
    // A light page takes the book's colour as is; a dark one a tint.
    let (colour, share) = match (book, light) {
        (Some(c), true) => (c, 1.0),
        (Some(c), false) => (c, 0.22),
        (None, true) => (accent, 0.30),
        (None, false) => (accent, 0.26),
    };
    let target = std::array::from_fn(|i| {
        let p = paper.get(i).copied().unwrap_or(255.0);
        p + (f32::from(colour.get(i).copied().unwrap_or(255)) - p) * share
    });
    over(paper, target)
}

/// The darkest fill that turns `paper` into `target`, so ink stays dark.
fn over(paper: [f32; 3], target: [f32; 3]) -> Color32 {
    let pairs = || paper.into_iter().zip(target);
    let darker = |(p, t): (f32, f32)| (p - t).max(0.0) / p.max(1.0);
    let lighter = |(p, t): (f32, f32)| (t - p).max(0.0) / (255.0 - p).max(1.0);
    // Darkening sets the strength; lightening only when nothing darkens.
    let dark = pairs().map(darker).fold(0.0_f32, f32::max);
    let need = match dark > 0.01 {
        true => dark,
        false => pairs().map(lighter).fold(0.0_f32, f32::max),
    };
    let alpha = need.clamp(0.12, 1.0);
    let channel = |(p, t): (f32, f32)| ((t - (1.0 - alpha) * p) / alpha).clamp(0.0, 255.0);
    let [r, g, b] = {
        let mut out = [0u8; 3];
        for (slot, pair) in out.iter_mut().zip(pairs()) {
            *slot = channel(pair).round() as u8;
        }
        out
    };
    Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `fill` over `under` shows, as egui blends it.
    fn shown(fill: Color32, under: f32) -> [f32; 3] {
        let a = f32::from(fill.a()) / 255.0;
        let c = [fill.r(), fill.g(), fill.b()];
        c.map(|c| f32::from(c) + under * (1.0 - a))
    }

    #[test]
    fn the_fill_turns_paper_to_the_book_colour_and_keeps_ink_dark() {
        let fill = over([250.0; 3], [255.0, 255.0, 187.0]);
        let paper = shown(fill, 250.0);
        assert!(
            (paper[2] - 187.0).abs() < 2.0 && paper[0] > 248.0,
            "{paper:?}"
        );
        let ink = shown(fill, 0.0);
        assert!(ink.iter().all(|c| *c < 90.0), "{ink:?}");
        // On a dark page the tint lightens, and is faint.
        let fill = over([30.0; 3], [60.0, 60.0, 40.0]);
        assert!(fill.a() < 64, "{fill:?}");
        assert!((shown(fill, 30.0)[0] - 60.0).abs() < 2.0);
    }

    #[test]
    fn speeds_read_short() {
        assert_eq!(rate_label(1.0), "1×");
        assert_eq!(rate_label(1.25), "1.25×");
        assert_eq!(rate_label(0.75), "0.75×");
        assert_eq!(rate_label(1.5), "1.5×");
    }
}
