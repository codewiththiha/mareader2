//! Reading aloud as rules: which words are lit, and how long is left.

pub mod countdown;
pub mod words;

use mareader_session::Anchor;
use mareader_speech::Spot;

/// Where reading starts: the place or character under the pointer, or the page.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReadFrom {
    pub anchor: Option<Anchor>,
    pub at: Option<Spot>,
    /// Where it stops: a selection's end, read alone by the voice.
    pub until: Option<Spot>,
}

impl ReadFrom {
    pub fn here(&self) -> bool {
        self.anchor.is_some() || self.at.is_some()
    }
}
