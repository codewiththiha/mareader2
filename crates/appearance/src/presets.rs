//! Presets: mareader's "Classic" looks, and the sections they show in.

use serde::{Deserialize, Serialize};

use crate::model::{Appearance, BaseMode, NoiseMode, TextureMode};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub group: String,
    pub appearance: Appearance,
}

fn classic(id: &str, name: &str, appearance: Appearance) -> Preset {
    Preset {
        id: id.into(),
        name: name.into(),
        group: "Classic".into(),
        appearance,
    }
}

/// Plain bases are not presets: the Mode buttons are their one home.
pub fn builtin_presets() -> Vec<Preset> {
    let d = Appearance::default();
    vec![
        classic(
            "sepia",
            "Sepia",
            Appearance {
                tint_hue: 34,
                tint_strength: 23,
                ..d
            },
        ),
        classic(
            "green",
            "Green",
            Appearance {
                tint_hue: 104,
                tint_strength: 20,
                ..d
            },
        ),
        classic(
            "night",
            "Night",
            Appearance {
                base: BaseMode::Dark,
                tint_hue: 110,
                tint_strength: 18,
                ..d
            },
        ),
        classic(
            "parchment",
            "Parchment",
            Appearance {
                tint_hue: 40,
                tint_strength: 28,
                texture: TextureMode::Paper,
                texture_opacity: 85,
                texture_scale: 110,
                noise: NoiseMode::Static,
                noise_intensity: 18,
                ..d
            },
        ),
        classic(
            "cinema",
            "Cinema",
            Appearance {
                base: BaseMode::Dim,
                tint_hue: 220,
                tint_strength: 15,
                noise: NoiseMode::Animated,
                noise_intensity: 30,
                ..d
            },
        ),
    ]
}

/// Where presets saved without a section show.
pub const UNSECTIONED: &str = "Custom";

/// A section of the gallery: its title and presets, in first-seen order.
#[derive(Debug, PartialEq)]
pub struct PresetGroup<'a> {
    pub name: &'a str,
    pub presets: Vec<&'a Preset>,
}

/// Sorts presets into sections, keeping the order each first appears.
pub fn group_presets(presets: &[Preset]) -> Vec<PresetGroup<'_>> {
    let mut groups: Vec<PresetGroup<'_>> = Vec::new();
    for p in presets {
        let name = match p.group.trim() {
            "" => UNSECTIONED,
            name => name,
        };
        match groups.iter_mut().find(|g| g.name == name) {
            Some(g) => g.presets.push(p),
            None => groups.push(PresetGroup {
                name,
                presets: vec![p],
            }),
        }
    }
    groups
}

/// The sections the user's presets already use, for saving into one.
pub fn section_names(presets: &[Preset]) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for p in presets {
        let name = p.group.trim();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// A slug of `name`, numbered past ids `existing` or the built-ins hold.
pub fn preset_id(name: &str, existing: &[Preset]) -> String {
    let mut slug = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = match slug.trim_matches('-') {
        "" => "preset",
        s => s,
    };
    let builtin = builtin_presets();
    let taken = |id: &str| existing.iter().chain(&builtin).any(|p| p.id == id);
    let mut id = slug.to_owned();
    let mut n = 2;
    while taken(&id) {
        id = format!("{slug}-{n}");
        n += 1;
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str, group: &str) -> Preset {
        Preset {
            id: id.into(),
            name: id.into(),
            group: group.into(),
            appearance: Appearance::default(),
        }
    }

    #[test]
    fn sections_keep_first_seen_order_and_collect_the_unsectioned() {
        let mut all = builtin_presets();
        all.extend([user("a", "Night"), user("b", " "), user("c", "Night ")]);
        let groups = group_presets(&all);
        let names: Vec<_> = groups.iter().map(|g| g.name).collect();
        assert_eq!(names, ["Classic", "Night", UNSECTIONED]);
        let night: Vec<_> = groups
            .get(1)
            .map(|g| g.presets.iter().map(|p| p.id.as_str()).collect())
            .unwrap_or_default();
        assert_eq!(night, ["a", "c"]);
        assert_eq!(section_names(all.get(5..).unwrap_or_default()), ["Night"]);
    }

    #[test]
    fn ids_are_slugs_numbered_past_taken_ones() {
        assert_eq!(preset_id("  Café -- nuit! ", &[]), "caf-nuit");
        assert_eq!(preset_id("???", &[]), "preset");
        assert_eq!(preset_id("Sepia", &[]), "sepia-2", "a built-in holds it");
        let taken = [user("owl", ""), user("owl-2", "")];
        assert_eq!(preset_id("Owl", &taken), "owl-3");
    }

    #[test]
    fn five_classics_with_unique_ids() {
        let p = builtin_presets();
        let ids: Vec<_> = p.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["sepia", "green", "night", "parchment", "cinema"]);
        assert!(p.iter().all(|p| p.group == "Classic"));
    }

    #[test]
    fn every_preset_is_already_sane() {
        for p in builtin_presets() {
            let mut a = p.appearance;
            a.sanitize();
            assert_eq!(a, p.appearance, "{}", p.id);
        }
    }
}
