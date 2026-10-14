//! A page's characters and pictures where they are drawn.
use std::ops::Range;

use parley::BreakReason;

use crate::flow::{Draw, Flow, Laid, PAD, Para};
use crate::hyphen::SHY;

/// One character's box on a page, in page units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageChar {
    pub ch: char,
    /// Left, top, width, height: the line's full height.
    pub rect: [f32; 4],
}

/// One line of a page's text, in reading order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageLine {
    pub chars: Vec<PageChar>,
    /// It goes on from the line before, which wrapped rather than ended.
    pub wraps: bool,
}

impl Flow {
    /// Page `page`'s text by line; list markers are left out.
    pub fn text(&self, page: usize) -> Vec<PageLine> {
        let mut out = Vec::new();
        let left = self.column(page).0;
        for piece in self.pages.get(page).map_or(&[][..], Vec::as_slice) {
            let Some(set) = self.blocks.get(piece.block) else {
                continue;
            };
            let x = left + set.x;
            let y = PAD + piece.dy;
            match (&set.draw, &*self.laid(piece.block)) {
                (Draw::Text { pad, .. }, Laid::Text { para, .. }) => {
                    lines(para, piece.units.clone(), (x + pad, y), &mut out);
                }
                (Draw::Table(grid), Laid::Table(rows)) => {
                    let [pad_x, pad_y] = grid.pad;
                    for r in piece.units.clone() {
                        let (Some(&(top, _)), Some(cells)) = (set.units.get(r), rows.get(r)) else {
                            continue;
                        };
                        for (c, cell) in cells.iter().enumerate() {
                            let at = (x + c as f32 * grid.column + pad_x, y + top + pad_y);
                            lines(cell, 0..cell.layout.len(), at, &mut out);
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Page `page`'s pictures: left, top, width, height in page units.
    pub fn pictures(&self, page: usize) -> Vec<[f32; 4]> {
        let left = self.column(page).0;
        let pieces = self.pages.get(page).map_or(&[][..], Vec::as_slice);
        pieces
            .iter()
            .filter_map(|piece| {
                let set = self.blocks.get(piece.block)?;
                let Draw::Image { rect, .. } = &set.draw else {
                    return None;
                };
                let [x, y, w, h] = *rect;
                Some([left + set.x + x, PAD + piece.dy + y, w, h])
            })
            .collect()
    }
}

/// Lines `range` of `para`, its origin at `x, y`, each char boxed.
fn lines(para: &Para, range: Range<usize>, (x, y): (f32, f32), out: &mut Vec<PageLine>) {
    let layout = &para.layout;
    for n in range {
        let Some(line) = layout.get(n) else {
            continue;
        };
        let m = line.metrics();
        let (top, height) = (y + m.block_min_coord, m.block_max_coord - m.block_min_coord);
        // Clusters left to right, as drawn; then back into text order.
        let mut at = x + m.inline_min_coord + m.offset;
        let mut clusters = Vec::new();
        for run in line.runs() {
            for c in run.visual_clusters() {
                // What draws no glyph takes no room, but a ligature shares.
                let drawn = c.is_ligature_continuation() || c.glyphs().next().is_some();
                let w = if drawn { c.advance() } else { 0.0 };
                clusters.push((c.text_range(), at, w));
                at += w;
            }
        }
        clusters.sort_by_key(|(r, ..)| r.start);
        let mut chars = Vec::new();
        for (r, left, w) in clusters {
            let s = para.text.get(r).unwrap_or_default();
            let kept: Vec<char> = s.chars().filter(|&c| c != SHY && c != '\n').collect();
            let share = w / kept.len().max(1) as f32;
            for (i, ch) in kept.into_iter().enumerate() {
                let rect = [left + share * i as f32, top, share, height];
                chars.push(PageChar { ch, rect });
            }
        }
        let wrapped = |b| matches!(b, BreakReason::Regular | BreakReason::Emergency);
        let before = n.checked_sub(1).and_then(|p| layout.get(p));
        let wraps = before.is_some_and(|l| wrapped(l.break_reason()));
        out.push(PageLine { chars, wraps });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::block::{Block, Content, Emphasis, Role, Text};
    use crate::flow::{PAGE_HEIGHT, PAGE_WIDTH, Typography};
    use crate::fonts::Fonts;
    use crate::picture::Picture;
    use crate::picture::tests::png;

    fn with(blocks: &[Block], typo: &Typography) -> Flow {
        Flow::new(std::sync::Arc::new(blocks.to_vec()), typo, Fonts::bundled())
    }

    /// Lines joined as a copy joins them.
    fn read(lines: &[PageLine]) -> String {
        let mut out = String::new();
        for (i, l) in lines.iter().enumerate() {
            if i > 0 && !l.wraps {
                out.push('\n');
            }
            out.extend(l.chars.iter().map(|c| c.ch));
        }
        out
    }

    #[test]
    fn a_page_reads_back_its_words_with_wrapped_lines_joined() {
        let body = "The river ran slow and brown past the customs house. ".repeat(12);
        let body = body.trim_end();
        let blocks = [
            Block::text(Role::Heading(1), "Chapter one"),
            Block::text(Role::Paragraph, body),
        ];
        let lines = with(&blocks, &Typography::default()).text(0);
        assert!(lines.len() > 3, "{}", lines.len());
        assert_eq!(read(&lines), format!("Chapter one\n{body}"));
        let column = (PAD - 1.0)..=(PAGE_WIDTH - PAD + 1.0);
        for l in &lines {
            let lefts: Vec<f32> = l.chars.iter().map(|c| c.rect[0]).collect();
            assert!(lefts.windows(2).all(|w| w.first() <= w.get(1)), "{lefts:?}");
            let ink = l.chars.iter().filter(|c| !c.ch.is_whitespace());
            for c in ink {
                let [x, y, w, h] = c.rect;
                assert!(column.contains(&x) && column.contains(&(x + w)), "{c:?}");
                assert!(w > 0.0 && h > 0.0 && y >= PAD - 1.0, "{c:?}");
            }
        }
    }

    #[test]
    fn justified_and_hyphenated_lines_box_each_letter_where_drawn() {
        let typo = Typography {
            justify: true,
            hyphenate: true,
            ..Typography::default()
        };
        let mut text = Text::new(Role::Paragraph);
        let words = "extraordinarily incomprehensible things ".repeat(20);
        text.push(&words, Emphasis::default());
        text.push_link("the linked end", Emphasis::default(), 1);
        let f = with(&[Block::new(Content::Text(text))], &typo);
        let lines = f.text(0);
        let read = read(&lines);
        assert_eq!(read, format!("{words}the linked end"));
        assert!(!read.contains(SHY) && !read.contains('-'));
        // Each box of the link starts and ends on letters' edges.
        let links = f.links(0);
        assert!(!links.is_empty());
        for l in &links {
            let [x, y, w, h] = l.rect;
            let middle = |c: &PageChar| c.rect[1] + c.rect[3] / 2.0;
            let on_row = |line: &&PageLine| {
                let first = line.chars.first();
                first.is_some_and(|c| (middle(c) - (y + h / 2.0)).abs() < h / 2.0)
            };
            let row = lines
                .iter()
                .find(on_row)
                .map_or(&[][..], |r| r.chars.as_slice());
            let starts = row.iter().any(|c| (c.rect[0] - x).abs() < 0.5);
            let ends = row
                .iter()
                .any(|c| (c.rect[0] + c.rect[2] - x - w).abs() < 0.5);
            assert!(starts && ends, "{l:?} {row:?}");
        }
    }

    #[test]
    fn a_paragraph_cut_by_a_page_goes_on_at_the_next() {
        let body = "word ".repeat(1200);
        let f = with(
            &[Block::text(Role::Paragraph, body.trim_end())],
            &Typography::default(),
        );
        assert!(f.page_count() > 1);
        let first = f.text(1).first().map(|l| l.wraps);
        assert_eq!(first, Some(true));
        let lead = f.text(0).first().map(|l| l.wraps);
        assert_eq!(lead, Some(false));
    }

    #[test]
    fn pictures_are_found_where_they_are_drawn() {
        let picture = Picture::probe(png(200, 100)).map(Arc::new);
        assert!(picture.is_ok(), "probe");
        let Ok(picture) = picture else { return };
        let blocks = [
            Block::text(Role::Paragraph, "Above."),
            Block::new(Content::Image {
                picture,
                cover: false,
            }),
        ];
        let f = with(&blocks, &Typography::default());
        assert_eq!(f.text(0).len(), 1);
        let shown = f.pictures(0);
        assert_eq!(shown.len(), 1, "{shown:?}");
        let Some(&[x, y, w, h]) = shown.first() else {
            return;
        };
        assert_eq!((w, h), (200.0, 100.0));
        assert!(x >= PAD && x + w <= PAGE_WIDTH - PAD, "{x}");
        assert!(y > PAD && y + h < PAGE_HEIGHT - PAD, "{y}");
        // The box holds the picture's pixels on the drawn page.
        let px = f.render(0, 1.0, [0, 0, PAGE_WIDTH as u32, PAGE_HEIGHT as u32]);
        let at = |px: &crate::Pixels, x: f32, y: f32| {
            let i = (y as usize * px.width as usize + x as usize) * 4;
            px.rgba.get(i..i + 3).map(<[u8]>::to_vec)
        };
        let inside = px.as_ref().and_then(|p| at(p, x + w / 2.0, y + h / 2.0));
        assert!(
            inside.as_ref().is_some_and(|c| c != &[255, 255, 255]),
            "{inside:?}"
        );
    }
}
