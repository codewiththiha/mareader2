use super::*;

impl MareaderApp {
    /// Restyles egui when the look changed; true if it did.
    pub(super) fn restyle(&mut self, ctx: &Context) -> bool {
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        if self.ui.live.poll(ctx) {
            self.ui.styled = None;
        }
        let reduced = self.ui.reduced_motion.load(Ordering::Relaxed);
        let want = (
            self.settings().appearance,
            self.settings().motion.clone(),
            reduced,
        );
        if self.ui.styled.as_ref() == Some(&want) {
            return false;
        }
        #[allow(unused_mut)]
        let mut tokens = theme::Tokens::new(&self.settings().appearance);
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        self.ui.live.overlay(&mut tokens);
        theme::apply(ctx, &tokens);
        motion::set(ctx, &self.settings().motion, reduced);
        self.ui.styled = Some(want);
        true
    }

    /// Panes whose book sets its own design: their text, so overridden.
    pub(super) fn book_designs(&self) -> Vec<(PaneId, TextSettings)> {
        let own = self.ui.texts.own(&self.settings().text);
        let lib = self.ui.library.model();
        let pane_text = |pane: PaneId| {
            let mine = own.iter().find(|(p, _)| *p == pane);
            mine.map_or(&self.settings().text, |(_, t)| *t)
        };
        let designed = |pane: PaneId| {
            let book = self.ui.library.book_in(&self.ui.documents, pane)?;
            let design = lib.book(book)?.design.value?;
            let book_design = match design {
                mareader_library::Design::Auto => BookDesign::Auto,
                mareader_library::Design::On => BookDesign::On,
                mareader_library::Design::Off => BookDesign::Off,
            };
            Some((
                pane,
                TextSettings {
                    book_design,
                    ..pane_text(pane).clone()
                },
            ))
        };
        self.tree()
            .panes()
            .into_iter()
            .filter_map(designed)
            .collect()
    }

    /// What the focused pane and the workspace show: which settings apply.
    pub fn settings_scope(&self) -> Scope {
        let docs = &self.ui.documents;
        let reading = docs.reading(self.active());
        let focused = match &reading {
            None => Shown::Nothing,
            Some(r) if r.document.is_reflowable() => Shown::Book {
                stream: r.mode == ViewMode::Vertical,
            },
            Some(_) => Shown::Pdf,
        };
        // An AI page on screen is set in the pane's type, like a book.
        let ai = |pane: PaneId| self.ui.page_ai.on_screen(pane);
        let text = |p: PaneId| docs.reflowable(p) || ai(p);
        let book_design = match focused {
            Shown::Book { .. } => self.ui.library.book_in(docs, self.active()).and_then(|id| {
                let book = self.ui.library.model().book(id)?;
                Some((id, book.design.value))
            }),
            _ => None,
        };
        Scope {
            book_design,
            focused,
            split: self.tree().len() > 1,
            any_text: self.tree().panes().into_iter().any(text),
            ai_pages: focused == Shown::Pdf && ai(self.active()),
            sideways: reading.is_some_and(|r| r.mode == ViewMode::Horizontal),
            explains: self.ui.study.explains(),
        }
    }

    /// How far toasts rise: over the page bar in the corner while it shows.
    pub(super) fn toast_lift(&self, ctx: &Context, stage: Rect) -> f32 {
        let corner = stage.max - egui::vec2(24.0, 24.0);
        let under = |(id, rect): &(PaneId, Rect)| {
            rect.contains(corner) && self.ui.page_bars.get(id).is_some_and(PageBar::shown)
        };
        let up = !self.ui.chromeless && self.ui.pane_rects.iter().any(under);
        let target = if up { page_bar::BAR_H } else { 0.0 };
        let time = crate::motion::secs(ctx, crate::toast::FADE);
        ctx.animate_value_with_time(egui::Id::new("toast-lift"), target, time)
    }

    /// What the bars show behind them: pane fills, then this frame's pages.
    pub(super) fn backdrop(&mut self, ctx: &Context, view: &PaneView) -> Backdrop {
        let drawn = self.ui.documents.take_drawn();
        if !self.settings().chrome.glass {
            return Backdrop::Solid;
        }
        let Some(gpu) = self.ui.documents.gpu() else {
            return Backdrop::Tint;
        };
        let t = theme::Tokens::get(ctx);
        let fill = |id| view.style(id).map_or(t.paper, |s| s.fill);
        let fills = self
            .ui
            .pane_rects
            .iter()
            .map(|(id, r)| Quad::fill(*r, fill(*id)));
        let mut quads: Vec<Quad> = fills.collect();
        // The library page's paper stands in for the panes.
        quads.extend(self.ui.library_rect.map(|r| Quad::fill(r, t.paper)));
        quads.extend(drawn);
        Backdrop::Glass(Glass {
            pass: gpu.pass.clone(),
            canvas: t.surface,
            quads,
        })
    }

    /// Blend's raw paper: each pane's own, or else shared by every pane.
    pub fn blend_paper(&self, pane: PaneId) -> Option<Rgb> {
        let paper = &self.settings().paper;
        if !paper.blend {
            return None;
        }
        if self.routing().colour() {
            return self.ui.documents.paper(pane, paper.area);
        }
        // The latest focused pane with a paper: a book's has none, as mareader.
        self.recent()
            .iter()
            .find_map(|&p| self.ui.documents.paper(p, paper.area))
    }

    /// The latest focused pane showing a PDF: shared blend reads its paper.
    pub(super) fn paper_source(&self) -> Option<PaneId> {
        let d = &self.ui.documents;
        self.recent()
            .iter()
            .copied()
            .find(|&p| d.page_count(p).is_some() && !d.reflowable(p))
    }

    /// Which families split panes own right now.
    pub(super) fn routing(&self) -> Routing {
        routing(self.settings(), self.tree().len() > 1)
    }

    pub(super) fn held(&self) -> Held {
        Held {
            settings: self.settings().clone(),
            looks: self.ui.looks.clone(),
            routed: self.ui.routed,
        }
    }

    pub(super) fn restore(&mut self, held: Held) {
        self.set_settings(held.settings.clone());
        self.ui.looks = held.looks;
        self.ui.routed = held.routed;
    }

    /// Whether the window or a pane jumped to another look since `before`.
    pub(super) fn leapt(&self, before: &Held) -> bool {
        let (old, new) = (&before.settings, self.settings());
        let was = routing(old, self.tree().len() > 1);
        let jumped = |id: PaneId| {
            let from = before.looks.shown(id, &old.appearance, was);
            from.leaps_to(&self.look(id))
        };
        old.paper.blend != new.paper.blend
            || old.appearance.leaps_to(&new.appearance)
            || self.tree().panes().into_iter().any(jumped)
    }

    /// A fade needs motion, the GPU's snapshot, and no slider held.
    pub(super) fn may_fade(&self, ctx: &Context) -> bool {
        let held = ctx.input(|i| i.pointer.any_down());
        motion::enabled(ctx) && self.ui.documents.gpu().is_some() && !held
    }

    /// `pane`'s look as drawn: the window's, or its own while split.
    pub fn look(&self, pane: PaneId) -> Appearance {
        let global = &self.settings().appearance;
        self.ui.looks.shown(pane, global, self.routing())
    }

    /// Fits the panes' own text to the panes and the focused one.
    pub(super) fn reconcile_texts(&mut self) {
        let on = self.settings().workspace.independent_text;
        let placed = self.tree().panes();
        let mut window = self.settings().text.clone();
        self.ui
            .texts
            .reconcile(on, (&placed, self.active()), &mut window);
        let _ = self.set_field("text", window);
    }

    /// `pane`'s text settings: the window's, or its own while independent.
    pub fn text(&self, pane: PaneId) -> &mareader_settings::TextSettings {
        self.ui.texts.of(pane, &self.settings().text)
    }

    /// Fits the panes' looks to this frame's routing and panes.
    pub(super) fn reconcile_looks(&mut self, ctx: &Context) {
        let now = self.routing();
        let placed = self.tree().panes();
        let mut state = self.ui.random ^ ctx.input(|i| i.time).to_bits();
        let mut random = || {
            state = splitmix(state);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut global = self.settings().appearance;
        let changes = (self.ui.routed, now);
        self.ui
            .looks
            .reconcile(changes, (&placed, self.active()), &mut global, &mut random);
        let _ = self.set_field("appearance", global);
        self.ui.random = state;
        self.ui.routed = now;
    }

    /// Each pane's theme this frame, and its blend paper.
    pub(super) fn pane_styles(&self, window: &theme::Tokens) -> Vec<(PaneId, PaneStyle)> {
        let own = self.routing().any();
        let style = |id: PaneId| {
            let tokens = if own {
                theme::Tokens::new(&self.look(id))
            } else {
                *window
            };
            let paper = self.blend_paper(id);
            // A book's pane takes its page's paper, as mareader's text panes.
            let plain = match self.ui.documents.reflowable(id) {
                true => theme::color(Rgb::from_unit(tokens.book.paper)),
                false => tokens.paper,
            };
            let fill = paper.map_or(plain, |raw| baked(&tokens, raw));
            let style = PaneStyle {
                tokens,
                paper,
                fill,
                own,
            };
            (id, style)
        };
        self.tree().panes().into_iter().map(style).collect()
    }

    /// The grain over every layer, menus and windows included, as mareader's.
    pub(super) fn film_grain(&self, ctx: &Context) {
        let (Some(grain), Some(gpu)) = (
            FilmGrain::of(&self.settings().appearance),
            self.ui.documents.gpu(),
        ) else {
            return;
        };
        let offset = crawl_offset(ctx, grain);
        let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("film-grain"));
        let shape = GrainCallback::window(gpu.grain.clone(), ctx.content_rect(), grain, offset);
        ctx.layer_painter(layer).add(shape);
    }

    /// The reader's half of the bar: rail toggle, tools and the menu's state.
    pub(super) fn reader_bar(&self, overlay: bool) -> ReaderBar {
        let routing = self.routing();
        ReaderBar {
            sidebar_toggle: !overlay && !self.ui.sidebar.is_open(),
            rail_docked: !overlay && self.ui.sidebar.is_open(),
            independence: routing.split.then_some(Independence {
                themes: routing.themes,
                textures: routing.textures,
            }),
            zoom: self.ui.documents.zoom_state(self.active()),
            mode: self.ui.documents.mode(self.active()),
            auto_scroll: self.ui.documents.drifting(self.active()),
            aloud: (self.ui.documents.reading(self.active())).map(|_| self.ui.read_aloud.active()),
            ink: (self.ui.documents.reflowable(self.active()))
                .then_some(self.settings().text.ink_contrast),
            text: (self.ui.documents.reflowable(self.active()))
                .then(|| mareader_engine::appearance_menu::QuickText::of(&self.settings().text)),
            dictionary: {
                let w = &self.settings().dictionary.window;
                w.open && !w.minimized
            },
        }
    }

    /// The rail hosts the lights while drawn; then the bar, unless overlay.
    pub(super) fn place_lights(&self, ctx: &Context, overlay: bool, route: Route) -> NativeLights {
        let rail = self.ui.sidebar.offset().filter(|_| route.has_rail());
        let bar_hosts = rail.is_none() && (!overlay || !route.has_rail());
        // Handing over from a rail, they fade in rather than pop.
        let secs = motion::secs(ctx, titlebar::FADE);
        let handoff = ctx.animate_bool_with_time(egui::Id::new("lights-bar"), bar_hosts, secs);
        match rail {
            Some(shift) => NativeLights {
                shift,
                opacity: 1.0,
            },
            None if bar_hosts => NativeLights {
                shift: 0.0,
                opacity: self.ui.titlebar.opacity().min(handoff),
            },
            None => NativeLights::default(),
        }
    }
}

/// `raw` paper themed as the pages are, so the backdrop matches them.
fn baked(t: &theme::Tokens, raw: Rgb) -> egui::Color32 {
    theme::color(Rgb::from_unit(t.page.apply(raw.to_unit())))
}

/// The lights' layer: backings, then the stand-ins drawn off a Mac.
fn lights_layer(ctx: &Context) -> egui::Painter {
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("lights"),
    ))
}

/// A dark disc just inside each glass light on light themes, edging its
/// sheen.
pub(super) fn back_lights(
    ctx: &Context,
    lights: NativeLights,
    spots: LightSpots,
    t: &theme::Tokens,
) {
    if t.dark || !spots.glass || lights.opacity <= 0.0 {
        return;
    }
    let painter = lights_layer(ctx);
    let top = ctx.content_rect().left_top().to_vec2();
    let fill = t.ink.gamma_multiply(0.9 * lights.opacity);
    let radius = (spots.size / 2.0 - 0.5).max(0.0);
    for at in spots.centres(lights.shift) {
        painter.circle_filled(at + top, radius, fill);
    }
}

/// Three dots where AppKit would put its buttons, for checking off a Mac.
pub(super) fn stand_in_lights(ctx: &Context, lights: NativeLights) {
    if lights.opacity <= 0.0 {
        return;
    }
    let painter = lights_layer(ctx);
    let top = ctx.content_rect().left_top().to_vec2();
    let colours = [(255, 95, 87), (254, 188, 46), (40, 200, 64)];
    let spots = LightSpots::STAND_IN;
    for ((r, g, b), at) in colours.into_iter().zip(spots.centres(lights.shift)) {
        let fill = egui::Color32::from_rgb(r, g, b).gamma_multiply(lights.opacity);
        painter.circle_filled(at + top, spots.size / 2.0, fill);
    }
}

/// Which families split panes own under `settings`.
fn routing(settings: &Settings, split: bool) -> Routing {
    let w = &settings.workspace;
    Routing {
        themes: w.independent_themes,
        textures: w.independent_textures,
        shared_base: w.shared_base_mode,
        split,
    }
}

/// SplitMix64: a well-spread next state from any state.
fn splitmix(state: u64) -> u64 {
    let mut z = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The looks' own keys: what reads this window's own state.
///
/// `None` for a key that is not one of them, so the next family is asked.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "save_preset" => Some(app.option_save_preset(key, value)),
        "chromeless" => Some(app.option_chromeless(key, value)),
        "menu" => Some(app.option_menu(key, value)),
        "label" => Some(app.option_label(key, value)),
        "reduced" => Some(app.option_reduced(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    fn option_save_preset(&mut self, _key: &str, value: &str) -> Result<(), String> {
        // `Name` or `Name/Section`, saving the look set so far.
        let (name, group) = value.split_once('/').unwrap_or((value, ""));
        let save = Command::SavePreset {
            name: name.to_owned(),
            group: group.to_owned(),
            look: self.settings().appearance,
        };
        self.queue(Sent::Engine(save));

        Ok(())
    }

    fn option_chromeless(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.chromeless = value == "true";
        Ok(())
    }

    fn option_menu(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.menu_shot = value == "true";
        Ok(())
    }

    fn option_label(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let mut l = self.settings().layout.clone();
        l.floating_label = value != "false";
        l.floating_label_persist = value == "persist";
        if value == "chapter" {
            l.floating_label_style = mareader_settings::FloatingLabelStyle::Chapter;
        };
        self.set_field("layout", l)?;

        Ok(())
    }

    fn option_reduced(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui
            .reduced_motion
            .store(value == "true", Ordering::Relaxed);
        Ok(())
    }
}
