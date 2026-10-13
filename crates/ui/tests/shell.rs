//! The whole app, headless: title bar, pin, panes and restyling.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_ui::MareaderApp;

fn harness(app: MareaderApp) -> Harness<'static, MareaderApp> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
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
