//! `memory BOOK [ROUNDS] | --library N | --seeded N`: costs and budgets.

// Linux only: it reads `/proc/self/status`. See `docs/memory.md`.
// Exits 1 when a budget breaks.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use egui_kittest::Harness;
use mareader_ui::command::Command;
use mareader_ui::library::{Action, FolderKeep, FolderPlan, SearchAction, Walk};
use mareader_ui::pages::CAP_BYTES;
use mareader_ui::{MareaderApp, OpenRequest};
use mareader_workspace::{PaneId, SplitAxis};

type App = Harness<'static, MareaderApp>;

const MIB: f64 = 1024.0 * 1024.0;
/// The library's cover textures, as `covers.rs` holds them.
const COVER_BUDGET: usize = 64 << 20;
/// Growth from the second round to the last that still reads as reuse.
const ROUND_SLACK: f64 = 16.0;
/// What a second pane on an open book may add.
const SECOND_PANE: f64 = 8.0;
/// What each book in the library may hold, file or not.
const PER_BOOK_KIB: f64 = 16.0;

/// A `/proc/self/status` field, in MiB.
fn status(field: &str) -> Option<f64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = text.lines().find(|l| l.starts_with(field))?;
    let kib: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib / 1024.0)
}

/// Forgets the peak so far, so the next one is the next step's own.
fn reset_peak() {
    std::fs::write("/proc/self/clear_refs", "5").ok();
}

/// One row's numbers, kept for the budgets.
#[derive(Clone, Copy, Debug, Default)]
struct Row {
    rss: f64,
    documents: usize,
    pages: usize,
    page_bytes: usize,
    texts: usize,
    jobs: usize,
    searches: usize,
    cover_bytes: usize,
}

/// Every row so far, and the budgets checked against them.
#[derive(Default)]
struct Audit {
    rows: Vec<(String, Row)>,
    budgets: Vec<(String, String, bool)>,
}

impl Audit {
    fn header() {
        println!(
            "| Step | RSS MiB | Peak MiB | Docs | Pages (MiB) | Text layers | Jobs | Searches \
             | Covers MiB | egui textures |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|");
    }

    fn row(&mut self, h: &mut App, step: &str) -> Row {
        h.render().ok();
        let app = h.state();
        let m = app.documents().meter();
        let row = Row {
            rss: status("VmRSS:").unwrap_or(0.0),
            documents: m.documents,
            pages: m.pages,
            page_bytes: m.page_bytes,
            texts: m.texts,
            jobs: m.jobs,
            searches: app.searches(),
            cover_bytes: app.library_state().covers().bytes(),
        };
        let textures = h.ctx.tex_manager().read().num_allocated();
        let peak = status("VmHWM:").unwrap_or(0.0);
        println!(
            "| {step} | {:.0} | {peak:.0} | {} | {} ({:.0}) | {} | {} | {} | {:.1} | {textures} |",
            row.rss,
            row.documents,
            row.pages,
            row.page_bytes as f64 / MIB,
            row.texts,
            row.jobs,
            row.searches,
            row.cover_bytes as f64 / MIB,
        );
        reset_peak();
        self.rows.push((step.to_owned(), row));
        row
    }

    fn check(&mut self, budget: &str, value: String, held: bool) {
        self.budgets.push((budget.to_owned(), value, held));
    }

    /// The caps every row must keep, whatever the workload.
    fn caps(&mut self) {
        let pages = self.rows.iter().map(|(_, r)| r.page_bytes).max();
        let covers = self.rows.iter().map(|(_, r)| r.cover_bytes).max();
        let (pages, covers) = (pages.unwrap_or(0), covers.unwrap_or(0));
        let value = format!("{:.0} MiB at most", pages as f64 / MIB);
        self.check(
            "Page rasters within their 256 MiB cap",
            value,
            pages <= CAP_BYTES,
        );
        let value = format!("{:.1} MiB at most", covers as f64 / MIB);
        let held = covers <= COVER_BUDGET;
        self.check("Cover textures within their 64 MiB budget", value, held);
    }

    /// Prints the budgets; true when every one held.
    fn report(mut self) -> bool {
        self.caps();
        println!();
        println!("| Budget | Measured | Held |");
        println!("|---|---|---|");
        for (budget, value, held) in &self.budgets {
            let word = if *held { "yes" } else { "**no**" };
            println!("| {budget} | {value} | {word} |");
        }
        self.budgets.iter().all(|(_, _, held)| *held)
    }
}

fn harness(app: MareaderApp) -> App {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .wgpu()
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn open(h: &mut App, pane: PaneId, path: &Path) {
    let request = OpenRequest {
        pane: Some(pane),
        ..OpenRequest::path(path.to_path_buf())
    };
    h.state_mut().queue(Command::Open(request));
    h.run();
}

/// Every page in turn, each landing at once and drawn.
fn read_through(h: &mut App, pane: PaneId) {
    let count = h.state().documents().page_count(pane).unwrap_or(0);
    for page in 0..count {
        h.state_mut().queue(Command::GoTo { pane, page });
        h.run();
        h.render().ok();
    }
}

/// The find bar's search over every page, answered on this thread.
fn search(h: &mut App, query: &str) {
    h.state_mut().set_option("find", query).ok();
    h.state_mut().queue(Command::ShowFind);
    h.run_steps(30);
}

fn close(h: &mut App, pane: PaneId) {
    h.state_mut().queue(Command::ClosePane(pane));
    h.run();
}

/// Nothing the app counts is left of a closed book.
fn emptied(r: &Row) -> bool {
    r.documents + r.pages + r.texts + r.jobs + r.searches == 0
}

fn book(path: &Path, rounds: usize) -> Result<bool, String> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0")?;
    app.set_option("panes", "2")?;
    let mut s = app.settings().clone();
    // Jumps land at once: the read-through is about pages, not glides.
    s.motion.scroll_jumps = false;
    app.queue(Command::SetSettings(Box::new(s)));
    let mut h = harness(app);
    let mut audit = Audit::default();
    Audit::header();
    audit.row(&mut h, "idle, two empty panes");
    let panes = h.state().panes();
    let (left, right) = (panes[0], panes[1]);
    open(&mut h, right, path);
    audit.row(&mut h, "opened in one pane");
    read_through(&mut h, right);
    audit.row(&mut h, "every page read");
    search(&mut h, "the");
    audit.row(&mut h, "searched for \"the\"");
    close(&mut h, right);
    let closed = audit.row(&mut h, "its pane closed");
    let value = format!(
        "{} documents, {} searches",
        closed.documents, closed.searches
    );
    audit.check(
        "A closed book leaves nothing counted",
        value,
        emptied(&closed),
    );
    // A leak grows with each round; memory kept for reuse levels off.
    let mut kept = Vec::new();
    for round in 2..=rounds.max(2) {
        h.state_mut().queue(Command::Split(SplitAxis::Horizontal));
        h.run();
        let pane = h.state().active_pane();
        open(&mut h, pane, path);
        read_through(&mut h, pane);
        search(&mut h, "the");
        close(&mut h, pane);
        let row = audit.row(&mut h, &format!("round {round}: read, searched, closed"));
        kept.push(row);
    }
    if let (Some(first), Some(last)) = (kept.first(), kept.last()) {
        let grew = last.rss - first.rss;
        let value = format!("{grew:+.0} MiB over rounds 2 to {}", rounds.max(2));
        audit.check("Rounds level off (no leak)", value, grew <= ROUND_SLACK);
        let all = kept.iter().all(emptied);
        audit.check("Every round closes to nothing", "see rows".into(), all);
    }
    // The same book twice: the second pane should cost nearly nothing.
    h.state_mut().queue(Command::FocusPane(left));
    h.state_mut().queue(Command::Split(SplitAxis::Horizontal));
    h.run();
    let panes = h.state().panes();
    open(&mut h, panes[0], path);
    let once = audit.row(&mut h, "opened again");
    open(&mut h, panes[1], path);
    let twice = audit.row(&mut h, "and in the second pane");
    let shared = twice.documents == once.documents && twice.pages <= once.pages + 1;
    let grew = twice.rss - once.rss;
    let value = format!("{grew:+.0} MiB, {} documents", twice.documents);
    let held = shared && grew <= SECOND_PANE;
    audit.check("A second pane on the book shares it", value, held);
    Ok(audit.report())
}

/// A one-page PDF of its own colour and number, a few hundred bytes.
fn tiny_pdf(n: usize) -> Vec<u8> {
    let channel = |k: usize| ((n * k) % 200 + 40) as f32 / 255.0;
    let (r, g, b) = (channel(37), channel(73), channel(151));
    let content = format!(
        "{r:.2} {g:.2} {b:.2} rg 0 0 300 420 re f 0 g \
         BT /F1 32 Tf 40 300 Td (Book {n}) Tj ET"
    );
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 420] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        format!("<< /Title (Book {n}) >>"),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).into_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
    for at in offsets {
        out.extend(format!("{at:010} 00000 n \n").into_bytes());
    }
    let size = objects.len() + 1;
    let trailer = format!("trailer\n<< /Size {size} /Root 1 0 R /Info 6 0 R >>\n");
    out.extend(format!("{trailer}startxref\n{xref}\n%%EOF\n").into_bytes());
    out
}

/// Writes `count` books an hour old, so no scan waits for them to settle.
fn write_books(dir: &Path, count: usize) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let old = SystemTime::now() - Duration::from_secs(3_600);
    for n in 0..count {
        let path = dir.join(format!("book-{n:05}.pdf"));
        std::fs::write(&path, tiny_pdf(n)).map_err(|e| e.to_string())?;
        let file = std::fs::File::options().write(true).open(&path);
        file.and_then(|f| f.set_modified(old))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Wheels down the page until `last` shows, or `bound` turns.
fn scroll_to(h: &mut App, last: Option<&str>, bound: usize) -> usize {
    use egui_kittest::kittest::Queryable as _;
    h.hover_at(egui::pos2(600.0, 450.0));
    for turn in 0..bound {
        if last.is_some_and(|l| h.query_all_by_label(l).next().is_some()) {
            return turn;
        }
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(2);
    }
    bound
}

/// The title the page labels the book from `file` with.
fn title_of(h: &App, file: &str) -> String {
    let lib = h.state().library();
    let mut books = lib.books();
    let hit = books.find(|b| b.path().is_some_and(|p| p.ends_with(file)));
    hit.map(|b| b.title.value.clone()).unwrap_or_default()
}

fn per_book(audit: &mut Audit, before: &Row, after: &Row, count: usize) {
    let kib = (after.rss - before.rss) * 1024.0 / count.max(1) as f64;
    let value = format!("{kib:.1} KiB a book over {count}");
    let held = kib <= PER_BOOK_KIB;
    audit.check("Each book's share of the library", value, held);
}

/// Searches the page, then lets it go.
fn library_search(h: &mut App, audit: &mut Audit, text: &str) {
    let query = SearchAction::Text(text.to_owned());
    h.state_mut().queue(Command::Library(Action::Search(query)));
    h.run_steps(4);
    audit.row(h, &format!("searched \"{text}\""));
    let clear = Command::Library(Action::Search(SearchAction::Clear));
    h.state_mut().queue(clear);
    h.run_steps(4);
}

fn library(count: usize) -> Result<bool, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let books = dir.path().join("Books");
    write_books(&books, count)?;
    let mut app = MareaderApp::default();
    app.set_option("workers", "0")?;
    app.set_option("library", "show")?;
    app.attach_library(dir.path().join("library"));
    let mut h = harness(app);
    let mut audit = Audit::default();
    Audit::header();
    let empty = audit.row(&mut h, "library page, empty");
    let plan = FolderPlan {
        keep: FolderKeep::InPlace,
        walk: Walk::default(),
    };
    let roots = vec![books.clone()];
    h.state_mut()
        .queue(Command::Library(Action::AddFolders { roots, plan }));
    for _ in 0..200 {
        h.run_steps(2);
        if h.state().library().len() >= count {
            break;
        }
    }
    let found = h.state().library().len();
    let added = audit.row(&mut h, &format!("{found} books read in place"));
    audit.check(
        "Every book is found",
        format!("{found} of {count}"),
        found == count,
    );
    per_book(&mut audit, &empty, &added, count);
    let last = title_of(&h, &format!("book-{:05}.pdf", count.saturating_sub(1)));
    if last.is_empty() {
        return Err("the last book has no title".into());
    }
    let turns = scroll_to(&mut h, Some(&last), 4_000);
    audit.row(&mut h, &format!("grid scrolled to the end ({turns} turns)"));
    library_search(&mut h, &mut audit, "Book 12");
    h.state_mut().set_option("layout", "list")?;
    h.run_steps(4);
    let turns = scroll_to(&mut h, Some(&last), 8_000);
    audit.row(&mut h, &format!("list scrolled to the end ({turns} turns)"));
    h.state_mut().queue(Command::ShowLibrary);
    h.run_steps(4);
    audit.row(&mut h, "back to reading");
    Ok(audit.report())
}

fn seeded(count: usize) -> Result<bool, String> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0")?;
    let mut h = harness(app);
    let mut audit = Audit::default();
    Audit::header();
    let before = audit.row(&mut h, "reader, no library");
    h.state_mut().set_option("books", &count.to_string())?;
    h.state_mut().set_option("library", "show")?;
    h.run_steps(4);
    let after = audit.row(&mut h, &format!("{count} books, page shown"));
    per_book(&mut audit, &before, &after, count);
    // Its titles repeat, so the end is a bound: about a row a turn.
    let turns = scroll_to(&mut h, None, count / 6 + 10);
    audit.row(&mut h, &format!("grid scrolled ({turns} turns)"));
    library_search(&mut h, &mut audit, "Raven");
    Ok(audit.report())
}

fn count(arg: Option<String>) -> Result<usize, String> {
    let text = arg.ok_or("give a count of books")?;
    text.parse().map_err(|_| format!("not a count: {text}"))
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .ok_or("usage: memory BOOK [ROUNDS] | memory --library N | memory --seeded N")?;
    let held = match first.as_str() {
        "--library" => library(count(args.next())?)?,
        "--seeded" => seeded(count(args.next())?)?,
        _ => {
            let rounds = args.next().and_then(|n| n.parse().ok()).unwrap_or(4);
            book(&PathBuf::from(first), rounds)?
        }
    };
    if !held {
        std::process::exit(1);
    }
    Ok(())
}
