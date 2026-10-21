//! MathML set as pictures: in lines, as centred blocks and in table cells.
use std::sync::Arc;

use mareader_epub::Book;
use mareader_reflow::{Picture, Typography};

const MATH: &[u8] = include_bytes!("books/math.epub");

fn open() -> Book {
    Book::open(Arc::new(MATH.to_vec()), &Typography::default()).unwrap()
}

/// Page `page`'s lines as text, each with its first letter's rect.
fn lines(book: &Book, page: usize) -> Vec<(String, [f32; 4])> {
    let text = book.text(page).into_iter();
    text.map(|l| {
        let first = l.chars.first().map_or([0.0; 4], |c| c.rect);
        (l.chars.iter().map(|c| c.ch).collect(), first)
    })
    .collect()
}

fn line(book: &Book, page: usize, words: &str) -> [f32; 4] {
    let found = lines(book, page)
        .into_iter()
        .find(|(t, _)| t.contains(words));
    found.unwrap_or_else(|| panic!("no line with {words}")).1
}

/// Every picture drawn on page `page`, with the box its hits cover.
fn drawn(book: &Book, page: usize) -> Vec<(Arc<Picture>, [f32; 4])> {
    let (w, h) = book.page_size();
    let mut out: Vec<(Arc<Picture>, [f32; 4])> = Vec::new();
    let step = 3.0;
    let mut y = 0.0;
    while y < h {
        let mut x = 0.0;
        while x < w {
            if let Some(p) = book.picture_at(page, x, y) {
                match out.iter_mut().find(|(q, _)| Arc::ptr_eq(q, &p)) {
                    Some((_, r)) => {
                        let (right, bottom) = ((r[0] + r[2]).max(x), (r[1] + r[3]).max(y));
                        r[0] = r[0].min(x);
                        r[1] = r[1].min(y);
                        r[2] = right - r[0];
                        r[3] = bottom - r[1];
                    }
                    None => out.push((p, [x, y, 0.0, 0.0])),
                }
            }
            x += step;
        }
        y += step;
    }
    out
}

#[test]
fn formulas_draw_as_ink_pictures_not_text() {
    let book = open();
    assert_eq!(book.title(), Some("Lines of Formulas"));
    let all: String = lines(&book, 0).into_iter().map(|(t, _)| t).collect();
    // Their letters are drawn, never read into the text.
    assert!(!all.contains("zqzq"), "{all}");
    assert!(all.contains("Hidden letters"));
    let pictures = drawn(&book, 0);
    let formulas: Vec<_> = pictures.iter().filter(|(p, _)| p.vector()).collect();
    assert!(formulas.len() >= 5, "{}", formulas.len());
    // Black formulas are line art, drawn in the theme's ink.
    assert!(formulas.iter().all(|(p, _)| p.ink()));
    let kept = book.pictures(0);
    assert_eq!(kept.len(), 1, "only the fallback picture keeps colours");
}

#[test]
fn an_inline_formula_sits_on_its_line() {
    let book = open();
    let text = line(&book, 0, "Einstein wrote");
    let after = lines(&book, 0)
        .into_iter()
        .find(|(t, _)| t.contains("Einstein wrote"))
        .unwrap()
        .0;
    assert!(after.contains("in nineteen five"), "{after}");
    let pictures = drawn(&book, 0);
    let on_line = pictures.iter().find(|(p, r)| {
        let mid = r[1] + r[3] / 2.0;
        p.vector() && mid > text[1] - 4.0 && mid < text[1] + text[3] + 4.0 && r[0] > text[0]
    });
    let (_, r) = on_line.expect("a formula on the line");
    // About the letters' height, in a line box: neither towers nor vanishes.
    assert!(
        r[3] > text[3] * 0.4 && r[3] < text[3] * 1.5,
        "{r:?} {text:?}"
    );
    assert!(r[2] > r[3], "E = mc² is wider than tall: {r:?}");
}

#[test]
fn display_math_is_a_centred_block() {
    let book = open();
    let before = line(&book, 0, "Quadratic roots");
    let after = line(&book, 0, "Gauss summed");
    let pictures = drawn(&book, 0);
    let (_, r) = pictures
        .iter()
        .find(|(p, r)| p.vector() && r[1] > before[1] + before[3] && r[1] + r[3] < after[1])
        .expect("the quadratic formula between its paragraphs");
    let (w, _) = book.page_size();
    let centre = r[0] + r[2] / 2.0;
    assert!((centre - w / 2.0).abs() < 12.0, "{centre} of {w}");
    // A fraction over a root stands taller than a line of text.
    assert!(r[3] > before[3] * 1.2, "{r:?}");
}

#[test]
fn formulas_in_cells_are_drawn() {
    let book = open();
    let area = line(&book, 0, "Circle area");
    let matrix = line(&book, 0, "Matrix");
    let pictures = drawn(&book, 0);
    let beside = |row: [f32; 4]| {
        pictures.iter().any(|(p, r)| {
            let mid = r[1] + r[3] / 2.0;
            p.vector() && r[0] > row[0] + 40.0 && (mid - (row[1] + row[3] / 2.0)).abs() < 30.0
        })
    };
    assert!(beside(area), "pi r squared in its cell");
    assert!(beside(matrix), "the matrix in its cell");
}

#[test]
fn formulas_too_deep_fall_back() {
    let book = open();
    let all: String = lines(&book, 0).into_iter().map(|(t, _)| t).collect();
    assert!(all.contains("deeply: deep fallback words"), "{all}");
    // With a picture of its own, that picture stands in instead.
    assert!(!all.contains("pictured formula"), "{all}");
    assert_eq!(book.pictures(0).len(), 1);
}
