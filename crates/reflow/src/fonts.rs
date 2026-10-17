//! Bundled faces, plus the machine's own when a reader picks one.
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use parley::{FontContext, LayoutContext};

/// The reading face; its weight axis covers every weight setting.
pub const SERIF: &str = "Literata";
/// The heading face, as mareader sets headings in the sans slot.
pub const SANS: &str = "Inter";
/// Code and preformatted text.
pub const MONO: &str = "JetBrains Mono";

/// Our own font files; a static, so their bytes have one address.
pub(crate) static BUNDLED: [&[u8]; 6] = [
    include_bytes!("../../../assets/fonts/Literata-Variable.ttf"),
    include_bytes!("../../../assets/fonts/Literata-Italic-Variable.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-Variable.ttf"),
];

/// Which generic family a face stands in for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Serif,
    Sans,
    Mono,
}

impl Kind {
    /// The bundled face of this kind: every stack's last resort.
    pub fn bundled(self) -> &'static str {
        match self {
            Self::Serif => SERIF,
            Self::Sans => SANS,
            Self::Mono => MONO,
        }
    }
}

/// A face shipped with the app; `id` is what settings store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Face {
    pub id: &'static str,
    /// Its family name, which is also what pickers show.
    pub name: &'static str,
    pub kind: Kind,
}

/// Every bundled face, in picker order.
pub const FACES: &[Face] = &[
    Face {
        id: "literata",
        name: SERIF,
        kind: Kind::Serif,
    },
    Face {
        id: "inter",
        name: SANS,
        kind: Kind::Sans,
    },
    Face {
        id: "jetbrains-mono",
        name: MONO,
        kind: Kind::Mono,
    },
];

/// One entry of a font stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Family {
    Named(String),
    /// The system's own face of a kind, such as its default serif.
    Generic(Kind),
}

/// The stacks text is set in, tried in order; Inter covers symbols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Faces {
    pub body: Vec<Family>,
    pub heading: Vec<Family>,
    /// Code, inline and preformatted.
    pub mono: Vec<Family>,
}

impl Default for Faces {
    fn default() -> Self {
        let one = |name: &str| vec![Family::Named(name.to_owned())];
        Self {
            body: one(SERIF),
            heading: one(SANS),
            mono: one(MONO),
        }
    }
}

/// A run's grey level when drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Tone {
    #[default]
    Body,
    /// Quotes: a little lighter than body text.
    Soft,
    /// The smallest heading, as mareader mutes h6.
    Muted,
}

/// What parley carries per run for drawing: tone, and a code ground.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Ink {
    pub(crate) tone: Tone,
    pub(crate) code: bool,
}

/// Font data plus layout scratch; one per layout job, never shared.
pub struct Fonts {
    pub(crate) fonts: FontContext,
    pub(crate) layout: LayoutContext<Ink>,
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Fonts")
    }
}

fn register(collection: &mut Collection) {
    for file in BUNDLED {
        collection.register_fonts(Blob::new(Arc::new(file)), None);
    }
}

/// The machine's faces and ours, scanned once; jobs take clones.
fn system() -> &'static Mutex<Collection> {
    static SYSTEM: OnceLock<Mutex<Collection>> = OnceLock::new();
    SYSTEM.get_or_init(|| {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: true,
        });
        register(&mut collection);
        Mutex::new(collection)
    })
}

impl Fonts {
    /// Bundled faces alone, generics mapped onto them: same layout anywhere.
    pub fn bundled() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        register(&mut collection);
        for (generic, kind) in [
            (GenericFamily::Serif, Kind::Serif),
            (GenericFamily::SansSerif, Kind::Sans),
            (GenericFamily::Monospace, Kind::Mono),
        ] {
            let id = collection.family_id(kind.bundled());
            collection.set_generic_families(generic, id.into_iter());
        }
        Self::with(collection)
    }

    /// The machine's faces as well; generics are the system's own.
    pub fn system() -> Self {
        let collection = system()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        Self::with(collection)
    }

    fn with(collection: Collection) -> Self {
        Self {
            fonts: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            layout: LayoutContext::new(),
        }
    }
}

/// Whether a face of this family name exists, bundled or installed.
pub fn has_family(name: &str) -> bool {
    let mut collection = system().lock().unwrap_or_else(PoisonError::into_inner);
    collection.family_id(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_is_registered() {
        let mut f = Fonts::bundled();
        let names: Vec<String> = f
            .fonts
            .collection
            .family_names()
            .map(str::to_owned)
            .collect();
        for face in FACES {
            assert!(names.iter().any(|n| n == face.name), "{names:?}");
        }
    }

    #[test]
    fn bundled_faces_count_as_installed_and_junk_does_not() {
        for face in FACES {
            assert!(has_family(face.name), "{}", face.name);
        }
        assert!(!has_family("No Such Face 7f3a"));
    }
}
