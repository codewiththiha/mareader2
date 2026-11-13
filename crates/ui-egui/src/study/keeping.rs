//! What a reader keeps on a book: its notes, bookmarks and highlights.

use super::{Bookmarking, Bookmarks, Highlighted, Note, Study, ink};
use crate::ui_command::{Commands, Sent, UiCommand};
use egui::Rect;
use mareader_appearance::Rgb;
use mareader_engine::command::Command;
use mareader_engine::study::Action;
use mareader_engine::study::loose::loose_content;
use mareader_engine::toast::Toast;
use mareader_library::{BookId, Library, MarkId, MarkKind, NewMark};
use mareader_settings::HighlightSettings;
use mareader_workspace::PaneId;
use std::sync::Arc;

impl Study {
    /// `book`'s bookmarks, each with where it is.
    pub fn bookmarks_of(&self, main: &Library, book: Option<BookId>, name: &str) -> Bookmarks {
        let (lib, id) = self.lib_of(main, book, name);
        let stamp = lib.marks_stamp();
        let listed = self.listed.borrow();
        // Drawing a pane lists its marks first; the bookmarks come along.
        if let Some(l) = listed.iter().find(|l| l.stamp == stamp && l.book == id) {
            return Arc::clone(&l.bookmarks);
        }
        let marks = lib.marks(id).into_iter();
        let kept = marks.filter(|m| m.kind == MarkKind::Bookmark);
        kept.map(|m| (m.id, m.spot)).collect()
    }

    /// Bookmarks the page, or takes back its bookmark with Undo offered.
    pub fn toggle_bookmark(
        &mut self,
        main: &mut Library,
        at: Bookmarking<'_>,
        wall_ms: u64,
        out: &mut Commands,
    ) {
        if let Some(id) = at.existing {
            self.act_remove(main, vec![id], wall_ms, out);
            return;
        }
        let (lib, book) = self.shelf(main, at.book, at.name);
        lib.add_mark(book, NewMark::bookmark(at.label, at.spot), wall_ms);
    }

    /// The book's highlights and bookmarks for the panel, in reading order.
    pub fn notes_of(
        &self,
        main: &Library,
        book: Option<BookId>,
        name: &str,
        look: &HighlightSettings,
    ) -> Vec<crate::notes_panel::Row> {
        let (lib, id) = match book.filter(|b| main.book(*b).is_some()) {
            Some(id) => (main, id),
            None => (&self.loose, BookId::original(loose_content(name))),
        };
        let mut out: Vec<crate::notes_panel::Row> = lib
            .marks(id)
            .into_iter()
            .filter(|m| m.kind != MarkKind::Explain)
            .map(|m| crate::notes_panel::Row {
                id: m.id,
                spot: m.spot,
                quote: m.word.clone(),
                note: m.note.value.clone(),
                ink: ink(m, look).color,
                pen: m.look().pen,
                added: m.added,
                bookmark: m.kind == MarkKind::Bookmark,
            })
            .collect();
        out.sort_by_key(|r| crate::notes_panel::order(&r.spot));
        out
    }

    /// The reader's highlighter: marks the text in `pen`; the mark and
    /// whether it is new.
    pub fn highlight(
        &mut self,
        main: &mut Library,
        at: Highlighted<'_>,
        wall_ms: u64,
    ) -> Option<(MarkId, bool)> {
        let color = at.color.map(|Rgb(r, g, b)| [r, g, b]);
        let new = NewMark::highlight(at.text, at.spot, at.pen, color);
        let (lib, book) = self.shelf(main, at.book, at.name);
        let before = lib.marks(book).len();
        let id = lib.add_mark(book, new, wall_ms)?;
        Some((id, lib.marks(book).len() > before))
    }

    /// Note pressed: highlights the text and opens a draft on it.
    pub fn write_note(
        &mut self,
        main: &mut Library,
        pane: PaneId,
        at: Highlighted<'_>,
        rect: Rect,
        wall_ms: u64,
    ) {
        let Some((id, made)) = self.highlight(main, at, wall_ms) else {
            return;
        };
        self.open_note(main, pane, id, rect, true);
        if let Some(n) = &mut self.note {
            n.placeholder = made;
        }
    }

    /// Opens `id`'s note beside `at`: a draft if `write`, else to read.
    pub(super) fn open_note(
        &mut self,
        main: &Library,
        pane: PaneId,
        id: MarkId,
        at: Rect,
        write: bool,
    ) {
        let Some(mark) = self.mark(main, id).cloned() else {
            return;
        };
        self.card = None;
        self.menu = None;
        self.note_rect = None;
        let draft = write.then(|| mark.note.value.clone().unwrap_or_default());
        self.note = Some(Note {
            pane,
            mark,
            at,
            seen: true,
            draft,
            placeholder: false,
            focus: 2,
        });
    }

    /// Keeps the open draft; blank deletes the note or its new highlight.
    pub(super) fn save_note(&mut self, main: &mut Library, id: MarkId, out: &mut Commands) {
        let Some(n) = self.note.take_if(|n| n.mark.id == id) else {
            return;
        };
        let Some(draft) = n.draft else {
            return;
        };
        let text = draft.trim();
        if text.is_empty() {
            if n.placeholder {
                self.drop_marks(main, &[id]);
            } else if n.mark.note.value.is_some() {
                self.forget_note(main, id, out);
            }
            return;
        }
        let wall = mareader_engine::library::wall_ms();
        if !main.set_note(id, Some(text.to_owned()), wall) {
            self.loose.set_note(id, Some(text.to_owned()), wall);
        }
    }

    /// Closes the note unsaved; a highlight made for it goes too.
    pub(super) fn cancel_note(&mut self, main: &mut Library) {
        if let Some(n) = self.note.take().filter(|n| n.placeholder) {
            self.drop_marks(main, &[n.mark.id]);
        }
    }

    /// Deletes `id`'s note, with Undo bringing its text back.
    pub(super) fn forget_note(&mut self, main: &mut Library, id: MarkId, out: &mut Commands) {
        let was = self.mark(main, id).and_then(|m| m.note.value.clone());
        let wall = mareader_engine::library::wall_ms();
        if !main.set_note(id, None, wall) {
            self.loose.set_note(id, None, wall);
        }
        if let Some(text) = was {
            let undo = Command::Study(Action::RestoreNote(id, text));
            out.push(Sent::Ui(UiCommand::Notify(
                Toast::info("Deleted note").undo(undo),
            )));
        }
    }

    /// The open note's mark, and its draft while written; for tests.
    pub fn note_open(&self) -> Option<(MarkId, Option<&str>)> {
        let n = self.note.as_ref()?;
        Some((n.mark.id, n.draft.as_deref()))
    }
}
