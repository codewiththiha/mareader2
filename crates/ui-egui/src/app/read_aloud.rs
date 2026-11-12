use super::*;
use mareader_engine::documents::OpenRequest;

impl MareaderApp {
    /// The voice booth opens with a voice tab and closes after it.
    pub(super) fn voices_shown(&mut self, ctx: &egui::Context) {
        use mareader_engine::settings::Tab;
        let s = self.settings().read_aloud.clone();
        let (booth, shelf) = (&mut self.ui.read_aloud.booth, &self.ui.read_aloud.shelf);
        let engine = match self.ui.settings_window.tab() {
            _ if !self.ui.settings_window.is_open() => None,
            Tab::ReadAloud => Some(s.engine.as_str()),
            Tab::Model(m) => Some(m.id),
            _ => None,
        };
        if let Some(engine) = engine {
            booth.open(ctx, engine, &s, shelf);
            booth.tick(ctx);
        } else if booth.is_open() {
            booth.close();
        }
    }

    /// The book read aloud, open again in some pane: it reads on there.
    pub(super) fn find_reading(&mut self) {
        let Some(origin) = self.ui.read_aloud.away() else {
            return;
        };
        let (library, documents) = (&self.ui.library, &self.ui.documents);
        let shows = |&p: &PaneId| origin.is(library.book_in(documents, p), documents.path(p));
        if let Some(pane) = self.tree().panes().into_iter().find(shows) {
            self.ui.read_aloud.rehome(&self.ui.documents, pane);
        }
    }

    /// Opens the book read aloud beside what shows, to read on there.
    pub(super) fn reopen_reading(&mut self, ctx: &Context, origin: crate::read_aloud::Origin) {
        let name = &origin.name;
        // Checked first, so a lost file opens no empty pane.
        let why = match &origin.path {
            Some(p) if !p.exists() => Some(format!("“{name}” was moved or deleted")),
            None => Some(format!("“{name}” didn't come from a file")),
            Some(_) => None,
        };
        if let Some(why) = why {
            let note = format!("{why}, so it can't open again. Reading goes on.");
            self.ui.toasts.push(Toast::info(note));
            return;
        }
        let shelved = origin
            .book
            .filter(|&b| self.ui.library.model().book(b).is_some());
        if let Some(book) = shelved {
            self.go(Route::Reader);
            return self.open_book(ctx, book, None, Placement::Beside);
        }
        let Some(path) = origin.path else {
            return;
        };
        let empty = self.ui.documents.title(self.active()).is_none();
        let placement = if empty {
            Placement::Into
        } else {
            Placement::Beside
        };
        let request = OpenRequest {
            pane: Some(self.active()),
            placement,
            ..OpenRequest::path(path)
        };
        self.apply(ctx, Sent::Engine(Command::Open(request)));
    }

    /// The read-aloud player, on any page: a card when the words are away.
    pub(super) fn player(&mut self, ctx: &Context, route: Route, out: &mut Commands) {
        let reader = route == Route::Reader;
        let spot = self.ui.read_aloud.spot();
        let shown = spot
            .clone()
            .filter(|_| reader)
            .and_then(|(pane, page, chars)| {
                let (_, rect) = self.ui.pane_rects.iter().find(|(p, _)| *p == pane)?;
                let room = rect.with_max_y(rect.bottom() - crate::page_bar::BAR_H);
                let on = self.ui.documents.visible(pane)?.contains(&page);
                match chars.and_then(|c| self.ui.documents.hit_box(pane, page, c)) {
                    Some(hit) => Some(room.intersects(hit)),
                    None => Some(on),
                }
            });
        let origin = self.ui.read_aloud.origin();
        let title = origin.map_or("Read aloud", |o| o.name.as_str()).to_owned();
        let book = origin.and_then(|o| o.book);
        let mut content = ctx.content_rect();
        content.min.y += crate::theme::TITLE_BAR_H;
        // Panes with looks of their own: it wears its book's, unlike the rest.
        let home = self.ui.read_aloud.home();
        let shows = |pane: &PaneId| self.ui.pane_rects.iter().any(|(p, _)| p == pane);
        let tint = (home.filter(shows))
            .filter(|_| reader && self.routing().any())
            .map(|pane| theme::Tokens::new(&self.look(pane)));
        let lift = match route {
            Route::Reader => self.toast_lift(ctx, ctx.content_rect()),
            Route::Library | Route::Dictionary => 0.0,
        };
        let dict = (reader).then(|| self.ui.dict.window_cover()).flatten();
        let toasts = self.ui.toasts.active_bed_for(
            ctx,
            ctx.content_rect(),
            lift,
            self.ui.read_aloud.island_anchor(),
            dict,
        );
        let parking = self.settings().workspace.edge_parking;
        let stage = crate::read_aloud::Stage {
            reader,
            panes: &self.ui.pane_rects,
            content,
            shown,
            title: &title,
            book,
            tint,
            toasts,
            dict,
            parking,
        };
        let mut settings = self.settings().read_aloud.clone();
        let library = &mut self.ui.library;
        let mut cover = |ui: &egui::Ui, rect: egui::Rect| {
            book.is_some_and(|b| library.paint_cover(ui, rect, b))
        };
        (self.ui.read_aloud).bar(ctx, stage, &mut settings, out, &mut cover);
        let _ = self.set_field("read_aloud", settings);
    }

    /// One voice at a time: the book pauses; clips, samples and words stop.
    pub(super) fn hush(&mut self) {
        self.ui.clips.stop();
        self.ui.read_aloud.hush();
    }
}
