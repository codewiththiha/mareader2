//! The notes panel: a book's highlights and notes, in reading order.

use egui::text::{LayoutJob, TextWrapping};
use egui::{Align, FontId, Galley, Id, Rect, RichText, ScrollArea, Sense, TextFormat, Ui, vec2};
use mareader_appearance::Rgb;
use mareader_library::{MarkId, Pen, Spot};
use mareader_pdf::active_entry;
use mareader_session::{Anchor, Document};
use std::sync::Arc;

use crate::command::{Command, Commands};
use crate::documents::Reading;
use crate::export::{self, Entry, Format, Sheet};
use crate::icons::Icon;
use crate::study::Action;
use crate::theme::{self, Tokens, color};
use crate::widgets;

const PAD_X: f32 = 12.0;
/// The colour rule's width and the gap after it.
const RULE: f32 = 3.0;
const RULE_GAP: f32 = 10.0;

/// One highlight or bookmark as the panel lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: MarkId,
    pub spot: Spot,
    pub quote: String,
    pub note: Option<String>,
    pub ink: Rgb,
    pub pen: Pen,
    /// When it was made, in wall milliseconds.
    pub added: u64,
    /// A bookmarked page rather than highlighted words.
    pub bookmark: bool,
}

/// Which rows show: everything, those with notes, or bookmarks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Notes,
    Bookmarks,
}

impl Filter {
    fn keeps(self, row: &Row) -> bool {
        match self {
            Self::All => true,
            Self::Notes => row.note.is_some(),
            Self::Bookmarks => row.bookmark,
        }
    }
}

/// Draws the panel for `reading`'s document and its `rows`.
pub fn show(ui: &mut Ui, reading: &Reading, rows: &[Row], out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let id = Id::new(("notes-filter", reading.doc));
    let mut filter: Filter = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let noted = rows.iter().filter(|r| r.note.is_some()).count();
    let kept = rows.iter().filter(|r| r.bookmark).count();
    if rows.is_empty() {
        empty(
            ui,
            "No highlights yet",
            "Select text, then highlight it or add a note.",
            &t,
        );
        return;
    }
    // The last bookmark gone, its filter goes too.
    if kept == 0 && filter == Filter::Bookmarks {
        filter = Filter::All;
    }
    let shown: Vec<&Row> = rows.iter().filter(|r| filter.keeps(r)).collect();
    ui.add_space(8.0);
    // The book and its tools, then the filters on a row of their own.
    ui.horizontal(|ui| {
        ui.add_space(PAD_X);
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(PAD_X - 6.0);
            // Exports carry highlights; bookmarks are places, not words.
            let words: Vec<&Row> = shown.iter().copied().filter(|r| !r.bookmark).collect();
            ui.add_enabled_ui(!words.is_empty(), |ui| {
                share(ui, reading, &words, &t, out);
            });
            ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
                let title = RichText::new(&reading.name).strong().color(t.ink);
                ui.add(egui::Label::new(title).truncate());
            });
        });
    });
    ui.add_space(2.0);
    let pad = egui::Margin::symmetric(PAD_X as i8, 0);
    egui::Frame::NONE.inner_margin(pad).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let chips = [
                (Filter::All, format!("All · {}", rows.len())),
                (Filter::Notes, format!("Notes · {noted}")),
                (Filter::Bookmarks, format!("Bookmarks · {kept}")),
            ];
            let some = |f: &&(Filter, String)| f.0 != Filter::Bookmarks || kept > 0;
            for (f, label) in chips.iter().filter(some) {
                if ui.selectable_label(filter == *f, label).clicked() {
                    filter = *f;
                }
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(id, filter));
    ui.add_space(4.0);
    if shown.is_empty() {
        empty(ui, "No notes yet", "Select text and choose Note.", &t);
        return;
    }
    let book = reading.document.as_ref();
    ScrollArea::vertical()
        .id_salt(("notes-list", reading.doc))
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in shown {
                if row.bookmark {
                    bookmark(ui, reading, book, row, &t, out);
                } else {
                    entry(ui, reading, row, &t, out);
                }
            }
            ui.add_space(12.0);
        });
}

/// Export, then Copy all: the list as shown, so a filter narrows both.
fn share(ui: &mut Ui, reading: &Reading, shown: &[&Row], t: &Tokens, out: &mut Commands) {
    let notes = shown.iter().filter(|r| r.note.is_some()).count();
    let tally = export::tally(shown.len(), notes);
    // Written only when asked for, not each frame.
    let render = |format| {
        let entries = entries(reading, shown);
        let sheet = Sheet {
            title: &reading.name,
            entries: &entries,
            now_ms: crate::library::wall_ms(),
        };
        export::render(&sheet, format)
    };
    let menu = Id::new(("notes-export", reading.doc));
    let open = egui::Popup::is_id_open(ui.ctx(), menu);
    let r = widgets::icon_button(ui, Icon::Download, "Export…", open);
    egui::Popup::from_toggle_button_response(&r)
        .id(menu)
        .sense(widgets::POPUP_SENSE)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(4.0)
        .show(|ui| {
            ui.set_width(200.0);
            let what = format!("Export {tally}");
            ui.label(RichText::new(what).size(t.text_small).color(t.muted));
            ui.add_space(2.0);
            for format in Format::ALL {
                let label = format!("{} (.{})", format.label(), format.extension());
                if widgets::menu_row(ui, Icon::Download, &label, None, false).clicked() {
                    out.push(Command::Export {
                        name: export::file_name(&reading.name, format),
                        format,
                        contents: render(format),
                    });
                    ui.close();
                }
            }
            widgets::close_on_key(ui);
        });
    if widgets::icon_button(ui, Icon::Copy, "Copy all as Markdown", false).clicked() {
        ui.ctx().copy_text(render(Format::Markdown));
        let copied = format!("Copied {tally}");
        out.push(Command::Notify(crate::toast::Toast::info(copied)));
    }
}

/// The rows as exported, each with its page and chapter.
fn entries(reading: &Reading, rows: &[&Row]) -> Vec<Entry> {
    let outline = reading.document.outline();
    let chapter = |page| {
        let i = active_entry(outline, page)?;
        outline.get(i).map(|e| e.title.clone())
    };
    rows.iter()
        .map(|r| {
            let page = page_of(&reading.document, r.spot);
            Entry {
                quote: r.quote.clone(),
                note: r.note.clone(),
                page,
                chapter: page.and_then(chapter),
                pen: r.pen,
                color: r.ink,
                added: r.added,
            }
        })
        .collect()
}

/// One highlight: its page, the words, and the note under them.
fn entry(ui: &mut Ui, reading: &Reading, row: &Row, t: &Tokens, out: &mut Commands) {
    let page = page_of(&reading.document, row.spot);
    let full = ui.available_width();
    let text_w = (full - PAD_X * 2.0 - RULE - RULE_GAP).max(40.0);
    let quote = clamp(ui, &row.quote, FontId::proportional(13.5), t.ink, text_w, 3);
    let note = row
        .note
        .as_deref()
        .map(|n| clamp(ui, n, FontId::proportional(13.0), t.muted, text_w - 18.0, 4));
    let meta = page.map(|p| format!("Page {}", p + 1)).unwrap_or_default();
    let meta_h = 16.0;
    let note_h = note.as_ref().map_or(0.0, |g| g.size().y + 6.0);
    let height = 10.0 + meta_h + quote.size().y + note_h + 10.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(full, height), Sense::click());
    let label = match &row.note {
        Some(n) => format!("{}. Note: {n}", row.quote),
        None => row.quote.clone(),
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let p = ui.painter();
    let card = rect.shrink2(vec2(6.0, 2.0));
    if resp.hovered() || resp.has_focus() {
        p.rect_filled(card, t.radius, t.hover);
    }
    let left = rect.left() + PAD_X;
    let top = rect.top() + 10.0;
    let rule = Rect::from_min_size(egui::pos2(left, top + meta_h), vec2(RULE, quote.size().y));
    p.rect_filled(rule, 1.5, color(row.ink));
    let x = left + RULE + RULE_GAP;
    let small = FontId::proportional(t.text_small);
    p.text(
        egui::pos2(x, top),
        egui::Align2::LEFT_TOP,
        meta,
        small,
        t.muted,
    );
    let qh = quote.size().y;
    p.galley(egui::pos2(x, top + meta_h), quote, t.ink);
    if let Some(g) = note {
        let y = top + meta_h + qh + 6.0;
        let mark = Rect::from_min_size(egui::pos2(x, y + 1.0), vec2(13.0, 13.0));
        Icon::Note.paint(ui, mark, t.muted);
        ui.painter().galley(egui::pos2(x + 18.0, y), g, t.muted);
    }
    if resp.clicked()
        && let Some(page) = page
    {
        out.push(Command::GoTo {
            pane: reading.pane,
            page,
        });
    }
    let noted = row.note.is_some();
    widgets::context_menu(&resp, |ui| {
        let write = if noted { "Edit note" } else { "Add note" };
        if widgets::menu_row(ui, Icon::Note, write, None, false).clicked() {
            if let Some(page) = page {
                out.push(Command::GoTo {
                    pane: reading.pane,
                    page,
                });
            }
            out.push(Command::Study(Action::WriteNoteIn(reading.pane, row.id)));
            ui.close();
        }
        if noted && widgets::menu_row(ui, Icon::Remove, "Delete note", None, false).clicked() {
            out.push(Command::Study(Action::DeleteNote(row.id)));
            ui.close();
        }
        ui.visuals_mut().override_text_color = Some(t.danger);
        let remove = widgets::menu_row(ui, Icon::Remove, "Remove highlight", None, false);
        if remove.clicked() {
            out.push(Command::Study(Action::Remove(vec![row.id])));
            ui.close();
        }
    });
}

/// One bookmark: its page, chapter and note, a ribbon for its rule.
fn bookmark(
    ui: &mut Ui,
    reading: &Reading,
    doc: &Document,
    row: &Row,
    t: &Tokens,
    out: &mut Commands,
) {
    let page = page_of(doc, row.spot);
    let full = ui.available_width();
    let text_w = (full - PAD_X * 2.0 - RULE - RULE_GAP).max(40.0);
    // Its chapter as the book stands now; the page says the rest.
    let chapter = page.and_then(|p| crate::bookmarks::chapter(doc, p));
    let at = page.map(|p| format!("Page {}", p + 1));
    let (name, meta) = match (chapter, at) {
        (Some(c), Some(at)) => (c, at),
        (None, Some(at)) => (at, "Bookmark".to_owned()),
        (_, None) => (row.quote.clone(), "Bookmark".to_owned()),
    };
    let title = clamp(ui, &name, FontId::proportional(13.5), t.ink, text_w, 2);
    let note = row
        .note
        .as_deref()
        .map(|n| clamp(ui, n, FontId::proportional(13.0), t.muted, text_w - 18.0, 4));
    let meta_h = 16.0;
    let note_h = note.as_ref().map_or(0.0, |g| g.size().y + 6.0);
    let height = 10.0 + meta_h + title.size().y + note_h + 10.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(full, height), Sense::click());
    let place = match page {
        Some(p) if meta != "Bookmark" => format!("Bookmark, page {}: {name}", p + 1),
        _ => format!("Bookmark: {name}"),
    };
    let label = match &row.note {
        Some(n) => format!("{place}. Note: {n}"),
        None => place,
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let p = ui.painter();
    if resp.hovered() || resp.has_focus() {
        p.rect_filled(rect.shrink2(vec2(6.0, 2.0)), t.radius, t.hover);
    }
    let left = rect.left() + PAD_X;
    let top = rect.top() + 10.0;
    // Centred on where a highlight's rule runs, the text in line.
    let mid = egui::pos2(left + RULE / 2.0, top + meta_h + 8.0);
    Icon::Bookmark.paint(ui, Rect::from_center_size(mid, vec2(12.0, 12.0)), t.accent);
    let x = left + RULE + RULE_GAP;
    let small = FontId::proportional(t.text_small);
    let p = ui.painter();
    p.text(
        egui::pos2(x, top),
        egui::Align2::LEFT_TOP,
        meta,
        small,
        t.muted,
    );
    let th = title.size().y;
    p.galley(egui::pos2(x, top + meta_h), title, t.ink);
    if let Some(g) = note {
        let y = top + meta_h + th + 6.0;
        let mark = Rect::from_min_size(egui::pos2(x, y + 1.0), vec2(13.0, 13.0));
        Icon::Note.paint(ui, mark, t.muted);
        ui.painter().galley(egui::pos2(x + 18.0, y), g, t.muted);
    }
    let go = |out: &mut Commands| {
        if let Some(page) = page {
            out.push(Command::GoTo {
                pane: reading.pane,
                page,
            });
        }
    };
    if resp.clicked() {
        go(out);
    }
    let noted = row.note.is_some();
    widgets::context_menu(&resp, |ui| {
        let write = if noted { "Edit note" } else { "Add note" };
        if widgets::menu_row(ui, Icon::Note, write, None, false).clicked() {
            go(out);
            out.push(Command::Study(Action::WriteNoteIn(reading.pane, row.id)));
            ui.close();
        }
        if noted && widgets::menu_row(ui, Icon::Remove, "Delete note", None, false).clicked() {
            out.push(Command::Study(Action::DeleteNote(row.id)));
            ui.close();
        }
        ui.visuals_mut().override_text_color = Some(t.danger);
        let remove = widgets::menu_row(ui, Icon::Remove, "Remove bookmark", None, false);
        if remove.clicked() {
            out.push(Command::Study(Action::Remove(vec![row.id])));
            ui.close();
        }
    });
}

/// `text` on one line's words, wrapped to `rows` at most.
fn clamp(
    ui: &Ui,
    text: &str,
    font: FontId,
    ink: egui::Color32,
    w: f32,
    rows: usize,
) -> Arc<Galley> {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut job = LayoutJob::single_section(flat, TextFormat::simple(font, ink));
    job.wrap = TextWrapping {
        max_width: w,
        max_rows: rows,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// Where `spot` starts: a PDF's page, or the page a book's text is on.
pub fn page_of(doc: &Document, spot: Spot) -> Option<usize> {
    match spot {
        Spot::Page { page, .. } => Some(page),
        Spot::Chars { from, .. } => Some(from[0]),
        Spot::Text { block, start, .. } => doc.locate(Anchor {
            block,
            offset: start,
        }),
        Spot::Blocks { from, .. } => doc.locate(Anchor {
            block: from[0],
            offset: from[1],
        }),
    }
}

/// The reading order of `spot`, to list highlights as the book runs.
pub fn order(spot: &Spot) -> (usize, usize) {
    match *spot {
        Spot::Page { page, rect } => {
            let y = rect.get(1).copied().unwrap_or_default().max(0.0);
            (page, y as usize)
        }
        Spot::Chars { from, .. } | Spot::Blocks { from, .. } => (from[0], from[1]),
        Spot::Text { block, start, .. } => (block, start),
    }
}

fn empty(ui: &mut Ui, title: &str, hint: &str, t: &Tokens) {
    let area = ui.max_rect().shrink2(vec2(28.0, 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(area)
            .layout(egui::Layout::top_down(Align::Center)),
        |ui| {
            ui.add_space(area.height() * 0.3);
            let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
            Icon::Note.paint(ui, r, t.muted);
            ui.add_space(6.0);
            ui.label(RichText::new(title).font(theme::medium(14.0)).color(t.ink));
            ui.label(RichText::new(hint).size(t.text_small).color(t.muted));
        },
    );
}
