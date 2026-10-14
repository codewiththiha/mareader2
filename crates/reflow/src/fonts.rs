//! Bundled faces only for now, so layout matches on every machine.
use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use parley::{FontContext, LayoutContext};

/// The reading face; its weight axis covers every weight setting.
pub const SERIF: &str = "Literata";
/// The heading face, as mareader sets headings in the sans slot.
pub const SANS: &str = "Inter";

const FACES: [&[u8]; 5] = [
    include_bytes!("../../../assets/fonts/Literata-Variable.ttf"),
    include_bytes!("../../../assets/fonts/Literata-Italic-Variable.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
];

/// Font data plus layout scratch; one per layout job, never shared.
pub struct Fonts {
    pub(crate) fonts: FontContext,
    pub(crate) layout: LayoutContext<()>,
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Fonts")
    }
}

impl Fonts {
    /// The bundled faces, with `serif` and `sans-serif` mapped onto them.
    pub fn bundled() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        for face in FACES {
            collection.register_fonts(Blob::new(std::sync::Arc::new(face)), None);
        }
        for (generic, name) in [
            (GenericFamily::Serif, SERIF),
            (GenericFamily::SansSerif, SANS),
        ] {
            let id = collection.family_id(name);
            collection.set_generic_families(generic, id.into_iter());
        }
        Self {
            fonts: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            layout: LayoutContext::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_families_are_registered() {
        let mut f = Fonts::bundled();
        let names: Vec<String> = f
            .fonts
            .collection
            .family_names()
            .map(str::to_owned)
            .collect();
        assert!(names.iter().any(|n| n == SERIF), "{names:?}");
        assert!(names.iter().any(|n| n == SANS), "{names:?}");
    }
}
