//! `epub:trigger`: clicks that show, hide and play other elements.
use std::collections::BTreeMap;
use std::sync::Arc;

use mareader_epub::{Action, Book, Destination, Inserts, Triggered};
use mareader_reflow::{Block, Content, Typography};

const BOOK: &[u8] = include_bytes!("books/triggers.epub");
const CHIME: &str = "/OEBPS/media/chime.mp3";

fn open() -> Book {
    Book::open(Arc::new(BOOK.to_vec()), &Typography::default()).unwrap()
}

fn text(book: &Book) -> String {
    let pages = 0..book.page_count();
    let lines = pages.flat_map(|p| book.text(p));
    lines
        .map(|l| l.chars.iter().map(|c| c.ch).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// What each trigger link on the pages sets off, in page order.
fn triggers(book: &Book) -> Vec<Vec<Triggered>> {
    let pages = 0..book.page_count();
    let mut seen: Vec<usize> = Vec::new();
    for link in pages.flat_map(|p| book.links(p)) {
        if let Destination::Trigger(n) = link.to
            && !seen.contains(&n)
        {
            seen.push(n);
        }
    }
    seen.into_iter().map(|n| book.trigger(n)).collect()
}

/// `book` laid with each target in `shown` as it says.
fn with(book: &Book, shown: &[(usize, bool)]) -> Book {
    let shown: BTreeMap<_, _> = shown.iter().copied().collect();
    let list = book.triggered(&shown).into_iter().map(Arc::new).collect();
    let inserts = Arc::new(Inserts { key: 1, list });
    book.reset(&Typography::default(), Some(&inserts)).unwrap()
}

/// The book's block holding `needle`, and where in it.
fn find<'b>(blocks: &'b [Block], needle: &str) -> (&'b Block, std::ops::Range<usize>) {
    for b in blocks {
        if let Content::Text(t) = &b.content
            && let Some(at) = t.text.find(needle)
        {
            return (b, at..at + needle.len());
        }
    }
    panic!("no block holds {needle:?}")
}

/// Whether every byte of `range` is hidden in `block`, or none is.
fn hidden(block: &Block, range: std::ops::Range<usize>) -> Option<bool> {
    let Content::Text(t) = &block.content else {
        return None;
    };
    let covered = |i: usize| (t.spans.iter()).any(|s| s.range.contains(&i) && s.emphasis.hidden);
    let all = range.clone().all(covered);
    let none = !range.clone().any(covered);
    (all || none).then_some(all)
}

fn chime(action: Action) -> Triggered {
    Triggered::Clip {
        action,
        path: CHIME.to_owned(),
        video: false,
    }
}

#[test]
fn clicks_on_observers_set_off_their_triggers() {
    let book = open();
    let got = triggers(&book);
    let sea = Triggered::Clip {
        action: Action::Play,
        path: "/OEBPS/media/sea.mp4".to_owned(),
        video: true,
    };
    let want = vec![
        vec![Triggered::Show(0, true)],
        vec![Triggered::Show(0, false)],
        vec![Triggered::Show(1, false)],
        vec![Triggered::Show(2, true)],
        vec![Triggered::Show(3, false)],
        vec![chime(Action::Play)],
        vec![chime(Action::Mute), Triggered::Show(4, true)],
        vec![sea],
        vec![chime(Action::Play)],
        vec![Triggered::Show(5, true)],
    ];
    assert_eq!(got, want);
    // A hidden sound has no card; the hover trigger makes no link.
    let all = text(&book);
    assert!(!all.contains("Play the audio"), "{all}");
    assert!(all.contains("Play the video: Waves"), "{all}");
}

#[test]
fn a_hidden_answer_is_laid_in_place_once_shown() {
    let book = open();
    assert!(!text(&book).contains("forty-two"));
    let all = text(&with(&book, &[(0, true)]));
    let answer = all.find("The answer is forty-two.").expect(&all);
    assert!(all.find("Hide answer").unwrap() < answer);
    assert!(answer < all.find("After the answer.").unwrap());
    // Styled as the chapter's other prose is.
    let ins = book.triggered(&BTreeMap::from([(0, true)]));
    let answer = ins[0].blocks.first().unwrap();
    let (after, _) = find(book.blocks(), "After the answer.");
    assert_eq!(answer.frame.look, after.frame.look);
    // Asked to stay as the book opens: nothing to lay.
    assert!(book.triggered(&BTreeMap::from([(0, false)])).is_empty());
    let all = text(&with(&book, &[(0, true), (0, false)]));
    assert!(!all.contains("forty-two"), "{all}");
}

#[test]
fn hiding_keeps_the_room_and_showing_gives_it_back() {
    let book = open();
    let blocks = book.blocks();
    let (hint, range) = find(blocks, "try counting sevens");
    assert_eq!(hidden(hint, range.clone()), Some(false));
    let ins = book.triggered(&BTreeMap::from([(1, true), (1, false)]));
    let laid = ins[0].blocks.first().unwrap();
    assert_eq!(hidden(laid, range.clone()), Some(true));
    // The words around it stay, with the same text: lines don't move.
    let (Content::Text(a), Content::Text(b)) = (&hint.content, &laid.content) else {
        panic!("not text");
    };
    assert_eq!(a.text, b.text);
    // A collapsed space before it may go with it; it looks the same.
    assert_eq!(hidden(laid, 0..range.start - 1), Some(false));
    // An inline `display: none` keeps its room, so it shows in place.
    let (tip, range) = find(blocks, "read slowly");
    assert_eq!(hidden(tip, range.clone()), Some(true));
    let ins = book.triggered(&BTreeMap::from([(2, true)]));
    assert_eq!(hidden(ins[0].blocks.first().unwrap(), range), Some(false));
    // The book's own `visibility: hidden` starts hidden too.
    let (muted, range) = find(blocks, "(muted)");
    assert_eq!(hidden(muted, range), Some(true));
}

#[test]
fn a_hidden_box_fades_with_its_text() {
    let book = open();
    let (boxed, range) = find(book.blocks(), "A boxed note.");
    let own = boxed.frame.boxes.len();
    assert!(own > 0);
    let ins = book.triggered(&BTreeMap::from([(3, false)]));
    let laid = ins[0].blocks.first().unwrap();
    assert_eq!(hidden(laid, range), Some(true));
    let faded = laid.frame.boxes.last().unwrap();
    let opacity = faded.finish.as_ref().map_or(1.0, |f| f.opacity);
    assert!(opacity < 0.01, "{opacity}");
}

#[test]
fn a_hidden_ending_shows_after_its_chapter_not_before_the_next() {
    let book = open();
    let pages = book.page_count();
    let shown = with(&book, &[(5, true)]);
    let all = text(&shown);
    let ending = all.find("The ending, revealed.").expect(&all);
    assert!(all.find("Reveal the ending").unwrap() < ending);
    assert_eq!(shown.page_count(), pages);
}
