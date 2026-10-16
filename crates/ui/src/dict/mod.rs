//! Dictionaries in the app: the lookup card, the panel, the packs.

mod card;
pub mod panel;

use egui::{Context, Rect, Vec2};
use mareader_dict::{Dictionaries, PACKS, Pack, Query, Reply, State, Ticket};
use mareader_settings::DictionarySettings;
use mareader_workspace::PaneId;

use crate::command::Commands;
use crate::theme::Tokens;

/// What the card, the panel and the settings ask of the dictionaries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Download(&'static str),
    Pause(&'static str),
    Cancel(&'static str),
    Remove(&'static str),
    CloseCard,
}

/// A lookup card, kept where its words were selected.
#[derive(Clone, Debug, PartialEq)]
struct Card {
    pane: PaneId,
    word: String,
    at: Rect,
    /// The pane's scroll when it opened; a scroll away closes it.
    offset: Option<Vec2>,
    ticket: Ticket,
    reply: Option<Reply>,
}

#[derive(Default)]
pub struct Dict {
    service: Dictionaries,
    card: Option<Card>,
    card_rect: Option<Rect>,
    panel: panel::Panel,
}

impl Dict {
    /// Keeps packs in `dir`; `waker` repaints the app as work lands.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach(
        &mut self,
        dir: std::path::PathBuf,
        waker: mareader_dict::Waker,
    ) -> Result<(), String> {
        self.service.attach(dir, waker)
    }

    /// Answers on this thread, as tests need.
    pub fn set_inline(&mut self, inline: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        self.service.set_inline(inline);
        let _ = inline;
    }

    pub fn attached(&self) -> bool {
        self.service.attached()
    }

    pub fn state(&self, pack: &Pack) -> &State {
        self.service.state(pack)
    }

    pub fn ready(&self) -> Vec<&'static Pack> {
        self.service.ready()
    }

    /// Look up pressed: the card for `word` beside it, answered in the
    /// default language.
    pub fn look_up(
        &mut self,
        pane: PaneId,
        word: String,
        at: Rect,
        offset: Option<Vec2>,
        to: Option<String>,
    ) {
        let query = Query::LookUp {
            word: word.clone(),
            to,
        };
        let ticket = self.service.ask(query);
        self.card = Some(Card {
            pane,
            word,
            at,
            offset,
            ticket,
            reply: None,
        });
    }

    /// The word the card shows, for tests.
    pub fn card_word(&self) -> Option<&str> {
        self.card.as_ref().map(|c| c.word.as_str())
    }

    /// The card's answers once they came, for tests.
    pub fn card_answers(&self) -> Option<Vec<String>> {
        let reply = self.card.as_ref()?.reply.as_ref()?;
        Some(
            reply
                .entries
                .iter()
                .map(|e| e.answer().to_owned())
                .collect(),
        )
    }

    /// Escape: closes the card; false if none was open.
    pub fn escape(&mut self) -> bool {
        self.card.take().is_some()
    }

    pub fn act(&mut self, action: Action) {
        let pack = |id: &str| PACKS.iter().find(|p| p.id == id);
        #[cfg(not(target_arch = "wasm32"))]
        match action {
            Action::Download(id) => pack(id).map(|p| self.service.download(p)),
            Action::Pause(id) => pack(id).map(|p| self.service.pause(p)),
            Action::Cancel(id) => pack(id).map(|p| self.service.cancel(p)),
            Action::Remove(id) => pack(id).map(|p| self.service.remove(p)),
            Action::CloseCard => self.card.take().map(|_| ()),
        };
        #[cfg(target_arch = "wasm32")]
        if action == Action::CloseCard {
            self.card = None;
        }
        let _ = pack;
    }

    /// Takes in replies and progress; true if a pack's state changed.
    pub fn poll(&mut self) -> bool {
        let (changed, replies) = self.service.poll();
        for reply in replies {
            match &mut self.card {
                Some(card) if card.ticket == reply.ticket => card.reply = Some(reply),
                _ => self.panel.receive(reply),
            }
        }
        if changed {
            // A pack came or went: what the panel showed may differ.
            self.panel.stale();
        }
        changed
    }

    /// After the panes: the card over them, if open.
    pub fn show(
        &mut self,
        ctx: &Context,
        panes: &[(PaneId, Rect)],
        offset: impl Fn(PaneId) -> Option<Vec2>,
        out: &mut Commands,
    ) {
        let pressed = ctx.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed()));
        let outside = pressed.is_some_and(|p| !self.card_rect.is_some_and(|r| r.contains(p)));
        let room = |pane| panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r);
        let moved = |c: &Card| match (c.offset, offset(c.pane)) {
            (Some(a), Some(b)) => (a - b).length() > 0.5,
            _ => false,
        };
        if outside
            || self
                .card
                .as_ref()
                .is_some_and(|c| room(c.pane).is_none() || moved(c))
        {
            self.card = None;
        }
        let t = Tokens::get(ctx);
        let attached = self.attached();
        self.card_rect = self.card.as_ref().and_then(|c| {
            let view = card::View {
                word: &c.word,
                reply: c.reply.as_ref(),
                attached,
                at: c.at,
                room: room(c.pane)?,
            };
            Some(card::show(ctx, &view, &t, out))
        });
    }

    /// The rail's Dictionary panel.
    pub fn panel(&mut self, ui: &mut egui::Ui, settings: &DictionarySettings, out: &mut Commands) {
        let Self { service, panel, .. } = self;
        panel.show(ui, service, settings, out);
    }
}

/// Bytes as megabytes, one decimal.
pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// A count with thin spaces between thousands.
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

/// The selection on one line, edge punctuation trimmed; none past `max`.
pub fn lookup_text(selected: &str, max: usize) -> Option<String> {
    let joined = selected.split_whitespace().collect::<Vec<_>>().join(" ");
    let edge = |c: char| !c.is_alphanumeric() && c != '%';
    let word = joined.trim_matches(edge);
    let fits = !word.is_empty() && word.chars().count() <= max;
    fits.then(|| word.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_becomes_a_tidy_lookup() {
        assert_eq!(lookup_text("  “Light,”\n", 40), Some("Light".into()));
        assert_eq!(lookup_text("100% sure.", 40), Some("100% sure".into()));
        assert_eq!(lookup_text("it’s", 40), Some("it’s".into()));
        assert_eq!(lookup_text("…", 40), None);
        assert_eq!(lookup_text("a long, long phrase", 6), None);
    }

    #[test]
    fn counts_and_sizes_read_well() {
        assert_eq!(count(110_640), "110\u{202f}640");
        assert_eq!(count(999), "999");
        assert_eq!(megabytes(2_875_539), "2.9 MB");
    }
}
