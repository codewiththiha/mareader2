//! The whole app, headless: title bar, pin, panes and restyling.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_ui::MareaderApp;

fn harness(app: MareaderApp) -> Harness<'static, MareaderApp> {
    sized(app, egui::vec2(1200.0, 800.0))
}

fn sized(app: MareaderApp, size: egui::Vec2) -> Harness<'static, MareaderApp> {
    let mut h = Harness::builder()
        .with_size(size)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(64)
        .build_eframe(|_| app);
    h.run_steps(3);
    h
}

#[test]
fn the_bar_appears_on_hover_and_hides_after_the_grace() {
    let mut h = harness(MareaderApp::default());
    assert!(!h.state().titlebar().visible());
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(2);
    assert!(h.state().titlebar().visible());
    h.hover_at(egui::pos2(600.0, 400.0));
    h.run_steps(2);
    assert!(h.state().titlebar().visible(), "the 400 ms grace keeps it");
    for _ in 0..40 {
        h.step();
    }
    assert!(!h.state().titlebar().visible());
}

#[test]
fn split_right_from_the_bar_adds_and_focuses_a_pane() {
    let mut h = harness(MareaderApp::default());
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(8);
    h.get_by_label("Split right").click();
    h.run_steps(2);
    assert_eq!(h.state().panes().len(), 2);
    assert_eq!(h.state().active_pane(), h.state().panes()[1]);
}

#[test]
fn a_pinned_bar_stays_after_the_pointer_leaves() {
    let mut app = MareaderApp::default();
    app.set_option("pinned", "true")
        .map_err(|e| e.to_string())
        .ok();
    let mut h = harness(app);
    h.hover_at(egui::pos2(600.0, 500.0));
    for _ in 0..40 {
        h.step();
    }
    assert!(h.state().titlebar().visible());
}

#[test]
fn the_pin_button_keeps_the_bar_after_the_pointer_leaves() {
    let mut h = harness(MareaderApp::default());
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(8);
    h.get_by_label("Pin title bar").click();
    h.run_steps(2);
    h.hover_at(egui::pos2(600.0, 500.0));
    for _ in 0..40 {
        h.step();
    }
    assert!(h.state().titlebar().visible());
}

#[test]
fn closing_a_pane_from_its_button_focuses_the_survivor() {
    let mut app = MareaderApp::default();
    app.set_option("panes", "2").map_err(|e| e.to_string()).ok();
    let mut h = harness(app);
    let survivor = h.state().panes()[0];
    h.hover_at(egui::pos2(900.0, 400.0));
    h.run_steps(2);
    h.get_by_label("Close pane").click();
    h.run_steps(2);
    assert_eq!(h.state().panes(), vec![survivor]);
    assert_eq!(h.state().active_pane(), survivor);
}

#[test]
fn an_appearance_change_restyles_on_the_frame_it_lands() {
    let mut h = harness(MareaderApp::default());
    let sepia = mareader_appearance::builtin_presets()
        .into_iter()
        .find(|p| p.id == "sepia")
        .map(|p| p.appearance)
        .unwrap_or_default();
    let want = mareader_ui::theme::Tokens::new(&sepia);
    h.state_mut()
        .queue(mareader_ui::command::Command::SetAppearance(sepia));
    h.step();
    assert_eq!(mareader_ui::theme::Tokens::get(&h.ctx), want);
    assert_eq!(h.ctx.global_style().visuals.panel_fill, want.surface);
}

fn open_appearance(h: &mut Harness<'static, MareaderApp>) {
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(2);
    h.get_by_label("Appearance").click();
    h.run_steps(2);
}

#[test]
fn the_appearance_popover_switches_mode_and_keeps_the_bar() {
    let mut h = harness(MareaderApp::default());
    open_appearance(&mut h);
    h.get_by_label("Dark").click();
    h.run_steps(2);
    let base = h.state().settings().appearance.base;
    assert_eq!(base, mareader_appearance::BaseMode::Dark);
    assert!(mareader_ui::theme::Tokens::get(&h.ctx).dark);
    h.hover_at(egui::pos2(600.0, 500.0));
    h.run_steps(40);
    assert!(
        h.state().titlebar().visible(),
        "an open popover holds the bar"
    );
}

#[test]
fn a_preset_swatch_applies_its_look() {
    let mut h = harness(MareaderApp::default());
    open_appearance(&mut h);
    h.get_by_label("Sepia").click();
    h.run_steps(2);
    let sepia = mareader_appearance::builtin_presets()
        .into_iter()
        .find(|p| p.id == "sepia")
        .map(|p| p.appearance);
    assert_eq!(Some(h.state().settings().appearance), sepia);
}

fn with(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    for (k, v) in options {
        app.set_option(k, v).map_err(|e| e.to_string()).ok();
    }
    harness(app)
}

#[test]
fn the_bar_toggle_docks_the_sidebar_and_hands_closing_to_it() {
    let mut h = harness(MareaderApp::default());
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(8);
    h.get_by_label("Toggle sidebar").click();
    h.run_steps(30);
    assert!(h.state().sidebar().is_open());
    assert!(h.query_by_label("Toggle sidebar").is_none());
    h.get_by_label("Close sidebar").click();
    h.run_steps(30);
    assert!(!h.state().sidebar().is_open());
}

#[test]
fn the_bar_band_starts_after_a_docked_sidebar() {
    let mut h = with(&[("sidebar", "outline")]);
    h.hover_at(egui::pos2(150.0, 20.0));
    h.run_steps(4);
    assert!(
        !h.state().titlebar().visible(),
        "the rail's header is not the band"
    );
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(4);
    assert!(h.state().titlebar().visible());
}

#[test]
fn the_switcher_changes_panel_and_a_reclick_keeps_it() {
    use egui::accesskit::Role;
    use mareader_ui::sidebar::Panel;
    let mut h = with(&[("sidebar", "thumbnails")]);
    h.get_by_role_and_label(Role::Button, "Outline").click();
    h.run_steps(2);
    assert_eq!(h.state().sidebar().panel(), Panel::Outline);
    h.get_by_role_and_label(Role::Button, "Outline").click();
    h.run_steps(2);
    assert!(h.state().sidebar().is_open());
    assert_eq!(h.state().sidebar().panel(), Panel::Outline);
}

#[test]
fn a_floating_sidebar_opens_from_the_edge_and_closes_after_the_grace() {
    let mut h = with(&[("float", "true")]);
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(8);
    assert!(h.query_by_label("Toggle sidebar").is_none());
    h.hover_at(egui::pos2(2.0, 400.0));
    h.run_steps(4);
    assert!(h.state().sidebar().is_open());
    h.hover_at(egui::pos2(150.0, 400.0));
    h.run_steps(30);
    assert!(h.state().sidebar().is_open(), "on the rail it stays");
    h.hover_at(egui::pos2(800.0, 400.0));
    h.run_steps(4);
    assert!(h.state().sidebar().is_open(), "the grace keeps it a moment");
    h.run_steps(30);
    assert!(!h.state().sidebar().is_open());
}

#[test]
fn docking_a_floating_sidebar_keeps_it_open() {
    let mut h = with(&[("float", "true"), ("sidebar", "outline")]);
    h.hover_at(egui::pos2(150.0, 400.0));
    h.run_steps(4);
    h.get_by_label("Dock sidebar").click();
    h.run_steps(4);
    assert!(!h.state().settings().chrome.sidebar_overlay);
    assert!(h.state().sidebar().is_open());
}

#[test]
fn escape_closes_the_sidebar() {
    let mut h = with(&[("sidebar", "outline")]);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!h.state().sidebar().is_open());
}

#[test]
fn a_command_schedules_the_frame_that_shows_it() {
    let mut h = with(&[("float", "true"), ("sidebar", "outline")]);
    h.run_steps(2);
    h.state_mut()
        .queue(mareader_ui::command::Command::CloseSidebar);
    // `run` steps only while the app asks for frames, like a real window.
    h.run();
    assert!(h.query_by_label("Close sidebar").is_none(), "faded out");
}

fn open_settings(h: &mut Harness<'static, MareaderApp>) {
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(8);
    // The bar's gear; a docked sidebar's header has one too.
    if let Some(gear) = h.query_all_by_label("Settings").last() {
        gear.click();
    }
    h.run_steps(4);
}

#[test]
fn the_gear_opens_settings_and_escape_closes_only_them() {
    let mut h = with(&[("sidebar", "outline")]);
    open_settings(&mut h);
    assert!(h.state().settings_window().is_open());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!h.state().settings_window().is_open());
    assert!(h.state().sidebar().is_open(), "Escape was the window's");
}

#[test]
fn a_settings_switch_applies_on_the_frame_it_flips() {
    use egui::accesskit::Role;
    let mut h = with(&[("settings", "layout")]);
    h.get_by_role_and_label(Role::CheckBox, "Overlay sidebar")
        .click();
    h.step();
    h.step();
    assert!(h.state().settings().chrome.sidebar_overlay);
}

#[test]
fn the_glass_switch_starts_on_and_turns_it_off() {
    use egui::accesskit::Role;
    let mut h = with(&[("settings", "layout")]);
    assert!(h.state().settings().chrome.glass);
    h.get_by_role_and_label(Role::CheckBox, "Frosted glass")
        .click();
    h.step();
    h.step();
    assert!(!h.state().settings().chrome.glass);
}

#[test]
fn the_animations_tab_leaves_with_its_master_switch() {
    use egui::accesskit::Role;
    use mareader_ui::settings_window::Tab;
    let mut h = with(&[("settings", "animations")]);
    assert!(
        h.query_by_role_and_label(Role::CheckBox, "Sidebar animation")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Layout").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "Animations")
        .click();
    h.run_steps(2);
    assert!(
        h.query_by_role_and_label(Role::Button, "Animations")
            .is_none()
    );
    let cx = h.state().window_context();
    let tab = h.state().settings_window().tab(h.state().settings(), &cx);
    assert_eq!(tab, Tab::Layout);
}

#[test]
fn split_pane_outline_colour_comes_from_the_theme_tab() {
    let mut h = with(&[("panes", "2"), ("settings", "theme")]);
    h.get_by_label("Red").click();
    h.run_steps(2);
    let look = h.state().settings().workspace.look;
    assert_eq!(look.outline_color, mareader_settings::Swatch::Red);
}

/// eframe's storage, minus the disk.
#[derive(Default)]
struct Memory(std::collections::HashMap<String, String>);

impl eframe::Storage for Memory {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }

    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }

    fn flush(&mut self) {}
}

#[test]
fn settings_survive_a_save_and_relaunch() {
    use eframe::App as _;
    let mut h = with(&[("float", "true"), ("theme", "dim")]);
    let mut disk = Memory::default();
    h.state_mut().save(&mut disk);
    let back = MareaderApp::from_storage(Some(&disk));
    assert_eq!(back.settings(), h.state().settings());
    assert!(back.settings().chrome.sidebar_overlay);
}

#[test]
fn the_pre_crate_settings_blob_still_loads() {
    let mut disk = Memory::default();
    let v0 = r#"{"titlebar_pinned":true,"animations":false,"sidebar_overlay":true}"#;
    disk.0
        .insert(mareader_ui::app::SETTINGS_KEY.to_owned(), v0.to_owned());
    let app = MareaderApp::from_storage(Some(&disk));
    assert!(app.titlebar().pinned && app.settings().chrome.sidebar_overlay);
    assert!(!app.settings().motion.enabled);
    assert!(app.settings().chrome.glass, "glass is new, so it starts on");
}

#[test]
fn open_appearance_from_settings_swaps_the_window_for_the_popover() {
    let mut h = with(&[("settings", "theme")]);
    h.get_by_label("Open Appearance").click();
    h.run_steps(3);
    assert!(!h.state().settings_window().is_open());
    assert!(egui::Popup::is_id_open(
        &h.ctx,
        egui::Id::new(mareader_ui::titlebar::APPEARANCE_POPOVER)
    ));
}

/// Steps `secs` of app time at 60 frames a second.
fn wait(h: &mut Harness<'static, MareaderApp>, secs: f32) {
    h.run_steps((secs * 60.0).ceil() as usize);
}

#[test]
fn a_toolbar_tooltip_names_the_control_and_its_shortcut() {
    let mut h = with(&[("platform", "gnome")]);
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(8);
    h.get_by_label("Settings").hover();
    wait(&mut h, 0.6);
    assert!(
        h.query_by_label("Ctrl+,").is_some(),
        "the shortcut rides in the tooltip"
    );
}

#[test]
fn a_narrow_bar_folds_tools_into_the_menu() {
    let mut app = MareaderApp::default();
    app.set_option("platform", "windows").ok();
    let mut h = sized(app, egui::vec2(640.0, 600.0));
    h.hover_at(egui::pos2(320.0, 20.0));
    h.run_steps(8);
    assert!(h.query_by_label("Search").is_some(), "the first tools stay");
    assert!(
        h.query_by_label("Split right").is_none(),
        "the last ones fold"
    );
    h.get_by_label("View & tools").click();
    h.run_steps(3);
    h.get_by_label("Split right").click();
    h.run_steps(3);
    assert_eq!(h.state().panes().len(), 2, "a folded tool still works");
}

#[test]
fn a_wide_bar_keeps_every_tool_out_of_the_menu() {
    let mut h = with(&[("platform", "windows")]);
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(8);
    h.get_by_label("View & tools").click();
    h.run_steps(3);
    assert_eq!(h.query_all_by_label("Split right").count(), 1, "bar only");
    assert!(h.query_by_label("Full screen").is_some());
}

#[test]
fn the_menu_lists_the_shortcuts_that_exist() {
    let mut h = with(&[("platform", "gnome")]);
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(8);
    h.get_by_label("View & tools").click();
    h.run_steps(3);
    assert!(h.query_by_label("Esc").is_none(), "folded until asked");
    h.get_by_label("Keyboard shortcuts").click();
    h.run_steps(3);
    assert!(h.query_by_label("Esc").is_some());
    assert!(h.query_by_label("F11").is_some());
}

#[test]
fn command_comma_opens_settings() {
    let mut h = harness(MareaderApp::default());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    h.run_steps(3);
    assert!(h.state().settings_window().is_open());
}

#[test]
fn a_toast_leaves_by_itself() {
    let mut h = with(&[("toast", "Copied")]);
    assert!(h.query_by_label("Copied").is_some());
    wait(&mut h, 3.0);
    assert!(h.state().toast().is_some(), "still up before 3.5 s");
    wait(&mut h, 1.0);
    assert!(h.state().toast().is_none());
    assert!(h.query_by_label("Copied").is_none());
}

#[test]
fn reading_a_toast_keeps_it_up() {
    let mut h = with(&[("toast", "Copied")]);
    h.get_by_label("Copied").hover();
    wait(&mut h, 6.0);
    assert!(h.state().toast().is_some(), "hover holds it");
    h.hover_at(egui::pos2(600.0, 300.0));
    wait(&mut h, 2.0);
    assert!(h.state().toast().is_none(), "gone after the grace");
}

#[test]
fn a_toast_closes_from_its_button() {
    let mut h = with(&[("toast", "error:Could not open the file")]);
    h.get_by_label("Dismiss").click();
    wait(&mut h, 0.5);
    assert!(h.state().toast().is_none());
}

#[test]
fn a_refused_split_says_why() {
    use mareader_workspace::SplitAxis;
    let mut h = with(&[("panes", "4")]);
    h.state_mut()
        .queue(mareader_ui::command::Command::Split(SplitAxis::Horizontal));
    h.run_steps(3);
    let toast = h.state().toast().map(|t| t.message.clone());
    assert!(toast.is_some_and(|m| m.contains("4 panes")));
}

#[test]
fn repaired_settings_say_so() {
    let mut disk = Memory::default();
    let bad = r#"{"version":1,"motion":{"zoom":"often"}}"#;
    disk.0
        .insert(mareader_ui::app::SETTINGS_KEY.to_owned(), bad.to_owned());
    let app = MareaderApp::from_storage(Some(&disk));
    let tone = app.toast().map(|t| t.tone);
    assert_eq!(tone, Some(mareader_ui::toast::Tone::Error));
}

fn lights(h: &Harness<'static, MareaderApp>) -> mareader_ui::platform::NativeLights {
    h.state().native_lights()
}

#[test]
fn an_open_sidebar_keeps_the_lights_while_the_bar_hides() {
    let mut h = with(&[("platform", "mac"), ("sidebar", "outline")]);
    h.hover_at(egui::pos2(700.0, 500.0));
    wait(&mut h, 1.0);
    assert!(!h.state().titlebar().visible());
    assert_eq!((lights(&h).shift, lights(&h).opacity), (0.0, 1.0));
}

#[test]
fn closing_the_sidebar_slides_the_lights_away() {
    let mut h = with(&[("platform", "mac"), ("sidebar", "outline")]);
    h.hover_at(egui::pos2(700.0, 500.0));
    wait(&mut h, 1.0);
    h.state_mut()
        .queue(mareader_ui::command::Command::CloseSidebar);
    wait(&mut h, 0.15);
    let mid = lights(&h);
    assert!(
        mid.shift < 0.0 && mid.shift > -mareader_ui::sidebar::WIDTH,
        "{mid:?}"
    );
    assert_eq!(mid.opacity, 1.0, "they ride the rail, no fade");
    wait(&mut h, 0.5);
    assert_eq!(lights(&h).opacity, 0.0, "gone with it; the bar is hidden");
}

#[test]
fn without_a_sidebar_the_lights_come_with_the_bar() {
    let mut h = with(&[("platform", "mac")]);
    h.hover_at(egui::pos2(700.0, 500.0));
    wait(&mut h, 1.0);
    assert_eq!(lights(&h).opacity, 0.0);
    h.hover_at(egui::pos2(600.0, 20.0));
    wait(&mut h, 0.5);
    assert_eq!((lights(&h).shift, lights(&h).opacity), (0.0, 1.0));
}

#[test]
fn in_overlay_mode_the_lights_slide_in_with_the_rail_only() {
    let mut h = with(&[("platform", "mac"), ("float", "true")]);
    h.hover_at(egui::pos2(600.0, 20.0));
    wait(&mut h, 0.5);
    assert!(h.state().titlebar().visible());
    assert_eq!(lights(&h).opacity, 0.0, "the bar never hosts them here");
    h.hover_at(egui::pos2(2.0, 400.0));
    wait(&mut h, 0.12);
    h.hover_at(egui::pos2(100.0, 400.0));
    let mid = lights(&h);
    assert!(mid.shift < 0.0 && mid.opacity == 1.0, "{mid:?}");
    wait(&mut h, 0.5);
    assert_eq!((lights(&h).shift, lights(&h).opacity), (0.0, 1.0));
}

#[test]
fn the_overlay_bar_leaves_no_gutter_for_lights() {
    let mut h = with(&[("platform", "mac"), ("float", "true")]);
    h.hover_at(egui::pos2(600.0, 20.0));
    wait(&mut h, 0.5);
    assert!(h.get_by_label("Library").rect().left() < 20.0);
    let mut h = with(&[("platform", "mac")]);
    h.hover_at(egui::pos2(600.0, 20.0));
    wait(&mut h, 0.5);
    assert!(h.get_by_label("Toggle sidebar").rect().left() >= 80.0);
}

#[test]
fn the_floating_rail_slides_in_solid_not_fading() {
    let mut h = with(&[("float", "true"), ("platform", "gnome")]);
    h.hover_at(egui::pos2(2.0, 400.0));
    wait(&mut h, 0.15);
    let shift = h.state().sidebar().offset().unwrap_or(0.0);
    assert!(shift < -20.0, "still on its way in: {shift}");
    let image = h.render().map_err(|e| e.to_string());
    let Ok(image) = image else {
        return;
    };
    // Inside the visible rail: its own surface, not blended with the page.
    let x = (mareader_ui::sidebar::WIDTH + shift - 10.0) as u32;
    let [r, g, b, _] = image.get_pixel(x, 400).0;
    let [pr, pg, pb, _] = image.get_pixel(900, 400).0;
    assert_ne!((r, g, b), (pr, pg, pb), "the rail shows");
    let settled = {
        h.hover_at(egui::pos2(100.0, 400.0));
        wait(&mut h, 0.6);
        h.render()
            .map(|i| i.get_pixel(278, 400).0)
            .unwrap_or([r, g, b, 255])
    };
    assert_eq!([r, g, b], settled[..3], "same colour mid-slide as at rest");
}
