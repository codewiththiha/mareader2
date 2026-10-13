//! Importing a mareader settings blob into this schema.

use mareader_appearance::Rgb;
use serde_json::{Map, Value, json};

use crate::load::{Loaded, finish, put};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportError {
    NotJson,
    NotMareader,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ImportError::NotJson => "not a JSON settings file",
            ImportError::NotMareader => "not a mareader settings file",
        })
    }
}

impl std::error::Error for ImportError {}

/// Reads mareader's `mareader.settings.v1` blob; bad values become defaults.
pub fn import_mareader(json: &str) -> Result<Loaded, ImportError> {
    let Ok(Value::Object(root)) = serde_json::from_str::<Value>(json) else {
        return Err(ImportError::NotJson);
    };
    if !looks_like_mareader(&root) {
        return Err(ImportError::NotMareader);
    }
    let mut notes = Vec::new();
    let root = to_current(root, &mut notes);
    let settings = finish(root, &mut notes);
    Ok(Loaded { settings, notes })
}

/// Fields only mareader wrote; our own blobs carry `version` instead.
pub(crate) fn looks_like_mareader(root: &Map<String, Value>) -> bool {
    let marks = [
        "layout",
        "tint_strength_halved",
        "gloss_color",
        "cefr_level",
        "dict",
    ];
    !root.contains_key("version")
        && (marks.iter().any(|k| root.contains_key(*k))
            || root.get("animations").is_some_and(Value::is_object))
}

/// Rewrites mareader's flat layout into our sections, as version 1.
pub(crate) fn to_current(
    mut root: Map<String, Value>,
    notes: &mut Vec<String>,
) -> Map<String, Value> {
    let mut out = Map::new();
    // mareader's one-shot gate: strengths saved before it use the old curve.
    let halve = root.get("tint_strength_halved") != Some(&Value::Bool(true));
    if let Some(mut look) = root.remove("appearance") {
        if halve {
            halve_tint(&mut look);
        }
        out.insert("appearance".to_owned(), look);
    }
    if let Some(Value::Array(mut presets)) = root.remove("user_presets") {
        for look in presets.iter_mut().filter_map(|p| p.get_mut("appearance")) {
            if halve {
                halve_tint(look);
            }
        }
        out.insert("user_presets".to_owned(), Value::Array(presets));
    }
    for key in ["titlebar_pinned", "library_titlebar_pinned"] {
        if let Some(v) = root.remove(key) {
            put(&mut out, "chrome", key, v);
        }
    }
    if let Some(v) = root.remove("default_zoom") {
        put(&mut out, "layout", "default_zoom", v);
    }
    let fit_checked = root.get("startup_fit_width") == Some(&Value::Bool(true));
    if let Some(Value::Object(layout)) = root.remove("layout") {
        for (key, v) in layout {
            match key.as_str() {
                "sidebar_overlay" => put(&mut out, "chrome", "sidebar_overlay", v),
                "blend_mode" => put(&mut out, "paper", "blend", v),
                "blend_area" => put(&mut out, "paper", "area", v),
                // `none` was never a real choice; old blobs predate Width.
                "default_fit" => {
                    let page = fit_checked && v == json!("page");
                    put(
                        &mut out,
                        "layout",
                        "startup_fit",
                        json!(if page { "page" } else { "width" }),
                    );
                }
                "floating_label_style" if v == json!("title") => {
                    put(&mut out, "layout", &key, json!("file_name"));
                }
                _ => put(&mut out, "layout", &key, v),
            }
        }
    }
    if let Some(motion) = root.remove("animations").filter(Value::is_object) {
        out.insert("motion".to_owned(), motion);
    }
    if let Some(Value::Object(mut text)) = root.remove("text") {
        if let Some(Value::String(align)) = text.get("column_align") {
            let align = align.to_lowercase();
            text.insert("column_align".to_owned(), json!(align));
        }
        out.insert("text".to_owned(), Value::Object(text));
    }
    study(&mut root, &mut out, notes);
    if let Some(dict) = root.remove("dict") {
        out.insert("dictionary".to_owned(), dict);
    }
    if let Some(Value::Object(ws)) = root.remove("workspace") {
        workspace(ws, &mut out, notes);
    }
    if root.remove("last_path").is_some_and(|v| !v.is_null()) {
        notes.push("last_path not imported; sessions track documents".to_owned());
    }
    out.insert("version".to_owned(), json!(1));
    out
}

fn halve_tint(look: &mut Value) {
    if let Some(v) = look.get("tint_strength").and_then(Value::as_u64) {
        look["tint_strength"] = json!(v.saturating_add(1) / 2);
    }
}

fn study(root: &mut Map<String, Value>, out: &mut Map<String, Value>, notes: &mut Vec<String>) {
    let mut gloss = Map::new();
    for (from, to) in [("gloss_color", "color"), ("gloss_opacity", "opacity")] {
        if let Some(v) = root.remove(from) {
            gloss.insert(to.to_owned(), v);
        }
    }
    if let Some(v) = root.remove("gloss_density") {
        gloss.insert("density".to_owned(), v);
    }
    let custom = root.remove("gloss_custom");
    if let Some(v) = custom.and_then(|v| hex(v, "gloss_custom", notes)) {
        gloss.insert("custom".to_owned(), v);
    }
    let mut cefr = Map::new();
    for (from, to) in [("cefr_enabled", "enabled"), ("cefr_level", "level")] {
        if let Some(v) = root.remove(from) {
            cefr.insert(to.to_owned(), v);
        }
    }
    if let Some(v) = root.remove("cefr_click_explain") {
        cefr.insert("click_explain".to_owned(), v);
    }
    put(out, "study", "gloss", Value::Object(gloss));
    put(out, "study", "cefr", Value::Object(cefr));
}

/// mareader's workspace block is camelCase with `pane*` look fields.
fn workspace(ws: Map<String, Value>, out: &mut Map<String, Value>, notes: &mut Vec<String>) {
    let mut look = Map::new();
    for (key, v) in ws {
        let snake = snake_case(&key);
        match snake.strip_prefix("pane_") {
            Some("outline_custom") => {
                if let Some(v) = hex(v, &key, notes) {
                    look.insert("outline_custom".to_owned(), v);
                }
            }
            Some(field @ ("outline_color" | "corners")) => {
                look.insert(field.to_owned(), json!(snake_value(&v)));
            }
            Some(field) => {
                look.insert(field.to_owned(), v);
            }
            None if snake == "library_click" => {
                put(out, "workspace", &snake, json!(snake_value(&v)));
            }
            None => put(out, "workspace", &snake, v),
        }
    }
    put(out, "workspace", "look", Value::Object(look));
}

fn snake_value(v: &Value) -> String {
    v.as_str().map(snake_case).unwrap_or_default()
}

fn snake_case(camel: &str) -> String {
    let mut out = String::with_capacity(camel.len() + 4);
    for c in camel.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Keeps a colour only when it is a real `#rrggbb`.
fn hex(v: Value, key: &str, notes: &mut Vec<String>) -> Option<Value> {
    let ok = v
        .as_str()
        .and_then(Rgb::from_hex)
        .map(|c| json!(c.to_hex()));
    if ok.is_none() {
        notes.push(format!("{key} is not a #rrggbb colour; default kept"));
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ColumnAlign, Corners, FloatingLabelStyle, LibraryClick, PaperArea, StartupFit, Swatch, load,
    };

    const BLOB: &str = r##"{
        "appearance": {"base": "dark", "tint_hue": 30, "tint_strength": 40},
        "user_presets": [{"id": "u1", "name": "Mine", "group": "User",
                          "appearance": {"tint_strength": 9}}],
        "default_zoom": 1.5,
        "last_path": "/books/a.pdf",
        "titlebar_pinned": true,
        "layout": {"default_fit": "none", "floating_label_style": "title",
                   "sidebar_overlay": true, "blend_mode": true, "blend_area": "edges",
                   "page_margin": 12},
        "animations": {"enabled": true, "zoom": false},
        "gloss_color": "violet", "gloss_custom": "#12345", "gloss_opacity": 0.8,
        "cefr_enabled": true, "cefr_level": "c1",
        "text": {"font_size": 20, "column_align": "Left", "serif_font": "system:georgia"},
        "dict": {"hover": false, "langs": ["mcfnlp-en-my"]},
        "workspace": {"libraryClick": "dragOnly", "independentThemes": true,
                      "paneOutlineWidth": 4, "paneOutlineColor": "green",
                      "paneOutlineCustom": "#abcdef", "paneGap": 8, "paneCorners": "rounded"}
    }"##;

    #[test]
    fn a_mareader_blob_maps_onto_every_section() {
        let got = import_mareader(BLOB).unwrap_or_default();
        let s = got.settings;
        assert_eq!(s.appearance.tint_strength, 20, "old curve halves");
        assert_eq!(
            s.user_presets.first().map(|p| p.appearance.tint_strength),
            Some(5)
        );
        assert!(s.chrome.titlebar_pinned && s.chrome.sidebar_overlay);
        assert_eq!(s.layout.default_zoom, 1.5);
        assert_eq!(s.layout.startup_fit, StartupFit::Width);
        assert_eq!(s.layout.floating_label_style, FloatingLabelStyle::FileName);
        assert_eq!(s.layout.page_margin, 12.0);
        assert!(s.paper.blend);
        assert_eq!(s.paper.area, PaperArea::Edges);
        assert!(s.motion.enabled && !s.motion.zoom);
        assert_eq!(s.study.gloss.color, Swatch::Custom);
        assert_eq!(
            s.study.gloss.custom.to_hex(),
            "#a58af0",
            "bad hex keeps default"
        );
        assert_eq!(s.study.gloss.opacity, 0.8);
        assert!(s.study.cefr.enabled);
        assert_eq!(s.text.font_size, 20.0);
        assert_eq!(s.text.column_align, ColumnAlign::Left);
        assert!(!s.dictionary.hover);
        assert_eq!(s.workspace.library_click, LibraryClick::DragOnly);
        assert!(s.workspace.independent_themes);
        let look = s.workspace.look;
        assert_eq!((look.outline_width, look.gap), (4.0, 8.0));
        assert_eq!(look.outline_color, Swatch::Green);
        assert_eq!(look.outline_custom.to_hex(), "#abcdef");
        assert_eq!(look.corners, Corners::Rounded);
        assert_eq!(got.notes.len(), 2, "{:?}", got.notes);
    }

    #[test]
    fn load_recognises_a_mareader_blob_too() {
        let s = load(BLOB).settings;
        assert_eq!(s.workspace.library_click, LibraryClick::DragOnly);
    }

    #[test]
    fn halved_blobs_keep_their_strength() {
        let json = r#"{"tint_strength_halved": true, "appearance": {"tint_strength": 40}}"#;
        let s = import_mareader(json).unwrap_or_default().settings;
        assert_eq!(s.appearance.tint_strength, 40);
    }

    #[test]
    fn our_own_blobs_are_refused() {
        let ours = crate::to_json(&crate::Settings::default());
        assert_eq!(import_mareader(&ours).err(), Some(ImportError::NotMareader));
        assert_eq!(import_mareader("[]").err(), Some(ImportError::NotJson));
    }
}
