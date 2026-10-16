//! The AI tab: who explains words, its model and address, and API keys.

use egui::{Align, Layout, RichText, ScrollArea, TextEdit, Ui, vec2};
use mareader_assist::{AppleState, Board, Job, KeyRow, KeyState, Load};
use mareader_settings::{AiSettings, Endpoint, Provider};
use mareader_study::Failure;

use super::{choice_row, divider, group, note, row, section};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

const FIELD_W: f32 = 260.0;

pub fn tab(ui: &mut Ui, ai: &mut AiSettings, board: Option<&Board>, out: &mut Commands) {
    let mac = cfg!(target_os = "macos");
    if mac && board.is_some_and(|b| b.apple == Load::Idle) {
        out.push(Command::Ai(Job::Probe));
    }
    section(ui, "Explain words with");
    group(ui, |ui| {
        let shown = Provider::ALL
            .into_iter()
            .filter(|p| mac || *p != Provider::Apple);
        for (i, p) in shown.enumerate() {
            if i > 0 {
                divider(ui);
            }
            let (title, detail) = describe(p, mac, board);
            if choice_row(ui, title, &detail, ai.provider == p) {
                ai.provider = p;
            }
        }
    });
    let p = ai.provider;
    match (p, board) {
        (Provider::Auto | Provider::Apple, _) => {
            if mac || p == Provider::Apple {
                trial(ui, p, board, out);
            } else {
                note(ui, "Choose a provider to explain words.");
            }
        }
        (server, board) => {
            let label = server.label();
            let models = board.map(|b| b.models(server));
            if let Some(e) = ai.endpoint_mut(server) {
                server_rows(ui, server, label, e, models, out);
            }
            trial(ui, server, board, out);
            if server.takes_keys() {
                keys(ui, server, board, out);
            }
        }
    }
}

/// A provider's title and detail; Automatic means nothing off a Mac.
fn describe(p: Provider, mac: bool, board: Option<&Board>) -> (&'static str, String) {
    match p {
        Provider::Auto if !mac => ("None", "Explain stays off until you choose".to_owned()),
        Provider::Apple => {
            let state = board.and_then(|b| b.apple.ready());
            let detail = match state {
                Some(AppleState::Ready) => "Ready, on this Mac, private and offline",
                Some(AppleState::NotReady(_)) => "Getting ready; the model is downloading",
                Some(AppleState::Off(_)) => "Turn on Apple Intelligence in System Settings",
                // The reason tells a missing helper from an unsupported Mac.
                Some(AppleState::Missing(why)) if !why.is_empty() => {
                    return (p.label(), format!("Not available: {why}"));
                }
                Some(AppleState::Missing(_)) => "Not available on this Mac",
                None => p.detail(),
            };
            (p.label(), detail.to_owned())
        }
        _ => (p.label(), p.detail().to_owned()),
    }
}

fn server_rows(
    ui: &mut Ui,
    p: Provider,
    label: &str,
    e: &mut Endpoint,
    models: Option<&Load<Vec<String>>>,
    out: &mut Commands,
) {
    section(ui, label);
    group(ui, |ui| {
        row(ui, "Model", "Blank uses the default", |ui| {
            if let Some(models) = models {
                model_menu(ui, p, &mut e.model, models, out);
            }
            let hint = match p.default_model() {
                "" => "The server's first",
                m => m,
            };
            field(ui, TextEdit::singleline(&mut e.model).hint_text(hint));
        });
        // Presets know their address; local and custom servers vary.
        if !p.needs_key() {
            divider(ui);
            address(ui, p, e);
        }
    });
    if p.needs_key() {
        egui::CollapsingHeader::new("Advanced")
            .id_salt(("ai-advanced", p.scope()))
            .show(ui, |ui| group(ui, |ui| address(ui, p, e)));
    }
}

fn address(ui: &mut Ui, p: Provider, e: &mut Endpoint) {
    row(ui, "Address", "Up to and including /v1", |ui| {
        let hint = match p.default_url() {
            "" => "https://host/v1/",
            url => url,
        };
        field(ui, TextEdit::singleline(&mut e.url).hint_text(hint));
    });
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
                        if ui.selectable_label(*model == *id, id).clicked() {
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

/// Explains a sample word with the provider as set.
fn trial(ui: &mut Ui, p: Provider, board: Option<&Board>, out: &mut Commands) {
    let Some(board) = board else {
        return;
    };
    let t = Tokens::get(ui.ctx());
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        let busy = matches!(board.tried(p), Load::Busy);
        if ui.add_enabled(!busy, egui::Button::new("Try it")).clicked() {
            out.push(Command::Ai(Job::Try(p)));
        }
        match board.tried(p) {
            Load::Idle => {}
            Load::Busy => {
                ui.label(RichText::new("Asking…").color(t.muted));
            }
            Load::Ready(tried) => {
                let secs = tried.millis as f64 / 1000.0;
                let said = format!("Works, in {secs:.1} s: “{}”", tried.meaning);
                ui.label(RichText::new(said).color(t.muted));
            }
            Load::Failed(f) => failure(ui, f),
        }
    });
}

fn keys(ui: &mut Ui, p: Provider, board: Option<&Board>, out: &mut Commands) {
    let Some(board) = board else {
        note(ui, "API keys show here once the AI service is running.");
        return;
    };
    if *board.keys(p) == Load::Idle {
        out.push(Command::Ai(Job::Keys(p)));
    }
    section(ui, "API keys");
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
        .stroke(egui::Stroke::new(1.0, t.line))
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
