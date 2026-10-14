//! Split panes' looks of their own: colour, texture, or both.

use crate::{Appearance, TextureMode};

/// A pane's own colour starts this strong when the window is untinted.
pub const PANE_TINT_STRENGTH: u8 = 35;

/// Which families a pane owns: the workspace settings and the split.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Routing {
    /// Independent themes: colour and texture are each pane's own.
    pub themes: bool,
    /// Independent textures: each pane gets a pattern of its own.
    pub textures: bool,
    /// Under independent themes, light, dark or dim stays shared.
    pub shared_base: bool,
    /// Both modes serve a split; one pane stands them down.
    pub split: bool,
}

impl Routing {
    /// Whether base and tint are each pane's.
    pub fn colour(self) -> bool {
        self.split && self.themes
    }

    /// Whether texture and its dials are each pane's.
    pub fn texture(self) -> bool {
        self.split && (self.themes || self.textures)
    }

    /// Whether any pane may differ from the window.
    pub fn any(self) -> bool {
        self.texture()
    }

    fn patterns(self) -> bool {
        self.split && self.textures
    }
}

/// Each pane's look while a mode is in effect; others show the window's.
#[derive(Clone, Debug)]
pub struct PaneLooks<K> {
    looks: Vec<(K, Appearance)>,
    /// The panes, and the one in front, at the last reconcile.
    placed: Vec<K>,
    front: Option<K>,
}

impl<K> Default for PaneLooks<K> {
    fn default() -> Self {
        Self {
            looks: Vec::new(),
            placed: Vec::new(),
            front: None,
        }
    }
}

impl<K: Copy + PartialEq> PaneLooks<K> {
    /// `pane`'s look as drawn; grain is always the window's.
    pub fn shown(&self, pane: K, global: &Appearance, r: Routing) -> Appearance {
        let mut look = self.get(pane).unwrap_or(*global);
        if !r.colour() {
            copy_colour(&mut look, global);
        } else if r.shared_base {
            look.base = global.base;
        }
        if !r.texture() {
            copy_texture(&mut look, global);
        }
        look.noise = global.noise;
        look.noise_intensity = global.noise_intensity;
        look
    }

    /// Lands an edit of `pane`'s look, each family where it is owned.
    pub fn edit(&mut self, pane: K, global: &mut Appearance, r: Routing, after: Appearance) {
        let mut look = self.shown(pane, global, r);
        if r.colour() {
            // A shared base switch is the window's, so every pane follows.
            if r.shared_base {
                global.base = after.base;
            }
            copy_colour(&mut look, &after);
        } else {
            copy_colour(global, &after);
        }
        if r.texture() {
            copy_texture(&mut look, &after);
        } else {
            copy_texture(global, &after);
        }
        global.noise = after.noise;
        global.noise_intensity = after.noise_intensity;
        global.sanitize();
        if r.any() {
            look.sanitize();
            self.set(pane, look);
        }
    }

    /// Follows this frame's routing and panes from the last frame's.
    pub fn reconcile(
        &mut self,
        (was, now): (Routing, Routing),
        (placed, active): (&[K], K),
        global: &mut Appearance,
        random: &mut impl FnMut() -> f64,
    ) {
        let before = std::mem::replace(&mut self.placed, placed.to_vec());
        let front = self.front.replace(active);
        if was.split && !now.split {
            // The survivor's look becomes the window's.
            if let (true, Some(&survivor)) = (was.any(), placed.first()) {
                *global = self.shown(survivor, global, was);
            }
            self.looks.clear();
            return;
        }
        if !now.any() {
            self.looks.clear();
            return;
        }
        self.looks.retain(|(k, _)| placed.contains(k));
        // The pane in front keeps its look; one born now defers to its elder.
        let elder = |k: &K| before.contains(k) && placed.contains(k);
        let keeper = [Some(active), front]
            .into_iter()
            .flatten()
            .find(elder)
            .or_else(|| placed.iter().copied().find(elder))
            .unwrap_or(active);
        let mut enabled = false;
        if now.colour() && !was.colour() {
            self.enable_colours(placed, keeper, global, random);
            enabled = true;
        }
        if now.patterns() && !was.patterns() {
            self.enable_patterns(placed, keeper, global, random);
            enabled = true;
        } else if was.patterns() && !now.patterns() {
            for (_, look) in &mut self.looks {
                copy_texture(look, global);
            }
        }
        if enabled {
            return;
        }
        let from = self.shown(keeper, global, now);
        for &pane in placed.iter().filter(|k| !before.contains(k)) {
            let look = self.distinct_look(from, global, now, random);
            self.set(pane, look);
        }
    }

    fn get(&self, pane: K) -> Option<Appearance> {
        self.looks.iter().find(|(k, _)| *k == pane).map(|(_, a)| *a)
    }

    fn set(&mut self, pane: K, look: Appearance) {
        match self.looks.iter_mut().find(|(k, _)| *k == pane) {
            Some((_, a)) => *a = look,
            None => self.looks.push((pane, look)),
        }
    }

    /// The keeper shows the window's colour; every other pane its own.
    fn enable_colours(
        &mut self,
        placed: &[K],
        keeper: K,
        global: &Appearance,
        random: &mut impl FnMut() -> f64,
    ) {
        // Old colours would otherwise count as taken.
        for (_, look) in &mut self.looks {
            copy_colour(look, global);
        }
        for &pane in placed {
            let from = self.get(pane).unwrap_or(*global);
            let look = if pane == keeper {
                from
            } else {
                self.distinct_colour(from, global, random)
            };
            self.set(pane, look);
        }
    }

    /// Every pane gets a texture unlike the others'; the keeper keeps one.
    fn enable_patterns(
        &mut self,
        placed: &[K],
        keeper: K,
        global: &Appearance,
        random: &mut impl FnMut() -> f64,
    ) {
        let mut taken = vec![global.texture];
        for &pane in placed {
            let mut look = self.get(pane).unwrap_or(*global);
            copy_texture(&mut look, global);
            if pane != keeper || look.texture == TextureMode::None {
                look.texture = distinct_texture(&taken, random());
                taken.push(look.texture);
            }
            self.set(pane, look);
        }
    }

    /// A new pane's look: `from` with what the modes make distinct.
    fn distinct_look(
        &self,
        from: Appearance,
        global: &Appearance,
        r: Routing,
        random: &mut impl FnMut() -> f64,
    ) -> Appearance {
        let mut look = if r.colour() {
            self.distinct_colour(from, global, random)
        } else {
            from
        };
        if r.patterns() {
            let mut taken: Vec<TextureMode> = self.looks.iter().map(|(_, a)| a.texture).collect();
            taken.push(global.texture);
            look.texture = distinct_texture(&taken, random());
        }
        look.sanitize();
        look
    }

    /// `from` tinted with a hue unlike every pane's and the window's.
    fn distinct_colour(
        &self,
        from: Appearance,
        global: &Appearance,
        random: &mut impl FnMut() -> f64,
    ) -> Appearance {
        let mut taken: Vec<u16> = self
            .looks
            .iter()
            .filter(|(_, a)| a.has_tint())
            .map(|(_, a)| a.tint_hue)
            .collect();
        if global.has_tint() {
            taken.push(global.tint_hue);
        }
        let mut look = from;
        look.tint_hue = distinct_hue(&taken, random());
        if !look.has_tint() {
            look.tint_strength = PANE_TINT_STRENGTH;
        }
        look.sanitize();
        look
    }
}

fn copy_colour(dst: &mut Appearance, src: &Appearance) {
    dst.base = src.base;
    dst.tint_hue = src.tint_hue;
    dst.tint_strength = src.tint_strength;
}

fn copy_texture(dst: &mut Appearance, src: &Appearance) {
    dst.texture = src.texture;
    dst.texture_opacity = src.texture_opacity;
    dst.texture_scale = src.texture_scale;
}

/// A hue in the middle half of the widest gap between `taken` hues.
fn distinct_hue(taken: &[u16], random: f64) -> u16 {
    let random = random.clamp(0.0, 1.0);
    let mut hues: Vec<f64> = taken.iter().map(|h| f64::from(h % 360)).collect();
    hues.sort_by(f64::total_cmp);
    hues.dedup();
    let (Some(&first), Some(&last)) = (hues.first(), hues.last()) else {
        return ((random * 360.0) as u16) % 360;
    };
    let (mut start, mut gap) = (last, first + 360.0 - last);
    for pair in hues.windows(2) {
        if let [a, b] = *pair
            && b - a > gap
        {
            (start, gap) = (a, b - a);
        }
    }
    let hue = start + gap * (0.25 + 0.5 * random);
    (hue.round() as u16) % 360
}

/// A painted texture not in `taken`, or any painted one if none is left.
fn distinct_texture(taken: &[TextureMode], random: f64) -> TextureMode {
    let painted = TextureMode::ALL
        .into_iter()
        .filter(|m| *m != TextureMode::None);
    let free: Vec<TextureMode> = painted.clone().filter(|m| !taken.contains(m)).collect();
    let pool = if free.is_empty() {
        painted.collect()
    } else {
        free
    };
    let pick = (random.clamp(0.0, 1.0) * pool.len() as f64) as usize;
    pool.get(pick.min(pool.len().saturating_sub(1)))
        .copied()
        .unwrap_or(TextureMode::Paper)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseMode, NoiseMode};

    const SPLIT: Routing = Routing {
        themes: true,
        textures: false,
        shared_base: false,
        split: true,
    };

    fn gap(a: u16, b: u16) -> u16 {
        let d = a.abs_diff(b) % 360;
        d.min(360 - d)
    }

    fn fixed(r: f64) -> impl FnMut() -> f64 {
        move || r
    }

    /// Panes 1 and 2 placed, 1 in front, `r` already reconciled.
    fn split(r: Routing, global: &mut Appearance) -> PaneLooks<u8> {
        let mut looks = PaneLooks::default();
        let one = Routing { split: false, ..r };
        looks.reconcile((one, one), (&[1], 1), global, &mut fixed(0.5));
        looks.reconcile((one, r), (&[1, 2], 2), global, &mut fixed(0.5));
        looks
    }

    #[test]
    fn any_hue_when_nothing_is_taken() {
        assert_eq!(distinct_hue(&[], 0.0), 0);
        assert_eq!(distinct_hue(&[], 0.5), 180);
        assert!(distinct_hue(&[], 1.0) < 360);
    }

    #[test]
    fn a_new_hue_lands_well_away_from_every_taken_one() {
        for r in [0.0, 0.3, 0.7, 1.0] {
            let h = distinct_hue(&[10], r);
            assert!(gap(h, 10) >= 90, "{h} too near 10");
            let taken = [0, 120, 240];
            let h = distinct_hue(&taken, r);
            assert!(taken.iter().all(|t| gap(h, *t) >= 30), "{h} near {taken:?}");
        }
    }

    #[test]
    fn a_new_texture_is_painted_and_unlike_the_taken() {
        let taken = [TextureMode::None, TextureMode::Grid, TextureMode::Paper];
        for r in [0.0, 0.5, 1.0] {
            let t = distinct_texture(&taken, r);
            assert!(!taken.contains(&t), "{t:?} repeats");
        }
        let all = TextureMode::ALL;
        assert_ne!(distinct_texture(&all, 1.0), TextureMode::None);
    }

    #[test]
    fn a_split_born_with_themes_on_colours_the_new_pane_only() {
        let mut global = Appearance::default();
        let looks = split(SPLIT, &mut global);
        assert_eq!(
            looks.shown(1, &global, SPLIT),
            global,
            "the elder keeps the look"
        );
        let two = looks.shown(2, &global, SPLIT);
        assert_eq!(two.tint_strength, PANE_TINT_STRENGTH);
        assert_eq!(two.base, global.base);
    }

    #[test]
    fn grain_and_unowned_families_are_the_window_s() {
        let r = Routing {
            themes: false,
            textures: true,
            ..SPLIT
        };
        let mut global = Appearance::default();
        let mut looks = split(r, &mut global);
        let two = looks.shown(2, &global, r);
        assert_ne!(two.texture, TextureMode::None, "a pattern of its own");
        let mut after = two;
        after.base = BaseMode::Dark;
        after.texture_scale = 200;
        after.noise = NoiseMode::Static;
        looks.edit(2, &mut global, r, after);
        assert_eq!(global.base, BaseMode::Dark, "colour is the window's here");
        assert_eq!(global.noise, NoiseMode::Static, "grain is always shared");
        assert_eq!(global.texture_scale, 100, "the texture stayed the pane's");
        assert_eq!(looks.shown(1, &global, r).noise, NoiseMode::Static);
        assert_eq!(looks.shown(2, &global, r).texture_scale, 200);
    }

    #[test]
    fn a_shared_base_switch_moves_every_pane() {
        let r = Routing {
            shared_base: true,
            ..SPLIT
        };
        let mut global = Appearance::default();
        let mut looks = split(r, &mut global);
        let mut after = looks.shown(2, &global, r);
        after.base = BaseMode::Dim;
        after.tint_hue = 300;
        looks.edit(2, &mut global, r, after);
        assert_eq!(looks.shown(1, &global, r).base, BaseMode::Dim);
        assert_eq!(looks.shown(2, &global, r).tint_hue, 300);
        assert_ne!(global.tint_hue, 300, "the hue stayed the pane's");
    }

    #[test]
    fn closing_to_one_pane_promotes_the_survivor_s_look() {
        let mut global = Appearance::default();
        let mut looks = split(SPLIT, &mut global);
        let two = looks.shown(2, &global, SPLIT);
        let one = Routing {
            split: false,
            ..SPLIT
        };
        looks.reconcile((SPLIT, one), (&[2], 2), &mut global, &mut fixed(0.5));
        assert_eq!(global, two);
        assert_eq!(looks.shown(2, &global, one), two);
    }

    #[test]
    fn turning_themes_off_shows_the_window_s_look_again() {
        let mut global = Appearance::default();
        let mut looks = split(SPLIT, &mut global);
        let off = Routing {
            themes: false,
            ..SPLIT
        };
        looks.reconcile((SPLIT, off), (&[1, 2], 2), &mut global, &mut fixed(0.5));
        assert_eq!(looks.shown(2, &global, off), global);
        assert_eq!(global, Appearance::default(), "nothing promoted");
    }

    #[test]
    fn enabling_in_a_split_keeps_the_front_pane_and_spreads_the_rest() {
        let off = Routing {
            themes: false,
            ..SPLIT
        };
        let mut global = Appearance {
            tint_strength: 50,
            tint_hue: 30,
            ..Appearance::default()
        };
        let mut looks = PaneLooks::default();
        let placed = [1, 2, 3];
        looks.reconcile((off, off), (&placed, 2), &mut global, &mut fixed(0.5));
        looks.reconcile((off, SPLIT), (&placed, 2), &mut global, &mut fixed(0.5));
        assert_eq!(looks.shown(2, &global, SPLIT), global);
        let (one, three) = (
            looks.shown(1, &global, SPLIT),
            looks.shown(3, &global, SPLIT),
        );
        assert_eq!(one.tint_strength, 50, "a tinted window keeps its strength");
        for h in [one.tint_hue, three.tint_hue] {
            assert!(gap(h, 30) >= 30, "{h} near the window's 30");
        }
        assert!(gap(one.tint_hue, three.tint_hue) >= 30);
    }

    #[test]
    fn enabling_textures_paints_every_pane_unlike_the_others() {
        let r = Routing {
            themes: false,
            textures: true,
            ..SPLIT
        };
        let off = Routing {
            textures: false,
            ..r
        };
        let placed = [1, 2, 3];
        for (window, r0) in [(TextureMode::None, 0.0), (TextureMode::Grid, 0.9)] {
            let mut global = Appearance {
                texture: window,
                ..Appearance::default()
            };
            let mut looks = PaneLooks::default();
            looks.reconcile((off, off), (&placed, 2), &mut global, &mut fixed(r0));
            looks.reconcile((off, r), (&placed, 2), &mut global, &mut fixed(r0));
            let shown = placed.map(|k| looks.shown(k, &global, r).texture);
            assert!(!shown.contains(&TextureMode::None), "{window:?}: {shown:?}");
            let mut unique = shown.to_vec();
            unique.sort_by_key(|m| *m as u8);
            unique.dedup();
            assert_eq!(unique.len(), 3, "{window:?}: {shown:?}");
            if window != TextureMode::None {
                assert_eq!(shown.get(1), Some(&window), "the front pane keeps its");
            }
        }
    }

    #[test]
    fn a_pane_split_later_gets_a_colour_of_its_own() {
        let mut global = Appearance::default();
        let mut looks = split(SPLIT, &mut global);
        looks.reconcile(
            (SPLIT, SPLIT),
            (&[1, 2, 3], 3),
            &mut global,
            &mut fixed(0.5),
        );
        let hues: Vec<u16> = [2, 3]
            .map(|k| looks.shown(k, &global, SPLIT).tint_hue)
            .into();
        assert!(
            hues.windows(2)
                .all(|w| matches!(*w, [a, b] if gap(a, b) >= 30))
        );
        looks.reconcile((SPLIT, SPLIT), (&[1, 3], 3), &mut global, &mut fixed(0.5));
        assert_eq!(
            looks.shown(2, &global, SPLIT),
            global,
            "a closed pane is forgotten"
        );
    }
}
