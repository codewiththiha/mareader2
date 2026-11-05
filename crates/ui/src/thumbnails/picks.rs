//! Pages picked in the thumbnail grid, and the menus that act on them.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use egui::{Align2, Button, Id, Response, RichText, Sense, Ui, vec2};
use mareader_session::{DocId, Document};
use mareader_settings::{PageTask, Settings, WordOutput, language_name};
use mareader_workspace::PaneId;

use crate::command::{Command, Commands};
use crate::documents::Reading;
use crate::icons::Icon;
use crate::page_ai::Action;
use crate::settings_window::Tab;
use crate::theme::Tokens;
use crate::widgets::menu_row;

/// What the page AI offers picked pages, as of this frame's settings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Offer {
    /// Some page task has a model.
    pub any: bool,
    /// The language Translate goes to, by tag and name.
    pub language: Option<(String, String)>,
    /// Valid recently used translation languages, most recent first.
    pub recent: Vec<(String, String)>,
    /// What the quick word list does, and the preset it uses.
    pub quick: (WordOutput, Option<String>),
}

impl Offer {
    pub fn of(settings: &Settings) -> Self {
        let ai = &settings.ai;
        let words = &settings.study.words;
        let language = ai.tasks.translate.language.as_deref().and_then(|tag| {
            let name = language_name(tag)?;
            Some((tag.to_owned(), name.to_owned()))
        });
        Self {
            any: PageTask::ALL.iter().any(|t| ai.page_preset(*t).is_some()),
            language,
            recent: ai
                .tasks
                .translate
                .recent
                .iter()
                .filter_map(|tag| language_name(tag).map(|name| (tag.clone(), name.to_owned())))
                .take(5)
                .collect(),
            quick: (words.instant().output, words.instant.clone()),
        }
    }
}

/// A still press this long on a page picks it, as in the library.
const LONG_PRESS: f64 = 0.45;

/// The pages picked, in the document they were counted in.
#[derive(Default, Clone)]
pub struct Picks {
    /// The pane and document the pages belong to.
    owner: Option<(PaneId, DocId)>,
    /// That document, to find picked pages again; held only while some are.
    counted: Option<Arc<Document>>,
    pages: BTreeSet<usize>,
    /// Where a Shift-click range starts.
    anchor: Option<usize>,
}

impl fmt::Debug for Picks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Picks")
            .field("owner", &self.owner)
            .field("pages", &self.pages)
            .finish_non_exhaustive()
    }
}

impl Picks {
    /// Follows the pane read; the same pages stay picked if they still are.
    pub fn follow(&mut self, reading: &Reading) {
        let here = (reading.pane, reading.doc);
        if self.owner == Some(here) {
            return;
        }
        let same_pane = self.owner.is_some_and(|(pane, _)| pane == reading.pane);
        let was = self.counted.take();
        // Results laid in or out move pages; new type or a new book ends it.
        self.pages = match (same_pane, was) {
            (true, Some(was)) => (self.pages.iter())
                .filter_map(|p| was.same_page(*p, &reading.document))
                .collect(),
            _ => BTreeSet::new(),
        };
        self.anchor = self
            .anchor
            .and_then(|a| self.pages.contains(&a).then_some(a));
        self.owner = Some(here);
        self.counted = Some(Arc::clone(&reading.document));
    }

    /// Ends a frame: the book stays held only while pages are picked.
    pub fn settle(&mut self, reading: &Reading) {
        self.counted = self.active().then(|| Arc::clone(&reading.document));
    }

    pub fn active(&self) -> bool {
        !self.pages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn contains(&self, page: usize) -> bool {
        self.pages.contains(&page)
    }

    /// The pages in reading order.
    pub fn pages(&self) -> Vec<usize> {
        self.pages.iter().copied().collect()
    }

    pub fn toggle(&mut self, page: usize) {
        if !self.pages.remove(&page) {
            self.pages.insert(page);
        }
        self.anchor = Some(page);
    }

    /// Adds every page from the last one picked to `page`.
    pub fn extend_to(&mut self, page: usize) {
        let from = self.anchor.unwrap_or(page);
        self.pages.extend(from.min(page)..=from.max(page));
        self.anchor = Some(page);
    }

    pub fn has_anchor(&self) -> bool {
        self.anchor.is_some()
    }

    pub fn all(&mut self, count: usize) {
        self.pages = (0..count).collect();
    }

    /// Lets go of every page; false if none was picked.
    pub fn clear(&mut self) -> bool {
        self.anchor = None;
        self.counted = None;
        !std::mem::take(&mut self.pages).is_empty()
    }
}

/// A still press held on page `page` picks it; true on its release.
pub fn long_press(ui: &Ui, resp: &Response, page: usize, picks: &mut Picks) -> bool {
    let key = Id::new("thumb-long-press");
    let (now, start, pressed, released, moved) = ui.input(|i| {
        let p = &i.pointer;
        (
            i.time,
            p.press_start_time(),
            p.any_pressed(),
            p.any_released(),
            p.is_decidedly_dragging(),
        )
    });
    // A new press forgets the last one.
    if pressed {
        ui.data_mut(|d| d.remove::<usize>(key));
    }
    let fired = ui.data(|d| d.get_temp::<usize>(key)) == Some(page);
    if released {
        if fired {
            ui.data_mut(|d| d.remove::<usize>(key));
        }
        return fired && resp.clicked();
    }
    // A press that wanders is no long press.
    let held = resp.is_pointer_button_down_on() && !moved;
    let Some(start) = start.filter(|_| held && !fired) else {
        return false;
    };
    let left = LONG_PRESS - (now - start);
    if left > 0.0 {
        ui.ctx().request_repaint_after_secs(left as f32);
        return false;
    }
    ui.data_mut(|d| d.insert_temp(key, page));
    if !picks.contains(page) {
        picks.toggle(page);
    }
    false
}

/// A click while picking, or with a modifier; false to go to the page.
pub fn click(ui: &Ui, page: usize, picks: &mut Picks) -> bool {
    let m = ui.input(|i| i.modifiers);
    match () {
        _ if m.shift && picks.has_anchor() => picks.extend_to(page),
        _ if m.command || m.shift || picks.active() => picks.toggle(page),
        _ => return false,
    }
    true
}

/// "3 pages selected", with All and Clear, over the grid.
pub fn bar(ui: &mut Ui, picks: &mut Picks, count: usize, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        let n = picks.len();
        let words = match n {
            1 => "1 page selected".to_owned(),
            n => format!("{n} pages selected"),
        };
        ui.label(RichText::new(words).size(t.text_small).color(t.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            if ui.small_button("Clear").clicked() {
                picks.clear();
            }
            if n < count && ui.small_button("All").clicked() {
                picks.all(count);
            }
        });
    });
}

/// The menu of the picked pages.
pub fn picks_menu(
    ui: &mut Ui,
    (pane, offer): (PaneId, &Offer),
    picks: &mut Picks,
    count: usize,
    t: &Tokens,
    out: &mut Commands,
) {
    let n = picks.len();
    let heading = match n {
        1 => "1 page".to_owned(),
        n => format!("{n} pages"),
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(heading).size(13.0).color(t.ink));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear = ui.add(Button::new("Clear").right_text("Esc"));
            clear.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Clear selection")
            });
            if clear.clicked() {
                picks.clear();
                ui.close();
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let width = ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0).max(1.0);
        if tile(ui, Icon::Copy, "Copy text", width, t).clicked() {
            out.push(Command::CopyPages {
                pane,
                pages: picks.pages(),
            });
            ui.close();
        }
        if tile(ui, Icon::Levels, "Word list", width, t).clicked() {
            out.push(Command::WordListWindow {
                pane,
                pages: picks.pages(),
            });
            ui.close();
        }
        if tile(ui, Icon::SelectSeveral, "Select all", width, t).clicked() {
            picks.all(count);
            ui.close();
        }
    });
    group(ui, "VOCABULARY", t);
    let quick = match offer.quick.0 {
        WordOutput::Copy => "Copy word list",
        WordOutput::Markdown => "Save word list (.md)",
        WordOutput::Epub => "Save word list (.epub)",
    };
    // The preset is a badge, not a keyboard shortcut or part of the name.
    let words = menu_row(ui, Icon::Levels, quick, None, false);
    if let Some(preset) = offer.quick.1.as_deref() {
        let font = egui::FontId::proportional(10.5);
        let galley = ui
            .painter()
            .layout_no_wrap(preset.to_owned(), font, t.accent);
        let size = galley.size() + vec2(10.0, 4.0);
        let rect = egui::Rect::from_center_size(
            egui::pos2(
                words.rect.right() - size.x / 2.0 - 6.0,
                words.rect.center().y,
            ),
            size,
        );
        ui.painter().rect_filled(rect, 8.0, t.accent_soft);
        ui.painter()
            .galley(rect.min + vec2(5.0, 2.0), galley, t.accent);
    }
    if words
        .on_hover_text("The words above your level, with their dictionary meanings")
        .clicked()
    {
        out.push(Command::WordList {
            pane,
            pages: picks.pages(),
        });
        ui.close();
    }
    if menu_row(ui, Icon::ViewOptions, "Word list…", None, false).clicked() {
        out.push(Command::WordListWindow {
            pane,
            pages: picks.pages(),
        });
        ui.close();
    }
    group(ui, "ASK AI", t);
    ai_rows(ui, (pane, offer), picks, out);
}

/// A frequent action, with a single accessible name and a generous target.
fn tile(ui: &mut Ui, icon: Icon, label: &str, width: f32, t: &Tokens) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(width, 54.0), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let fill = if r.is_pointer_button_down_on() {
        t.pressed
    } else if r.hovered() || r.has_focus() {
        t.hover
    } else {
        t.card
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    let icon_rect =
        egui::Rect::from_center_size(rect.center_top() + vec2(0.0, 15.0), vec2(16.0, 16.0));
    icon.paint(ui, icon_rect, t.ink);
    ui.painter().text(
        rect.center_bottom() - vec2(0.0, 9.0),
        Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(10.5),
        t.ink,
    );
    r
}

fn group(ui: &mut Ui, label: &str, t: &Tokens) {
    ui.add_space(6.0);
    ui.label(RichText::new(label).size(10.5).color(t.muted).strong());
}

/// Summarize, translate and simplify the picked pages, a group at a time.
fn ai_rows(ui: &mut Ui, (pane, offer): (PaneId, &Offer), picks: &Picks, out: &mut Commands) {
    if !offer.any {
        if menu_row(ui, Icon::Settings, "Set up AI…", None, false).clicked() {
            out.push(Command::OpenSettings(Tab::Ai));
            ui.close();
        }
        return;
    }
    let run = |task, tag| {
        let pages = picks.pages();
        Command::PageAi(Action::RunPages {
            pane,
            pages,
            task,
            tag,
        })
    };
    if menu_row(ui, Icon::Summarize, "Summarize", None, false).clicked() {
        out.push(run(PageTask::Summarize, None));
        ui.close();
    }
    let button = Button::image_and_text(Icon::Translate.image(16.0), "Translate")
        .right_text(egui::containers::menu::SubMenuButton::RIGHT_ARROW)
        .image_tint_follows_text_color(true)
        .frame_when_inactive(false)
        .min_size(vec2(ui.available_width(), 32.0));
    let (response, _) = egui::containers::menu::SubMenuButton::from_button(button).ui(ui, |ui| {
        ui.set_width(180.0);
        if !offer.recent.is_empty() {
            let t = Tokens::get(ui.ctx());
            group(ui, "RECENT", &t);
            for (tag, name) in &offer.recent {
                let on = offer
                    .language
                    .as_ref()
                    .is_some_and(|(selected, _)| selected == tag);
                if menu_row(ui, Icon::Translate, name, None, on).clicked() {
                    out.push(run(PageTask::Translate, Some(tag.clone())));
                    ui.close();
                }
            }
            ui.separator();
        }
        if menu_row(ui, Icon::Translate, "More languages…", None, false).clicked() {
            let pages = picks.pages();
            let at = ui.max_rect().right_top();
            out.push(Command::PageAi(Action::PagesMenu { pane, pages, at }));
            ui.close();
        }
    });
    ui.data_mut(|d| d.insert_temp(response.id.with("submenu-trigger"), true));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Translate"));
    let simplify = menu_row(ui, Icon::Simplify, "Simplify", None, false);
    if simplify
        .on_hover_text("Rewrites the text in plain English")
        .clicked()
    {
        out.push(run(PageTask::Simplify, None));
        ui.close();
    }
}

/// The menu of one page, picked or not.
pub fn page_menu(ui: &mut Ui, pane: PaneId, page: usize, picks: &mut Picks, out: &mut Commands) {
    if menu_row(ui, Icon::Copy, "Copy page text", None, false).clicked() {
        let pages = vec![page];
        out.push(Command::CopyPages { pane, pages });
        ui.close();
    }
    if menu_row(ui, Icon::SelectSeveral, "Select", None, false).clicked() {
        picks.toggle(page);
        ui.close();
    }
}
