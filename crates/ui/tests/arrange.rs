//! The arranged order: cards moved by dropping them between others.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_library::{Home, Item, Library, SortKey};
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::library::{Action, ShelfAction};

fn reader(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("library", "show").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn library<'a>(h: &'a Harness<'static, MareaderApp>) -> &'a Library {
    h.state().library()
}

fn top(h: &Harness<'static, MareaderApp>) -> Vec<Item> {
    library(h).arranged(None, Home::Everything)
}

/// What a card shows as its label.
fn label(h: &Harness<'static, MareaderApp>, item: Item) -> String {
    match item {
        Item::Book(id) => library(h).book(id).unwrap().title.value.clone(),
        Item::Shelf(id) => library(h).shelf(id).unwrap().name.value.clone(),
    }
}

/// A point `along` the card's width, from its left, mid-height.
fn on(h: &Harness<'static, MareaderApp>, item: Item, along: f32) -> egui::Pos2 {
    let rect = h.get_by_label(&label(h, item)).rect();
    egui::pos2(rect.left() + rect.width() * along, rect.center().y)
}

fn shelve(h: &mut Harness<'static, MareaderApp>, action: ShelfAction) {
    h.state_mut().queue(Command::Library(Action::Shelf(action)));
    h.run();
    h.run();
}

/// Shelves are made through the page's own action, at the top level.
fn new_shelf(h: &mut Harness<'static, MareaderApp>, name: &str) -> Item {
    shelve(h, ShelfAction::New);
    let id = h.state().library_state().at().expect("a new shelf opens");
    shelve(h, ShelfAction::Rename(id, name.into()));
    shelve(h, ShelfAction::GoTo(None));
    Item::Shelf(id)
}

/// Carries `item` by its middle to `along` the width of `onto`.
fn drop_on(h: &mut Harness<'static, MareaderApp>, item: Item, onto: Item, along: f32) {
    let (from, to) = (on(h, item, 0.5), on(h, onto, along));
    drag(h, from, to);
}

/// Presses on `from`, moves in steps past egui's drag threshold, lets go.
fn drag(h: &mut Harness<'static, MareaderApp>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run();
    h.drag_at(from);
    h.run();
    for step in 1..=6 {
        h.hover_at(from.lerp(to, step as f32 / 6.0));
        h.run();
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

#[test]
fn a_card_dropped_on_anothers_side_moves_there() {
    let mut h = reader(&[("books", "4")]);
    let before = top(&h);
    let (first, last) = (before[0], before[3]);
    drop_on(&mut h, last, first, 0.2);
    assert_eq!(top(&h), [last, before[0], before[1], before[2]]);
    // On the right half, it goes after.
    drop_on(&mut h, last, before[1], 0.8);
    assert_eq!(top(&h), [before[0], before[1], last, before[2]]);
}

#[test]
fn a_shelf_moves_by_its_edges_and_takes_books_in_its_middle() {
    let mut h = reader(&[("books", "3")]);
    let shelf = new_shelf(&mut h, "Poetry");
    let books = top(&h);
    assert_eq!(books.last(), Some(&shelf), "a new shelf joins the end");
    drop_on(&mut h, shelf, books[0], 0.2);
    assert_eq!(top(&h).first(), Some(&shelf), "shelves mix among books");
    // A book on its outer quarter lands beside it, not on it.
    let book = books[2];
    drop_on(&mut h, book, shelf, 0.1);
    assert_eq!(top(&h).first(), Some(&book));
    assert!(library(&h).shelved().is_empty());
    drop_on(&mut h, book, shelf, 0.5);
    assert_eq!(library(&h).shelved().len(), 1, "the middle files it");
}

#[test]
fn a_drop_while_sorted_shows_the_order_it_made() {
    let mut h = reader(&[("books", "3"), ("sort", "title")]);
    let shown = |h: &Harness<'static, MareaderApp>| {
        let mut items = top(h);
        items.sort_by_key(|i| label(h, *i));
        items
    };
    let sorted = shown(&h);
    drop_on(&mut h, sorted[2], sorted[0], 0.2);
    let view = h.state().settings().library;
    assert_eq!((view.sort, view.ascending), (SortKey::Arranged, true));
    let order = top(&h);
    let at = |item| order.iter().position(|i| *i == item);
    assert_eq!(at(sorted[2]).map(|n| n + 1), at(sorted[0]), "{order:?}");
}

#[test]
fn the_list_files_by_drag_but_keeps_its_order() {
    let mut h = reader(&[("books", "3"), ("layout", "list")]);
    let before = top(&h);
    let (first, last) = (before[0], before[2]);
    let row = |h: &Harness<'static, MareaderApp>, item| {
        let rect = h.get_by_label(&label(h, item)).rect();
        egui::pos2(rect.center().x, rect.top() + 4.0)
    };
    let (from, to) = (row(&h, last), row(&h, first));
    drag(&mut h, from, to);
    assert_eq!(top(&h), before);
}
