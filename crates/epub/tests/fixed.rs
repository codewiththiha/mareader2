//! Fixed-layout pages drawn at their own viewports, then a reflowed one.
use std::sync::Arc;

use mareader_epub::Book;
use mareader_reflow::{PageChar, Typography};

const FXL: &[u8] = include_bytes!("books/fxl.epub");

fn open() -> Book {
    Book::open(Arc::new(FXL.to_vec()), &Typography::default()).unwrap()
}

/// How much a page `width` by `height` px is scaled to fit the page box.
fn fit(book: &Book, (width, height): (f32, f32)) -> f32 {
    let (w, h) = book.page_size();
    (w / width).min(h / height)
}

/// Page `page`'s lines as text, with their letters, in drawing order.
fn lines(book: &Book, page: usize) -> Vec<(String, Vec<PageChar>)> {
    let text = book.text(page).into_iter();
    text.map(|l| (l.chars.iter().map(|c| c.ch).collect(), l.chars))
        .collect()
}

/// The first line holding `words`: its drawing order, first letter's rect.
fn line(book: &Book, page: usize, words: &str) -> (usize, [f32; 4]) {
    let all = lines(book, page);
    let (at, (_, chars)) = all
        .iter()
        .enumerate()
        .find(|(_, (t, _))| t.contains(words))
        .unwrap();
    (at, chars[0].rect)
}

fn near(a: f32, b: f32, room: f32) -> bool {
    (a - b).abs() <= room
}

#[test]
fn a_fixed_book_keeps_each_page_its_own_shape() {
    let book = open();
    assert_eq!(book.title(), Some("Pages as Drawn"));
    assert!(book.page_count() >= 5);
    assert!((0..4).all(|p| book.fixed(p)));
    let aspect = |p: usize| {
        let (w, h) = book.size(p);
        w / h
    };
    assert!(near(aspect(0), 0.75, 0.01), "{}", aspect(0));
    assert!(near(aspect(2), 1.6, 0.01), "{}", aspect(2));
    // A picture in the spine is a page the picture's shape.
    assert!(near(aspect(3), 1.5, 0.01), "{}", aspect(3));
    // Each fills the page box one way, never spilling out of it.
    let (w, h) = book.page_size();
    for p in 0..4 {
        let (pw, ph) = book.size(p);
        assert!(pw <= w + 0.5 && ph <= h + 0.5);
        assert!(near(pw, w, 0.5) || near(ph, h, 0.5), "{p}: {pw}x{ph}");
    }
}

#[test]
fn a_reflowable_item_in_a_fixed_book_reflows() {
    let book = open();
    let river = book.page_count() - 1;
    assert!(!book.fixed(river));
    assert_eq!(book.size(river), book.page_size());
    assert!(
        lines(&book, river)
            .iter()
            .any(|(t, _)| t.contains("Reflowing river"))
    );
}

#[test]
fn the_cover_on_the_far_side_shows_alone() {
    let book = open();
    assert!(!book.right_to_left());
    assert_eq!(book.lead(), 1);
    // The cover page is its picture, edge to edge.
    let s = fit(&book, (600.0, 800.0));
    let pictures = book.pictures(0);
    let [x, y, w, h] = pictures[0];
    assert!(near(x, 0.0, 1.0) && near(y, 0.0, 1.0), "{x} {y}");
    assert!(
        near(w, 600.0 * s, 1.0) && near(h, 800.0 * s, 1.0),
        "{w} {h}"
    );
}

#[test]
fn positioned_boxes_sit_where_the_book_puts_them() {
    let book = open();
    let s = fit(&book, (600.0, 800.0));
    // Left and top, inside the box's padding.
    let (_, [x, y, ..]) = line(&book, 1, "Lantern harbour");
    assert!(near(x, 110.0 * s, 3.0 * s), "{x}");
    assert!(y >= 205.0 * s && y <= 240.0 * s, "{y}");
    // Right: its right edge 20px from the page's.
    let (_, [x, ..]) = line(&book, 1, "Corner stamp");
    assert!(near(x, 460.0 * s, 3.0 * s), "{x}");
    // Bottom without top: its last line ends 40px above the page's foot.
    let all = lines(&book, 1);
    let foot = all.iter().find(|(t, _)| t.contains("ferry")).unwrap();
    let bottom = foot
        .1
        .iter()
        .map(|c| c.rect[1] + c.rect[3])
        .fold(0.0, f32::max);
    assert!(near(bottom, 760.0 * s, 10.0 * s), "{bottom}");
    // The flow runs from the top, past the boxes taken out of it.
    let (_, [_, first, ..]) = line(&book, 1, "Morning gulls");
    let (_, [_, next, ..]) = line(&book, 1, "Salt wind");
    assert!(first < 10.0 * s, "{first}");
    assert!(near(next - first, 30.0 * s, 3.0 * s), "{first} {next}");
}

#[test]
fn percentages_are_of_the_page_and_z_index_orders_painting() {
    let book = open();
    let s = fit(&book, (800.0, 500.0));
    let (_, [x, y, ..]) = line(&book, 2, "Grazing hills");
    assert!(near(x, 200.0 * s, 3.0 * s), "{x}");
    assert!(y >= 245.0 * s && y <= 270.0 * s, "{y}");
    // The flow first, then positioned boxes by z-index, ties in order.
    let at = |w: &str| line(&book, 1, w).0;
    let order = [
        at("Salt wind"),
        at("ferry"),
        at("Corner stamp"),
        at("Quiet tide"),
        at("Lantern harbour"),
    ];
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
}

#[test]
fn the_root_background_covers_the_whole_page() {
    let book = open();
    let pixels = book.render(1, 0.25, [0, 0, 8, 8]).unwrap();
    // The page's top left corner, where no text is: the sky picture.
    let at = (2 * 8 + 2) * 4;
    let rgba = &pixels.rgba[at..at + 4];
    assert_eq!(rgba[3], 255, "{rgba:?}");
    let sky = [40u8, 120, 200];
    let close = rgba.iter().zip(sky).all(|(a, b)| a.abs_diff(b) <= 8);
    assert!(close, "{rgba:?}");
}
