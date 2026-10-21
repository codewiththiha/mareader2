//! `epub:trigger`: a click on one element shows, hides or plays another.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use mareader_reflow::{Anchor, Block, Content, Picture, Text};

use crate::splice::Insert;
use crate::xhtml::{Href, Refs};

/// Triggers read from one chapter, and actions one click sets off.
const MAX_TRIGGERS: usize = 1 << 16;
const MAX_ACTIONS: usize = 1024;
/// How many blocks past a change are searched for the text to match again.
const RESYNC: usize = 64;
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
    /// Its chapter, in [`Refs::chapters`].
    pub(crate) chapter: usize,
}

/// A chapter with targets: its file and the book's blocks it holds.
#[derive(Debug, Clone)]
pub(crate) struct Laid {
    pub(crate) path: String,
    pub(crate) blocks: Range<usize>,
    /// Its links, in [`Refs::hrefs`].
    pub(crate) links: Range<usize>,
    pub(crate) fixed: bool,
    /// Its own place in [`Refs::chapters`].
    pub(crate) at: usize,
    /// Its scripts, if it has any.
    pub(crate) scripts: crate::script::Found,
}

/// What a link is matched by: where it leads, or what a click sets off.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key<'r> {
    Leads(&'r Href),
    Fires(&'r [(Action, String)]),
}

impl<'r> Key<'r> {
    fn of(refs: &'r Refs, href: &'r Href) -> Option<Self> {
        match href {
            Href::Trigger(n) => {
                let actions = refs.triggers.get(usize::try_from(*n).ok()?)?;
                Some(Self::Fires(actions))
            }
            Href::Transcript { .. } => None,
            href => Some(Self::Leads(href)),
        }
    }
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

/// Points `blocks`' links, read into `fresh`, at the book's; drops others.
pub(crate) fn remap(blocks: &mut [Block], fresh: &Refs, book: &Refs, laid: &Laid) {
    // The chapter's own links in order, so the nth of each matches again.
    let mut ours: HashMap<Key, Vec<u32>> = HashMap::new();
    let mut anywhere: HashMap<Key, u32> = HashMap::new();
    for (i, href) in book.hrefs.iter().enumerate() {
        let (Ok(n), Some(key)) = (u32::try_from(i), Key::of(book, href)) else {
            continue;
        };
        if laid.links.contains(&i) {
            ours.entry(key).or_default().push(n);
        }
        anywhere.entry(key).or_insert(n);
    }
    let mut seen: HashMap<Key, usize> = HashMap::new();
    let map: Vec<Option<u32>> = (fresh.hrefs.iter())
        .map(|href| {
            let Some(key) = Key::of(fresh, href) else {
                let Href::Transcript { card, track } = href else {
                    return None;
                };
                return card_link(fresh, book, laid, (*card, *track));
            };
            let nth = seen.entry(key).or_default();
            let mine = ours
                .get(&key)
                .and_then(|v| v.get(*nth).or(v.last()).copied());
            *nth += 1;
            mine.or_else(|| anywhere.get(&key).copied())
        })
        .collect();
    let to = |t: u32| {
        // Scripted pages' links name their nodes, not the book's links.
        if crate::script::unlink(t).is_some() {
            return Some(t);
        }
        usize::try_from(t)
            .ok()
            .and_then(|t| map.get(t).copied().flatten())
    };
    for block in blocks {
        relink(block, &to);
    }
}

/// The book's link to the same card's transcript, if the chapter has it.
fn card_link(
    fresh: &Refs,
    book: &Refs,
    laid: &Laid,
    (card, track): (u32, Option<u32>),
) -> Option<u32> {
    let mine = fresh.cards.get(usize::try_from(card).ok()?)?;
    let paths =
        |c: &crate::subtitles::Card| c.tracks.iter().map(|t| t.path.clone()).collect::<Vec<_>>();
    let same = |c: &&crate::subtitles::Card| {
        let here = c.caption.is_some_and(|b| laid.blocks.contains(&b));
        here && c.clip == mine.clip && paths(c) == paths(mine)
    };
    let theirs = book.cards.iter().find(same)?;
    match track {
        None => Some(theirs.toggle),
        Some(t) => theirs.picks.get(usize::try_from(t).ok()?).copied(),
    }
}

/// Swaps each link's target by `to`, dropping those it has none for.
fn relink(block: &mut Block, to: &dyn Fn(u32) -> Option<u32>) {
    let text = |t: &mut Text| {
        t.links
            .retain_mut(|l| to(l.target).map(|n| l.target = n).is_some());
        for inline in &mut t.inlines {
            inline.link = inline.link.and_then(to);
        }
    };
    match &mut block.content {
        Content::Text(t) => text(t),
        Content::Image { links, .. } => {
            links.retain_mut(|a| to(a.target).map(|n| a.target = n).is_some());
        }
        Content::Table(table) => {
            table
                .rows
                .iter_mut()
                .flat_map(|r| r.cells.iter_mut())
                .for_each(text);
        }
        _ => {}
    }
}

/// Inserts laying `new` where it differs from `base`, which starts at `first`.
pub(crate) fn changes(
    base: &[Block],
    mut new: Vec<Block>,
    first: usize,
    language: Option<&str>,
) -> Vec<Insert> {
    share_pictures(&mut new, base);
    let runs = widen(runs(base, &new), base, &new);
    let anchor = |block| Anchor { block, offset: 0 };
    (runs.into_iter())
        .map(|(old, laid)| Insert {
            id: TRIGGERED | u128::try_from(first + old.start).unwrap_or_default(),
            from: anchor(first + old.start),
            to: anchor(first + old.end),
            blocks: Arc::new(new.get(laid).unwrap_or_default().to_vec()),
            language: language.map(str::to_owned),
        })
        .collect()
}

/// Gives `new` the book's own pictures where they are the same images.
fn share_pictures(new: &mut [Block], base: &[Block]) {
    let mut own: Vec<Arc<Picture>> = Vec::new();
    let mut covers: Vec<Arc<Picture>> = Vec::new();
    for block in base {
        pictures_of(block, &mut own);
        if let Content::Image {
            picture,
            cover: true,
            ..
        } = &block.content
        {
            covers.push(Arc::clone(picture));
        }
    }
    for block in new {
        for_pictures(block, &mut |p| {
            if let Some(o) = own.iter().find(|o| o.same(p)) {
                *p = Arc::clone(o);
            }
        });
        // The book made a picture of its opening its cover.
        if let Content::Image { picture, cover, .. } = &mut block.content {
            *cover |= covers.iter().any(|c| Arc::ptr_eq(c, picture));
        }
    }
}

fn pictures_of(block: &Block, out: &mut Vec<Arc<Picture>>) {
    out.extend(block.frame.marker_picture.clone());
    let inlines: Vec<&mareader_reflow::Inline> = match &block.content {
        Content::Image { picture, .. } => {
            out.push(Arc::clone(picture));
            Vec::new()
        }
        Content::Text(t) => t.inlines.iter().collect(),
        Content::Table(table) => (table.rows.iter())
            .flat_map(|r| r.cells.iter())
            .flat_map(|c| c.inlines.iter())
            .collect(),
        _ => Vec::new(),
    };
    out.extend(inlines.into_iter().map(|i| Arc::clone(&i.picture)));
}

fn for_pictures(block: &mut Block, f: &mut dyn FnMut(&mut Arc<Picture>)) {
    if let Some(p) = &mut block.frame.marker_picture {
        f(p);
    }
    let inlines: Vec<&mut mareader_reflow::Inline> = match &mut block.content {
        Content::Image { picture, .. } => {
            f(picture);
            Vec::new()
        }
        Content::Text(t) => t.inlines.iter_mut().collect(),
        Content::Table(table) => (table.rows.iter_mut())
            .flat_map(|r| r.cells.iter_mut())
            .flat_map(|c| c.inlines.iter_mut())
            .collect(),
        _ => Vec::new(),
    };
    for inline in inlines {
        f(&mut inline.picture);
    }
}

/// Changed runs: the blocks of `base` and those of `new` laid instead.
fn runs(base: &[Block], new: &[Block]) -> Vec<(Range<usize>, Range<usize>)> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < base.len() || j < new.len() {
        if base.get(i).is_some_and(|b| new.get(j) == Some(b)) {
            (i, j) = (i + 1, j + 1);
            continue;
        }
        let meets = |(a, b): (usize, usize)| {
            let pair = base.get(i + a).zip(new.get(j + b));
            pair.is_some_and(|(x, y)| x == y)
        };
        // The nearest pair of blocks that match again, if near enough.
        let pairs = (1..=2 * RESYNC).flat_map(|k| (0..=k).map(move |a| (a, k - a)));
        let near = pairs
            .filter(|&(a, b)| a <= RESYNC && b <= RESYNC)
            .find(|&p| meets(p));
        let rest = (base.len().saturating_sub(i), new.len().saturating_sub(j));
        let (a, b) = near.unwrap_or(rest);
        out.push((i..i + a, j..j + b));
        (i, j) = (i + a, j + b);
    }
    out
}

/// Runs grown to whole boxes, rows and pages, and never empty.
fn widen(
    runs: Vec<(Range<usize>, Range<usize>)>,
    base: &[Block],
    new: &[Block],
) -> Vec<(Range<usize>, Range<usize>)> {
    let tied = |blocks: &[Block], at: usize| {
        let pair = at
            .checked_sub(1)
            .and_then(|p| blocks.get(p))
            .zip(blocks.get(at));
        pair.is_some_and(|(a, b)| shares(a, b))
    };
    let mut out: Vec<(Range<usize>, Range<usize>)> = Vec::new();
    for (mut old, mut laid) in runs {
        loop {
            let can_back = old.start > 0 && laid.start > 0;
            let can_ahead = old.end < base.len() && laid.end < new.len();
            let back = can_back && (tied(base, old.start) || tied(new, laid.start));
            let ahead = can_ahead && (tied(base, old.end) || tied(new, laid.end));
            let empty = old.is_empty() || laid.is_empty();
            let (back, ahead) = match (back || ahead, empty) {
                (false, true) => (!can_ahead && can_back, can_ahead),
                _ => (back, ahead),
            };
            if !back && !ahead {
                break;
            }
            if back {
                (old.start, laid.start) = (old.start - 1, laid.start - 1);
            }
            if ahead {
                (old.end, laid.end) = (old.end + 1, laid.end + 1);
            }
        }
        match out.last_mut() {
            Some((o, l)) if old.start <= o.end => {
                (o.end, l.end) = (o.end.max(old.end), l.end.max(laid.end));
            }
            _ => out.push((old, laid)),
        }
    }
    out
}

/// Whether two blocks share a box, a grid row, a float or a fixed page.
fn shares(a: &Block, b: &Block) -> bool {
    fn one<T>(x: &Option<Arc<T>>, y: &Option<Arc<T>>) -> bool {
        x.as_ref()
            .zip(y.as_ref())
            .is_some_and(|(x, y)| Arc::ptr_eq(x, y))
    }
    let (a, b) = (&a.frame, &b.frame);
    let boxed = a
        .boxes
        .iter()
        .any(|x| b.boxes.iter().any(|y| Arc::ptr_eq(x, y)));
    let cells = a.cell.as_ref().zip(b.cell.as_ref());
    let row = cells.is_some_and(|(x, y)| {
        Arc::ptr_eq(&x.row, &y.row) || Arc::ptr_eq(&x.row.columns, &y.row.columns)
    });
    boxed || row || one(&a.float, &b.float) || one(&a.sheet, &b.sheet) || one(&a.spot, &b.spot)
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

    fn words(s: &str) -> Block {
        Block::text(Role::Paragraph, s)
    }

    #[test]
    fn only_changed_blocks_are_laid_again() {
        let base = [words("a"), words("b"), words("c"), words("d")];
        let new = vec![words("a"), words("B"), words("c"), words("d")];
        let laid = changes(&base, new, 10, None);
        assert_eq!(laid.len(), 1);
        let [one] = laid.as_slice() else {
            return;
        };
        assert_eq!((one.from.block, one.to.block), (11, 12));
        assert_eq!(one.blocks.len(), 1);
        assert_eq!(one.id, TRIGGERED | 11);
    }

    #[test]
    fn a_new_block_takes_a_neighbour_and_runs_merge() {
        let base = [words("a"), words("c"), words("e")];
        let new = vec![words("a"), words("b"), words("c"), words("e"), words("f")];
        let laid = changes(&base, new, 0, None);
        let spans: Vec<_> = laid
            .iter()
            .map(|i| (i.from.block, i.to.block, i.blocks.len()))
            .collect();
        assert_eq!(spans, [(1, 3, 4)]);
        let gone = changes(&base, vec![words("a"), words("e")], 0, None);
        let spans: Vec<_> = gone
            .iter()
            .map(|i| (i.from.block, i.to.block, i.blocks.len()))
            .collect();
        assert_eq!(spans, [(1, 3, 1)]);
    }

    #[test]
    fn runs_grow_to_the_whole_box() {
        let boxed = Arc::new(mareader_reflow::Boxed::default());
        let in_box = |s: &str, b: &Arc<mareader_reflow::Boxed>| {
            let mut block = words(s);
            block.frame.boxes = vec![Arc::clone(b)];
            block
        };
        let base = [
            words("a"),
            in_box("b", &boxed),
            in_box("c", &boxed),
            words("d"),
        ];
        let other = Arc::new(mareader_reflow::Boxed::default());
        let new = vec![
            words("a"),
            in_box("b", &other),
            in_box("C", &other),
            words("d"),
        ];
        let laid = changes(&base, new, 0, None);
        let spans: Vec<_> = laid.iter().map(|i| (i.from.block, i.to.block)).collect();
        assert_eq!(spans, [(1, 3)]);
    }
}
