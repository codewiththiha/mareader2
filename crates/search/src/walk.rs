//! Moving between hits: which one is next, and how far to scroll to it.

use std::ops::Range;

/// The next hit, wrapping at either end; the first or last from none.
pub fn step(len: usize, active: Option<usize>, forward: bool) -> Option<usize> {
    let last = len.checked_sub(1)?;
    Some(match (active, forward) {
        (Some(i), true) if i < last => i + 1,
        (Some(_), true) | (None, true) => 0,
        (Some(i), false) if i > 0 && i <= last => i - 1,
        (Some(_), false) | (None, false) => last,
    })
}

/// The first hit on `page` or after it, else the first of all.
pub fn from_page(pages: &[usize], page: usize) -> Option<usize> {
    let at = pages.partition_point(|&p| p < page);
    if at < pages.len() {
        Some(at)
    } else {
        (!pages.is_empty()).then_some(0)
    }
}

/// How much of the readable band stays above a revealed hit.
const BIAS: f32 = 0.35;

/// The scroll showing `hit` within `view`, the readable band; None if shown.
pub fn reveal(hit: Range<f32>, view: Range<f32>, scroll: f32) -> Option<f32> {
    let band = view.end - view.start;
    if band <= 0.0 || hit.end - hit.start > band {
        return Some((scroll + hit.start - view.start).max(0.0));
    }
    if hit.start >= view.start && hit.end <= view.end {
        return None;
    }
    let to = scroll + hit.start - view.start - band * BIAS;
    Some(to.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_wrap_both_ways() {
        assert_eq!(step(3, Some(2), true), Some(0));
        assert_eq!(step(3, Some(0), false), Some(2));
        assert_eq!(step(3, Some(1), true), Some(2));
        assert_eq!(step(3, None, true), Some(0));
        assert_eq!(step(3, None, false), Some(2));
        assert_eq!(step(1, Some(0), true), Some(0));
        assert_eq!(step(0, None, true), None);
        // A stale index past the end restarts.
        assert_eq!(step(2, Some(7), true), Some(0));
        assert_eq!(step(2, Some(7), false), Some(1));
    }

    #[test]
    fn the_first_hit_is_found_from_the_page_read() {
        let pages = [0, 2, 2, 5];
        assert_eq!(from_page(&pages, 0), Some(0));
        assert_eq!(from_page(&pages, 1), Some(1));
        assert_eq!(from_page(&pages, 5), Some(3));
        assert_eq!(from_page(&pages, 6), Some(0), "wraps to the start");
        assert_eq!(from_page(&[], 3), None);
    }

    #[test]
    fn a_shown_hit_stays_and_others_land_on_the_bias_line() {
        // Scrolled to 600; the band runs 100..700 on screen.
        assert_eq!(reveal(200.0..220.0, 100.0..700.0, 600.0), None);
        assert_eq!(reveal(100.0..700.0, 100.0..700.0, 600.0), None);
        let below = reveal(900.0..920.0, 100.0..700.0, 600.0);
        assert_eq!(below, Some(600.0 + 800.0 - 600.0 * BIAS));
        let above = reveal(50.0..70.0, 100.0..700.0, 600.0);
        assert_eq!(above, Some(600.0 - 50.0 - 600.0 * BIAS));
        // Near the top it never goes past the start.
        assert_eq!(reveal(-500.0..-480.0, 100.0..700.0, 50.0), Some(0.0));
        // Taller than the band: its top meets the band's.
        assert_eq!(reveal(0.0..900.0, 100.0..700.0, 600.0), Some(500.0));
    }
}
