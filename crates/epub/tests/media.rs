//! Clips as cards that play outside, and fallbacks a book names.
use std::sync::Arc;

use mareader_epub::{Book, Destination, Link};
use mareader_reflow::{Content, Typography};

const MEDIA: &[u8] = include_bytes!("books/media.epub");
const MATH: &[u8] = include_bytes!("books/math.epub");

fn open(bytes: &[u8]) -> Book {
    Book::open(Arc::new(bytes.to_vec()), &Typography::default()).unwrap()
}

/// Every page's links, with the page each is on.
fn links(book: &Book) -> Vec<(usize, Link)> {
    let pages = 0..book.page_count();
    pages
        .flat_map(|p| book.links(p).into_iter().map(move |l| (p, l)))
        .collect()
}

/// The page showing `words`.
fn page_of(book: &Book, words: &str) -> Option<usize> {
    (0..book.page_count()).find(|&p| {
        let text: String = book
            .text(p)
            .iter()
            .flat_map(|l| l.chars.iter().map(|c| c.ch))
            .collect();
        text.contains(words)
    })
}

fn media_paths(book: &Book) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = (links(book).into_iter())
        .filter_map(|(_, l)| match l.to {
            Destination::Media { path, video } => Some((path, video)),
            _ => None,
        })
        .collect();
    out.dedup();
    out
}

#[test]
fn clips_play_from_the_book_by_their_declared_type() {
    let book = open(MEDIA);
    let paths = media_paths(&book);
    let clip = ("/OEBPS/clip.mp4".to_owned(), true);
    let song = ("/OEBPS/song.mp3".to_owned(), false);
    assert!(paths.contains(&clip) && paths.contains(&song), "{paths:?}");
    let video = book.media(&clip.0).unwrap();
    assert_eq!(
        (video.extension, video.data.as_slice()),
        ("mp4", &b"zqclip: stands in for a video"[..])
    );
    assert_eq!(book.media(&song.0).map(|m| m.extension), Some("mp3"));
}

#[test]
fn files_that_dont_play_are_never_handed_out() {
    let book = open(MEDIA);
    let tool = ("/OEBPS/tool.bin".to_owned(), true);
    assert!(!media_paths(&book).contains(&tool));
    assert!(page_of(&book, "The video can't play here: Tool").is_some());
    assert!(
        book.media(&tool.0).is_none(),
        "an octet stream isn't a clip"
    );
    assert!(book.media("/OEBPS/poster.png").is_none());
    assert!(book.media("/OEBPS/missing.mp4").is_none());
    // A book with nothing to play keeps no copy of itself.
    assert!(open(MATH).media("/OEBPS/formula.png").is_none());
}

#[test]
fn cards_show_in_place_of_their_fallback_text() {
    let book = open(MEDIA);
    let page = page_of(&book, "Play the video: Waves").unwrap();
    assert!(page_of(&book, "Play the audio: song.mp3").is_some());
    assert!(
        page_of(&book, "zq").is_none(),
        "fallbacks for older readers stay hidden"
    );
    // The poster is drawn, and clicking it plays the clip.
    let card = (links(&book).into_iter())
        .find(|(p, l)| {
            *p == page
                && matches!(&l.to, Destination::Media { video: true, .. })
                && l.rect[3] > 30.0
        })
        .map(|(_, l)| l.rect)
        .unwrap();
    let middle = (card[0] + card[2] / 2.0, card[1] + card[3] / 2.0);
    let poster = book.picture_at(page, middle.0, middle.1).unwrap();
    assert_eq!((poster.width, poster.height), (64, 36));
}

#[test]
fn switches_objects_and_noscript_show_their_fallbacks() {
    let book = open(MEDIA);
    for words in [
        "Switch default words",
        "Object fallback words",
        "Noscript words",
    ] {
        assert!(page_of(&book, words).is_some(), "{words}");
    }
}

#[test]
fn manifest_fallbacks_stand_in_for_what_wont_show() {
    let book = open(MEDIA);
    // A picture of a type the reader can't decode shows its fallback.
    let pictures: Vec<(u32, u32)> = (book.blocks().iter())
        .filter_map(|b| match &b.content {
            Content::Image { picture, .. } => Some((picture.width, picture.height)),
            _ => None,
        })
        .collect();
    assert!(pictures.contains(&(40, 20)), "{pictures:?}");
    // A spine item in a foreign type reads as its fallback chapter.
    let legacy = page_of(&book, "Legacy fallback words.").unwrap();
    let toc = book.toc().iter().find(|e| e.title == "Legacy").unwrap();
    assert_eq!(book.locate(toc.at), legacy);
}

#[test]
fn image_map_areas_lead_where_they_say() {
    let book = open(MEDIA);
    let end = page_of(&book, "The end words.").unwrap();
    let to_end = (links(&book).into_iter()).find(|(_, l)| l.to == Destination::Page(end));
    let (page, link) = to_end.unwrap();
    let map = book
        .picture_at(page, link.rect[0] + 1.0, link.rect[1] + 1.0)
        .unwrap();
    assert_eq!((map.width, map.height), (100, 40));
    // The area is the map's left half, its full height.
    let inside = book.picture_at(page, link.rect[0] + link.rect[2] + 2.0, link.rect[1] + 1.0);
    assert!(
        inside.is_some_and(|p| Arc::ptr_eq(&p, &map)),
        "the picture goes on past it"
    );
}
