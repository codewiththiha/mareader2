use std::sync::Arc;

use mareader_reflow::Typography;

use super::*;
use crate::{Book, Destination};

fn none(_: &str) -> Option<Vec<u8>> {
    None
}

fn open(text: &str, format: Format, name: &str) -> Book {
    let epub = convert(text.as_bytes(), format, Some(name), &none).unwrap();
    Book::open(Arc::new(epub), &Typography::default()).unwrap()
}

/// Every page's text, pages split by a form feed.
fn text(book: &Book) -> String {
    let pages: Vec<String> = (0..book.page_count())
        .map(|p| {
            let lines = book.text(p).into_iter();
            let lines = lines.map(|l| l.chars.iter().map(|c| c.ch).collect::<String>());
            lines.collect::<Vec<_>>().join("\n")
        })
        .collect();
    pages.join("\u{c}")
}

fn titles(book: &Book) -> Vec<(String, usize)> {
    book.toc()
        .iter()
        .map(|e| (e.title.clone(), e.depth))
        .collect()
}

/// A paragraph wrapped at `width` columns, as old text files are.
fn wrap(words: &str, width: usize) -> String {
    let mut lines = vec![String::new()];
    for w in words.split_whitespace() {
        let line = lines.last_mut().unwrap();
        if !line.is_empty() && line.len() + 1 + w.len() > width {
            lines.push(w.to_owned());
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(w);
        }
    }
    lines.join("\n")
}

const PROSE: &str = "The river ran slow and brown past the old customs house, where \
    the boats unloaded rice and timber at dawn. Nobody remembered who had built the \
    jetty, and nobody asked, for the jetty had always been there and always would be.";

#[test]
fn formats_are_told_by_extension_as_mareader_names_them() {
    assert_eq!(Format::of_name("notes.MD"), Some(Format::Markdown));
    assert_eq!(
        Format::of_name("a/b/readme.markdown"),
        Some(Format::Markdown)
    );
    assert_eq!(Format::of_name("x.mdown"), Some(Format::Markdown));
    assert_eq!(Format::of_name("war.txt"), Some(Format::Plain));
    assert_eq!(Format::of_name("war.text"), Some(Format::Plain));
    assert_eq!(Format::of_name("book.epub"), None);
    assert_eq!(Format::of_name("README"), None);
}

#[test]
fn text_decodes_from_its_bom_utf_8_or_a_guessed_legacy_encoding() {
    assert_eq!(decode(b"\xEF\xBB\xBFcaf\xC3\xA9"), "café");
    assert_eq!(decode(b"\xFF\xFEc\0a\0f\0\xE9\0"), "café");
    assert_eq!(decode("plain café".as_bytes()), "plain café");
    let french = "Le café était fermé, mais la gérante a ouvert la fenêtre et \
        servi un thé très chaud à l'étranger qui attendait près de l'église.";
    let (latin, _, _) = encoding_rs::WINDOWS_1252.encode(french);
    assert_eq!(decode(&latin), french);
    let chinese = "第一章 河流\n河水缓慢地流过老海关大楼，船只在黎明时分卸下大米和木材。\
        没有人记得是谁建造了码头，也没有人问起。";
    let (gbk, _, _) = encoding_rs::GBK.encode(chinese);
    assert_eq!(decode(&gbk), chinese);
}

#[test]
fn empty_and_oversized_files_are_refused_as_epubs_are() {
    let err = convert(b" \n\n \t\n", Format::Plain, None, &none);
    assert_eq!(err.err(), Some(OpenError::Empty));
    let huge = vec![b'a'; MAX_TEXT + 1];
    let err = convert(&huge, Format::Markdown, None, &none);
    assert_eq!(err.err(), Some(OpenError::TooLarge));
}

#[test]
fn a_text_file_is_a_real_epub_titled_by_its_name() {
    let epub = convert(
        PROSE.as_bytes(),
        Format::Plain,
        Some("/x/The River.txt"),
        &none,
    );
    let epub = epub.unwrap();
    assert!(crate::sniff(&epub));
    let book = Book::open(Arc::new(epub), &Typography::default()).unwrap();
    assert_eq!(book.title(), Some("The River"));
    // No headings: the one entry is the book.
    assert_eq!(titles(&book), vec![("The River".to_owned(), 0)]);
}

#[test]
fn wrapped_prose_joins_into_paragraphs_and_verse_keeps_its_lines() {
    let prose = format!("{}\n\n{}", wrap(PROSE, 72), wrap(PROSE, 72));
    let verse = "Once upon a midnight dreary, while I pondered, weak and weary,\n\
        Over many a quaint and curious volume of forgotten lore,\n\
        While I nodded, nearly napping, suddenly there came a tapping,";
    let d = plain::draft(&format!("{prose}\n\n{verse}"), None);
    let body = d.chapters.concat();
    assert!(body.contains("customs house, where the boats"), "{body}");
    assert!(body.contains("weak and weary,<br/>Over many"), "{body}");
    assert!(
        body.contains("forgotten lore,<br/>While I nodded"),
        "{body}"
    );
    assert_eq!(body.matches("<br/>").count(), 2, "{body}");
}

#[test]
fn a_word_hyphenated_at_a_wrap_joins_without_a_space() {
    // Wrapped at 41: each break falls where the next word wouldn't fit.
    let text = "It was a long road, and a very well-\n\
        known one, down to the old customs house\n\
        where the boats came in with rice at dawn";
    let book = open(text, Format::Plain, "t.txt");
    assert!(
        text_of(&book).contains("very well-known one"),
        "{}",
        text_of(&book)
    );
}

fn text_of(book: &Book) -> String {
    text(book).replace('\n', " ")
}

#[test]
fn chapter_headings_make_the_contents_and_start_pages() {
    let para = wrap(PROSE, 70);
    let src = format!(
        "THE RIVER\n\n{para}\n\nPART ONE\n\nCHAPTER I.\n\n{para}\n\nChapter 2: The Jetty\n\n\
         {para}\n\nIV.\n\n{para}\n\nEpilogue\n\n{para}\n\n# not a heading\n\n{para}"
    );
    let book = open(&src, Format::Plain, "t.txt");
    let toc = titles(&book);
    let expected = [
        ("PART ONE", 0),
        ("CHAPTER I.", 1),
        ("Chapter 2: The Jetty", 1),
        ("IV.", 1),
        ("Epilogue", 1),
    ];
    let expected: Vec<(String, usize)> =
        expected.iter().map(|(t, d)| (t.to_string(), *d)).collect();
    assert_eq!(toc, expected);
    // Each heading opens its own page.
    let pages: Vec<usize> = book.toc().iter().map(|e| book.locate(e.at)).collect();
    assert!(pages.windows(2).all(|w| w[0] < w[1]), "{pages:?}");
    // Markup in plain text stays text.
    assert!(text(&book).contains("# not a heading"));
}

#[test]
fn a_line_that_only_starts_like_a_heading_stays_prose() {
    for line in [
        "Chapter and verse, he said.",
        "Book lovers came early",
        "Part of it",
    ] {
        assert_eq!(
            plain::draft(line, None)
                .toc
                .first()
                .map(|e| e.title.as_str()),
            Some("Untitled")
        );
    }
}

#[test]
fn chinese_chapter_headings_are_found() {
    let src =
        "第一卷 起\n\n第一章 河流\n\n河水缓慢地流过老海关大楼。\n\n第二章 码头\n\n没有人记得。";
    let book = open(src, Format::Plain, "小说.txt");
    let toc = titles(&book);
    assert_eq!(
        toc.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
        ["第一卷 起", "第一章 河流", "第二章 码头"]
    );
    assert_eq!(toc.iter().map(|(_, d)| *d).collect::<Vec<_>>(), [0, 1, 1]);
}

#[test]
fn one_text_always_packs_to_the_same_bytes() {
    let pack = || convert(PROSE.as_bytes(), Format::Plain, Some("a.txt"), &none).unwrap();
    assert_eq!(pack(), pack());
}

#[test]
fn files_without_blank_lines_take_a_line_or_an_indent_as_a_paragraph() {
    let lines: Vec<String> = (0..40)
        .map(|i| format!("Line {i} is a short sentence."))
        .collect();
    let d = plain::draft(&lines.join("\n"), None);
    let body = d.chapters.concat();
    assert_eq!(body.matches("<p>").count(), 40);
    let printed: Vec<String> = (0..20)
        .map(|i| format!("    Paragraph {i} starts indented and\nruns on to a second line."))
        .collect();
    let d = plain::draft(&printed.join("\n"), None);
    let body = d.chapters.concat();
    assert_eq!(body.matches("<p>").count(), 20);
    assert!(body.contains("indented and runs on"), "{body}");
}

const MD: &str = "---
title: \"The River Book\"
author:
  - Ann Writer
lang: en
---

Before the first chapter.

# The River

Read on [to the jetty](#the-jetty) or see a note.[^dawn]

- [x] Unload the rice
- [ ] Count the boats

> [!NOTE]
> Boats come at dawn.

## Customs House

# The Jetty

Back [to the river](#the-river).

### A deep part

#### Too deep for the contents

[^dawn]: Before the sun is up.
";

#[test]
fn markdown_takes_its_title_author_and_language_from_front_matter() {
    let d = markdown::draft(MD, Some("file".into()), &none);
    assert_eq!(d.title, "The River Book");
    assert_eq!(d.author.as_deref(), Some("Ann Writer"));
    assert_eq!(d.language.as_deref(), Some("en"));
    let book = open(MD, Format::Markdown, "file.md");
    assert_eq!(book.title(), Some("The River Book"));
    assert!(
        !text(&book).contains("Ann Writer"),
        "front matter isn't shown"
    );
}

#[test]
fn markdown_without_front_matter_is_titled_by_its_first_heading() {
    let d = markdown::draft(
        "Some intro.\n\n## Notes on boats\n\ntext",
        Some("f".into()),
        &none,
    );
    assert_eq!(d.title, "Notes on boats");
    let d = markdown::draft("just text", Some("file name".into()), &none);
    assert_eq!(d.title, "file name");
}

#[test]
fn each_top_heading_opens_a_chapter_and_headings_make_the_contents() {
    let d = markdown::draft(MD, None, &none);
    // The text before the first `#` is a chapter of its own.
    assert_eq!(d.chapters.len(), 3);
    let book = open(MD, Format::Markdown, "file.md");
    let toc = titles(&book);
    let expected = [
        ("The River", 0),
        ("Customs House", 1),
        ("The Jetty", 0),
        ("A deep part", 1),
    ];
    let expected: Vec<(String, usize)> =
        expected.iter().map(|(t, d)| (t.to_string(), *d)).collect();
    assert_eq!(toc, expected);
    let page = |i: usize| book.locate(book.toc()[i].at);
    assert!(page(0) < page(2), "the jetty starts its own page");
}

#[test]
fn links_to_headings_in_other_chapters_land_on_their_pages() {
    let book = open(MD, Format::Markdown, "file.md");
    let page = |i: usize| book.locate(book.toc()[i].at);
    let (river, jetty) = (page(0), page(2));
    let to = |p: usize| -> Vec<Destination> { book.links(p).into_iter().map(|l| l.to).collect() };
    assert!(
        to(river).contains(&Destination::Page(jetty)),
        "{:?}",
        to(river)
    );
    assert!(
        to(jetty).contains(&Destination::Page(river)),
        "{:?}",
        to(jetty)
    );
}

#[test]
fn footnotes_link_across_chapters_and_tasks_show_their_boxes() {
    let book = open(MD, Format::Markdown, "file.md");
    let all = text(&book);
    assert!(all.contains("\u{25a0} Unload the rice"), "{all}");
    assert!(all.contains("\u{25a1} Count the boats"), "{all}");
    let d = markdown::draft(MD, None, &none);
    let unmarked = d
        .chapters
        .concat()
        .matches("<li style=\"list-style: none\">")
        .count();
    assert_eq!(unmarked, 2, "a task's box is its only marker");
    assert!(all.contains("Note\nBoats come at dawn."), "{all}");
    assert!(all.contains("1 Before the sun is up."), "{all}");
    let river = book.locate(book.toc()[0].at);
    let note_page = (0..book.page_count())
        .find(|&p| text_on(&book, p).contains("Before the sun"))
        .unwrap();
    assert!(note_page > river);
    let links: Vec<Destination> = book.links(river).into_iter().map(|l| l.to).collect();
    assert!(links.contains(&Destination::Page(note_page)), "{links:?}");
}

fn text_on(book: &Book, page: usize) -> String {
    let lines = book.text(page).into_iter();
    lines
        .flat_map(|l| l.chars.into_iter().map(|c| c.ch))
        .collect()
}

#[test]
fn local_pictures_are_packed_and_remote_ones_keep_their_alt_text() {
    let mut png = Vec::new();
    let img = image::RgbImage::from_pixel(40, 30, image::Rgb([200, 40, 40]));
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let asked = std::cell::RefCell::new(Vec::new());
    let read = |path: &str| {
        asked.borrow_mut().push(path.to_owned());
        (path == "pics/red dot.png").then(|| png.clone())
    };
    let md = "# Pictures\n\n![a red dot](pics/red%20dot.png)\n\n![](https://x.org/a.png \"web\")\n\n\
              ![missing one](nowhere.png)\n";
    let d = markdown::draft(md, None, &read);
    assert_eq!(d.pictures.len(), 1);
    assert_eq!(*asked.borrow(), ["pics/red dot.png", "nowhere.png"]);
    let epub = convert(md.as_bytes(), Format::Markdown, Some("p.md"), &read).unwrap();
    let book = Book::open(Arc::new(epub), &Typography::default()).unwrap();
    let pictures: usize = (0..book.page_count()).map(|p| book.pictures(p).len()).sum();
    assert_eq!(pictures, 1);
    assert!(
        text(&book).contains("missing one"),
        "a refused picture shows its alt"
    );
}

#[test]
fn markdown_keeps_what_epubs_show_tables_code_and_lists() {
    let md = "# T\n\n| Cargo | Boats |\n|---|---|\n| Rice | 12 |\n\n```rust\nlet boats = 12;\n```\n\n\
              1. one\n2. two\n\nTerm\n: its definition\n\n~~gone~~ and *kept* <b>raw</b>\n";
    let book = open(md, Format::Markdown, "t.md");
    let all = text(&book);
    for want in [
        "Cargo",
        "Rice",
        "12",
        "let boats = 12;",
        "one",
        "two",
        "its definition",
        "gone and kept raw",
    ] {
        assert!(all.contains(want), "{want:?} in {all}");
    }
}
