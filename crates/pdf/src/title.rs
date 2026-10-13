//! What to call a document: a trustworthy `/Title`, else the file name.

const MAX_TITLE_CHARS: usize = 200;
const PLACEHOLDERS: [&str; 5] = [
    "untitled",
    "unknown",
    "document",
    "no title",
    "pdf document",
];
/// Extensions a name may carry: ours, then ones we never open.
const EXTENSIONS: [&str; 9] = [
    "pdf", "epub", "doc", "docx", "ps", "dvi", "tex", "ppt", "pptx",
];

/// The document's own title when usable, else the file name's stem.
pub fn display_name(title: Option<&str>, file_name: Option<&str>) -> Option<String> {
    title
        .map(str::trim)
        .filter(|t| is_usable_title(t))
        .map(str::to_string)
        .or_else(|| file_name.and_then(file_stem))
}

/// True when a title says more than the file name would.
pub fn is_usable_title(t: &str) -> bool {
    if t.is_empty() || t.chars().count() > MAX_TITLE_CHARS {
        return false;
    }
    if PLACEHOLDERS.contains(&t.to_lowercase().as_str()) {
        return false;
    }
    // Distiller and friends leave source paths and URLs behind.
    if t.contains("://") || t.contains('\\') || t.contains('%') {
        return false;
    }
    !looks_like_file_name(t) && t.chars().any(char::is_alphanumeric)
}

/// The last path component, its document extension removed.
pub fn file_stem(path: &str) -> Option<String> {
    let p = path.trim().trim_end_matches(['/', '\\']);
    let last = p.rsplit(['/', '\\']).next().unwrap_or(p).trim();
    let stem = strip_extension(last);
    (!stem.is_empty()).then(|| stem.to_string())
}

/// The shapes a downloader leaves in `/Title`.
fn looks_like_file_name(t: &str) -> bool {
    if strip_extension(t) != t {
        return true;
    }
    // ISBN or UPC: nine or more digits, separators aside.
    let run: String = t.chars().filter(|c| *c != '-' && *c != ' ').collect();
    let isbn = run.len() >= 9
        && run
            .chars()
            .all(|c| c.is_ascii_digit() || c == 'x' || c == 'X')
        && run.chars().any(|c| c.is_ascii_digit());
    // Snake case has no spaces; a trailing `_N` counter is allowed.
    isbn || (strip_counter(t).contains('_') && !t.contains(' '))
}

fn strip_counter(t: &str) -> &str {
    match t.rsplit_once('_') {
        Some((base, n))
            if !base.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) =>
        {
            base
        }
        _ => t,
    }
}

fn strip_extension(s: &str) -> &str {
    match s.rsplit_once('.') {
        Some((base, ext))
            if !base.is_empty() && EXTENSIONS.contains(&ext.to_lowercase().as_str()) =>
        {
            base
        }
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_title_wins_over_the_file_name() {
        let got = display_name(Some("The Rust Programming Language"), Some("/tmp/trpl.pdf"));
        assert_eq!(got.as_deref(), Some("The Rust Programming Language"));
    }

    #[test]
    fn junk_titles_fall_back_to_the_file_name() {
        let path = "/Books/Programming Pearls (2nd Edition) - Jon Bentley.pdf";
        let pearls = Some("Programming Pearls (2nd Edition) - Jon Bentley");
        for junk in [
            "file:///F|/Mis%20docum",
            "untitled",
            "  Document ",
            "C:\\scans\\x",
            "pearls.pdf",
            "978-0-201-65788-3",
            "programming_pearls",
            "---",
            "",
        ] {
            assert_eq!(
                display_name(Some(junk), Some(path)).as_deref(),
                pearls,
                "{junk}"
            );
        }
    }

    #[test]
    fn copy_counters_and_dotted_titles_are_fine() {
        assert!(is_usable_title("Dune_1"));
        assert!(is_usable_title("Vol. 2"));
        assert!(is_usable_title("Node.js in Action"));
    }

    #[test]
    fn stems_drop_only_document_extensions() {
        assert_eq!(file_stem("C:\\a\\Book.PDF").as_deref(), Some("Book"));
        assert_eq!(file_stem("notes.v2").as_deref(), Some("notes.v2"));
        assert_eq!(file_stem(".pdf").as_deref(), Some(".pdf"));
        assert_eq!(file_stem("/").as_deref(), None);
        assert_eq!(display_name(None, None), None);
    }
}
