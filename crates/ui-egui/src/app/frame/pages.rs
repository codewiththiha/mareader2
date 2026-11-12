use super::*;

struct PageContext {
    view: PaneView,
    tokens: theme::Tokens,
    every: bool,
    source: Option<PaneId>,
    corner: f32,
    blend: bool,
    pinned_bottom: f32,
    lit: Option<(PaneId, Lit)>,
    shading: Option<crate::vocab::Look>,
}

impl MareaderApp {
    pub(super) fn rail(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        route: Route,
        commands: &mut Commands,
    ) -> (f32, bool) {
        let overlay = self.settings().chrome.sidebar_overlay;
        let rail = RailContext {
            overlay,
            slide: self.settings().motion.sidebar_slide,
            platform: self.ui.platform,
            bar_bottom: self.ui.titlebar.band_bottom(ctx),
        };
        let tabs = self.tabs();
        if let Some(made) = self.ui.library.take_made() {
            self.ui.shelf_tree.made(self.ui.library.model(), made);
        }
        let organizer = self.ui.library.organizer();
        let paused = organizer
            .paused()
            .map(ui_library::organizing::Paused::short);
        let shelves = PanelContext {
            model: self.ui.library.model(),
            tabs: &tabs,
            active: self.active(),
            click: self.settings().workspace.library_click,
            home: self.settings().library.home,
            held: self.ui.library.held(),
            organizer: ui_library::Organizer {
                busy: self.ui.library.busy(),
                paused: paused.as_deref(),
            },
        };
        // Gathered only while the panel shows.
        let notes = match self.ui.sidebar.is_open() && self.ui.sidebar.panel() == Panel::Notes {
            true => {
                let book = self.ui.library.book_in(&self.ui.documents, self.active());
                let name = self.ui.documents.title(self.active()).unwrap_or_default();
                let look = self.settings().study.highlights.clone();
                self.ui
                    .study
                    .notes_of(self.ui.library.model(), book, name, &look)
            }
            false => Vec::new(),
        };
        let offer = crate::thumbnails::Offer::of(self.settings());
        self.ui.thumbs.set_offer(offer);
        let active = self.active();
        let shelf_tree = &mut self.ui.shelf_tree;
        let documents = &mut self.ui.documents;
        let (outline, thumbs) = (&mut self.ui.outline, &mut self.ui.thumbs);
        // The rail's panels serve the focused pane's document.
        let mut body =
            |ui: &mut egui::Ui, panel: Panel, recentre: bool, out: &mut Commands| match panel {
                Panel::Outline => documents.reading(active).is_some_and(|r| {
                    outline_panel::show(ui, &r, outline, recentre, out);
                    true
                }),
                Panel::Thumbnails => documents.thumbnails(ui, active, thumbs, recentre, out),
                Panel::Library => library_panel::show(ui, &shelves, shelf_tree, out),
                Panel::Notes => documents.reading(active).is_some_and(|r| {
                    notes_panel::show(ui, &r, &notes, out);
                    true
                }),
            };
        // The rail serves reading; the library keeps it, unseen, for return.
        let inset = match (route.has_rail(), overlay) {
            (false, _) => 0.0,
            (true, true) => self
                .ui
                .sidebar
                .show_floating(ctx, &rail, &mut body, commands),
            (true, false) => self.ui.sidebar.show_docked(ui, &rail, &mut body, commands),
        };
        (inset, overlay)
    }

    pub(super) fn pages(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        route: Route,
        drop_target: Option<PaneId>,
        commands: &mut Commands,
    ) -> PaneView {
        let context = self.page_context(ctx, drop_target);
        self.shots(ctx);
        self.central(ui, ctx, route, &context, commands);
        context.view
    }

    fn page_context(&mut self, ctx: &Context, drop_target: Option<PaneId>) -> PageContext {
        let tokens = theme::Tokens::get(ctx);
        let view = PaneView {
            active: self.active(),
            look: self.settings().workspace.look,
            chrome_bottom: self.ui.titlebar.band_bottom(ctx),
            drop_target,
            styles: self.pane_styles(&tokens),
            recent: self.recent_books(),
        };
        // Each pane reads its own paper while it has a colour of its own.
        let every = self.routing().colour();
        let source = self.paper_source();
        let corner = view.corner(self.tree().len() > 1, &tokens);
        let blend = self.settings().paper.blend;
        let pinned_bottom = if self.settings().chrome.pinned(self.route().bar()) {
            view.chrome_bottom
        } else {
            f32::NEG_INFINITY
        };
        let lit = found_in(&self.ui.search, &self.ui.documents, &self.ui.find).map(|f| Lit {
            hits: f.hits.clone(),
            active: self.ui.find.active(),
        });
        let lit = self.ui.find.pane().zip(lit);
        let cefr = &self.settings().study.cefr;
        let shading = cefr.enabled.then_some(crate::vocab::Look {
            level: cefr.level.band(),
            mark: cefr.mark,
            click: cefr.click_explain,
            hover: self.settings().dictionary.hover,
            ink: self.settings().study.highlights.cefr,
            opacity: self.settings().study.highlights.opacity,
        });
        PageContext {
            view,
            tokens,
            every,
            source,
            corner,
            blend,
            pinned_bottom,
            lit,
            shading,
        }
    }

    fn shots(&mut self, ctx: &Context) {
        if self.ui.read_aloud_shot
            && let Some(reading) = self.ui.documents.reading(self.active())
        {
            self.ui.read_aloud_shot = false;
            let settings = self.settings().read_aloud.clone();
            let from = ReadFrom::default();
            (self.ui.read_aloud).start(ctx, &reading, from, &settings);
        }
        let shot = std::mem::take(&mut self.ui.transcript_shot);
        for (card, track) in shot {
            if !self
                .ui
                .documents
                .open_transcript(self.active(), card, track)
            {
                self.ui.transcript_shot.push((card, track));
            }
        }
        let shot = std::mem::take(&mut self.ui.trigger_shot);
        for (target, visible) in shot {
            if !self
                .ui
                .documents
                .set_trigger(self.active(), target, visible)
            {
                self.ui.trigger_shot.push((target, visible));
            }
        }
        // A screenshot's clicks wait for the scripts' first pages.
        if let Some(first) = self.ui.script_shot.first()
            && self.ui.documents.pick_script(self.active(), first)
        {
            for selector in self.ui.script_shot.split_off(1) {
                self.ui.documents.pick_script(self.active(), &selector);
            }
            self.ui.script_shot.clear();
        }
        for toast in self.ui.scripts.tick(ctx, &mut self.ui.documents) {
            self.ui.toasts.push(toast);
        }
        for (doc, clip) in self.ui.documents.take_clips() {
            self.ui.clips.act(&doc, clip, &mut self.ui.read_aloud);
        }
        self.ui.clips.tick();
        self.find_reading();
        let (library, documents) = (&self.ui.library, &self.ui.documents);
        self.ui
            .read_aloud
            .remember(|pane| crate::read_aloud::Origin {
                book: library.book_in(documents, pane),
                path: documents.path(pane).map(std::path::Path::to_path_buf),
                name: documents.title(pane).unwrap_or_default().to_owned(),
            });
        let aloud = self.settings().read_aloud.clone();
        self.ui.read_aloud.tick(ctx, &mut self.ui.documents, &aloud);
        self.ui.documents.reveal_read_markers();
    }

    fn central(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        route: Route,
        context: &PageContext,
        commands: &mut Commands,
    ) {
        let view = &context.view;
        let tokens = context.tokens;
        let every = context.every;
        let source = context.source;
        let corner = context.corner;
        let blend = context.blend;
        let pinned_bottom = context.pinned_bottom;
        let lit = &context.lit;
        // Every setting is read before any field of `self` is borrowed below.
        let (layout, window, library_look) = (
            self.settings().layout.clone(),
            self.settings().text.clone(),
            self.settings().library,
        );
        let highlight_look = self.settings().study.highlights.clone();
        let dictionary_look = self.settings().dictionary.clone();
        let lift = self.tree().len() > 1;
        let tree = self.tree().clone();
        let lifted = self.ui.lifting.map(|l| l.pane);
        let pinned = self.settings().chrome.pinned(route.bar());
        let page_top = self.ui.titlebar.page_top(ctx, route.bar(), pinned);
        let menu_shot = self.ui.menu_shot;
        let highlights = &highlight_look;
        let dictionary = &dictionary_look;
        let read_aloud = &self.ui.read_aloud;
        let documents = &mut self.ui.documents;
        let (library, library_view) = (&mut self.ui.library, &library_look);
        let study = &mut self.ui.study;
        let page_ai = &mut self.ui.page_ai;
        study.begin_frame();
        let vocab = &mut self.ui.vocab;
        let shading = context.shading;
        let (layout, window) = (&layout, &window);
        let texts = &self.ui.texts;
        self.ui.library_rect = None;
        // The dictionary page needs all of `self`: drawn outside the closure.
        if route == Route::Dictionary {
            self.ui.pane_rects.clear();
            self.ui.dict.page(ui, dictionary, page_top, commands);
            return;
        }
        // Copied out above: the closures below need `&mut self`.
        let mut library_rect = None;
        let rects = egui::CentralPanel::no_frame().show(ui, |ui| {
            if route == Route::Library {
                library_rect = Some(ui.max_rect());
                ui_library::view::show(ui, library, library_view, page_top, commands);
                return Vec::new();
            }
            ui_panes::show(ui, &tree, lifted, view, commands, |ui, id| {
                let style = view.style(id);
                let cx = ViewContext {
                    active: id == view.active,
                    lift,
                    tokens: style.map_or(tokens, |s| s.tokens),
                    top_inset: (pinned_bottom - ui.max_rect().top()).max(0.0),
                    page: PageLook::new(layout, texts.of(id, window)),
                    paper: style.and_then(|s| s.paper),
                    corner,
                    sample: blend && (every || Some(id) == source),
                    marks: pane_marks(study, library, documents, id, highlights),
                    bookmarks: pane_bookmarks(study, library, documents, id),
                    highlights: highlights.clone(),
                    card: study.card_mark(id),
                    picked: study.picked(id),
                    shading,
                    menu_shot: menu_shot && id == view.active,
                    found: lit
                        .as_ref()
                        .filter(|(p, _)| *p == id)
                        .map(|(_, l)| l.clone()),
                    aloud: read_aloud.lit(id),
                    ai: pane_ai(page_ai, ctx, id, library, documents),
                };
                documents.show(ui, id, &cx, Some(&mut *vocab))
            })
        });
        self.ui.pane_rects = rects.inner;
        self.ui.library_rect = library_rect;
    }
}

fn pane_marks(
    study: &Study,
    library: &LibraryState,
    documents: &Documents,
    pane: PaneId,
    look: &mareader_settings::HighlightSettings,
) -> Arc<[crate::study::marks::Shown]> {
    let book = library.book_in(documents, pane);
    let name = documents.title(pane).unwrap_or_default();
    study.marks_of(library.model(), book, name, look)
}

fn pane_bookmarks(
    study: &Study,
    library: &LibraryState,
    documents: &Documents,
    pane: PaneId,
) -> Vec<usize> {
    bookmarks::on_pages(study, library, documents, pane)
        .into_iter()
        .map(|(_, page)| page)
        .collect()
}

fn pane_ai(
    page_ai: &mut crate::page_ai::PageAi,
    ctx: &Context,
    pane: PaneId,
    library: &LibraryState,
    documents: &Documents,
) -> crate::page_ai::PaneAi {
    let book = library.book_in(documents, pane);
    let name = documents.title(pane).unwrap_or_default();
    page_ai.overlays(ctx, pane, library.model(), documents, (book, name))
}
