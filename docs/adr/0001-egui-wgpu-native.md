# ADR 0001: native egui + wgpu, one process

- **Status:** accepted, 2026-10-13
- **Context:** mareader is Tauri + Leptos/WASM + pdf.js. Each pane is an iframe
  with its own JS realm and WASM instance, which isolates memory per pane. The
  cost is a webview per window, pdf.js memory (about 3–15× native hayro in our
  benchmark) and a large lifecycle surface: realms, frame transport, disposal
  counters, Deep CI.
- **Decision:** one Rust process. eframe 0.36 with the wgpu renderer, egui for
  every surface, hayro for PDF and our own EPUB pipeline. PdfCraft proves this
  stack at Acrobat-class scale, with the same egui version and patterns
  (custom-painted widgets, kittest, headless shots).
- **Consequences:**
  - Memory is owned by Rust values, so teardown is `Drop` plus explicit job
    cancellation. The realm machinery has no counterpart.
  - CSS effects become code. `backdrop-filter` becomes a blur pass, CSS filters
    become the page shader, and transitions become `animate_*` with `motion::secs`.
  - No HTML. EPUB needs its own layout (ADR 0002).
  - The window chrome is ours on Windows and Linux; macOS keeps the native
    traffic lights.
