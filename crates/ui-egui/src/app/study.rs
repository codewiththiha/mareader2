use super::*;

impl MareaderApp {
    /// What a word list of `pane`'s `pages` is made from.
    pub(super) fn word_ask(
        &self,
        pane: PaneId,
        pages: Vec<usize>,
        list: mareader_settings::WordList,
    ) -> Option<crate::word_list::Ask> {
        let reading = self.ui.documents.reading(pane)?;
        Some(crate::word_list::Ask {
            document: reading.document,
            title: self.ui.documents.title(pane).unwrap_or_default().to_owned(),
            pages,
            level: self.settings().study.cefr.level.band(),
            list,
        })
    }

    /// Moves a word list on; made, it is copied or saved.
    pub(super) fn poll_word_list(&mut self, ctx: &egui::Context) {
        if !self.ui.word_lists.busy() {
            return;
        }
        // Answers land by waker, but a list's own steps need frames.
        ctx.request_repaint();
        let toast = match self
            .ui
            .word_lists
            .poll(&mut self.ui.vocab, &mut self.ui.dict)
        {
            None => return,
            Some(Err(toast)) => toast,
            Some(Ok((made, list))) => {
                let title = self
                    .ui
                    .documents
                    .title(self.active())
                    .unwrap_or_default()
                    .to_owned();
                match crate::word_list::deliver(ctx, &self.ui.files, &title, &made, list.output) {
                    Some(toast) => toast,
                    None => return,
                }
            }
        };
        self.ui.toasts.push(toast);
    }

    /// Who says English words on cards, which picks the accent they show.
    pub(super) fn english_voice(&self) -> Option<crate::sounds::EnglishVoice> {
        let others = self.settings().dictionary.other_voices;
        let picked = || (self.ui.read_aloud).english_voice(&self.settings().read_aloud, others);
        self.ui.english.clone().or_else(picked)
    }

    /// How the open cards' words sound, as the settings write them.
    pub(super) fn card_sounds(&mut self, now: f64) -> Vec<(String, Vec<crate::sounds::Line>)> {
        let words = [self.ui.study.card_word(), self.ui.dict.card_word()];
        let mut words: Vec<String> = words.into_iter().flatten().map(str::to_owned).collect();
        words.extend(self.ui.dict.sound_words());
        let english = self.english_voice();
        let voice = crate::sounds::spoken(english.as_ref().map(|v| v.tag.as_str()));
        let pronunciation = self.settings().study.pronunciation;
        let shown = (&pronunciation, voice);
        let sounds = &mut self.ui.sounds;
        let lines = |w: String| sounds.lines(&w, shown, now).map(|l| (w, l));
        words.into_iter().filter_map(lines).collect()
    }

    /// Asks the tagger for the open card's word's role.
    pub(super) fn ask_role(&mut self) {
        if let Some((word, context)) = self.ui.study.card_words() {
            self.ui.vocab.ask_role(word, context);
        }
    }

    /// A click on a shaded word: explained like a selected one.
    pub(super) fn explain_shaded(
        &mut self,
        ctx: &Context,
        pane: PaneId,
        page: usize,
        chars: std::ops::Range<usize>,
        rect: Rect,
    ) {
        let Some(asked) = self.ui.documents.asked_at(pane, page, chars) else {
            return;
        };
        self.ui.documents.clear_selection(pane);
        self.ui.dict.escape();
        let book = self.ui.library.book_in(&self.ui.documents, pane);
        let name = self.ui.documents.title(pane).unwrap_or_default().to_owned();
        let at = Explained {
            pane,
            book,
            name: &name,
            asked,
            rect,
        };
        let model = self.ui.library.model_mut();
        self.ui
            .study
            .explain(ctx, model, at, mareader_engine::library::wall_ms());
        self.ask_role();
    }

    /// Resting on a shaded word looks it up, when a pack can answer.
    pub(super) fn rest(&mut self, pane: PaneId, (word, context): (String, String), at: Rect) {
        if !self.settings().dictionary.hover
            || self.ui.dict.ready().is_empty()
            || self.ui.study.card_open()
        {
            return;
        }
        let offset = self.ui.documents.offset(pane);
        let to = self.settings().dictionary.default_lang.clone();
        if self.ui.dict.rest(pane, word.clone(), at, offset, to) {
            self.ui.vocab.ask_role(&word, &context);
        }
    }

    /// Look up: the pane's selection, its card beside it.
    pub(super) fn look_up(&mut self, pane: PaneId) {
        let Some((text, rect)) = self.ui.documents.looked_up(pane) else {
            return;
        };
        let max = mareader_study::MAX_GLOSS_CHARS;
        let Some(word) = crate::dict::lookup_text(&text, max) else {
            let rule = format!("Look up takes a word or short phrase, up to {max} characters.");
            self.ui.toasts.push(Toast::info(rule));
            return;
        };
        // An open window answers instead; selection stays.
        if self.settings().dictionary.window.open && self.route() == Route::Reader {
            let _ = self.set_field("dictionary.window.minimized", false);
            self.ui.dict.escape();
            self.ui.dict.feed(&word);
            return;
        }
        // Its sentence, so the card can name the word's class there.
        let context = self.ui.documents.asked(pane).map(|(a, _)| a.context);
        self.ui.documents.clear_selection(pane);
        // One card at a time.
        self.ui.study.escape(self.ui.library.model_mut());
        let offset = self.ui.documents.offset(pane);
        let to = self.settings().dictionary.default_lang.clone();
        self.ui.dict.look_up(pane, word.clone(), rect, offset, to);
        self.ui
            .vocab
            .ask_role(&word, context.as_deref().unwrap_or(&word));
    }

    /// A highlighter: the pane's selection, marked in `pen`.
    pub(super) fn highlight(
        &mut self,
        pane: PaneId,
        pen: mareader_library::Pen,
        color: Option<Rgb>,
    ) {
        let Some((spot, text)) = self.ui.documents.highlighted(pane) else {
            return;
        };
        self.ui.documents.clear_selection(pane);
        let book = self.ui.library.book_in(&self.ui.documents, pane);
        let name = self.ui.documents.title(pane).unwrap_or_default().to_owned();
        let at = Highlighted {
            book,
            name: &name,
            text,
            spot,
            pen,
            color,
        };
        let model = self.ui.library.model_mut();
        self.ui
            .study
            .highlight(model, at, mareader_engine::library::wall_ms());
    }

    /// Note: highlights the selection in the bar's first pen, then
    /// opens a draft on it.
    pub(super) fn note(&mut self, pane: PaneId) {
        let Some((spot, text)) = self.ui.documents.highlighted(pane) else {
            return;
        };
        let rect = self.ui.documents.looked_up(pane).map(|(_, r)| r);
        self.ui.documents.clear_selection(pane);
        let look = &self.settings().study.highlights;
        let pen = look.bar.iter().find_map(|i| match i {
            mareader_settings::BarItem::Highlight(pen) => Some(*pen),
            _ => None,
        });
        let book = self.ui.library.book_in(&self.ui.documents, pane);
        let name = self.ui.documents.title(pane).unwrap_or_default().to_owned();
        let at = Highlighted {
            book,
            name: &name,
            text,
            spot,
            pen: pen.unwrap_or_default(),
            color: None,
        };
        let model = self.ui.library.model_mut();
        let rect = rect.unwrap_or(Rect::NOTHING);
        self.ui
            .study
            .write_note(model, pane, at, rect, mareader_engine::library::wall_ms());
    }

    /// Bookmarks the pane's page, or takes back the bookmark on it.
    pub(super) fn bookmark(&mut self, pane: PaneId) {
        let Some(reading) = self.ui.documents.reading(pane) else {
            return;
        };
        let page = reading.current;
        let doc = &reading.document;
        let kept = bookmarks::on_pages(&self.ui.study, &self.ui.library, &self.ui.documents, pane);
        // A result's later pages share its first page's bookmark.
        let here = crate::notes_panel::page_of(doc, bookmarks::spot(doc, page)).unwrap_or(page);
        let existing = kept.iter().find(|(_, p)| *p == here).map(|(id, _)| *id);
        let name = self.ui.documents.title(pane).unwrap_or_default().to_owned();
        let at = ui_study::Bookmarking {
            book: self.ui.library.book_in(&self.ui.documents, pane),
            name: &name,
            spot: bookmarks::spot(doc, page),
            label: bookmarks::label(doc, page),
            existing,
        };
        let mut out = Commands::new();
        let model = self.ui.library.model_mut();
        self.ui
            .study
            .toggle_bookmark(model, at, mareader_engine::library::wall_ms(), &mut out);
        self.ui.queued.extend(out);
    }

    /// Explain: the pane's selected word, marked, its card open.
    pub(super) fn explain(&mut self, ctx: &Context, pane: PaneId) {
        if self.ui.documents.selection(pane).is_none() {
            return;
        }
        let Some((asked, rect)) = self.ui.documents.asked(pane) else {
            let rule = format!(
                "The AI Dictionary takes one word, up to {} characters.",
                mareader_study::MAX_GLOSS_CHARS
            );
            self.ui.toasts.push(Toast::info(rule));
            return;
        };
        self.ui.documents.clear_selection(pane);
        self.ui.dict.escape();
        let book = self.ui.library.book_in(&self.ui.documents, pane);
        let name = self.ui.documents.title(pane).unwrap_or_default().to_owned();
        let at = Explained {
            pane,
            book,
            name: &name,
            asked,
            rect,
        };
        let model = self.ui.library.model_mut();
        self.ui
            .study
            .explain(ctx, model, at, mareader_engine::library::wall_ms());
        self.ask_role();
    }
}
