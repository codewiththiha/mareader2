//! `epub:trigger`: a click on one element shows, hides or plays another.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use mareader_reflow::{Anchor, Block, Boxed, Content, Emphasis, Finish, Picture, Span, Text, clip};

use crate::splice::Insert;

/// Triggers read from one chapter, and actions one click sets off.
const MAX_TRIGGERS: usize = 512;
pub(crate) const MAX_ACTIONS: usize = 32;
/// Elements shown or hidden per book.
pub(crate) const MAX_TARGETS: usize = 4096;
/// Insert ids: the second bit from the top, then the block.
const TRIGGERED: u128 = 1 << 126;

/// What a trigger does to the element it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Show,
    Hide,
    Play,
    Pause,
    Resume,
    Mute,
    Unmute,
}

impl Action {
    fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "show" => Self::Show,
            "hide" => Self::Hide,
            "play" => Self::Play,
            "pause" => Self::Pause,
            "resume" => Self::Resume,
            "mute" => Self::Mute,
            "unmute" => Self::Unmute,
            _ => return None,
        })
    }

    /// Whether it changes what shows, rather than a clip.
    pub(crate) fn shows(self) -> bool {
        matches!(self, Self::Show | Self::Hide)
    }
}

/// What a click sets off, resolved against the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Triggered {
    /// Shows or hides target `n`, for [`crate::Book::triggered`].
    Show(usize, bool),
    /// A clip in the book: `action` on it, and whether it is a video.
    Clip {
        action: Action,
        path: String,
        video: bool,
    },
}

impl Triggered {
    /// What a click on a link setting off `all` does, as a tooltip.
    pub fn label(all: &[Self]) -> String {
        let clip = all.iter().find_map(|t| match t {
            Self::Clip { action, video, .. } => Some((*action, *video)),
            Self::Show(..) => None,
        });
        if let Some((action, video)) = clip {
            let verb = match action {
                Action::Play | Action::Show | Action::Hide => "Play",
                Action::Pause => "Pause",
                Action::Resume => "Resume",
                Action::Mute => "Mute",
                Action::Unmute => "Unmute",
            };
            let what = if video { "video" } else { "audio" };
            return format!("{verb} the {what}");
        }
        let shows = all.iter().any(|t| matches!(t, Self::Show(_, true)));
        let hides = all.iter().any(|t| matches!(t, Self::Show(_, false)));
        match (shows, hides) {
            (true, true) => "Show or hide part of the page",
            (false, true) => "Hide part of the page",
            _ => "Show more",
        }
        .to_owned()
    }
}

/// A chapter's triggers by the id of the element clicked.
#[derive(Debug, Default)]
pub(crate) struct Chapter {
    /// Observer id to its actions and the ids they act on.
    pub(crate) observers: HashMap<String, Vec<(Action, String)>>,
    /// Ids that some trigger shows or hides.
    pub(crate) targets: HashSet<String>,
    /// Ids that some trigger plays, pauses or mutes.
    pub(crate) played: HashSet<String>,
}

/// An element some trigger shows or hides.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    /// `path#id`.
    pub(crate) key: String,
    /// The book's boxes open around it; the rest are its own.
    pub(crate) boxes: usize,
    /// Shown as the book opens.
    pub(crate) visible: bool,
    pub(crate) place: Place,
}

/// Where a target's content is.
#[derive(Debug, Clone)]
pub(crate) enum Place {
    /// In the pages, from one place to another; found once all is read.
    Laid(Option<Range<Anchor>>),
    /// Hidden with `display: none`: kept aside, laid by block `at`.
    Held(Held),
}

/// A hidden block element's blocks and where they go once shown.
#[derive(Debug, Clone)]
pub(crate) struct Held {
    pub(crate) blocks: Vec<Block>,
    /// The book's block that followed it.
    pub(crate) at: usize,
    /// It began its chapter.
    pub(crate) opens: bool,
}

/// The triggers in `xhtml`, read ahead of its elements.
pub(crate) fn read(xhtml: &str) -> Chapter {
    let mut chapter = Chapter::default();
    if !xhtml.contains("trigger") {
        return chapter;
    }
    let sink = Scan(RefCell::default());
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(xhtml));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    for (observer, action, target) in tokenizer.sink.0.into_inner() {
        match action.shows() {
            true => chapter.targets.insert(target.clone()),
            false => chapter.played.insert(target.clone()),
        };
        let actions = chapter.observers.entry(observer).or_default();
        if actions.len() < MAX_ACTIONS {
            actions.push((action, target));
        }
    }
    chapter
}

/// Events a click stands for; others never come from a reader.
fn clicked(event: &str) -> bool {
    matches!(
        event.trim().to_ascii_lowercase().as_str(),
        "click" | "activate" | "domactivate" | "mouseup" | "mousedown" | "touchend" | "touchstart"
    )
}

/// An attribute by its name without a prefix, as `ev:observer`.
fn attr<'t>(tag: &'t Tag, name: &str) -> Option<&'t str> {
    let local = |n: &str| n.rsplit(':').next().unwrap_or_default().to_owned();
    let found = tag.attrs.iter().find(|a| local(&a.name.local) == name);
    found.map(|a| a.value.trim()).filter(|v| !v.is_empty())
}

/// Triggers read so far: observer, action, target.
struct Scan(RefCell<Vec<(String, Action, String)>>);

impl TokenSink for Scan {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let Token::TagToken(tag) = token else {
            return TokenSinkResult::Continue;
        };
        let name = tag.name.rsplit(':').next().unwrap_or_default();
        if tag.kind != TagKind::StartTag || name != "trigger" || !tag.name.contains(':') {
            return TokenSinkResult::Continue;
        }
        let mut found = self.0.borrow_mut();
        let event = attr(&tag, "event").is_none_or(clicked);
        let action = attr(&tag, "action").and_then(Action::parse);
        let (observer, target) = (attr(&tag, "observer"), attr(&tag, "ref"));
        if let (true, Some(action), Some(o), Some(t)) = (event, action, observer, target)
            && found.len() < MAX_TRIGGERS
        {
            found.push((o.to_owned(), action, t.to_owned()));
        }
        TokenSinkResult::Continue
    }
}

/// Finds where each laid target starts and ends, now all is read.
pub(crate) fn find_targets(refs: &mut crate::xhtml::Refs) {
    for t in refs.targets.iter_mut() {
        if let Place::Laid(range) = &mut t.place {
            let start = refs.places.get(&t.key).copied();
            let end = refs.ends.get(&t.key).copied();
            *range = start
                .zip(end)
                .map(|(a, b)| a..b)
                .filter(|r| r.start < r.end);
        }
    }
}

/// Boxes faded or restored, by the box they copy and whether hidden.
type Faded = HashMap<(usize, bool), Arc<Boxed>>;

/// What to lay in place of one of the book's blocks.
#[derive(Default)]
struct Plan {
    before: Vec<Block>,
    after: Vec<Block>,
    /// Byte ranges to hide or show, and the boxes from which on to fade.
    edits: Vec<(Range<usize>, bool, Option<usize>)>,
}

/// The book's blocks redone so each target in `shown` is as it says.
pub(crate) fn inserts(
    blocks: &[Block],
    targets: &[Target],
    shown: &BTreeMap<usize, bool>,
) -> Vec<Insert> {
    let mut plans: BTreeMap<usize, Plan> = BTreeMap::new();
    for (&n, &visible) in shown {
        let Some(t) = targets.get(n).filter(|t| t.visible != visible) else {
            continue;
        };
        match &t.place {
            Place::Laid(Some(range)) => laid(&mut plans, blocks, (range, t.boxes), !visible),
            Place::Laid(None) => {}
            // Held targets start hidden, so here they show.
            Place::Held(held) => held_in(&mut plans, blocks, held),
        }
    }
    let mut faded = HashMap::new();
    let mut out = Vec::with_capacity(plans.len());
    for (at, plan) in plans {
        let Some(base) = blocks.get(at) else {
            continue;
        };
        let mut block = base.clone();
        for (range, hidden, boxes) in &plan.edits {
            edit(&mut block, range.clone(), *hidden, (*boxes, &mut faded));
        }
        let mut list = plan.before;
        if let Some(first) = list.first_mut() {
            first.break_before = block.break_before;
            block.break_before = false;
        }
        list.push(block);
        list.extend(plan.after);
        out.push(Insert {
            id: TRIGGERED | at as u128,
            from: Anchor {
                block: at,
                offset: 0,
            },
            to: Anchor {
                block: at + 1,
                offset: 0,
            },
            blocks: Arc::new(list),
            language: None,
        });
    }
    out
}

/// Plans hiding or showing a laid target's text, pictures and boxes.
fn laid(
    plans: &mut BTreeMap<usize, Plan>,
    blocks: &[Block],
    (range, boxes): (&Range<Anchor>, usize),
    hidden: bool,
) {
    let (from, to) = (range.start, range.end);
    let last = match to.offset {
        0 => to.block.saturating_sub(1),
        _ => to.block,
    };
    for b in from.block..=last.min(blocks.len().saturating_sub(1)) {
        let start = if b == from.block { from.offset } else { 0 };
        let end = if b == to.block { to.offset } else { usize::MAX };
        // Its own boxes fade only where it holds the whole block.
        let len = match blocks.get(b).map(|b| &b.content) {
            Some(Content::Text(t)) => t.text.len(),
            _ => 0,
        };
        let whole = start == 0 && end >= len;
        let plan = plans.entry(b).or_default();
        plan.edits
            .push((start..end, hidden, whole.then_some(boxes)));
    }
}

/// Plans laying a held target's blocks by the book's block it went with.
fn held_in(plans: &mut BTreeMap<usize, Plan>, blocks: &[Block], held: &Held) {
    let next = blocks.get(held.at);
    let before = held.opens || next.is_some_and(|b| !b.break_before) || held.at == 0;
    match before {
        true if next.is_some() => {
            let plan = plans.entry(held.at).or_default();
            plan.before.extend(held.blocks.iter().cloned());
        }
        _ => {
            let Some(prev) = held.at.checked_sub(1).filter(|p| *p < blocks.len()) else {
                return;
            };
            let mut list = held.blocks.clone();
            if let Some(first) = list.first_mut() {
                first.break_before = false;
            }
            plans.entry(prev).or_default().after.extend(list);
        }
    }
}

/// Hides or shows bytes `range` of `block`, and fades or restores its boxes.
fn edit(
    block: &mut Block,
    range: Range<usize>,
    hidden: bool,
    (boxes, faded): (Option<usize>, &mut Faded),
) {
    match &mut block.content {
        Content::Text(text) => {
            set_hidden(text, range.clone(), hidden);
            for inline in text.inlines.iter_mut().filter(|i| range.contains(&i.at)) {
                if let Some(blank) = hidden.then(|| blank(&inline.picture)).flatten() {
                    inline.picture = blank;
                }
            }
        }
        Content::Image { picture, .. } if hidden => {
            if let Some(blank) = blank(picture) {
                *picture = blank;
            }
        }
        _ => {}
    }
    let Some(from) = boxes else {
        return;
    };
    for b in block.frame.boxes.iter_mut().skip(from) {
        // One box spans many blocks: each takes the same faded copy.
        let key = (Arc::as_ptr(b) as usize, hidden);
        let done = faded.entry(key).or_insert_with(|| {
            let mut boxed = (**b).clone();
            let mut finish = boxed.finish.as_deref().cloned().unwrap_or_default();
            finish.opacity = if hidden { 0.0 } else { 1.0 };
            boxed.finish = (finish != Finish::default()).then(|| Arc::new(finish));
            Arc::new(boxed)
        });
        *b = Arc::clone(done);
    }
}

/// Marks bytes `range` of `text` hidden or shown, keeping its other styles.
pub(crate) fn set_hidden(text: &mut Text, range: Range<usize>, hidden: bool) {
    let range = clip(&text.text, range);
    if range.is_empty() {
        return;
    }
    let plain = Emphasis {
        hidden,
        ..Emphasis::default()
    };
    let mut out = Vec::with_capacity(text.spans.len() + 2);
    let mut covered = range.start;
    for s in std::mem::take(&mut text.spans) {
        if s.range.end <= range.start || s.range.start >= range.end {
            out.push(s);
            continue;
        }
        let (a, b) = (s.range.start.max(range.start), s.range.end.min(range.end));
        if s.range.start < a {
            out.push(Span {
                range: s.range.start..a,
                emphasis: s.emphasis,
            });
        }
        if covered < a && hidden {
            out.push(Span {
                range: covered..a,
                emphasis: plain,
            });
        }
        out.push(Span {
            range: a..b,
            emphasis: Emphasis {
                hidden,
                ..s.emphasis
            },
        });
        covered = b;
        if s.range.end > b {
            out.push(Span {
                range: b..s.range.end,
                emphasis: s.emphasis,
            });
        }
    }
    if covered < range.end && hidden {
        out.push(Span {
            range: covered..range.end,
            emphasis: plain,
        });
    }
    out.sort_by_key(|s| s.range.start);
    text.spans = out;
}

/// A see-through picture of `picture`'s size, which takes its room.
fn blank(picture: &Picture) -> Option<Arc<Picture>> {
    let (w, h) = (picture.width.max(1), picture.height.max(1));
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{h}' viewBox='0 0 {w} {h}'/>"
    );
    Picture::svg(svg.into_bytes(), Vec::new())
        .ok()
        .map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_reflow::Role;

    #[test]
    fn triggers_are_read_by_observer_with_any_prefix() {
        let xhtml = r#"<html xmlns:epub="http://www.idpf.org/2007/ops"
            xmlns:ev="http://www.w3.org/2001/xml-events"><head>
            <epub:trigger ev:observer="go" ev:event="click" action="play" ref="clip"/>
            <ops:trigger xev:observer="go" xev:event="click" action="hide" ref="hint"/>
            <epub:trigger ev:observer="go" ev:event="mouseover" action="show" ref="x"/>
            <epub:trigger ev:observer="go" ev:event="click" action="explode" ref="y"/>
            <trigger observer="go" event="click" action="show" ref="z"/>
            </head><body/></html>"#;
        let chapter = read(xhtml);
        let actions = chapter.observers.get("go").cloned().unwrap_or_default();
        assert_eq!(
            actions,
            [
                (Action::Play, "clip".to_owned()),
                (Action::Hide, "hint".to_owned())
            ]
        );
        assert_eq!(chapter.targets, HashSet::from(["hint".to_owned()]));
        assert!(read("<p>no triggers</p>").observers.is_empty());
    }

    #[test]
    fn hiding_marks_only_the_range_and_keeps_other_styles() {
        let mut text = Text::new(Role::Paragraph);
        text.push("one ", Emphasis::default());
        let bold = Emphasis {
            bold: true,
            ..Emphasis::default()
        };
        text.push("two", bold);
        text.push(" three", Emphasis::default());
        set_hidden(&mut text, 2..9, true);
        let hidden: Vec<_> = (text.spans.iter())
            .filter(|s| s.emphasis.hidden)
            .map(|s| (s.range.clone(), s.emphasis.bold))
            .collect();
        assert_eq!(hidden, [(2..4, false), (4..7, true), (7..9, false)]);
        assert!(
            text.spans
                .iter()
                .any(|s| s.range == (4..7) && s.emphasis.bold)
        );
        set_hidden(&mut text, 0..usize::MAX, false);
        assert!(text.spans.iter().all(|s| !s.emphasis.hidden));
        assert!(text.spans.iter().any(|s| s.emphasis.bold));
    }

    #[test]
    fn a_shown_held_block_goes_before_the_one_that_followed_it() {
        let para = |s: &str, chapter: bool| {
            let mut t = Text::new(Role::Paragraph);
            t.push(s, Emphasis::default());
            let mut b = Block::new(Content::Text(t));
            b.break_before = chapter;
            b
        };
        let blocks = vec![para("a", true), para("c", false), para("d", true)];
        let held = |at: usize, opens: bool| Target {
            key: "/x#h".to_owned(),
            boxes: 0,
            visible: false,
            place: Place::Held(Held {
                blocks: vec![para("b", opens)],
                at,
                opens,
            }),
        };
        let texts = |ins: &Insert| -> Vec<(String, bool)> {
            (ins.blocks.iter())
                .map(|b| match &b.content {
                    Content::Text(t) => (t.text.clone(), b.break_before),
                    _ => (String::new(), b.break_before),
                })
                .collect()
        };
        let shown = BTreeMap::from([(0, true)]);
        let got = inserts(&blocks, &[held(1, false)], &shown);
        assert_eq!(got.len(), 1);
        assert_eq!(texts(&got[0]), [("b".into(), false), ("c".into(), false)]);
        // At a chapter's end it follows the block before, not the next chapter.
        let got = inserts(&blocks, &[held(2, false)], &shown);
        assert_eq!(got[0].from.block, 1);
        assert_eq!(texts(&got[0]), [("c".into(), false), ("b".into(), false)]);
        // Opening its chapter, it takes the page break.
        let got = inserts(&blocks, &[held(2, true)], &shown);
        assert_eq!(texts(&got[0]), [("b".into(), true), ("d".into(), false)]);
        // Already as asked: nothing to lay.
        assert!(inserts(&blocks, &[held(1, false)], &BTreeMap::from([(0, false)])).is_empty());
    }
}
