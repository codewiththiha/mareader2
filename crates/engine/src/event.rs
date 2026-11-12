//! What the session reports back; a front end drains it once a frame.

use crate::toast::Toast;

/// One result of an command that the session cannot show itself.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A note for the reader; the front end decides how it looks.
    Toast(Toast),
    /// F6 stepped past the last pane: the keys go to the parked tab.
    FocusEdgeTab,
    /// F6 moved to a pane: whatever widget held the keys lets go.
    ReleaseFocus,
}
