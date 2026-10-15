//! The living grid's state: the hole a carry opens, and card springs.

use std::collections::{HashMap, HashSet};

use egui::{Pos2, Vec2, vec2};

use super::Pick;

/// How stiff the springs are: cards settle in about a fifth of a second.
const OMEGA: f32 = 30.0;
/// Closer and slower than these, a card is where it belongs.
const NEAR: f32 = 0.25;
const SLOW: f32 = 4.0;
/// The longest step one integration takes, for steadiness.
const STEP: f32 = 1.0 / 120.0;
/// How long the pointer rests on a place before the hole moves there.
const DWELL: f64 = 0.18;

/// What can sit in a slot and move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Card(Pick),
    /// The plus card that ends the grid.
    Add,
}

#[derive(Clone, Copy, Debug)]
struct Spring {
    pos: Vec2,
    vel: Vec2,
    seen: u64,
}

impl Spring {
    /// Critically damped toward `target`; true while still moving.
    fn step(&mut self, target: Vec2, dt: f32) -> bool {
        let mut left = dt;
        while left > 0.0 {
            let h = left.min(STEP);
            let pull = (target - self.pos) * (OMEGA * OMEGA) - self.vel * (2.0 * OMEGA);
            self.vel += pull * h;
            self.pos += self.vel * h;
            left -= h;
        }
        let moving = (target - self.pos).length() > NEAR || self.vel.length() > SLOW;
        if !moving {
            (self.pos, self.vel) = (target, Vec2::ZERO);
        }
        moving
    }
}

/// Where the lifted cards will land: `size` slots from `at` among the rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hole {
    pub at: usize,
    pub size: usize,
    /// Where it opened, so a drop back there changes nothing.
    pub start: usize,
    /// Where the pointer asks for it, and since when.
    pending: Option<(usize, f64)>,
}

impl Hole {
    fn new(at: usize, size: usize) -> Self {
        Self {
            at,
            size,
            start: at,
            pending: None,
        }
    }

    /// Points the hole at `want`; it moves after [`DWELL`] there.
    /// Returns the wait left.
    pub fn aim(&mut self, want: Option<usize>, now: f64) -> Option<f64> {
        let Some(want) = want.filter(|w| *w != self.at) else {
            self.pending = None;
            return None;
        };
        let since = match self.pending {
            Some((at, since)) if at == want => since,
            _ => now,
        };
        let left = DWELL - (now - since);
        if left <= 0.0 {
            (self.at, self.pending) = (want, None);
            return None;
        }
        self.pending = Some((want, since));
        Some(left)
    }

    /// What slot `slot` shows: a rest index, or none for the hole.
    pub fn rest_at(&self, slot: usize) -> Option<usize> {
        match slot {
            s if s < self.at => Some(s),
            s if s < self.at + self.size => None,
            s => Some(s - self.size),
        }
    }
}

/// Cards carried off this level, hidden from the grid while held.
#[derive(Debug)]
pub struct Lift {
    /// In the order shown.
    pub moving: Vec<Pick>,
    set: HashSet<Pick>,
    pub hole: Hole,
}

impl Lift {
    pub fn holds(&self, pick: Pick) -> bool {
        self.set.contains(&pick)
    }
}

/// The card a drag picked up, and where in it the pointer pressed.
#[derive(Clone, Copy, Debug)]
pub struct Grab {
    pub pick: Pick,
    pub offset: Vec2,
}

/// The grid's moving parts, kept between frames for cards drawn.
#[derive(Debug, Default)]
pub struct Motion {
    springs: HashMap<Slot, Spring>,
    /// Columns and card width; a change snaps every card.
    layout: (usize, u32),
    frame: u64,
    dt: f32,
    animate: bool,
    busy: bool,
    pub lift: Option<Lift>,
    pub grab: Option<Grab>,
    /// The grid's content origin this frame, on screen.
    pub origin: Pos2,
    /// A card's size, its text included.
    pub card: Vec2,
    /// Where the ghost was last drawn, in content space.
    pub ghost: Option<Vec2>,
}

impl Motion {
    /// Starts a frame of the grid.
    pub fn begin(&mut self, (cols, card): (usize, f32), dt: f32, animate: bool) {
        let layout = (cols, card.round() as u32);
        if layout != self.layout || !animate {
            self.springs.clear();
            self.layout = layout;
        }
        self.frame += 1;
        (self.dt, self.animate, self.busy) = (dt, animate, false);
    }

    /// Where `slot` is drawn this frame, on its way to `target`.
    pub fn place(&mut self, slot: Slot, target: Vec2) -> Vec2 {
        if !self.animate {
            return target;
        }
        let frame = self.frame;
        let spring = self.springs.entry(slot).or_insert(Spring {
            pos: target,
            vel: Vec2::ZERO,
            seen: frame,
        });
        spring.seen = frame;
        self.busy |= spring.step(target, self.dt);
        spring.pos
    }

    /// Ends a frame: forgets cards not drawn; true while any moves.
    pub fn end(&mut self) -> bool {
        let frame = self.frame;
        self.springs.retain(|_, s| s.seen == frame);
        self.busy
    }

    /// Lifts `moving` off `shown`; nothing if none of it shows.
    pub fn pick_up(&mut self, mut moving: Vec<Pick>, shown: &[Pick]) {
        let set: HashSet<Pick> = moving.iter().copied().collect();
        let first = shown.iter().position(|p| set.contains(p));
        let Some(at) = first else {
            return;
        };
        let place: HashMap<Pick, usize> = shown
            .iter()
            .enumerate()
            .filter(|(_, p)| set.contains(*p))
            .map(|(n, p)| (*p, n))
            .collect();
        moving.retain(|p| place.contains_key(p));
        moving.sort_by_key(|p| place.get(p).copied());
        let hole = Hole::new(at, moving.len());
        self.lift = Some(Lift { moving, set, hole });
    }

    /// Drops any carry without a landing, as the list does.
    pub fn forget(&mut self) {
        (self.lift, self.grab, self.ghost) = (None, None, None);
    }

    /// Ends a carry: lifted cards fly from the ghost to their slots.
    pub fn put_down(&mut self) {
        let lift = self.lift.take();
        let ghost = self.ghost.take();
        self.grab = None;
        let (Some(lift), Some(from)) = (lift, ghost) else {
            return;
        };
        if !self.animate {
            return;
        }
        let frame = self.frame;
        for (n, pick) in lift.moving.into_iter().enumerate() {
            // A selection fans out a little as it lands.
            let spring = Spring {
                pos: from + vec2(n as f32 * 4.0, n as f32 * 4.0),
                vel: Vec2::ZERO,
                seen: frame,
            };
            self.springs.insert(Slot::Card(pick), spring);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_library::BookId;

    fn book(n: u128) -> Pick {
        Pick::Book(BookId(n))
    }

    #[test]
    fn the_hole_moves_only_where_the_pointer_rests() {
        let mut hole = Hole::new(0, 1);
        assert_eq!(hole.aim(Some(0), 0.0), None, "already there");
        // Passing over a place on the way leaves the hole put.
        assert!(hole.aim(Some(1), 1.0).is_some());
        assert!(hole.aim(Some(3), 1.1).is_some());
        assert!(hole.aim(Some(3), 1.2).is_some());
        assert_eq!(hole.at, 0);
        assert_eq!(hole.aim(Some(3), 1.3), None);
        assert_eq!(hole.at, 3);
        assert_eq!(
            (hole.rest_at(2), hole.rest_at(3), hole.rest_at(4)),
            (Some(2), None, Some(3))
        );
        // Leaving for empty space forgets what was asked.
        hole.aim(Some(1), 2.0);
        hole.aim(None, 2.1);
        assert!(hole.aim(Some(1), 2.2).is_some());
    }

    #[test]
    fn springs_settle_on_target_without_flying_past_it() {
        let mut motion = Motion::default();
        let slot = Slot::Card(book(1));
        motion.begin((4, 150.0), 1.0 / 60.0, true);
        motion.place(slot, Vec2::ZERO);
        let mut frames = 0;
        loop {
            motion.begin((4, 150.0), 1.0 / 60.0, true);
            let at = motion.place(slot, vec2(170.0, 0.0));
            assert!(at.x <= 170.5, "{at:?}");
            frames += 1;
            if !motion.end() {
                break;
            }
            assert!(frames < 60, "still moving after a second");
        }
        assert!(frames > 3, "it eased");
    }

    #[test]
    fn a_new_layout_or_motion_off_puts_cards_straight_in_place() {
        let mut motion = Motion::default();
        let slot = Slot::Add;
        motion.begin((4, 150.0), 1.0 / 60.0, true);
        motion.place(slot, Vec2::ZERO);
        motion.begin((5, 120.0), 1.0 / 60.0, true);
        assert_eq!(motion.place(slot, vec2(90.0, 0.0)), vec2(90.0, 0.0));
        motion.begin((5, 120.0), 1.0 / 60.0, false);
        assert_eq!(motion.place(slot, vec2(10.0, 0.0)), vec2(10.0, 0.0));
        assert!(!motion.end());
    }

    #[test]
    fn a_lift_keeps_the_order_shown_and_opens_where_the_first_was() {
        let mut motion = Motion::default();
        let shown = [book(1), book(2), book(3), book(4)];
        motion.pick_up(vec![book(4), book(2), book(9)], &shown);
        let lift = motion.lift.as_ref().unwrap();
        assert_eq!(lift.moving, [book(2), book(4)]);
        assert_eq!((lift.hole.at, lift.hole.size), (1, 2));
        assert!(lift.holds(book(4)) && !lift.holds(book(3)));
        let mut none = Motion::default();
        none.pick_up(vec![book(9)], &shown);
        assert!(none.lift.is_none());
    }
}
