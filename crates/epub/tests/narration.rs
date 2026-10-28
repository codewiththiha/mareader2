//! A book read aloud: its overlays as phrases of the text.
use std::sync::Arc;

use mareader_epub::{Book, Phrase};
use mareader_reflow::{Content, Typography};

const NARRATED: &[u8] = include_bytes!("books/narration.epub");
const MEDIA: &[u8] = include_bytes!("books/media.epub");
const VOICE: &str = "/OEBPS/audio/voice.wav";

fn open(bytes: &[u8]) -> Book {
    Book::open(Arc::new(bytes.to_vec()), &Typography::default()).unwrap()
}

/// The text a phrase reads, if it starts and ends in one paragraph.
fn said(book: &Book, phrase: &Phrase) -> String {
    let (start, end) = (phrase.start, phrase.end);
    let Content::Text(t) = &book.blocks()[start.block].content else {
        return String::new();
    };
    let until = match end.block == start.block {
        true => end.offset,
        false => t.text.len(),
    };
    t.text[start.offset..until].trim().to_owned()
}

#[test]
fn each_phrase_reads_its_own_words() {
    let book = open(NARRATED);
    let phrases = &book.narration().phrases;
    // Nine timed in the first chapter, one for all of the second.
    assert_eq!(phrases.len(), 10, "{phrases:#?}");
    let words: Vec<String> = phrases[..3].iter().map(|p| said(&book, p)).collect();
    let first = [
        "Read Aloud",
        "The first sentence is read.",
        "Then the second one follows.",
    ];
    assert_eq!(words, first);
    // A sentence ends mid-paragraph; the next starts in the same one.
    assert_eq!(phrases[1].start.block, phrases[2].start.block);
    assert!(phrases[1].end <= phrases[2].start);
    assert_eq!(
        said(&book, &phrases[4]),
        "A whole paragraph spoken at once."
    );
    assert_eq!(said(&book, &phrases[7]), "After the table.");
}

#[test]
fn phrases_carry_their_clips_in_seconds() {
    let book = open(NARRATED);
    let phrases = &book.narration().phrases;
    let clips: Vec<(f64, Option<f64>)> = (phrases.iter())
        .map(|p| (p.audio.begin, p.audio.end))
        .collect();
    assert_eq!(clips[0], (0.0, Some(0.4)));
    assert_eq!(clips[2], (0.8, Some(1.2)));
    assert_eq!(clips[4], (1.3, Some(1.6)));
    // The last plays to the end of its file.
    assert_eq!(clips[9], (2.0, None));
    assert!(phrases.iter().all(|p| p.audio.path == VOICE));
    let voice = book.media(VOICE).unwrap();
    assert_eq!((voice.extension, &voice.data[..4]), ("wav", &b"RIFF"[..]));
}

#[test]
fn cells_leave_together_and_notes_and_page_numbers_can_be_skipped() {
    let book = open(NARRATED);
    let phrases = &book.narration().phrases;
    let (one, two) = (&phrases[5], &phrases[6]);
    // Both cells are in one table block, which they span.
    assert_eq!((one.start, one.end), (two.start, two.end));
    assert_eq!(one.end.block, one.start.block + 1);
    assert_eq!((one.escape, two.escape), (Some(1), Some(1)));
    assert!(phrases.iter().filter(|p| p.escape.is_some()).count() == 2);
    assert!(phrases[3].skips.page_numbers && !phrases[3].skips.notes);
    assert!(phrases[8].skips.notes);
    let plain = phrases.iter().filter(|p| !p.skips.any()).count();
    assert_eq!(plain, 8);
}

#[test]
fn a_phrase_without_a_fragment_reads_its_whole_chapter() {
    let book = open(NARRATED);
    let last = book.narration().phrases.last().unwrap().clone();
    assert_eq!(said(&book, &last), "All at Once");
    assert_eq!(last.end.block, book.blocks().len());
    assert_eq!(last.start.offset, 0);
}

#[test]
fn the_book_names_its_narrator_and_length() {
    let book = open(NARRATED);
    let narration = book.narration();
    assert_eq!(narration.narrator.as_deref(), Some("A Tester"));
    assert_eq!(narration.duration, Some(3.0));
}

#[test]
fn a_book_without_overlays_has_no_narration() {
    let book = open(MEDIA);
    assert!(book.narration().phrases.is_empty());
}
