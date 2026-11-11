//! What the settings window is showing: a tab's name, not its drawing.

/// Which page of the settings window is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    /// Colours, a book's text and a PDF's paper.
    #[default]
    Appearance,
    Layout,
    /// The engine, its voice and how books are read aloud.
    ReadAloud,
    /// The pens, the selection menu and the AI's reserved look.
    Highlights,
    Vocabulary,
    Dictionary,
    /// What the AI does: the AI Dictionary, the library, pages.
    Ai,
    /// The models tasks ask, and their providers' keys.
    Models,
    Panes,
    /// The master switch, then each motion.
    Motion,
    /// A downloaded voice model's voices; shown once it is ready.
    Model(&'static mareader_speech::Model),
}

impl Tab {
    pub const ALL: [Tab; 10] = [
        Tab::Appearance,
        Tab::Layout,
        Tab::ReadAloud,
        Tab::Highlights,
        Tab::Vocabulary,
        Tab::Dictionary,
        Tab::Ai,
        Tab::Models,
        Tab::Panes,
        Tab::Motion,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Appearance => "Appearance",
            Tab::Layout => "Layout",
            Tab::ReadAloud => "Read aloud",
            Tab::Highlights => "Highlights",
            Tab::Vocabulary => "Vocabulary",
            Tab::Dictionary => "Dictionary",
            Tab::Ai => "AI",
            Tab::Models => "AI models",
            Tab::Panes => "Panes",
            Tab::Motion => "Motion",
            Tab::Model(m) => m.name,
        }
    }
}
