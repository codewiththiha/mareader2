//! Reads a narrated book aloud: its player, the phrase lit, the bar.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

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
            .inner_margin(Margin::same(6));
        let mut close = false;
        egui::Area::new(Id::new("read-aloud-bar"))
            .fade_in(false)
            .order(Order::Foreground)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(at)
            .constrain_to(rect)
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.horizontal(|ui| close = controls(ui, s, settings, &t));
                });
            });
        if close {
            self.session = None;
        }
    }
}

/// The bar's buttons; true when closed.
fn controls(ui: &mut Ui, s: &mut Session, settings: &mut ReadAloudSettings, t: &Tokens) -> bool {
    ui.add(Icon::ReadAloud.image(16.0).tint(t.muted));
    let heard = s.heard.unwrap_or(s.started);
    let back = s.narration.previous(heard, s.skip);
    let back_button = ui.add_enabled_ui(back.is_some(), |ui| {
        widgets::small_icon_button(ui, Icon::SkipBack, "Previous phrase")
    });
    if back_button.inner.clicked() {
        s.step(back);
    }
    let (icon, name) = match s.paused || s.finished {
        true => (Icon::Resume, "Play"),
        false => (Icon::Pause, "Pause"),
    };
    if widgets::icon_button(ui, icon, name, false).clicked() {
        s.toggle();
    }
    let next = s.narration.next(heard, s.skip);
    let next_button = ui.add_enabled_ui(next.is_some(), |ui| {
        widgets::small_icon_button(ui, Icon::SkipForward, "Next phrase")
    });
    if next_button.inner.clicked() {
        s.step(next);
    }
    if let Some(out) = s.narration.escape(heard, s.skip) {
        let leave = widgets::small_icon_button(ui, Icon::LeaveTable, "Skip the rest of this table");
        if leave.clicked() {
            s.step(Some(out));
        }
    }
    let speed = RichText::new(rate_label(settings.rate)).size(t.text_small);
    ui.menu_button(speed, |ui| {
        for rate in RATES {
            let on = (settings.rate - rate).abs() < 0.01;
            if ui.selectable_label(on, rate_label(rate)).clicked() {
                settings.rate = rate;
                ui.close();
            }
        }
    })
    .response
    .on_hover_text("Speed");
    ui.menu_button(RichText::new("⋯").size(t.text), |ui| {
        ui.checkbox(&mut settings.skip_notes, "Skip notes");
        ui.checkbox(&mut settings.skip_page_numbers, "Skip page numbers");
        ui.checkbox(&mut settings.follow, "Turn pages to follow");
    })
    .response
    .on_hover_text("Read-aloud options");
    if let Some(why) = s.fault.as_ref().map(fault_text) {
        ui.label(RichText::new(why).size(t.text_small).color(t.danger));
    } else if s.finished {
        ui.label(RichText::new("The end").size(t.text_small).color(t.muted));
    }
    widgets::small_icon_button(ui, Icon::Close, "Stop reading aloud").clicked()
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
fn fetch(files: Files) -> Fetch {
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
                    for b in layer.text.boxes(f..i) {
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

/// The book's highlight laid lightly over the page; the accent if none.
fn fill(book: Option<[u8; 3]>, t: &Tokens) -> Color32 {
    match (book, t.dark) {
        (Some([r, g, b]), false) => Color32::from_rgba_unmultiplied(r, g, b, 150),
        (Some([r, g, b]), true) => Color32::from_rgba_unmultiplied(r, g, b, 64),
        (None, false) => t.accent.gamma_multiply(0.30),
        (None, true) => t.accent.gamma_multiply(0.26),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_read_short() {
        assert_eq!(rate_label(1.0), "1×");
        assert_eq!(rate_label(1.25), "1.25×");
        assert_eq!(rate_label(0.75), "0.75×");
        assert_eq!(rate_label(1.5), "1.5×");
    }
}
