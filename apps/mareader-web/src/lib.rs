//! Browser entry point: the same app, for previewing work in progress.

#[cfg(target_arch = "wasm32")]
mod web {
    use mareader_ui::MareaderApp;
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::prelude::wasm_bindgen;

    /// `?preset=sepia&panes=2` style options, as the shot tool takes them.
    fn options() -> Vec<(String, String)> {
        let query = web_sys::window()
            .and_then(|w| w.location().search().ok())
            .unwrap_or_default();
        query
            .trim_start_matches('?')
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    #[wasm_bindgen(start)]
    pub fn start() {
        wasm_bindgen_futures::spawn_local(async {
            let canvas = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("mareader"))
                .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
            let Some(canvas) = canvas else {
                return;
            };
            let runner = eframe::WebRunner::new();
            let run = runner.start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| {
                    let mut app = MareaderApp::from_creation(cc);
                    for (k, v) in options() {
                        let _ = app.set_option(&k, &v);
                    }
                    Ok(Box::new(app))
                }),
            );
            if let Err(e) = run.await {
                web_sys::console::error_1(&e);
            }
        });
    }
}
