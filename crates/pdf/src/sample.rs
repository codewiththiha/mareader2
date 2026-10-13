//! A generated PDF for tests, screenshots and the web demo.

const BODY: [&str; 12] = [
    "This page was written by mareader itself, so the demo needs no file.",
    "Each page is a few lines of Helvetica: enough ink to see it render,",
    "little enough that a thousand pages generate in a blink.",
    "",
    "Scroll with the wheel or a touchpad; pages arrive as you reach them.",
    "A page that is still drawing shows plain paper for a moment.",
    "",
    "Every seventh page turns sideways, so the column keeps its centre",
    "while widths change underneath it.",
    "",
    "The quick brown fox jumps over the lazy dog. 0123456789",
    "Sphinx of black quartz, judge my vow.",
];

/// `pages` pages titled `title`, each saying which page it is.
pub fn pdf(pages: usize, title: &str) -> Vec<u8> {
    let pages = pages.max(1);
    // 1 catalog, 2 page tree, 3 and 4 fonts, 5 info, then page pairs.
    let first = 6;
    let kids: Vec<String> = (0..pages)
        .map(|i| format!("{} 0 R", first + 2 * i))
        .collect();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            kids.join(" ")
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        format!("<< /Title ({}) /Producer (mareader) >>", escape(title)),
    ];
    for i in 0..pages {
        let landscape = (i + 1) % 7 == 0;
        let (w, h) = if landscape { (792, 612) } else { (612, 792) };
        let text = page_text(i + 1, pages, w, h);
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] \
             /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> \
             /Contents {} 0 R >>",
            first + 2 * i + 1
        ));
        objs.push(format!(
            "<< /Length {} >>\nstream\n{text}\nendstream",
            text.len()
        ));
    }
    assemble(&objs)
}

fn page_text(n: usize, total: usize, w: u32, h: u32) -> String {
    let top = h - 96;
    let mut s = format!("BT /F1 30 Tf 72 {top} Td (Page {n} of {total}) Tj ET\n");
    s += &format!("0.85 0.45 0.2 rg 72 {} 96 4 re f 0 g\n", top - 18);
    s += &format!("BT /F2 12 Tf 16 TL 72 {} Td\n", top - 56);
    for line in BODY {
        s += &format!("({}) '\n", escape(line));
    }
    s += "ET\n";
    s += &format!("BT /F2 10 Tf {} 40 Td ({n}) Tj ET", w / 2 - 6);
    s
}

/// Writes the objects with a valid cross-reference table.
fn assemble(objs: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objs.len());
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let size = objs.len() + 1;
    out.extend(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {size} /Root 1 0 R /Info 5 0 R >>\n").as_bytes());
    out.extend(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// A PDF literal string escapes its delimiters and backslash.
fn escape(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii)
        .flat_map(|c| match c {
            '(' | ')' | '\\' => vec!['\\', c],
            c => vec![c],
        })
        .collect()
}
