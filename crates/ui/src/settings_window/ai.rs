//! The AI tab: which model each task uses, saved models, and providers.

use egui::{Align, Layout, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, pos2, vec2};
use mareader_assist::{AppleState, Board, Job, KeyRow, KeyState, Load, lanes};
use mareader_settings::{AiSettings, LANGUAGES, ModelPreset, PresetId, Provider};
use mareader_study::Failure;

use super::{divider, group, note, row, section};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

const FIELD_W: f32 = 260.0;
const PICK_W: f32 = 240.0;
const BOOKS_LANGUAGE: &str = "The book's language";

/// What the running service adds to the settings.
pub struct Live<'a> {
    pub board: &'a Board,
    /// Tasks a saved model runs now.
    pub in_use: &'a dyn Fn(PresetId) -> usize,
}

pub fn tab(ui: &mut Ui, ai: &mut AiSettings, live: Option<Live<'_>>, out: &mut Commands) {
    let board = live.as_ref().map(|l| l.board);
    if cfg!(target_os = "macos") && board.is_some_and(|b| b.apple == Load::Idle) {
        out.push(Command::Ai(Job::Probe));
    }
    if let Some(board) = board {
        fetch_models(ui, ai, board, out);
    }
    tasks(ui, ai, board);
    models(ui, ai, live.as_ref(), out);
    for p in used_servers(ai) {
        provider(ui, ai, p, board, out);
    }
}

/// The providers saved models use, each once, in the menu's order.
fn used_servers(ai: &AiSettings) -> Vec<Provider> {
    let used = |p: &Provider| ai.presets.iter().any(|m| m.provider == *p);
    Provider::SERVERS.into_iter().filter(used).collect()
}

/// Local servers list their models afresh each time the tab opens.
fn fetch_models(ui: &Ui, ai: &AiSettings, board: &Board, out: &mut Commands) {
    // The tab was hidden if it wasn't drawn the frame before.
    let seen = egui::Id::new("ai-tab-frame");
    let frame = ui.ctx().cumulative_frame_nr();
    let last: Option<u64> = ui.data(|d| d.get_temp(seen));
    ui.data_mut(|d| d.insert_temp(seen, frame));
    let opened = last.is_none_or(|l| l + 1 < frame);
    for p in used_servers(ai) {
        let idle = *board.models(p) == Load::Idle;
        if p.local() && !board.fetching(p) && (opened || idle) {
            out.push(Command::Ai(Job::Models(p)));
        }
    }
}

fn tasks(ui: &mut Ui, ai: &mut AiSettings, board: Option<&Board>) {
    section(ui, "Tasks");
    let apple = ai
        .explain_preset()
        .is_some_and(|p| p.provider == Provider::Apple);
    let state = board.and_then(|b| b.apple.ready()).filter(|_| apple);
    let speaks = |tag: &str| state.and_then(|s| s.speaks(tag));
    group(ui, |ui| {
        let tip = "The model the Explain card asks";
        row(ui, "Explain words", tip, |ui| {
            let explain = &mut ai.tasks.explain.preset;
            preset_picker(ui, "ai-explain-model", &ai.presets, explain);
        });
        divider(ui);
        let tip = "The meaning's language; the rest stays in the book's";
        row(ui, "Meaning in", tip, |ui| {
            language_picker(ui, &mut ai.tasks.explain.language, &speaks);
        });
    });
    if ai.presets.is_empty() {
        note(ui, "Add a model below, then choose it here.");
    }
    let chosen = ai.tasks.explain.language.as_deref();
    let name = chosen.and_then(mareader_settings::language_name);
    if let (Some(tag), Some(name)) = (chosen, name)
        && speaks(tag) == Some(false)
    {
        note(
            ui,
            &format!("Apple Intelligence doesn't write {name}. Pick a language it lists."),
        );
    }
}

/// A saved model for a task, or none.
fn preset_picker(ui: &mut Ui, salt: &str, presets: &[ModelPreset], chosen: &mut Option<PresetId>) {
    let title = |id: PresetId| presets.iter().find(|p| p.id == id).map(ModelPreset::title);
    let shown = chosen.and_then(title).unwrap_or_else(|| "None".to_owned());
    egui::ComboBox::from_id_salt(salt)
        .width(PICK_W)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.selectable_value(chosen, None, "None");
            for p in presets {
                ui.selectable_value(chosen, Some(p.id), p.title());
            }
        });
}

/// The book's language or a listed one; ones Apple can't write are greyed.
fn language_picker(
    ui: &mut Ui,
    chosen: &mut Option<String>,
    speaks: &dyn Fn(&str) -> Option<bool>,
) {
    let name = chosen.as_deref().and_then(mareader_settings::language_name);
    egui::ComboBox::from_id_salt("ai-meaning-language")
        .width(PICK_W)
        .height(320.0)
        .selected_text(name.unwrap_or(BOOKS_LANGUAGE))
        .show_ui(ui, |ui| {
            ui.selectable_value(chosen, None, BOOKS_LANGUAGE);
            for (tag, name) in LANGUAGES {
                let on = chosen.as_deref() == Some(tag);
                let button = egui::Button::selectable(on, name);
                let r = ui.add_enabled(speaks(tag) != Some(false), button);
                if r.clicked() {
                    *chosen = Some(tag.to_owned());
                }
                r.on_disabled_hover_text("Apple Intelligence doesn't write this language");
            }
        });
}

/// The open model's id, kept while the window lives.
fn open_id() -> egui::Id {
    egui::Id::new("ai-open-model")
}

fn models(ui: &mut Ui, ai: &mut AiSettings, live: Option<&Live<'_>>, out: &mut Commands) {
    let board = live.map(|l| l.board);
    let mut open = ui.data(|d| d.get_temp::<Option<u32>>(open_id()).flatten());
    section(ui, "Models");
    let mut gone = None;
    group(ui, |ui| {
        let uses = |ai: &AiSettings, id| task_names(ai, id);
        for i in 0..ai.presets.len() {
            let Some(p) = ai.presets.get(i) else {
                break;
            };
            let id = p.id;
            let busy = live.map_or(0, |l| (l.in_use)(id));
            let detail = summary(p, board, busy, &uses(ai, id));
            let is_open = open == Some(id.0);
            if header(ui, &p.title(), &detail, is_open).clicked() {
                open = (!is_open).then_some(id.0);
            }
            if is_open
                && let Some(p) = ai.presets.get_mut(i)
                && editor(ui, p, board, out)
            {
                gone = Some(id);
            }
            divider(ui);
        }
        if let Some(id) = add_model(ui, ai) {
            open = Some(id.0);
        }
    });
    if let Some(id) = gone {
        ai.remove_preset(id);
        open = None;
    }
    ui.data_mut(|d| d.insert_temp(open_id(), open));
    note(
        ui,
        "Each task asks the model you choose for it. Save one model several \
         times, as with another name or a different “At once”, to give a \
         task its own room.",
    );
}

/// The tasks using `id`, as the row's small print names them.
fn task_names(ai: &AiSettings, id: PresetId) -> String {
    let explain = ai.tasks.explain.preset == Some(id);
    if explain {
        "Explain words".to_owned()
    } else {
        String::new()
    }
}

/// Provider, model, room and use, on one line.
fn summary(p: &ModelPreset, board: Option<&Board>, busy: usize, uses: &str) -> String {
    let mut parts = vec![p.provider.label().to_owned()];
    match p.provider {
        Provider::Apple => parts.push(apple_status(board).to_owned()),
        _ => parts.push(match p.model() {
            "" => "the server's first model".to_owned(),
            model => model.to_owned(),
        }),
    }
    if let Some(b) = board {
        parts.push(format!("{} at once", b.capacity(p)));
    }
    if busy > 0 {
        parts.push(format!("{busy} running"));
    }
    if !uses.is_empty() {
        parts.push(uses.to_owned());
    }
    parts.join(" · ")
}

fn apple_status(board: Option<&Board>) -> &'static str {
    match board.and_then(|b| b.apple.ready()) {
        Some(AppleState::Ready(_)) => "ready",
        Some(AppleState::NotReady(_)) => "downloading",
        Some(AppleState::Off(_)) => "off in System Settings",
        Some(AppleState::Missing(_)) => "not on this Mac",
        None => "on this Mac",
    }
}

/// A model's row: its title, small print and a chevron; click to open.
fn header(ui: &mut Ui, title: &str, detail: &str, open: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), 52.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, open, title));
    let painter = ui.painter().with_clip_rect(rect.shrink2(vec2(0.0, 2.0)));
    let font = egui::FontId::proportional(t.text);
    let ink = if r.hovered() { t.accent } else { t.ink };
    let top = pos2(rect.left(), rect.center().y - 9.0);
    painter.text(top, egui::Align2::LEFT_CENTER, title, font, ink);
    let small = egui::FontId::proportional(t.text_small);
    let below = pos2(rect.left(), rect.center().y + 9.0);
    painter.text(below, egui::Align2::LEFT_CENTER, detail, small, t.muted);
    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let mark =
        egui::Rect::from_center_size(pos2(rect.right() - 9.0, rect.center().y), vec2(16.0, 16.0));
    chevron.image(16.0).tint(t.muted).paint_at(ui, mark);
    r
}

/// Runs `edit`; a name never typed follows what the model runs.
fn renaming(p: &mut ModelPreset, edit: impl FnOnce(&mut ModelPreset)) {
    let named = p.name == ModelPreset::suggested_name(p.provider, &p.model);
    edit(p);
    if named {
        p.name = ModelPreset::suggested_name(p.provider, &p.model);
    }
}

/// Edits one model; true when asked to delete it.
fn editor(ui: &mut Ui, p: &mut ModelPreset, board: Option<&Board>, out: &mut Commands) -> bool {
    renaming(p, |p| fields(ui, p, board, out));
    room_note(ui, p, board);
    divider(ui);
    actions(ui, p, board, out)
}

fn fields(ui: &mut Ui, p: &mut ModelPreset, board: Option<&Board>, out: &mut Commands) {
    divider(ui);
    row(ui, "Name", "How tasks list this model", |ui| {
        let hint = ModelPreset::suggested_name(p.provider, &p.model);
        let edit = TextEdit::singleline(&mut p.name).hint_text(hint);
        field(ui, edit.char_limit(ModelPreset::NAME_MAX));
    });
    divider(ui);
    row(ui, "Provider", "", |ui| provider_picker(ui, p));
    if p.provider.server() {
        divider(ui);
        model_row(ui, p, board, out);
    }
    divider(ui);
    let tip = "Tasks this model may run at the same time";
    row(ui, "At once", tip, |ui| {
        let mut n = f32::from(p.at_once);
        let max = f32::from(ModelPreset::AT_ONCE_MAX);
        widgets::stepper(ui, &mut n, 1.0..=max, 1.0, |v| format!("{v:.0}"), "at once");
        p.at_once = (n.round() as u8).clamp(1, ModelPreset::AT_ONCE_MAX);
    });
}

/// Apple only where it runs; a switch forgets the old provider's model.
fn provider_picker(ui: &mut Ui, p: &mut ModelPreset) {
    let mac = cfg!(target_os = "macos");
    let before = p.provider;
    egui::ComboBox::from_id_salt(("ai-provider", p.id.0))
        .width(PICK_W)
        .selected_text(p.provider.label())
        .show_ui(ui, |ui| {
            let shown = Provider::ALL
                .into_iter()
                .filter(|q| mac || *q != Provider::Apple || *q == before);
            for q in shown {
                ui.selectable_value(&mut p.provider, q, q.label())
                    .on_hover_text(q.detail());
            }
        });
    if p.provider != before {
        p.model.clear();
    }
}

fn model_row(ui: &mut Ui, p: &mut ModelPreset, board: Option<&Board>, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let server = p.provider;
    let lost = board.is_some_and(|b| missing(&p.model, b.models(server)));
    let gone = format!(
        "{} isn't on the server now; choose another.",
        p.model.trim()
    );
    let tip = if lost {
        gone.as_str()
    } else {
        "Blank uses the default"
    };
    row(ui, "Model", tip, |ui| {
        if let Some(b) = board {
            refresh(ui, server, b, out);
            model_menu(ui, server, &mut p.model, b.models(server), out);
        }
        let hint = match server.default_model() {
            "" => "The server's first",
            m => m,
        };
        let mut edit = TextEdit::singleline(&mut p.model).hint_text(hint);
        if lost {
            edit = edit.text_color(t.muted);
        }
        let r = field(ui, edit);
        if lost {
            widgets::tip(&r, &gone, None);
        }
    });
    if lost {
        ui.label(RichText::new(&gone).size(t.text_small).color(t.danger));
        ui.add_space(8.0);
    }
}

/// How much room the model has, and the honest catch with keys.
fn room_note(ui: &mut Ui, p: &ModelPreset, board: Option<&Board>) {
    if !p.provider.takes_keys() {
        return;
    }
    let keys = board.and_then(|b| b.keys(p.provider).ready());
    let Some(keys) = keys.filter(|k| !k.is_empty()) else {
        return;
    };
    let n = lanes(keys);
    let total = u32::from(p.at_once).saturating_mul(n);
    let keys = if n == 1 {
        "1 usable key"
    } else {
        "usable keys"
    };
    let said = match n {
        1 => format!("With {keys}, up to {total} at once."),
        _ => format!(
            "With {n} {keys}, up to {total} at once; requests take turns. \
             Keys from one account share its rate limits."
        ),
    };
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(said).size(t.text_small).color(t.muted));
    ui.add_space(8.0);
}

/// Try and Delete; a second click confirms the delete.
fn actions(ui: &mut Ui, p: &ModelPreset, board: Option<&Board>, out: &mut Commands) -> bool {
    let t = Tokens::get(ui.ctx());
    let confirm = egui::Id::new(("ai-delete", p.id.0));
    let mut asked: bool = ui.data(|d| d.get_temp(confirm)).unwrap_or(false);
    let mut delete = false;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if let Some(board) = board {
            let busy = matches!(board.tried(p.id), Load::Busy);
            if ui.add_enabled(!busy, egui::Button::new("Try it")).clicked() {
                out.push(Command::Ai(Job::Try(p.clone())));
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if asked { "Delete for good" } else { "Delete" };
            let text = RichText::new(label).color(t.danger);
            if ui.button(text).clicked() {
                delete = asked;
                asked = !asked;
            }
            if asked && ui.button("Keep").clicked() {
                asked = false;
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(confirm, asked && !delete));
    if let Some(board) = board {
        tried(ui, board.tried(p.id));
    }
    ui.add_space(8.0);
    delete
}

fn tried(ui: &mut Ui, tried: &Load<mareader_assist::Tried>) {
    let t = Tokens::get(ui.ctx());
    let muted = |s: String| RichText::new(s).size(t.text_small).color(t.muted);
    match tried {
        Load::Idle => {}
        Load::Busy => {
            ui.add_space(4.0);
            ui.label(muted("Asking…".to_owned()));
        }
        Load::Ready(tried) => {
            let secs = tried.millis as f64 / 1000.0;
            ui.add_space(4.0);
            ui.label(muted(format!("Works, in {secs:.1} s: “{}”", tried.meaning)));
        }
        Load::Failed(f) => {
            ui.add_space(4.0);
            failure(ui, f);
        }
    }
}

/// The last row: a menu of providers; picking one saves a model.
fn add_model(ui: &mut Ui, ai: &mut AiSettings) -> Option<PresetId> {
    let mac = cfg!(target_os = "macos");
    let full = ai.presets.len() >= AiSettings::PRESETS_MAX;
    let mut made = None;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), super::ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add_enabled_ui(!full, |ui| {
                ui.menu_button("+  Add a model", |ui| {
                    let shown = Provider::ALL
                        .into_iter()
                        .filter(|p| mac || *p != Provider::Apple);
                    for p in shown {
                        let r = ui.button(p.label()).on_hover_text(p.detail());
                        if r.clicked() {
                            made = Some(p);
                            ui.close();
                        }
                    }
                });
            });
        },
    );
    let id = ai.add_preset(made?, "");
    // The first model explains words straight away.
    if ai.tasks.explain.preset.is_none() {
        ai.tasks.explain.preset = Some(id);
    }
    Some(id)
}

/// One provider's address and keys, shared by its models.
fn provider(
    ui: &mut Ui,
    ai: &mut AiSettings,
    p: Provider,
    board: Option<&Board>,
    out: &mut Commands,
) {
    let Some(e) = ai.endpoint_mut(p) else {
        return;
    };
    section(ui, p.label());
    // Presets know their address; local and custom servers vary.
    let address = |ui: &mut Ui, url: &mut String| {
        row(ui, "Address", "Up to and including /v1", |ui| {
            let hint = match p.default_url() {
                "" => "https://host/v1/",
                url => url,
            };
            field(ui, TextEdit::singleline(url).hint_text(hint));
        });
    };
    if !p.needs_key() {
        group(ui, |ui| address(ui, &mut e.url));
    }
    if p.takes_keys() {
        keys(ui, p, board, out);
    }
    if p.needs_key() {
        egui::CollapsingHeader::new("Advanced")
            .id_salt(("ai-advanced", p.scope()))
            .show(ui, |ui| group(ui, |ui| address(ui, &mut e.url)));
    }
}

/// Whether the server's list lacks `model`; Ollama may add ":latest".
fn missing(model: &str, models: &Load<Vec<String>>) -> bool {
    let model = model.trim();
    let Load::Ready(list) = models else {
        return false;
    };
    !model.is_empty() && !list.iter().any(|m| same(model, m))
}

/// Whether `model` names `listed`; Ollama reads "x" as "x:latest".
fn same(model: &str, listed: &str) -> bool {
    let model = model.trim();
    listed == model || listed.strip_suffix(":latest") == Some(model)
}

/// Asks the server for its models again; a spinner while it answers.
fn refresh(ui: &mut Ui, p: Provider, board: &Board, out: &mut Commands) {
    if board.fetching(p) {
        let r = ui.add_sized(vec2(24.0, 24.0), egui::Spinner::new().size(12.0));
        widgets::name(&r, "Refreshing models");
    } else if widgets::small_icon_button(ui, Icon::Rescan, "Refresh models").clicked() {
        out.push(Command::Ai(Job::Models(p)));
    }
}

/// The server's models, fetched when the menu first opens.
fn model_menu(
    ui: &mut Ui,
    p: Provider,
    model: &mut String,
    models: &Load<Vec<String>>,
    out: &mut Commands,
) {
    let menu = ui.menu_button("Choose", |ui| {
        ui.set_max_width(FIELD_W);
        match models {
            Load::Idle | Load::Busy => {
                ui.label("Loading models…");
            }
            Load::Failed(f) => {
                failure(ui, f);
                if ui.button("Try again").clicked() {
                    out.push(Command::Ai(Job::Models(p)));
                }
            }
            Load::Ready(list) if list.is_empty() => {
                ui.label("The server lists no models.");
            }
            Load::Ready(list) => {
                ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for id in list {
                        if ui.selectable_label(same(model, id), id).clicked() {
                            model.clone_from(id);
                            ui.close();
                        }
                    }
                });
            }
        }
    });
    if menu.response.clicked() && matches!(models, Load::Idle | Load::Failed(_)) {
        out.push(Command::Ai(Job::Models(p)));
    }
}

fn keys(ui: &mut Ui, p: Provider, board: Option<&Board>, out: &mut Commands) {
    let Some(board) = board else {
        note(ui, "API keys show here once the AI service is running.");
        return;
    };
    if *board.keys(p) == Load::Idle {
        out.push(Command::Ai(Job::Keys(p)));
    }
    group(ui, |ui| {
        match board.keys(p) {
            Load::Ready(rows) => {
                for r in rows {
                    key_row(ui, p, r, out);
                    divider(ui);
                }
            }
            Load::Failed(f) => {
                ui.add_space(8.0);
                failure(ui, f);
                ui.add_space(8.0);
                divider(ui);
            }
            Load::Idle | Load::Busy => {}
        }
        add_key(ui, p, out);
    });
    if let Some(f) = board.problems.get(&p) {
        ui.add_space(6.0);
        failure(ui, f);
    }
    let text = match p {
        Provider::Custom => "Only if the server asks for one. ",
        _ => "",
    };
    note(
        ui,
        &format!(
            "{text}Keys are encrypted on this computer. With several, requests \
             take turns, and a key the provider turns away is set aside."
        ),
    );
}

fn key_row(ui: &mut Ui, p: Provider, r: &KeyRow, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let (state, banned) = match &r.state {
        KeyState::Ready => ("Ready".to_owned(), false),
        KeyState::Resting { secs } => (format!("Resting, {secs} s"), false),
        KeyState::Banned { reason } => (format!("Set aside: {reason}"), true),
    };
    row(ui, &r.shown, &state, |ui| {
        let job = |id: &str| (p, id.to_owned());
        if widgets::small_icon_button(ui, Icon::Remove, "Remove key").clicked() {
            let (p, id) = job(&r.id);
            out.push(Command::Ai(Job::RemoveKey(p, id)));
        }
        let (icon, label) = match banned {
            true => (Icon::Restore, "Use again"),
            false => (Icon::Ban, "Set aside"),
        };
        if widgets::small_icon_button(ui, icon, label).clicked() {
            let (p, id) = job(&r.id);
            out.push(Command::Ai(match banned {
                true => Job::RecoverKey(p, id),
                false => Job::BanKey(p, id),
            }));
        }
        let color = if banned { t.danger } else { t.muted };
        ui.label(
            RichText::new(state.as_str())
                .size(t.text_small)
                .color(color),
        );
    });
}

/// A field for a new key; it empties once the key is sent.
fn add_key(ui: &mut Ui, p: Provider, out: &mut Commands) {
    let id = egui::Id::new(("ai-new-key", p.scope()));
    let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), super::ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            let t = Tokens::get(ui.ctx());
            ui.add(Icon::Key.image(15.0).tint(t.muted));
            let edit = TextEdit::singleline(&mut text)
                .password(true)
                .hint_text("Paste an API key");
            let field = field(ui, edit);
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let add = ui.add_enabled(!text.trim().is_empty(), egui::Button::new("Add"));
            if (add.clicked() || entered) && !text.trim().is_empty() {
                out.push(Command::Ai(Job::AddKey(p, std::mem::take(&mut text))));
            }
        },
    );
    ui.data_mut(|d| d.insert_temp(id, text));
}

/// A text field outlined like the appearance menu's.
fn field(ui: &mut Ui, edit: TextEdit<'_>) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let frame = egui::Frame::new()
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(8, 4));
    ui.add(edit.frame(frame).desired_width(FIELD_W - 16.0))
}

fn failure(ui: &mut Ui, f: &Failure) {
    let t = Tokens::get(ui.ctx());
    ui.vertical(|ui| {
        ui.label(RichText::new(f.why.message()).color(t.danger));
        if let Some(detail) = &f.detail {
            ui.label(RichText::new(detail).size(t.text_small).color(t.muted));
        }
    });
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use mareader_assist::Reply;

    use super::*;

    #[test]
    fn a_model_off_the_list_is_missing() {
        let list = Load::Ready(vec!["llama3.2:latest".to_owned()]);
        assert!(!missing("llama3.2", &list));
        assert!(!missing("llama3.2:latest", &list));
        assert!(!missing("", &list));
        assert!(missing("qwen3:4b", &list));
        // Unknown until the server answers.
        assert!(!missing("qwen3:4b", &Load::Busy));
    }

    struct State {
        ai: AiSettings,
        board: Board,
        out: Commands,
        shown: bool,
        fonts: bool,
    }

    fn asked(s: &State) -> usize {
        let models = |c: &&Command| matches!(c, Command::Ai(Job::Models(_)));
        s.out.iter().filter(models).count()
    }

    fn harness(ai: AiSettings) -> Harness<'static, State> {
        let state = State {
            ai,
            board: Board::default(),
            out: Vec::new(),
            shown: true,
            fonts: false,
        };
        Harness::new_ui_state(
            |ui, s: &mut State| {
                // Named fonts work from the frame after they're installed.
                if !s.fonts {
                    crate::theme::install_fonts(ui.ctx());
                    s.fonts = true;
                    ui.ctx().request_repaint();
                } else if s.shown {
                    let live = Live {
                        board: &s.board,
                        in_use: &|_| 0,
                    };
                    tab(ui, &mut s.ai, Some(live), &mut s.out);
                }
            },
            state,
        )
    }

    fn no_models() -> AiSettings {
        AiSettings {
            presets: Vec::new(),
            tasks: Default::default(),
            ..AiSettings::default()
        }
    }

    #[test]
    fn opening_the_tab_asks_ollama_for_its_models() {
        let mut ai = no_models();
        ai.add_preset(Provider::Ollama, "llama3:8b");
        let mut h = harness(ai);
        h.step();
        h.step();
        // This board never starts the job, so each frame asks again.
        assert!(asked(h.state()) > 0);
        let p = Provider::Ollama;
        let s = h.state_mut();
        s.board.start(&Job::Models(p));
        let list = vec!["qwen3:4b".to_owned()];
        s.board.finish(Reply::Models(p, Ok(list)));
        s.out.clear();
        h.step();
        h.step();
        assert_eq!(asked(h.state()), 0, "no refetch while the tab stays open");
        // Opened, the model says it's gone from the server.
        h.get_by_label("Ollama · llama3:8b").click();
        h.step();
        h.step();
        h.get_by_label("Refresh models");
        h.get_by_label("llama3:8b isn't on the server now; choose another.");
        h.state_mut().shown = false;
        h.step();
        h.state_mut().shown = true;
        h.step();
        assert_eq!(asked(h.state()), 1, "reopening fetches again");
    }

    #[test]
    fn a_named_model_keeps_its_name_and_an_unnamed_one_follows_its_model() {
        let mut ai = no_models();
        let id = ai.add_preset(Provider::OpenAi, "");
        let mut p = ai.preset(id).cloned().unwrap_or_default();
        renaming(&mut p, |p| p.model = "gpt-4.1-nano".into());
        assert_eq!(p.name, "OpenAI · gpt-4.1-nano");
        renaming(&mut p, |p| p.provider = Provider::OpenRouter);
        assert_eq!(p.name, "OpenRouter · gpt-4.1-nano");
        p.name = "Fast".into();
        renaming(&mut p, |p| p.model = "gpt-4.1".into());
        assert_eq!(p.name, "Fast");
    }

    #[test]
    fn the_first_model_added_explains_words() {
        let mut h = harness(no_models());
        h.step();
        h.step();
        h.get_by_label("+  Add a model").click();
        h.step();
        h.step();
        h.get_by_label("Custom server").click();
        h.step();
        h.step();
        let ai = &h.state().ai;
        let made = ai.explain_preset().map(|p| p.provider);
        assert_eq!(made, Some(Provider::Custom));
        // Its provider's keys show below, and it opened.
        h.get_by_label("Add");
        h.get_by_label("Try it");
    }
}
