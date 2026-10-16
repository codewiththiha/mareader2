//! Reading stored settings: migrate the JSON, then parse each section alone.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::{SCHEMA_VERSION, Settings};

/// Loaded settings plus what had to be repaired, for the log.
#[derive(Debug, Default)]
pub struct Loaded {
    pub settings: Settings,
    pub notes: Vec<String>,
}

/// Never fails: anything unreadable falls back to defaults, section by section.
pub fn load(json: &str) -> Loaded {
    let mut notes = Vec::new();
    let root = match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(root)) => root,
        Ok(_) | Err(_) => {
            notes.push("settings unreadable; using defaults".to_owned());
            return Loaded {
                settings: Settings::default(),
                notes,
            };
        }
    };
    let settings = finish(root, &mut notes);
    Loaded { settings, notes }
}

/// Brings a JSON object up to date, then parses it.
fn finish(mut root: Map<String, Value>, notes: &mut Vec<String>) -> Settings {
    migrate(&mut root, notes);
    parse_sections(root, notes)
}

/// Compact JSON in the current schema.
pub fn to_json(settings: &Settings) -> String {
    serde_json::to_string(settings).unwrap_or_default()
}

fn migrate(root: &mut Map<String, Value>, notes: &mut Vec<String>) {
    let stored = root.get("version").and_then(Value::as_u64).unwrap_or(0);
    if stored > u64::from(SCHEMA_VERSION) {
        notes.push(format!(
            "settings v{stored} are newer; unknown fields dropped"
        ));
    }
    if stored < 1 {
        v0_to_v1(root);
    }
    if stored < 2 {
        v1_to_v2(root);
    }
    if stored < 3 {
        v2_to_v3(root);
    }
    root.insert("version".to_owned(), json!(SCHEMA_VERSION));
}

/// v3 added Look up to the selection menu, after Explain if shown.
fn v2_to_v3(root: &mut Map<String, Value>) {
    let bar = root
        .get_mut("study")
        .and_then(|s| s.get_mut("highlights"))
        .and_then(|h| h.get_mut("bar"))
        .and_then(Value::as_array_mut);
    let Some(bar) = bar else {
        return;
    };
    let item = json!("look_up");
    if bar.contains(&item) {
        return;
    }
    let near = |name: &str| bar.iter().position(|v| v == name);
    let at = near("explain")
        .or_else(|| near("copy"))
        .map_or(0, |i| i + 1);
    bar.insert(at, item);
}

/// v2 moved Column Width to the text, so each pane may have its own.
fn v1_to_v2(root: &mut Map<String, Value>) {
    let layout = root.get_mut("layout").and_then(Value::as_object_mut);
    if let Some(v) = layout.and_then(|l| l.remove("column_width_pct")) {
        put(root, "text", "column_width_pct", v);
    }
}

/// v0 is the flat blob the shell saved before this crate existed.
fn v0_to_v1(root: &mut Map<String, Value>) {
    let moves = [
        ("titlebar_pinned", "chrome", "titlebar_pinned"),
        ("sidebar_overlay", "chrome", "sidebar_overlay"),
        ("animations", "motion", "enabled"),
        ("sidebar_slide", "motion", "sidebar_slide"),
    ];
    for (from, section, field) in moves {
        if let Some(v) = root.remove(from) {
            put(root, section, field, v);
        }
    }
    let Some(Value::Object(mut look)) = root.remove("workspace") else {
        return;
    };
    if let Some(Value::String(c)) = look.get("outline_color") {
        let c = c.to_lowercase();
        look.insert("outline_color".to_owned(), json!(c));
    }
    if let Some(Value::Array(rgb)) = look.remove("outline_custom") {
        let byte = |i: usize| rgb.get(i).and_then(Value::as_u64).unwrap_or(0).min(255);
        let hex = format!("#{:02x}{:02x}{:02x}", byte(0), byte(1), byte(2));
        look.insert("outline_custom".to_owned(), json!(hex));
    }
    if let Some(Value::Bool(rounded)) = look.remove("rounded") {
        let corners = if rounded { "rounded" } else { "square" };
        look.insert("corners".to_owned(), json!(corners));
    }
    put(root, "workspace", "look", Value::Object(look));
}

/// Sets `root[section][field]`, creating the section when missing.
fn put(root: &mut Map<String, Value>, section: &str, field: &str, v: Value) {
    let entry = root.entry(section).or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    if let Value::Object(map) = entry {
        map.insert(field.to_owned(), v);
    }
}

/// One bad section resets only itself, never its neighbours.
fn parse_sections(mut root: Map<String, Value>, notes: &mut Vec<String>) -> Settings {
    let mut s = Settings::default();
    macro_rules! sections {
        ($($field:ident),+) => {$(
            if let Some(v) = root.remove(stringify!($field)) {
                section(&mut s.$field, v, stringify!($field), notes);
            }
        )+};
    }
    sections!(
        appearance,
        user_presets,
        chrome,
        layout,
        paper,
        motion,
        text,
        study,
        dictionary,
        workspace,
        library
    );
    s.sanitize();
    s
}

fn section<T: DeserializeOwned>(slot: &mut T, v: Value, key: &str, notes: &mut Vec<String>) {
    match serde_json::from_value(v) {
        Ok(parsed) => *slot = parsed,
        Err(e) => notes.push(format!("settings.{key} reset to defaults: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Corners, Swatch};

    #[test]
    fn an_empty_object_is_all_defaults() {
        let got = load("{}");
        assert_eq!(got.settings, Settings::default());
        assert!(got.notes.is_empty());
    }

    #[test]
    fn garbage_falls_back_with_a_note() {
        let got = load("not json");
        assert_eq!(got.settings, Settings::default());
        assert_eq!(got.notes.len(), 1);
    }

    #[test]
    fn a_bad_section_resets_only_itself() {
        let json = r#"{"version":1,"motion":{"zoom":"yes"},"chrome":{"titlebar_pinned":true}}"#;
        let got = load(json);
        assert!(
            got.settings.chrome.titlebar_pinned,
            "the good section survives"
        );
        assert_eq!(got.settings.motion, Default::default());
        assert!(got.notes.iter().any(|n| n.contains("settings.motion")));
    }

    #[test]
    fn the_v0_shell_blob_migrates() {
        let v0 = r#"{
            "appearance": {"base": "dim"},
            "titlebar_pinned": true,
            "animations": false,
            "sidebar_overlay": true,
            "sidebar_slide": false,
            "workspace": {"gap": 6.0, "outline_width": 3.0, "outline_color": "Custom",
                          "outline_custom": [255, 0, 16], "rounded": true, "shadow": true}
        }"#;
        let got = load(v0);
        let s = got.settings;
        assert!(got.notes.is_empty(), "{:?}", got.notes);
        assert!(s.chrome.titlebar_pinned && s.chrome.sidebar_overlay);
        assert!(!s.motion.enabled && !s.motion.sidebar_slide);
        let look = s.workspace.look;
        assert_eq!((look.gap, look.outline_width), (6.0, 3.0));
        assert_eq!(look.outline_color, Swatch::Custom);
        assert_eq!(look.outline_custom.to_hex(), "#ff0010");
        assert_eq!(look.corners, Corners::Rounded);
        assert!(look.shadow);
    }

    #[test]
    fn v1_column_width_moves_to_the_text() {
        let v1 = r#"{"version":1,"layout":{"column_width_pct":80,"page_margin":8}}"#;
        let got = load(v1);
        assert!(got.notes.is_empty(), "{:?}", got.notes);
        assert_eq!(got.settings.text.column_width_pct, 80.0);
        assert_eq!(got.settings.layout.page_margin, 8.0);
        assert_eq!(got.settings.version, SCHEMA_VERSION);
    }

    #[test]
    fn v2_menus_gain_look_up_after_explain() {
        use crate::{BarItem, Pen};
        let v2 = r#"{"version":2,"study":{"highlights":{"bar":["explain",{"highlight":"full"},"copy"]}}}"#;
        let bar = load(v2).settings.study.highlights.bar;
        let want = [
            BarItem::Explain,
            BarItem::LookUp,
            BarItem::Highlight(Pen::Full),
            BarItem::Copy,
        ];
        assert_eq!(bar, want);
        let bare = r#"{"version":2,"study":{"highlights":{"bar":[]}}}"#;
        let bar = load(bare).settings.study.highlights.bar;
        assert_eq!(bar, [BarItem::LookUp]);
        let v3 = r#"{"version":3,"study":{"highlights":{"bar":["copy"]}}}"#;
        assert_eq!(load(v3).settings.study.highlights.bar, [BarItem::Copy]);
    }

    #[test]
    fn loading_clamps_out_of_range_values() {
        let json = r#"{"version":1,"text":{"font_size":400,"line_height":0.2}}"#;
        let text = load(json).settings.text;
        assert_eq!(text.font_size, 32.0);
        assert_eq!(text.line_height, 1.0);
    }
}
