//! The packs on offer: which parquet joins which two languages.

// Pinned to a wikidict commit, so a pack's bytes never change under it.
const REPO: &str = "https://raw.githubusercontent.com/codewiththiha/wikidict/e9276c9df7d55f7223d440dd7228d6102e96e2c1/packs";
const CDN: &str = "https://cdn.jsdelivr.net/gh/codewiththiha/wikidict@e9276c9df7d55f7223d440dd7228d6102e96e2c1/packs";
const FASTLY: &str = "https://fastly.jsdelivr.net/gh/codewiththiha/wikidict@e9276c9df7d55f7223d440dd7228d6102e96e2c1/packs";

/// One downloadable dictionary, as mareader offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pack {
    /// Its id and its files' stem.
    pub id: &'static str,
    /// The headword language; English in every pack so far.
    pub source: &'static str,
    /// The definition language.
    pub target: &'static str,
    pub entries: u64,
    /// The parquet's size when listed, for a size before the download.
    pub bytes: u64,
}

/// Every pack the app offers. A language is one more row.
pub const PACKS: [Pack; 8] = [
    Pack {
        id: "mcfnlp-en-my",
        source: "en",
        target: "my",
        entries: 107_861,
        bytes: 1_532_419,
    },
    Pack {
        id: "jmdict-en-jp",
        source: "en",
        target: "jp",
        entries: 421_385,
        bytes: 6_915_158,
    },
    Pack {
        id: "cedict-en-zh",
        source: "en",
        target: "zh",
        entries: 178_950,
        bytes: 3_834_540,
    },
    Pack {
        id: "krdict-en-ko",
        source: "en",
        target: "ko",
        entries: 106_822,
        bytes: 2_503_216,
    },
    Pack {
        id: "volubilis-en-th",
        source: "en",
        target: "th",
        entries: 191_708,
        bytes: 3_135_314,
    },
    Pack {
        id: "wiktionary-en-fr",
        source: "en",
        target: "fr",
        entries: 128_246,
        bytes: 2_671_738,
    },
    Pack {
        id: "wiktionary-en-es",
        source: "en",
        target: "es",
        entries: 146_587,
        bytes: 2_724_098,
    },
    Pack {
        id: "wiktionary-en-de",
        source: "en",
        target: "de",
        entries: 161_737,
        bytes: 3_087_656,
    },
];

impl Pack {
    /// The pack with this id, if it is offered.
    pub fn get(id: &str) -> Option<&'static Pack> {
        PACKS.iter().find(|p| p.id == id)
    }

    /// Where to fetch it: the repository, then two mirrors of it.
    pub fn urls(&self) -> [String; 3] {
        [REPO, CDN, FASTLY].map(|base| format!("{base}/{}.parquet", self.id))
    }

    /// "English → Burmese".
    pub fn label(&self) -> String {
        format!(
            "{} → {}",
            crate::lang::name(self.source),
            crate::lang::name(self.target)
        )
    }

    /// Whether it joins `a` and `b`, either way round.
    pub fn joins(&self, a: &str, b: &str) -> bool {
        (self.source == a && self.target == b) || (self.source == b && self.target == a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pack_is_fetched_from_three_places_by_its_id() {
        for pack in PACKS {
            let urls = pack.urls();
            assert!(
                urls.iter()
                    .all(|u| u.ends_with(&format!("/{}.parquet", pack.id)))
            );
            assert!(urls.iter().all(|u| u.starts_with("https://")));
            assert_eq!(Pack::get(pack.id), Some(&pack));
        }
        assert_eq!(Pack::get("nope"), None);
        assert_eq!(PACKS[0].label(), "English → Burmese");
    }
}
