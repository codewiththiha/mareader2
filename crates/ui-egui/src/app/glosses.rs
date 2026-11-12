use super::*;
use crate::ui_command::{Intent, UiCommand};

impl MareaderApp {
    pub(super) fn glosses(&mut self, ctx: &Context, out: &mut Commands) {
        self.staged(ctx, out);
        self.seeds(ctx, out);
        self.gloss_events(ctx, out);
        self.poll_services(ctx);
        let sounds = self.show_study(ctx, out);
        self.show_page_ai(ctx, out, &sounds);
    }

    fn staged(&mut self, ctx: &Context, out: &mut Commands) {
        let pane = self.active();
        let sheets = self.ui.documents.sheets(pane);
        let shown = |page| sheets.iter().any(|s| s.page == page);
        match self.ui.staged.pop_front() {
            // Its page is turned to first, then selected once laid out.
            Some(mut staged) if !sheets.is_empty() && !shown(staged.page) => {
                if !staged.turned {
                    out.push(Command::Intent(Intent::GoTo {
                        pane,
                        page: staged.page,
                    }));
                }
                staged.turned = true;
                self.ui.staged.push_front(staged);
                ctx.request_repaint();
            }
            Some(Staged {
                page,
                text,
                explain,
                look_up,
                pen,
                note,
                bookmark,
                pick,
                ..
            }) if !sheets.is_empty() => {
                let first = self.marks(pane).first().map(|m| m.id);
                match (bookmark, first) {
                    (true, _) => out.push(Command::Intent(Intent::Bookmark(Some(pane)))),
                    // Picking starts, then a later `pick` takes all in.
                    (false, Some(id)) if text.is_empty() && !self.ui.study.picking() => {
                        let start = crate::study::Action::SelectSeveral(pane, id);
                        out.push(Command::Intent(Intent::Study(start)));
                    }
                    (false, Some(_)) if text.is_empty() => {
                        out.push(Command::Intent(Intent::Study(
                            crate::study::Action::PickAll,
                        )));
                        if pick {
                            egui::Popup::open_id(ctx, egui::Id::new(crate::study::PICK_STYLE));
                        }
                    }
                    _ => out.push(Command::Intent(Intent::SelectText { pane, page, text })),
                }
                if explain {
                    out.push(Command::Intent(Intent::Explain(pane)));
                }
                if look_up {
                    out.push(Command::Intent(Intent::LookUp(pane)));
                }
                if let Some(pen) = pen {
                    let color = None;
                    out.push(Command::Intent(Intent::Highlight { pane, pen, color }));
                }
                if let Some((text, save)) = note {
                    out.push(Command::Intent(Intent::Note(pane)));
                    out.push(Command::Intent(Intent::Study(
                        crate::study::Action::TypeNote(text, save),
                    )));
                }
                // The next waits a frame, as each selection replaces the last.
                if !self.ui.staged.is_empty() {
                    ctx.request_repaint();
                }
            }
            Some(staged) => self.ui.staged.push_front(staged),
            None => {}
        }
    }

    fn seeds(&mut self, ctx: &Context, out: &mut Commands) {
        let pane = self.active();
        let sheets = self.ui.documents.sheets(pane);
        let shown = |page| sheets.iter().any(|s| s.page == page);
        if let Some(pages) = self
            .ui
            .word_window_seed
            .take_if(|p| p.first().is_some_and(|&p| shown(p)))
        {
            out.push(Command::Intent(Intent::WordListWindow { pane, pages }));
        }
        if let Some((page, task, follow, last)) = self.ui.page_ai_seed.clone()
            && shown(page)
        {
            let book = self.ui.library.book_in(&self.ui.documents, pane);
            let name = self.ui.documents.title(pane).unwrap_or_default().to_owned();
            let ai_look = self.settings().ai.clone();
            let asked = crate::page_ai::Asked {
                pane,
                page,
                book,
                name: &name,
                documents: &self.ui.documents,
                ai: &self.ui.ai,
                settings: &ai_look,
            };
            let now = ctx.input(|i| i.time);
            let done = match follow {
                false if last > page => {
                    let pages: Vec<usize> = (page..=last).collect();
                    self.ui.page_ai.run_pages(asked, &pages, task);
                    true
                }
                true if self.ui.page_ai.can_read(&asked) => {
                    let model = self.ui.library.model();
                    self.ui.page_ai.follow(asked, model, task, true, now, out);
                    true
                }
                true => false,
                false => self
                    .ui
                    .page_ai
                    .seed(asked, self.ui.library.model_mut(), task, now),
            };
            if done {
                self.ui.page_ai_seed = None;
            }
            ctx.request_repaint();
        }
    }

    fn gloss_events(&mut self, ctx: &Context, out: &mut Commands) {
        for (pane, gloss) in self.ui.documents.take_gloss() {
            if study_gloss(&gloss) {
                self.study_gloss(ctx, out, pane, gloss);
            } else if page_gloss(&gloss) {
                self.page_gloss(ctx, out, pane, gloss);
            } else {
                self.pill_gloss(ctx, out, pane, gloss);
            }
        }
    }

    fn study_gloss(&mut self, ctx: &Context, out: &mut Commands, pane: PaneId, gloss: Gloss) {
        match gloss {
            Gloss::Explain => out.push(Command::Intent(Intent::Explain(pane))),
            Gloss::LookUp => out.push(Command::Intent(Intent::LookUp(pane))),
            Gloss::Note => out.push(Command::Intent(Intent::Note(pane))),
            Gloss::ReadAloud => {
                if let Some(sel) = self.ui.documents.selection(pane) {
                    let (a, b) = sel.ordered();
                    let spot = |c: mareader_session::Caret| mareader_speech::Spot {
                        page: c.page,
                        char: c.index,
                    };
                    let from = ReadFrom {
                        anchor: None,
                        at: Some(spot(a)),
                        until: Some(spot(b)),
                    };
                    self.ui.documents.clear_selection(pane);
                    out.push(Command::Intent(Intent::ReadAloud { pane, from }));
                }
            }
            Gloss::ReadFrom(from) => out.push(Command::Intent(Intent::ReadAloud { pane, from })),
            Gloss::Highlight(pen, color) => {
                out.push(Command::Intent(Intent::Highlight { pane, pen, color }))
            }
            Gloss::Copied => out.push(Command::Ui(UiCommand::Notify(Toast::info("Copied")))),
            Gloss::Open { mark, at } => {
                self.ui
                    .study
                    .open_mark(ctx, self.ui.library.model(), pane, mark, at);
                self.ask_role();
            }
            Gloss::Shaded { page, chars, at } => self.explain_shaded(ctx, pane, page, chars, at),
            Gloss::Rest { word, context, at } => self.rest(pane, (word, context), at),
            Gloss::Menu { mark, at, words } => {
                self.ui.menu_shot = false;
                let model = self.ui.library.model();
                self.ui.study.open_menu(model, pane, mark, at, words);
            }
            Gloss::Anchor(at) => self.ui.study.anchor(pane, at),
            Gloss::Pick(mark) => self.ui.study.pick(pane, mark),
            Gloss::Toggle(mark) => self.ui.study.toggle(pane, mark),
            Gloss::Seen { page, all } => self.ui.study.seen(pane, page, all),
            _ => {}
        }
    }

    fn page_gloss(&mut self, ctx: &Context, _out: &mut Commands, pane: PaneId, gloss: Gloss) {
        match gloss {
            Gloss::Lift(origin) if self.tree().len() > 1 => {
                let home = self.ui.pane_rects.iter().find(|(id, _)| *id == pane);
                if let Some(&(_, home)) = home {
                    let since = ctx.input(|i| i.time);
                    let lift = crate::lift::Lift {
                        pane,
                        origin,
                        home,
                        since,
                    };
                    self.ui.lifting = Some(lift);
                    self.focus(pane);
                    ctx.request_repaint();
                }
            }
            Gloss::Lift(_) => {}
            Gloss::PageMenu {
                page,
                at,
                place,
                caret,
            } => {
                let aloud = self.ui.documents.reading(pane).map(|_| ReadFrom {
                    anchor: place,
                    at: caret.map(|char| mareader_speech::Spot { page, char }),
                    until: None,
                });
                self.ui.page_ai.open_menu(pane, page, at, aloud);
                if let Some(pin) = self.ui.documents.menu_pin(pane, page, at, caret) {
                    self.ui.page_ai.pin_menu(pin);
                }
            }
            Gloss::Bubble { page, index, at } => {
                let now = ctx.input(|i| i.time);
                self.ui.page_ai.pick(pane, page, index, at, now);
            }
            Gloss::BubbleMenu { page, index, at } => {
                self.ui.page_ai.open_bubble_menu(pane, page, index, at);
            }
            _ => {}
        }
    }

    fn pill_gloss(&mut self, ctx: &Context, out: &mut Commands, pane: PaneId, gloss: Gloss) {
        match gloss {
            Gloss::Pill(crate::page_ai::PillPress::Stop) => self.ui.page_ai.stop_pill(pane),
            Gloss::Pill(crate::page_ai::PillPress::Continue) => {
                let Some(reading) = self.ui.documents.reading(pane) else {
                    return;
                };
                let book = self.ui.library.book_in(&self.ui.documents, pane);
                let name = self.ui.documents.title(pane).unwrap_or_default().to_owned();
                let ai_look = self.settings().ai.clone();
                let asked = crate::page_ai::Asked {
                    pane,
                    page: reading.current,
                    book,
                    name: &name,
                    documents: &self.ui.documents,
                    ai: &self.ui.ai,
                    settings: &ai_look,
                };
                let now = ctx.input(|i| i.time);
                let model = self.ui.library.model();
                self.ui.page_ai.resume(asked, model, now, out);
            }

            _ => {}
        }
    }

    fn poll_services(&mut self, ctx: &Context) {
        for (pane, _) in &self.ui.pane_rects {
            if self.ui.documents.sheets(*pane).is_empty() {
                self.ui.study.hold(*pane);
            }
        }
        let ai_look = self.settings().ai.clone();
        if let Some(explainer) = self.ui.ai.follow(&ai_look) {
            self.ui.study.set_explainer(explainer);
        }
        self.take_organized();
        self.ui.study.receive(self.ui.library.model_mut());
        let (changed, role) = self.ui.vocab.poll();
        if changed {
            // Answers came: the pages plan their shading again.
            ctx.request_repaint();
        }
        if self.ui.read_aloud.shelf.poll() {
            // A voice model came or went: the pronunciation pack follows.
            self.ui.sounds.voices_changed();
            ctx.request_repaint();
        }
        if self.ui.sounds.poll(ctx.input(|i| i.time)) {
            ctx.request_repaint();
        }
        let memory = self.settings().read_aloud.speech_memory_mb as usize;
        self.ui.read_aloud.shelf.set_speech_memory(memory << 20);
        (self.ui.read_aloud.shelf).set_model_idle(self.settings().read_aloud.model_idle());
        // Whichever card asked takes it; each checks the word is its own.
        if let Some((word, role)) = role {
            self.ui.dict.set_role(&word, role.clone());
            self.ui.study.set_role(&word, role);
        }
    }

    fn show_study(
        &mut self,
        ctx: &Context,
        out: &mut Commands,
    ) -> Vec<(String, Vec<crate::sounds::Line>)> {
        let sounds = self.card_sounds(ctx.input(|i| i.time));
        // Read before the mutable borrows below.
        let (gloss_look, highlight_look) = (
            self.settings().study.gloss.clone(),
            self.settings().study.highlights.clone(),
        );
        let dictionary_look = self.settings().dictionary.clone();
        let aloud_look = self.settings().read_aloud.clone();
        let looks = (&gloss_look, &highlight_look);
        let model = self.ui.library.model();
        let others = dictionary_look.other_voices;
        let dict = &dictionary_look;
        let looking = self.ui.dict.card_word().is_some() || dict.window.open;
        if others && (looking || self.route() == Route::Dictionary) {
            // Copied out first: `list_voices` borrows the reader mutably.
            self.ui.read_aloud.list_voices(ctx, &aloud_look);
        }
        let saying = self.ui.read_aloud.saying();
        let (speakers, offers) = self.ui.read_aloud.speakers(&aloud_look, others);
        self.ui.dict.set_voice(crate::dict::Voice {
            saying: saying.map(str::to_owned),
            sounds: sounds.clone(),
            speakers,
            offers,
        });
        let heard = crate::sounds::Heard {
            saying,
            sounds: &sounds,
        };
        let panes = (&self.ui.pane_rects[..], heard);
        self.ui.study.show(ctx, model, panes, looks, out);
        sounds
    }

    fn show_page_ai(
        &mut self,
        ctx: &Context,
        out: &mut Commands,
        sounds: &[(String, Vec<crate::sounds::Line>)],
    ) {
        let gpu = self.ui.documents.gpu();
        self.ui
            .page_ai
            .receive(ctx, self.ui.library.model_mut(), gpu, out);
        let ai_look = self.settings().ai.clone();
        let around = crate::page_ai::Around {
            main: self.ui.library.model(),
            documents: &self.ui.documents,
            ai: &self.ui.ai,
            settings: &ai_look,
        };
        self.ui.page_ai.tick(ctx, &around, out);
        // A book lays its pane's shown results and transcripts into its pages.
        let wanted = mareader_engine::transcript::wanted(&ai_look);
        for pane in self.tree().panes() {
            let book = self.ui.library.book_in(&self.ui.documents, pane);
            let name = self.ui.documents.title(pane).unwrap_or_default().to_owned();
            let model = self.ui.library.model();
            let who = (book, name.as_str());
            let ai = self
                .ui
                .page_ai
                .inserts(model, pane, &self.ui.documents, who);
            let own = self.ui.documents.transcripts(pane, &wanted);
            let shown = self.ui.documents.triggered(pane);
            let ai = mareader_engine::transcript::combine(ai, own);
            if let Some(inserts) = mareader_engine::transcript::combine(ai, shown) {
                self.ui.documents.set_inserts(pane, inserts);
            }
        }
        let model = self.ui.library.model();
        self.ui.page_ai.show(
            ctx,
            &ai_look,
            model,
            (&self.ui.pane_rects, &self.ui.documents),
            out,
        );
        if let Some(toast) = self.ui.page_copy.poll(ctx) {
            self.ui.toasts.push(toast);
        }
        self.ui
            .dict
            .set_exact_only(!self.settings().dictionary.near);
        self.ui.dict.poll();
        self.poll_word_list(ctx);
        let settings = self.settings().clone();
        let services = (&mut self.ui.vocab, &mut self.ui.dict, &self.ui.files);
        self.ui.word_window.show(ctx, &settings, services, out);
        let documents = &self.ui.documents;
        let offset = |pane| documents.offset(pane);
        let saying = self.ui.read_aloud.saying();
        let heard = crate::sounds::Heard { saying, sounds };
        self.ui
            .dict
            .show(ctx, (&self.ui.pane_rects, heard), offset, out);
    }
}

fn study_gloss(gloss: &Gloss) -> bool {
    matches!(
        gloss,
        Gloss::Explain
            | Gloss::LookUp
            | Gloss::Note
            | Gloss::ReadAloud
            | Gloss::ReadFrom(..)
            | Gloss::Highlight(..)
            | Gloss::Copied
            | Gloss::Open { .. }
            | Gloss::Shaded { .. }
            | Gloss::Rest { .. }
            | Gloss::Menu { .. }
            | Gloss::Anchor(..)
            | Gloss::Pick(..)
            | Gloss::Toggle(..)
            | Gloss::Seen { .. }
    )
}

fn page_gloss(gloss: &Gloss) -> bool {
    matches!(
        gloss,
        Gloss::Lift(..) | Gloss::PageMenu { .. } | Gloss::Bubble { .. } | Gloss::BubbleMenu { .. }
    )
}
