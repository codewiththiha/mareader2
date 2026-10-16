//! The chapter tree a PDF carries, flattened in reading order.

use std::collections::HashSet;

use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::Dict;
use hayro::hayro_syntax::object::dict::keys::{FIRST, NEXT, OUTLINES, TITLE};

use crate::dest::{PageIds, Targets};
use crate::strings::decode_text;

/// Entries past this are dropped; real books stay far below it.
pub const MAX_ENTRIES: usize = 20_000;
/// The deepest level kept as its own indent; deeper ones share it.
pub const MAX_DEPTH: u8 = 5;

/// One chapter: its title, 0-based page and nesting level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    pub title: String,
    pub page: usize,
    pub depth: u8,
}

/// The last entry starting at or before `page`; ties go deeper.
pub fn active_entry(entries: &[OutlineEntry], page: usize) -> Option<usize> {
    if entries.is_sorted_by_key(|e| e.page) {
        return entries.partition_point(|e| e.page <= page).checked_sub(1);
    }
    entries.iter().rposition(|e| e.page <= page)
}

/// Reads `/Outlines`; entries whose target can't be found are left out.
pub(crate) fn read(pdf: &Pdf, pages: &PageIds) -> Vec<OutlineEntry> {
    let Some(targets) = Targets::new(pdf, pages) else {
        return Vec::new();
    };
    let Some(first) = targets
        .catalog
        .get::<Dict<'_>>(OUTLINES)
        .and_then(|o| o.get::<Dict<'_>>(FIRST))
    else {
        return Vec::new();
    };
    walk(first, &targets)
}

/// Depth-first, siblings in order; a node seen twice ends its branch.
fn walk(first: Dict<'_>, targets: &Targets<'_>) -> Vec<OutlineEntry> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    // Each frame is the next sibling to visit and its depth.
    let mut stack = vec![(first, 0_usize)];
    while let Some((node, depth)) = stack.pop() {
        if out.len() >= MAX_ENTRIES || node.obj_id().is_some_and(|id| !seen.insert(id)) {
            continue;
        }
        if let Some(next) = node.get::<Dict<'_>>(NEXT) {
            stack.push((next, depth));
        }
        if let Some(child) = node.get::<Dict<'_>>(FIRST) {
            stack.push((child, depth + 1));
        }
        let title = node
            .get::<hayro::hayro_syntax::object::String<'_>>(TITLE)
            .map(|s| tidy(&decode_text(s.as_ref())))
            .unwrap_or_default();
        if let (false, Some(page)) = (title.is_empty(), targets.of_node(&node)) {
            let depth = u8::try_from(depth).unwrap_or(u8::MAX).min(MAX_DEPTH);
            out.push(OutlineEntry { title, page, depth });
        }
    }
    out
}

/// Line breaks and runs of spaces in a title become one space.
fn tidy(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;
    use crate::{Document, OpenError};
    use std::sync::Arc;

    type Out = Result<(), OpenError>;

    fn open(bytes: Vec<u8>) -> Result<Document, OpenError> {
        Document::open(Arc::new(bytes))
    }

    fn entry(title: &str, page: usize, depth: u8) -> OutlineEntry {
        OutlineEntry {
            title: title.into(),
            page,
            depth,
        }
    }

    /// Pages as `sample::with_objects` numbers them: 6, 8, 10 …
    fn page_ref(page: usize) -> String {
        format!("{} 0 R", 6 + 2 * page)
    }

    #[test]
    fn the_sample_has_chapters_with_sections_in_reading_order() -> Out {
        let doc = open(sample::pdf(12, "Sample"))?;
        let titles: Vec<_> = doc
            .outline()
            .iter()
            .map(|e| (e.title.as_str(), e.page, e.depth))
            .collect();
        assert_eq!(titles.first().copied(), Some(("Chapter 1: Opening", 0, 0)));
        assert!(titles.contains(&("1.1 Scrolling", 1, 1)));
        assert!(titles.contains(&("Chapter 3: Reading", 10, 0)));
        assert!(doc.outline().is_sorted_by_key(|e| e.page));
        Ok(())
    }

    #[test]
    fn named_and_action_destinations_resolve_like_direct_ones() -> Out {
        let extra = [
            // 1: the outline root; 2-5: entries; 6: name tree; 7: old names.
            "<< /First {E1} /Last {E4} >>".to_string(),
            format!(
                "<< /Title (Direct) /Dest [{} /Fit] /Next {{E2}} >>",
                page_ref(2)
            ),
            format!(
                "<< /Title (Action) /A << /S /GoTo /D [{} /XYZ 0 0 0] >> /Next {{E3}} >>",
                page_ref(3)
            ),
            "<< /Title (By string) /Dest (deep) /Next {E4} >>".to_string(),
            "<< /Title (By name) /Dest /old >>".to_string(),
            format!(
                "<< /Kids [<< /Limits [(a) (z)] /Names [(deep) << /D [{} /Fit] >>] >>] >>",
                page_ref(4)
            ),
            format!("<< /old [{} /Fit] >>", page_ref(1)),
        ];
        let catalog = "/Outlines {E0} /Names << /Dests {E5} >> /Dests {E6}";
        let doc = open(sample::with_objects(6, catalog, &extra))?;
        assert_eq!(
            doc.outline(),
            [
                entry("Direct", 2, 0),
                entry("Action", 3, 0),
                entry("By string", 4, 0),
                entry("By name", 1, 0),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_looping_outline_ends_instead_of_hanging() -> Out {
        let extra = [
            "<< /First {E1} >>".to_string(),
            format!("<< /Title (A) /Dest [{} /Fit] /Next {{E2}} >>", page_ref(0)),
            format!(
                "<< /Title (B) /Dest [{} /Fit] /Next {{E1}} /First {{E2}} >>",
                page_ref(1)
            ),
        ];
        let doc = open(sample::with_objects(3, "/Outlines {E0}", &extra))?;
        assert_eq!(doc.outline(), [entry("A", 0, 0), entry("B", 1, 0)]);
        Ok(())
    }

    #[test]
    fn blank_titles_lost_targets_and_wrapped_lines_are_cleaned() -> Out {
        let extra = [
            "<< /First {E1} >>".to_string(),
            format!(
                "<< /Title (   ) /Dest [{} /Fit] /Next {{E2}} >>",
                page_ref(0)
            ),
            "<< /Title (Gone) /Dest [99 0 R /Fit] /Next {E3} >>".to_string(),
            "<< /Title (Nowhere) /Dest /missing /Next {E4} >>".to_string(),
            format!(
                "<< /Title (Two\\nlines  here) /Dest [{} /Fit] >>",
                page_ref(1)
            ),
        ];
        let doc = open(sample::with_objects(3, "/Outlines {E0}", &extra))?;
        assert_eq!(doc.outline(), [entry("Two lines here", 1, 0)]);
        Ok(())
    }

    #[test]
    fn a_file_without_an_outline_has_none() -> Out {
        let doc = open(sample::with_objects(2, "", &[]))?;
        assert!(doc.outline().is_empty());
        Ok(())
    }

    #[test]
    fn the_last_entry_at_or_before_the_page_is_active() {
        let entries = [
            entry("Chapter 1", 2, 0),
            entry("1.1", 2, 1),
            entry("1.2", 8, 1),
            entry("Chapter 2", 19, 0),
        ];
        assert_eq!(active_entry(&entries, 0), None);
        assert_eq!(active_entry(&entries, 2), Some(1));
        assert_eq!(active_entry(&entries, 7), Some(1));
        assert_eq!(active_entry(&entries, 8), Some(2));
        assert_eq!(active_entry(&entries, 999), Some(3));
        assert_eq!(active_entry(&[], 3), None);
        // Out of order: still the last listed entry at or before.
        let jumbled = [
            entry("Late", 30, 0),
            entry("First", 2, 0),
            entry("Mid", 9, 0),
        ];
        assert_eq!(active_entry(&jumbled, 25), Some(2));
        assert_eq!(active_entry(&jumbled, 1), None);
    }
}
