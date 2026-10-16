//! A clicked word's role in its sentence, and that sense's level.

#[cfg(not(target_arch = "wasm32"))]
pub use tagger::Roles;

/// What the tagger and the dataset say of one word where it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Role {
    /// The tag, as `VBD`.
    pub pos: String,
    /// Its word class, as "verb"; "other" when the tag names none.
    pub kind: &'static str,
    /// That sense's level, else none if the dataset lacks it.
    pub level: Option<f64>,
}

impl Role {
    /// How a card names it: "verb · C1".
    pub fn label(&self) -> String {
        let class = match self.kind {
            "other" => self.pos.as_str(),
            kind => kind,
        };
        match self.level {
            Some(level) => format!("{class} · {}", cefr::level::level_to_cefr(level)),
            None => class.to_owned(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod tagger {
    use std::path::Path;

    use cefr::pos::Tagger;
    use cefr::tags::kind_of;

    use super::Role;
    use crate::store::Store;

    /// The loaded model; loading takes a moment, so it is kept.
    pub struct Roles {
        tagger: Tagger,
    }

    impl Roles {
        pub fn load(model: &Path) -> Result<Self, String> {
            let tagger = Tagger::from_model_path(model).map_err(|e| format!("{e:#}"))?;
            Ok(Self { tagger })
        }

        /// `word`'s role in `sentence`, its level read from `store`.
        pub fn of(&self, word: &str, sentence: &str, store: Option<&Store>) -> Option<Role> {
            let tag = self.tagger.pos_in_context(word, sentence)?;
            let key = crate::normalize(word);
            let sense = store.and_then(|s| s.sense(&key, &tag.pos).ok().flatten());
            Some(Role {
                kind: kind_of(&tag.pos),
                level: sense.map(|s| s.level),
                pos: tag.pos,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_reads_as_its_class_and_level() {
        let role = |kind, level| Role {
            pos: "VBD".into(),
            kind,
            level,
        };
        assert_eq!(role("verb", Some(5.2)).label(), "verb · C1");
        assert_eq!(role("verb", None).label(), "verb");
        assert_eq!(role("other", Some(1.0)).label(), "VBD · A1");
    }
}
