//! Links between a book's files, resolved inside the book only.

/// `href` from `base` as an archive path; `None` for scheme links.
pub(crate) fn resolve(base: &str, href: &str) -> Option<String> {
    let path = href.trim().split(['#', '?']).next().unwrap_or_default();
    if path.is_empty() {
        return None;
    }
    if path
        .split_once(':')
        .is_some_and(|(scheme, _)| !scheme.contains('/'))
    {
        return None;
    }
    let path = percent_decode(path);
    let joined = if path.starts_with('/') {
        path
    } else {
        let dir = base.rsplit_once('/').map_or("", |(dir, _)| dir);
        format!("{dir}/{path}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for segment in joined.split('/') {
        match segment {
            "" | "." => {}
            // Climbing past the root stays at the root.
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    (!parts.is_empty()).then(|| format!("/{}", parts.join("/")))
}

fn percent_decode(s: &str) -> String {
    let hex = |b: &u8| {
        char::from(*b)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
    };
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let pair = (
            bytes.get(i + 1).and_then(hex),
            bytes.get(i + 2).and_then(hex),
        );
        if let (b'%', (Some(hi), Some(lo))) = (b, pair) {
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn relative_links_resolve_against_the_chapter() {
        let base = "/OEBPS/text/c1.xhtml";
        let r = |h| resolve(base, h);
        assert_eq!(r("../images/a.png").as_deref(), Some("/OEBPS/images/a.png"));
        assert_eq!(r("./b%20c.jpg#x").as_deref(), Some("/OEBPS/text/b c.jpg"));
        assert_eq!(r("/cover.jpg").as_deref(), Some("/cover.jpg"));
        assert_eq!(r("../../../../x.png").as_deref(), Some("/x.png"));
    }

    #[test]
    fn links_out_of_the_book_are_refused() {
        let r = |h| resolve("/a.xhtml", h);
        assert_eq!(r("https://example.com/a.png"), None);
        assert_eq!(r("data:image/png;base64,AAAA"), None);
        assert_eq!(r("file:///etc/passwd"), None);
        assert_eq!(r("#note"), None);
        assert_eq!(r(""), None);
    }
}
