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

/// Numbered pages with a chapter every five, two sections in each.
pub fn pdf(pages: usize, title: &str) -> Vec<u8> {
    let pages = pages.max(1);
    let extra = outline(&chapters(pages));
    build(pages, title, "/Outlines {E0}", &extra)
}

/// For tests: `catalog` joins the catalog; `{E0}` … name `extra`.
pub fn with_objects(pages: usize, catalog: &str, extra: &[String]) -> Vec<u8> {
    build(pages.max(1), "Test", catalog, extra)
}

fn build(pages: usize, title: &str, catalog: &str, extra: &[String]) -> Vec<u8> {
    // 1 catalog, 2 page tree, 3 and 4 fonts, 5 info, then page pairs.
    let first = 6;
    let base = first + 2 * pages;
    let refs = |text: &str| {
        (0..extra.len()).fold(text.to_string(), |t, i| {
            t.replace(&format!("{{E{i}}}"), &format!("{} 0 R", base + i))
        })
    };
    let kids: Vec<String> = (0..pages)
        .map(|i| format!("{} 0 R", first + 2 * i))
        .collect();
    let mut objs = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {} >>", refs(catalog)),
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
    objs.extend(extra.iter().map(|e| refs(e)));
    assemble(&objs)
}

/// The sample's chapters: title, 0-based page and depth.
fn chapters(pages: usize) -> Vec<(String, usize, usize)> {
    const NAMES: [&str; 6] = [
        "Opening", "Zooming", "Reading", "Turning", "Finding", "Closing",
    ];
    const SECTIONS: [&str; 2] = ["Scrolling", "Spreads"];
    let mut out = Vec::new();
    for (k, start) in (0..pages).step_by(5).enumerate() {
        let name = NAMES.get(k % NAMES.len()).copied().unwrap_or("More");
        out.push((format!("Chapter {}: {name}", k + 1), start, 0));
        for (j, (section, at)) in SECTIONS.iter().zip([1, 3]).enumerate() {
            if start + at < pages {
                out.push((format!("{}.{} {section}", k + 1, j + 1), start + at, 1));
            }
        }
    }
    out
}

/// The root, then one object per entry, linked as a tree.
fn outline(entries: &[(String, usize, usize)]) -> Vec<String> {
    let at = |i: usize| format!("{{E{}}}", i + 1);
    let depth = |i: usize| entries.get(i).map_or(0, |e| e.2);
    let mut objs = vec![format!(
        "<< /Type /Outlines /First {{E1}} /Count {} >>",
        entries.len()
    )];
    for (i, (title, page, d)) in entries.iter().enumerate() {
        let parent = (0..i)
            .rev()
            .find(|&j| depth(j) + 1 == *d)
            .map_or("{E0}".to_string(), at);
        let mut obj = format!(
            "<< /Title ({}) /Parent {parent} /Dest [{} 0 R /Fit]",
            escape(title),
            6 + 2 * page
        );
        if i + 1 < entries.len() && depth(i + 1) == d + 1 {
            obj += &format!(" /First {}", at(i + 1));
        }
        let rest = entries.iter().enumerate().skip(i + 1);
        let next = rest.take_while(|(_, e)| e.2 >= *d).find(|(_, e)| e.2 == *d);
        if let Some((j, _)) = next {
            obj += &format!(" /Next {}", at(j));
        }
        objs.push(obj + " >>");
    }
    objs
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
