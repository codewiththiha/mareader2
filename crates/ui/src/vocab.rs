//! Words above the reader's level, shaded on the pages on screen;
//! see `docs/study.md` § Vocabulary.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use egui::{CursorIcon, Id, Pos2, Rect, Ui};
use mareader_cefr::{Part, Role, State, Ticket, Vocabulary};
use mareader_pdf::Bounds;
use mareader_session::{DocId, TextLayer};
use mareader_settings::Reserved;

use crate::selection::Pages;
use crate::study::marks::draw;
use crate::study::words;

/// Shaded words per page at most; a book's stream page holds many.
const PAGE_CAP: usize = 5_000;
/// How long the pointer rests on a word before it is looked up.
const DWELL: f64 = 0.16;
/// Pages remembered; more and the memo starts over.
const MEMO_CAP: usize = 96;
/// How much stronger the word under the pointer is drawn.
const HOVER_LIFT: f32 = 0.2;

/// What the Vocabulary tab asks of the dataset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Download,
    Pause,
    Cancel,
    Remove,
}

/// How shading looks and acts in a pane, from the settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// The reader's band; words above it are shaded.
    pub level: u8,
    /// A click on a shaded word explains it.
    pub click: bool,
    /// Resting on a shaded word looks it up.
    pub hover: bool,
    pub ink: Reserved,
    pub opacity: f32,
}

/// One page's hard words and their boxes, while nothing changed.
struct Memo {
    layer: usize,
    level: u8,
    generation: u64,
    words: Arc<Vec<(Range<usize>, Vec<Bounds>)>>,
}

/// A shaded word on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Shaded {
    pub page: usize,
    /// Its characters in the page's text layer.
    pub chars: Range<usize>,
    pub rects: Vec<Rect>,
}

impl Shaded {
    pub fn contains(&self, p: Pos2) -> bool {
        self.rects.iter().any(|r| r.contains(p))
    }

    pub fn bounds(&self) -> Rect {
        let all = self.rects.iter().copied().reduce(|a, b| a.union(b));
        all.unwrap_or(Rect::NOTHING)
    }
}

#[derive(Default)]
pub struct Vocab {
    service: Vocabulary,
    memo: HashMap<(DocId, usize), Memo>,
    /// The role asked for the open card, if any.
    role: Option<Ticket>,
}

impl Vocab {
    /// Keeps the dataset in `dir`; `waker` repaints the app as work lands.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach(
        &mut self,
        dir: std::path::PathBuf,
        waker: mareader_cefr::Waker,
    ) -> Result<(), String> {
        self.memo.clear();
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

    pub fn state(&self, part: Part) -> &State {
        self.service.state(part)
    }

    pub fn words(&self) -> Option<u64> {
        self.service.words()
    }

    pub fn act(&mut self, action: Action) {
        #[cfg(not(target_arch = "wasm32"))]
        match action {
            Action::Download => self.service.download(),
            Action::Pause => self.service.pause(),
            Action::Cancel => self.service.cancel(),
            Action::Remove => self.service.remove(),
        }
        let _ = action;
    }

    /// Asks for the open card's word's role; a newer card's replaces it.
    pub fn ask_role(&mut self, word: &str, sentence: &str) {
        self.role = self.service.role(word, sentence);
    }

    /// Takes in answers; the open card's role if it came.
    pub fn poll(&mut self) -> (bool, Option<Role>) {
        let polled = self.service.poll();
        let mut role = None;
        for (ticket, answer) in polled.roles {
            if Some(ticket) == self.role {
                self.role = None;
                role = answer;
            }
        }
        (polled.changed, role)
    }

    /// `page`'s hard words, planned again only when something changed.
    fn page(
        &mut self,
        doc: DocId,
        page: usize,
        layer: &Arc<TextLayer>,
        level: u8,
    ) -> Arc<Vec<(Range<usize>, Vec<Bounds>)>> {
        let id = Arc::as_ptr(layer).addr();
        let generation = self.service.generation();
        if let Some(m) = self.memo.get(&(doc, page))
            && (m.layer, m.level, m.generation) == (id, level, generation)
        {
            return Arc::clone(&m.words);
        }
        let (text, index) = words::read(layer);
        let joined: String = text.iter().collect();
        let spans = self.service.plan(&joined, level, PAGE_CAP);
        let words: Vec<_> = spans
            .into_iter()
            .filter_map(|s| {
                let first = (*index.get(s.start)?)?;
                let last = (*index.get(s.end.checked_sub(1)?)?)?;
                let chars = first..last + 1;
                Some((chars.clone(), layer.text.boxes(chars)))
            })
            .collect();
        let words = Arc::new(words);
        if self.memo.len() >= MEMO_CAP {
            self.memo.clear();
        }
        let memo = Memo {
            layer: id,
            level,
            generation,
            words: Arc::clone(&words),
        };
        self.memo.insert((doc, page), memo);
        words
    }

    /// The shaded words on the pages on screen.
    pub fn place(&mut self, look: &Look, pages: &Pages<'_>) -> Vec<Shaded> {
        let mut out = Vec::new();
        for sheet in pages.sheets.iter().filter(|s| s.shown.is_positive()) {
            let Some(layer) = pages.texts.get(pages.doc, sheet.page) else {
                continue;
            };
            for (chars, boxes) in self.page(pages.doc, sheet.page, &layer, look.level).iter() {
                let rects: Vec<Rect> = (boxes.iter())
                    .map(|b| sheet.to_screen(*b).intersect(sheet.shown))
                    .filter(|r| r.is_positive())
                    .collect();
                if !rects.is_empty() {
                    let (page, chars) = (sheet.page, chars.clone());
                    out.push(Shaded { page, chars, rects });
                }
            }
        }
        out
    }
}

/// Each shaded word in the reserved look; the one under the pointer
/// stronger.
pub fn paint(ui: &Ui, shaded: &[Shaded], look: &Look, hovered: Option<usize>) {
    let painter = ui.painter();
    for (i, word) in shaded.iter().enumerate() {
        let lift = if hovered == Some(i) { HOVER_LIFT } else { 0.0 };
        for r in &word.rects {
            draw(
                painter,
                *r,
                look.ink.pen,
                look.ink.color,
                look.opacity + lift,
            );
        }
    }
}

/// The shaded word under `p`.
pub fn hit(shaded: &[Shaded], p: Pos2) -> Option<usize> {
    shaded.iter().position(|w| w.contains(p))
}

/// The shaded word the pointer rests on, if it is free to rest.
pub fn hovered(ui: &Ui, shaded: &[Shaded]) -> Option<usize> {
    let (pos, down) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.any_down()));
    let pos = pos.filter(|_| !down && ui.rect_contains_pointer(ui.clip_rect()))?;
    hit(shaded, pos)
}

/// What a rest is on, when it began and whether it was told.
type Rest = Option<((usize, usize), f64, bool)>;

/// The word the pointer has rested on long enough, once per rest.
pub fn dwell(
    ui: &Ui,
    id: Id,
    shaded: &[Shaded],
    hovered: Option<usize>,
    look: &Look,
) -> Option<usize> {
    if hovered.is_some() && look.click {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let now = ui.input(|i| i.time);
    let on = hovered.and_then(|i| Some((i, shaded.get(i)?)));
    let key = on.map(|(_, w)| (w.page, w.chars.start));
    let rest: Rest = ui.data(|d| d.get_temp(id)).flatten();
    let (since, told) = match rest {
        Some((k, since, told)) if Some(k) == key => (since, told),
        _ => (now, false),
    };
    let (Some(key), Some((index, _)), true) = (key, on, look.hover) else {
        ui.data_mut(|d| d.insert_temp::<Rest>(id, None));
        return None;
    };
    let ready = !told && now - since >= DWELL;
    ui.data_mut(|d| d.insert_temp::<Rest>(id, Some((key, since, told || ready))));
    if !told && !ready {
        ui.ctx()
            .request_repaint_after_secs((DWELL - (now - since)) as f32);
    }
    ready.then_some(index)
}
