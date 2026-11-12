//! The registry as a script sees it: ids a ledger can cite, and a row for
//! every intent there is.

use mareader_appearance::Appearance;
use mareader_engine::appearance_menu::QuickText;
use mareader_engine::command::{Independence, Intent};
use mareader_engine::command_spec::SPECS;
use mareader_engine::dict::{Action as DictAction, Download};
use mareader_engine::export::Format;
use mareader_engine::page_ai::Action as PageAiAction;
use mareader_engine::pane::{Arrow, Step, ViewMode, ZoomCommand};
use mareader_engine::read_aloud::ReadFrom;
use mareader_engine::settings::Tab;
use mareader_engine::study::Action as StudyAction;
use mareader_library::{BookId, Pen, View};
use mareader_speech::{Item, KOKORO, Spot};
use mareader_workspace::{PaneId, SplitAxis};

#[test]
fn ids_are_unique_dotted_and_summaries_say_something() {
    let mut seen = std::collections::BTreeSet::new();
    for spec in SPECS {
        assert!(seen.insert(spec.id), "duplicate id {}", spec.id);
        assert!(
            !spec.summary.trim().is_empty(),
            "{} has no summary",
            spec.id
        );
        let parts: Vec<&str> = spec.id.split('.').collect();
        assert!(parts.len() >= 2, "{} is not dotted", spec.id);
        let named = |part: &str| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        };
        assert!(parts.iter().all(|p| named(p)), "{} is not dotted", spec.id);
    }
}

/// One of every variant, so a row `id` never returns is caught here.
///
/// `id` is an exhaustive match, so a new variant cannot build without a row;
/// what that cannot show is a row the match never names. Building each
/// variant and asking for its spec does.
#[test]
fn every_variant_has_a_row() {
    let mut intents = pane_intents();
    intents.extend(page_intents());
    intents.extend(mark_intents());
    intents.extend(reading_intents());
    intents.extend(settings_intents());
    assert_eq!(intents.len(), SPECS.len(), "a variant has no row");
    for intent in &intents {
        assert!(intent.spec().is_some(), "{} has no row", intent.id());
    }
}

/// What lands in a pane, what leaves one, and the library that fills them.
fn pane_intents() -> Vec<Intent> {
    let pane = PaneId(1);
    let book = BookId(1);
    vec![
        Intent::Split(SplitAxis::Horizontal),
        Intent::FocusPane(pane),
        Intent::CyclePane { forward: true },
        Intent::ClosePane(pane),
        Intent::SetRatio(vec![], 0.5),
        Intent::Zoom(ZoomCommand::Step(Step::In)),
        Intent::View(ViewMode::Single),
        Intent::Open(mareader_engine::documents::OpenRequest::bytes(
            vec![],
            "a.pdf",
        )),
        Intent::OpenBook(book),
        Intent::PlaceBook {
            book,
            pane: Some(pane),
            placement: mareader_engine::documents::Placement::Into,
        },
        Intent::ForgetPositions {
            books: vec![],
            paths: vec![],
        },
        Intent::CopyPages {
            pane,
            pages: vec![],
        },
        Intent::Export {
            name: String::new(),
            format: Format::Markdown,
            contents: String::new(),
        },
        Intent::Library(mareader_engine::library::Action::ShowAdd),
    ]
}

/// What moves the page a pane shows.
fn page_intents() -> Vec<Intent> {
    let pane = PaneId(1);
    vec![
        Intent::Arrow(Arrow::Left),
        Intent::Page { forward: true },
        Intent::AutoScroll,
        Intent::FirstPage,
        Intent::LastPage,
        Intent::GoTo { pane, page: 0 },
        Intent::Turn {
            pane,
            forward: true,
        },
        Intent::WordList {
            pane,
            pages: vec![],
        },
        Intent::WordListWindow {
            pane,
            pages: vec![],
        },
    ]
}

/// What the reader marks, and what a marked word is asked.
fn mark_intents() -> Vec<Intent> {
    let pane = PaneId(1);
    vec![
        Intent::Bookmark(Some(pane)),
        Intent::Highlight {
            pane,
            pen: Pen::default(),
            color: None,
        },
        Intent::Note(pane),
        Intent::SelectText {
            pane,
            page: 0,
            text: String::new(),
        },
        Intent::Explain(pane),
        Intent::LookUp(pane),
        Intent::Study(StudyAction::Close),
        Intent::Dict(DictAction::CloseCard),
        Intent::Vocab(Download::Start),
        Intent::Pronunciation(Download::Start),
    ]
}

/// What is heard, and where the voice that says it comes from.
fn reading_intents() -> Vec<Intent> {
    let pane = PaneId(1);
    // A const, so the borrow is 'static as `RevealVoice` needs.
    let model: &'static mareader_speech::Model = &KOKORO;
    vec![
        Intent::ReadAloud {
            pane,
            from: ReadFrom::default(),
        },
        Intent::ToggleReadAloud,
        Intent::StopReadAloud,
        Intent::PlaceReadMarker {
            pane,
            spot: Spot::default(),
        },
        Intent::SayWord(String::new()),
        Intent::SayWordWith {
            word: String::new(),
            voice: String::new(),
        },
        Intent::StopSaying,
        Intent::TryVoice,
        Intent::TryVoiceOf(String::new()),
        Intent::StopVoiceTry,
        Intent::RevealVoice(model, "en"),
        Intent::VoiceModel(Item::Model("kokoro"), Download::Start),
    ]
}

/// What is kept between launches, and the AI's own asks.
fn settings_intents() -> Vec<Intent> {
    let pane = PaneId(1);
    let quick = QuickText::of(&mareader_settings::TextSettings::default());
    vec![
        Intent::OpenSettings(Tab::Appearance),
        Intent::SetSettings(Box::default()),
        Intent::SetField {
            path: String::new(),
            value: String::new(),
        },
        Intent::SetAppearance(Appearance::default()),
        Intent::SetQuickText(quick),
        Intent::SetInkContrast(1.0),
        Intent::SetIndependence(Independence {
            themes: false,
            textures: false,
        }),
        Intent::SavePreset {
            name: String::new(),
            group: String::new(),
            look: Appearance::default(),
        },
        Intent::DeletePreset(String::new()),
        Intent::SetDictionary(Box::default()),
        Intent::SetLibraryView(View::default()),
        Intent::SetSidebarOverlay(false),
        Intent::SetTitlebarPinned(false),
        Intent::PageAi(PageAiAction::StopFollow(pane)),
        #[cfg(not(target_arch = "wasm32"))]
        Intent::Ai(mareader_settings::Job::Probe),
    ]
}
