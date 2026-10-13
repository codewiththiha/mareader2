use std::time::{Duration, Instant};

use mareader_pdf::sample;

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
    let ticket = s.open(bytes, Some("/books/fallback-name.pdf".into()));
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
fn a_broken_file_reports_why() {
    let mut s = inline();
    let t = s.open(Arc::new(b"%PDF-garbage".to_vec()), None);
    let events = until(&mut s, |_| true);
    let failed = events.iter().any(
        |e| matches!(e, Event::Opened { ticket, result: Err(OpenError::Invalid) } if *ticket == t),
    );
    assert!(failed, "{events:?}");
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
