//! The built-in presets: mareader's "Classic" group, value for value.

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

#[cfg(test)]
mod tests {
    use super::*;

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
