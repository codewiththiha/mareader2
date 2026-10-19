use super::*;

#[test]
fn a_pdf_page_loses_its_folios_and_running_heads_and_joins_its_words() {
    let line = |text: &str, starts| PdfLine {
        text: text.into(),
        starts,
    };
    let lines = [
        line("Chapter 3 · The Sea", true),
        line("The ship left the har-", true),
        line("bour at dawn, and the well-", false),
        line("Known captain waved.", false),
        line("A new paragraph begins", true),
        line("here.", false),
        line("- 41 -", true),
    ];
    let running = ["Chapter 2 · The Sea".to_owned()];
    let got = pdf_pieces(&lines, &running);
    let texts: Vec<String> = got
        .iter()
        .filter_map(|p| match p {
            Piece::Text { text, .. } => Some(text.clone()),
            Piece::Kept(_) => None,
        })
        .collect();
    assert_eq!(
        texts,
        [
            "The ship left the harbour at dawn, and the well- Known captain waved.",
            "A new paragraph begins here."
        ]
    );
    for page_number in ["12", "xii", "Page 3", "3 of 10", "– 7 –"] {
        assert!(folio(page_number), "{page_number}");
    }
    for words in ["12 Angry Men", "civil", "mix", "2024 was", "Pages"] {
        assert!(!folio(words), "{words}");
    }
}
