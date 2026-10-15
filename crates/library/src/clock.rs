//! Hybrid logical time: stamps that order changes across machines.

use serde::{Deserialize, Serialize};

use crate::id::{DeviceId, parse_hex};

/// When and by whom: compared field by field, later is larger, never tied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Stamp {
    pub ms: u64,
    pub counter: u32,
    pub device: DeviceId,
}

impl From<Stamp> for String {
    /// Fixed-width hex, so comparing the strings compares the stamps.
    fn from(s: Stamp) -> Self {
        format!("{:013x}-{:08x}-{}", s.ms, s.counter, s.device)
    }
}

impl TryFrom<String> for Stamp {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        let mut parts = s.split('-');
        let mut next = |digits| {
            parts
                .next()
                .map_or(Err("a short stamp".into()), |p| parse_hex(p, digits))
        };
        let ms = u64::try_from(next(13)?).map_err(|e| e.to_string())?;
        let counter = u32::try_from(next(8)?).map_err(|e| e.to_string())?;
        let device = u64::try_from(next(16)?).map_err(|e| e.to_string())?;
        Ok(Self {
            ms,
            counter,
            device: DeviceId(device),
        })
    }
}

/// Makes this device's stamps: never backwards, and past any seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clock {
    last: Stamp,
}

impl Clock {
    pub fn new(device: DeviceId) -> Self {
        Self {
            last: Stamp {
                ms: 0,
                counter: 0,
                device,
            },
        }
    }

    pub fn device(&self) -> DeviceId {
        self.last.device
    }

    /// A stamp past every one made or seen, even if the wall clock fell.
    pub fn tick(&mut self, wall_ms: u64) -> Stamp {
        let last = self.last;
        let (ms, counter) = if wall_ms > last.ms {
            (wall_ms, 0)
        } else if last.counter == u32::MAX {
            (last.ms.saturating_add(1), 0)
        } else {
            (last.ms, last.counter + 1)
        };
        self.last = Stamp {
            ms,
            counter,
            ..last
        };
        self.last
    }

    /// Takes in another device's stamp, so the next tick passes it.
    pub fn observe(&mut self, seen: Stamp) {
        if (seen.ms, seen.counter) > (self.last.ms, self.last.counter) {
            self.last = Stamp {
                device: self.last.device,
                ..seen
            };
        }
    }
}

/// A value and the stamp of its last write: last writer wins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field<T> {
    #[serde(rename = "v")]
    pub value: T,
    #[serde(rename = "t")]
    pub at: Stamp,
}

impl<T> Field<T> {
    pub fn new(value: T, at: Stamp) -> Self {
        Self { value, at }
    }

    /// Keeps the later of the two writes; order of merging never matters.
    pub fn merge(&mut self, other: Self) {
        if other.at > self.at {
            *self = other;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: DeviceId = DeviceId(1);
    const B: DeviceId = DeviceId(2);

    #[test]
    fn ticks_never_go_back_even_when_the_wall_clock_does() {
        let mut clock = Clock::new(A);
        let first = clock.tick(1_000);
        let second = clock.tick(900);
        let third = clock.tick(1_000);
        assert!(
            first < second && second < third,
            "{first:?} {second:?} {third:?}"
        );
        assert_eq!(clock.tick(2_000).ms, 2_000);
    }

    #[test]
    fn a_fast_peer_cannot_lock_this_device_out() {
        // B's clock runs an hour ahead; after seeing it, A still wins.
        let mut a = Clock::new(A);
        let hour_ahead = Clock::new(B).tick(3_600_000);
        a.observe(hour_ahead);
        assert!(a.tick(1_000) > hour_ahead);
    }

    #[test]
    fn stamps_write_so_text_order_is_time_order() {
        let early = Stamp {
            ms: 9,
            counter: 300,
            device: B,
        };
        let late = Stamp {
            ms: 10,
            counter: 0,
            device: A,
        };
        let (e, l) = (String::from(early), String::from(late));
        assert!(e < l, "{e} {l}");
        assert_eq!(Stamp::try_from(l).unwrap(), late);
        assert!(Stamp::try_from("12-3".to_owned()).is_err());
    }

    #[test]
    fn fields_merge_the_same_whichever_comes_first() {
        let mut clock = Clock::new(A);
        let older = Field::new("old", clock.tick(1));
        let newer = Field::new("new", clock.tick(2));
        let mut one = older.clone();
        one.merge(newer.clone());
        let mut two = newer.clone();
        two.merge(older);
        assert_eq!(one, two);
        assert_eq!(one.value, "new");
    }
}
