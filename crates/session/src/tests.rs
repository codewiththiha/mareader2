use std::time::{Duration, Instant};

use mareader_pdf::{LinkTarget, sample};

use super::*;

type Out = Result<(), Box<dyn std::error::Error>>;

const ALL: Budget = Budget {
    items: usize::MAX,
    bytes: usize::MAX,
};

fn inline() -> Session {
    Session::new(0, || {})
}

/// Polls until `pred` holds or two seconds pass.
fn until(s: &mut Session, mut pred: impl FnMut(&Event) -> bool) -> Vec<Event> {
    let start = Instant::now();
    let mut seen = Vec::new();
    while start.elapsed() < Duration::from_secs(2) {
        let batch = s.poll(ALL);
        let hit = batch.iter().any(&mut pred);
        seen.extend(batch);
        if hit {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    seen
}

fn open_sample(s: &mut Session, pages: usize, title: &str) -> Result<DocId, String> {
    let bytes = Arc::new(sample::pdf(pages, title));
    let ticket = s.open(
        Source::Bytes(bytes),
        Some("/books/fallback-name.pdf".into()),
        Arc::default(),
    );
    let events = until(s, |e| matches!(e, Event::Opened { .. }));
    events
        .into_iter()
        .find_map(|e| match e {
            Event::Opened { ticket: t, result } if t == ticket => Some(result),
            _ => None,
        })
        .ok_or("never opened")?
        .map_err(|e| e.to_string())
}

fn key(doc: DocId, page: usize) -> PageKey {
    PageKey {
        doc,
        page,
        scale: Scale::covering(0.25),
        part: Part::Whole,
    }
}

fn rendered(events: &[Event]) -> Vec<usize> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Rendered { key, result: Ok(_) } => Some(key.page),
            _ => None,
        })
        .collect()
}

#[test]
fn opening_names_the_document_by_title_or_file() -> Out {
    let mut s = inline();
    let id = open_sample(&mut s, 2, "A Field Guide")?;
    assert_eq!(s.get(id).map(|e| e.name.as_str()), Some("A Field Guide"));
    let junk = open_sample(&mut s, 2, "untitled")?;
    assert_eq!(s.get(junk).map(|e| e.name.as_str()), Some("fallback-name"));
    assert_eq!(s.len(), 2);
    Ok(())
}

#[test]
fn a_broken_or_missing_file_reports_why() {
    let mut s = inline();
    let t = s.open(
        Source::Bytes(Arc::new(b"%PDF-garbage".to_vec())),
        None,
        Arc::default(),
    );
    let missing = s.open(
        Source::Path("/nowhere/at/all.pdf".into()),
        None,
        Arc::default(),
    );
    let mut events = s.poll(ALL);
    events.extend(s.poll(ALL));
    let error = |want: Ticket| {
        events.iter().find_map(|e| match e {
            Event::Opened {
                ticket,
                result: Err(e),
            } if *ticket == want => Some(e.clone()),
            _ => None,
        })
    };
    let cut = OpenError::Pdf(mareader_pdf::OpenError::CutShort);
    assert_eq!(error(t), Some(cut));
    assert!(error(t).and_then(|e| e.hint()).is_some());
    assert!(matches!(error(missing), Some(OpenError::Unreadable(_))));
    assert!(s.is_empty());
}

#[test]
fn only_the_latest_demand_is_drawn() -> Out {
    let mut s = inline();
    let id = open_sample(&mut s, 9, "Pages")?;
    s.want([key(id, 0), key(id, 1), key(id, 2)]);
    // The reader scrolled on before any of those were drawn.
    s.want([key(id, 7), key(id, 8)]);
    let mut got = Vec::new();
    for _ in 0..6 {
        got.extend(rendered(&s.poll(ALL)));
    }
    assert_eq!(got, [7, 8]);
    assert_eq!(s.pending(), 0);
    Ok(())
}

#[test]
fn text_is_read_beside_rasters_and_dropped_when_unwanted() -> Out {
    let mut s = inline();
    let id = open_sample(&mut s, 6, "Text")?;
    let text = |page| Want::Text { doc: id, page };
    s.want([text(0), text(1)]);
    // Scrolled on: page 1's text and a raster are wanted now.
    s.want([Want::from(key(id, 4)), text(3)]);
    let mut events = Vec::new();
    for _ in 0..6 {
        events.extend(s.poll(ALL));
    }
    let read: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Text {
                page,
                result: Ok(t),
                ..
            } => Some((*page, t.links.len())),
            _ => None,
        })
        .collect();
    assert_eq!(read, [(3, 2)]);
    assert_eq!(rendered(&events), [4]);
    assert_eq!(s.pending(), 0);
    Ok(())
}

#[test]
fn paper_is_read_for_wanted_pages_and_dropped_for_closed_ones() -> Out {
    let mut s = inline();
    let id = open_sample(&mut s, 3, "Paper")?;
    s.want([Want::Paper { doc: id, page: 1 }]);
    let events = until(&mut s, |e| matches!(e, Event::Paper { .. }));
    let read: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Paper {
                page,
                result: Ok(p),
                ..
            } => Some((*page, p.whole.map(|c| (c.0, c.1, c.2)))),
            _ => None,
        })
        .collect();
    assert_eq!(read, [(1, Some((255, 255, 255)))]);
    // Asked for, then closed before the poll: nothing arrives.
    s.want([Want::Paper { doc: id, page: 2 }]);
    s.close(id);
    assert!(s.poll(ALL).is_empty());
    Ok(())
}

#[test]
fn a_poll_hands_over_no_more_than_its_budget() -> Out {
    let mut s = Session::new(2, || {});
    let id = open_sample(&mut s, 4, "Pages")?;
    s.want((0..4).map(|p| key(id, p)));
    let start = Instant::now();
    while s.pending() > 0 && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let one = Budget {
        items: 1,
        bytes: usize::MAX,
    };
    assert_eq!(rendered(&s.poll(one)).len(), 1);
    let tiny = Budget { items: 9, bytes: 1 };
    assert_eq!(rendered(&s.poll(tiny)).len(), 1, "always one");
    assert_eq!(rendered(&s.poll(ALL)).len(), 2);
    Ok(())
}

#[test]
fn workers_draw_pages_in_the_background() -> Out {
    let mut s = Session::new(2, || {});
    let id = open_sample(&mut s, 5, "Threads")?;
    s.want((0..5).map(|p| key(id, p)));
    let mut got = Vec::new();
    let start = Instant::now();
    while got.len() < 5 && start.elapsed() < Duration::from_secs(5) {
        got.extend(rendered(&s.poll(ALL)));
        std::thread::sleep(Duration::from_millis(2));
    }
    got.sort_unstable();
    assert_eq!(got, [0, 1, 2, 3, 4]);
    Ok(())
}

#[test]
fn closing_drops_the_document_and_its_late_pages() -> Out {
    let mut s = inline();
    let id = open_sample(&mut s, 3, "Gone")?;
    s.want([key(id, 0)]);
    s.close(id);
    assert!(rendered(&s.poll(ALL)).is_empty());
    assert!(s.get(id).is_none());
    // Demand for a closed document is ignored outright.
    s.want([key(id, 1)]);
    assert_eq!(s.pending(), 0);
    Ok(())
}

#[test]
fn a_long_book_costs_only_what_is_on_screen() -> Out {
    let mut s = inline();
    let start = Instant::now();
    let id = open_sample(&mut s, 10_000, "Long")?;
    let opened = start.elapsed();
    let doc = s.get(id).map(|e| Arc::clone(&e.doc)).ok_or("open")?;
    assert_eq!(doc.page_count(), 10_000);
    s.want((5_000..5_004).map(|p| key(id, p)));
    assert_eq!(s.pending(), 4);
    assert!(opened < Duration::from_secs(10), "opened in {opened:?}");
    Ok(())
}

#[test]
fn a_path_opens_on_a_worker_and_names_the_document() -> Out {
    let name = format!("mareader-session-{}", std::process::id());
    let dir = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("Field Notes.pdf");
    std::fs::write(&path, sample::pdf(1, "untitled"))?;
    let mut s = Session::new(1, || {});
    s.open(Source::Path(path), None, Arc::default());
    let events = until(&mut s, |e| matches!(e, Event::Opened { .. }));
    let id = events.iter().find_map(|e| match e {
        Event::Opened { result: Ok(id), .. } => Some(*id),
        _ => None,
    });
    let got = id.and_then(|id| s.get(id)).map(|e| e.name.clone());
    assert_eq!(got.as_deref(), Some("Field Notes"));
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn finished_pages_wait_in_a_bounded_queue() -> Out {
    let mut s = Session::new(2, || {});
    let id = open_sample(&mut s, 60, "Many")?;
    s.want((0..60).map(|p| key(id, p)));
    let start = Instant::now();
    // Workers stop at the cap until someone polls.
    while s.pending() > 60 - 32 && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(s.pending(), 60 - 32);
    let mut got = 0;
    while got < 60 && start.elapsed() < Duration::from_secs(5) {
        got += rendered(&s.poll(ALL)).len();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(got, 60);
    Ok(())
}

#[test]
fn an_epub_opens_by_its_bytes_and_draws_its_pages() -> Out {
    let mut s = inline();
    let bytes = Arc::new(mareader_epub::sample::epub(2, "Book"));
    let ticket = s.open(
        Source::Bytes(bytes),
        Some("book.bin".into()),
        Arc::default(),
    );
    let events = until(&mut s, |e| matches!(e, Event::Opened { .. }));
    let opened = events.into_iter().find_map(|e| match e {
        Event::Opened { ticket: t, result } if t == ticket => Some(result),
        _ => None,
    });
    let id = opened.ok_or("never opened")??;
    let entry = s.get(id).ok_or("no entry")?;
    assert_eq!(entry.name, "Book");
    assert!(entry.doc.is_reflowable());
    let outline: Vec<(&str, u8)> = entry
        .doc
        .outline()
        .iter()
        .map(|e| (e.title.as_str(), e.depth))
        .collect();
    let listed = [
        ("Chapter 1", 0),
        ("Part 1.2", 1),
        ("Chapter 2", 0),
        ("Part 2.2", 1),
    ];
    assert_eq!(outline, listed);
    let size = entry.doc.page_sizes().first().copied().ok_or("no pages")?;
    let raster = entry.doc.render(0, 0.5)?;
    assert_eq!(
        (raster.width, raster.height),
        ((size.width * 0.5) as u32, (size.height * 0.5) as u32)
    );
    let tile = entry.doc.render_tile(0, 4.0, 1, 1)?;
    assert!(tile.width > 0 && tile.height > 0);
    // The cover is a picture and no text; the chapter reads back.
    assert_eq!(entry.doc.text_layer(0)?, TextLayer::default());
    assert_eq!(entry.doc.pictures(0).len(), 1);
    assert!(entry.doc.text_layer(9999).is_err());
    let text = entry.doc.text_layer(1)?.text;
    let words = text.text(0..text.len());
    assert!(
        words.starts_with("Chapter 1\n1.0 The river ran slow"),
        "{words}"
    );
    assert!(words.contains("the boats unloaded rice"), "{words}");
    Ok(())
}

#[test]
fn a_book_s_contents_and_links_follow_its_setting() -> Out {
    let mut s = inline();
    let book = open_epub(&mut s, 2)?;
    let entry = s.get(book).ok_or("no entry")?;
    let chapter_two = |doc: &Document| {
        let e = doc.outline().iter().find(|e| e.title == "Chapter 2");
        e.map(|e| e.page)
    };
    let before = chapter_two(&entry.doc).ok_or("no chapter 2")?;
    let mut targets = Vec::new();
    for page in 0..entry.doc.page_count() {
        let layer = entry.doc.text_layer(page)?;
        targets.extend(layer.links.into_iter().map(|l| l.target));
    }
    assert!(targets.contains(&LinkTarget::Page(before)), "{targets:?}");
    let web = |t: &LinkTarget| matches!(t, LinkTarget::Web(u) if u.contains("github.com"));
    assert!(targets.iter().any(web), "{targets:?}");
    // Bigger type: the chapter, and the link to it, move on.
    s.retypeset(book, typed(30.0), 0);
    let events = until(&mut s, |e| matches!(e, Event::Retypeset { .. }));
    let (_, new, _) = swaps(&events).first().copied().ok_or("no swap")?;
    let doc = s.get(new).map(|e| Arc::clone(&e.doc)).ok_or("no entry")?;
    let after = chapter_two(&doc).ok_or("no chapter 2")?;
    assert!(after > before, "{after} > {before}");
    let page = (0..doc.page_count()).find(|&p| {
        doc.text_layer(p)
            .is_ok_and(|l| l.links.iter().any(|l| l.target == LinkTarget::Page(after)))
    });
    assert!(page.is_some(), "a link leads to page {after}");
    Ok(())
}

fn open_epub(s: &mut Session, chapters: usize) -> Result<DocId, String> {
    let bytes = Arc::new(mareader_epub::sample::epub(chapters, "Book"));
    let ticket = s.open(Source::Bytes(bytes), None, Arc::default());
    let events = until(s, |e| matches!(e, Event::Opened { .. }));
    events
        .into_iter()
        .find_map(|e| match e {
            Event::Opened { ticket: t, result } if t == ticket => Some(result),
            _ => None,
        })
        .ok_or("never opened")?
        .map_err(|e| e.to_string())
}

#[test]
fn an_epub_previews_the_requested_page_then_completes_in_the_background() -> Out {
    let mut s = inline();
    let bytes = Arc::new(mareader_epub::sample::epub(8, "Progress"));
    let target = 8;
    let ticket = s.open_at(
        Source::Bytes(bytes),
        None,
        Arc::default(),
        OpenTarget {
            page: target,
            place: None,
        },
    );
    let events = until(&mut s, |e| matches!(e, Event::Preview { .. }));
    let doc = events.iter().find_map(|e| match e {
        Event::Preview { ticket: t, doc } if *t == ticket => Some(*doc),
        _ => None,
    });
    let doc = doc.ok_or("the requested page was not previewed")?;
    let partial = s.get(doc).ok_or("no preview entry")?;
    assert!(!partial.doc.is_complete());
    assert!(partial.doc.page_count() > target + 2);
    assert_eq!(s.pending(), 0, "continuation waits for the target raster");
    assert!(s.poll(ALL).is_empty());

    s.want([key(doc, target), key(doc, target + 1)]);
    let first = s.poll(ALL);
    assert_eq!(rendered(&first), [target]);
    let second = s.poll(ALL);
    assert_eq!(rendered(&second), [target + 1]);

    let events = until(&mut s, |e| matches!(e, Event::Completed { .. }));
    let mut latest = doc;
    let mut pages = s.get(doc).map_or(0, |e| e.doc.page_count());
    let mut grew = false;
    for event in &events {
        if let Event::Updated {
            ticket: got,
            old,
            new,
        } = event
            && *got == ticket
        {
            assert_eq!(*old, latest);
            let before = s.get(*old).ok_or("no previous partial")?.doc.page_count();
            let after = s.get(*new).ok_or("no updated partial")?;
            assert!(!after.doc.is_complete());
            assert!(after.doc.page_count() >= before);
            grew |= after.doc.page_count() > pages;
            pages = after.doc.page_count();
            latest = *new;
        }
    }
    let completed = events.iter().find_map(|e| match e {
        Event::Completed {
            ticket: t,
            old,
            new,
            ..
        } if *t == ticket => Some((*old, *new)),
        _ => None,
    });
    let (old, new) = completed.ok_or("the spine did not complete")?;
    assert!(grew, "partial documents gain pages before completion");
    assert_eq!(old, latest);
    assert!(s.get(old).is_some());
    assert!(s.get(new).is_some_and(|e| e.doc.is_complete()));
    assert!(s.get(new).is_some_and(|e| e.doc.page_count() > target + 2));
    Ok(())
}

fn swaps(events: &[Event]) -> Vec<(DocId, DocId, usize)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Retypeset { old, new, previous } => Some((*old, *new, previous.page_count())),
            _ => None,
        })
        .collect()
}

fn typed(font_size: f32) -> Arc<Typography> {
    let text = mareader_settings::TextSettings {
        font_size,
        ..Default::default()
    };
    Arc::new(typography(&text))
}

#[test]
fn a_newer_retypeset_in_a_slot_replaces_the_older() -> Out {
    let mut s = inline();
    let book = open_epub(&mut s, 4)?;
    s.retypeset(book, typed(22.0), 0);
    s.retypeset(book, typed(26.0), 0);
    let mut events = until(&mut s, |e| matches!(e, Event::Retypeset { .. }));
    while s.pending() > 0 {
        events.extend(s.poll(ALL));
    }
    let swaps = swaps(&events);
    assert_eq!(swaps.len(), 1, "{swaps:?}");
    let (old, new, before) = swaps.first().copied().ok_or("no swap")?;
    assert_eq!(old, book);
    assert!(s.get(book).is_some(), "the old book stays for other panes");
    let entry = s.get(new).ok_or("no entry")?;
    assert!(entry.doc.page_count() > before);
    assert_eq!((entry.name.as_str(), entry.typo.font_size), ("Book", 26.0));
    Ok(())
}

#[test]
fn each_slot_sets_one_book_in_its_own_type() -> Out {
    let mut s = inline();
    let book = open_epub(&mut s, 4)?;
    s.retypeset(book, typed(22.0), 1);
    s.retypeset(book, typed(30.0), 2);
    let mut events = until(&mut s, |e| matches!(e, Event::Retypeset { .. }));
    while s.pending() > 0 {
        events.extend(s.poll(ALL));
    }
    let mut sizes: Vec<f32> = swaps(&events)
        .iter()
        .filter_map(|(_, new, _)| s.get(*new).map(|e| e.typo.font_size))
        .collect();
    // Urgent jobs run newest first; the order doesn't matter.
    sizes.sort_by(f32::total_cmp);
    assert_eq!(sizes, [22.0, 30.0]);
    // Closing the book drops what is still asked of it.
    s.retypeset(book, typed(40.0), 1);
    s.close(book);
    while s.pending() > 0 {
        assert!(s.poll(ALL).is_empty());
    }
    Ok(())
}

#[test]
fn a_book_opens_in_the_type_asked() -> Out {
    let mut s = inline();
    let small = open_epub(&mut s, 2)?;
    let bytes = Arc::new(mareader_epub::sample::epub(2, "Book"));
    let ticket = s.open(Source::Bytes(bytes), None, typed(30.0));
    let events = until(&mut s, |e| matches!(e, Event::Opened { .. }));
    let big = events.into_iter().find_map(|e| match e {
        Event::Opened { ticket: t, result } if t == ticket => result.ok(),
        _ => None,
    });
    let pages = |id| s.get(id).map_or(0, |e| e.doc.page_count());
    let big = big.ok_or("never opened")?;
    assert!(
        pages(big) > pages(small),
        "{} > {}",
        pages(big),
        pages(small)
    );
    assert_eq!(s.get(big).map(|e| e.typo.font_size), Some(30.0));
    Ok(())
}

/// Opens `source` named `name`; the document's id, or why not.
fn open_named(s: &mut Session, source: Source, name: &str) -> Result<DocId, String> {
    let ticket = s.open(source, Some(name.into()), Arc::default());
    let events = until(s, |e| matches!(e, Event::Opened { .. }));
    let opened = events.into_iter().find_map(|e| match e {
        Event::Opened { ticket: t, result } if t == ticket => Some(result),
        _ => None,
    });
    opened.ok_or("never opened")?.map_err(|e| e.to_string())
}

#[test]
fn markdown_and_text_open_by_name_as_converted_books() -> Out {
    let mut s = inline();
    let md = b"# The River\n\nBoats at dawn.\n\n## The Jetty\n\nNobody asked.\n";
    let id = open_named(&mut s, Source::Bytes(Arc::new(md.to_vec())), "notes.md")?;
    let entry = s.get(id).ok_or("no entry")?;
    assert!(entry.doc.is_reflowable());
    assert_eq!(entry.name, "The River");
    let outline: Vec<(&str, u8)> = entry
        .doc
        .outline()
        .iter()
        .map(|e| (e.title.as_str(), e.depth))
        .collect();
    assert_eq!(outline, [("The River", 0), ("The Jetty", 1)]);
    let txt = b"The river ran slow and brown past the old customs house.";
    let id = open_named(&mut s, Source::Bytes(Arc::new(txt.to_vec())), "/b/War.TXT")?;
    let entry = s.get(id).ok_or("no entry")?;
    assert!(entry.doc.is_reflowable());
    assert_eq!(entry.name, "War");
    // The same bytes under another name aren't converted.
    let other = open_named(&mut s, Source::Bytes(Arc::new(md.to_vec())), "notes.bin");
    assert!(other.is_err());
    Ok(())
}

/// A one-pixel PNG.
const DOT: [u8; 70] = [
    0x89, 0x50, 0x4e, 0x47, 0xd, 0xa, 0x1a, 0xa, 0x0, 0x0, 0x0, 0xd, 0x49, 0x48, 0x44, 0x52, 0x0,
    0x0, 0x0, 0x1, 0x0, 0x0, 0x0, 0x1, 0x8, 0x6, 0x0, 0x0, 0x0, 0x1f, 0x15, 0xc4, 0x89, 0x0, 0x0,
    0x0, 0xd, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0x60, 0xf8, 0x5f, 0xf, 0x0, 0x2,
    0x87, 0x1, 0x80, 0xeb, 0x47, 0xba, 0x92, 0x0, 0x0, 0x0, 0x0, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

#[test]
fn a_markdown_file_s_pictures_are_read_from_beside_it() -> Out {
    let dir = std::env::temp_dir().join(format!("mareader-md-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("pics"))?;
    std::fs::write(dir.join("pics/dot.png"), DOT)?;
    let md = dir.join("notes.md");
    std::fs::write(
        &md,
        "# Dots\n\n![a dot](pics/dot.png)\n\n![gone](pics/none.png)\n",
    )?;
    let mut s = inline();
    let name = md.to_string_lossy().into_owned();
    let id = open_named(&mut s, Source::Path(md), &name);
    let _ = std::fs::remove_dir_all(&dir);
    let entry = s.get(id?).ok_or("no entry")?;
    let Document::Epub { book, .. } = entry.doc.as_ref() else {
        return Err("not a book".into());
    };
    let pictures: usize = (0..book.page_count()).map(|p| book.pictures(p).len()).sum();
    assert_eq!(pictures, 1);
    Ok(())
}
