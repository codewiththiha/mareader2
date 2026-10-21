//! The page model as a book's scripts use it.

use std::sync::mpsc;
use std::time::Duration;

use mareader_script::{Ask, Engine, Page, Scripts};

fn page(body: &str, script: &str) -> Page {
    Page {
        chapter: 3,
        xhtml: format!(
            "<?xml version=\"1.0\"?><html xmlns=\"http://www.w3.org/1999/xhtml\">\
             <head><title>T</title></head><body>{body}</body></html>"
        ),
        scripts: vec![script.to_owned()],
        styles: vec![".gone { display: none }".to_owned()],
    }
}

fn shown(change: &mareader_script::Change) -> String {
    change.page.clone().expect("the page changed")
}

#[test]
fn a_quiz_shows_its_answer_on_click() {
    let p = page(
        r#"<p>Q1</p><button id="b">Show</button><p id="a" class="gone">Answer</p>"#,
        r#"document.getElementById('b').addEventListener('click', function () {
             var a = document.querySelector('#a');
             if (getComputedStyle(a).display === 'none') a.classList.remove('gone');
             this.textContent = 'Hide';
             alert('shown');
           });"#,
    );
    let (mut engine, first) = Engine::new(&p);
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    let page = shown(&first);
    assert!(page.contains(r#"<p id="a" class="gone">"#), "{page}");
    // The button listens, so it is marked for clicks.
    assert!(
        page.contains(r#"<button id="b" data-mareader-node="#),
        "{page}"
    );
    let change = engine.ask(&Ask::Pick("#b".into()));
    assert!(change.errors.is_empty(), "{:?}", change.errors);
    let page = shown(&change);
    assert!(page.contains(r#"<p id="a" class="">Answer</p>"#), "{page}");
    assert!(page.contains(">Hide</button>"), "{page}");
    assert_eq!(change.alerts, ["shown"]);
    // Nothing changed: no page is sent again.
    assert_eq!(engine.ask(&Ask::Pick("#nothing".into())).page, None);
}

#[test]
fn controls_are_drawn_as_text_and_answer_clicks() {
    let p = page(
        r#"<label><input type="checkbox" id="c"/> I agree</label>
           <input type="radio" name="r" id="r1" checked="checked"/><input type="radio" name="r" id="r2"/>
           <select id="s"><option>One</option><option>Two</option></select>
           <input type="text" id="t" placeholder="Your answer"/><p id="out"></p>"#,
        r#"document.addEventListener('change', function (e) {
             document.getElementById('out').textContent =
               e.target.id + '=' + (e.target.type === 'checkbox' ? e.target.checked : e.target.value);
           });"#,
    );
    let (mut engine, first) = Engine::new(&p);
    let page = shown(&first);
    assert!(page.contains("\u{25a1}</span>"), "{page}");
    assert!(page.contains("\u{25c6}</span>"), "{page}");
    assert!(page.contains("One \u{25bc}</span>"), "{page}");
    assert!(page.contains("Your answer</span>"), "{page}");
    let field = first.fields.iter().find(|(_, f)| f.hint == "Your answer");
    let (node, _) = field.expect("the text field is listed");

    let page = shown(&engine.ask(&Ask::Pick("#c".into())));
    assert!(
        page.contains("\u{25a0}</span>") && page.contains("c=true"),
        "{page}"
    );
    let page = shown(&engine.ask(&Ask::Pick("#r2".into())));
    assert!(
        page.contains("\u{25c7}</span>\u{25c6}") || page.contains("r2=on"),
        "{page}"
    );
    let page = shown(&engine.ask(&Ask::Pick("#s".into())));
    assert!(
        page.contains("Two \u{25bc}") && page.contains("s=Two"),
        "{page}"
    );
    let change = engine.ask(&Ask::Value(*node, "Paris".into()));
    let page = shown(&change);
    assert!(
        page.contains("Paris</span>") && page.contains("t=Paris"),
        "{page}"
    );
    assert!(change.fields.iter().any(|(_, f)| f.value == "Paris"));
}

#[test]
fn markup_selectors_and_handlers_work_like_a_browser() {
    let p = page(
        r#"<ul id="l"><li>a</li><li class="x">b</li><li>c</li></ul>
           <div id="d" onclick="this.setAttribute('data-hit', 'yes'); return false"></div>"#,
        r#"var l = document.getElementById('l');
           l.innerHTML += '<li data-k="v">d &amp; e</li>';
           var last = l.querySelector('li:last-child');
           last.dataset.seen = '1';
           document.title = document.querySelectorAll('ul > li:nth-child(odd)').length + '';
           l.querySelector('.x + li').style.color = 'red';"#,
    );
    let (mut engine, first) = Engine::new(&p);
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    let page = shown(&first);
    assert!(
        page.contains(r#"<li data-k="v" data-seen="1">d &amp; e</li>"#),
        "{page}"
    );
    assert!(page.contains("<title>2</title>"), "{page}");
    assert!(page.contains(r#"<li style="color: red">c</li>"#), "{page}");
    let page = shown(&engine.ask(&Ask::Pick("#d".into())));
    assert!(page.contains(r#"data-hit="yes""#), "{page}");
}

#[test]
fn clicks_handled_by_a_parent_mark_its_children() {
    let p = page(
        r#"<div id="quiz"><p class="choice">A</p><p class="choice">B</p><p>plain</p></div>"#,
        r#"document.body.addEventListener('click', function (e) {
             if (e.target.classList.contains('choice')) e.target.classList.add('picked');
           });"#,
    );
    let (mut engine, first) = Engine::new(&p);
    let page = shown(&first);
    assert_eq!(
        page.matches(r#"class="choice" data-mareader-node"#).count(),
        2,
        "{page}"
    );
    assert!(page.contains("<p>plain</p>"), "{page}");
    let page = shown(&engine.ask(&Ask::Pick(".choice".into())));
    assert!(page.contains(r#"class="choice picked""#), "{page}");
}

#[test]
fn timers_run_when_due() {
    let p = page(
        r#"<p id="t">wait</p>"#,
        r#"setTimeout(function () { document.getElementById('t').textContent = 'done'; }, 30);"#,
    );
    let (mut engine, _) = Engine::new(&p);
    let due = engine.due().expect("a timer is set");
    assert!(due <= Duration::from_millis(30));
    std::thread::sleep(due + Duration::from_millis(5));
    let page = shown(&engine.tick());
    assert!(page.contains(">done</p>"), "{page}");
    assert_eq!(engine.due(), None);
}

#[test]
fn runaway_scripts_stop_and_the_page_lives_on() {
    let p = page(
        r#"<button id="b">go</button><p id="o">0</p>"#,
        r#"while (true) {}
           document.getElementById('o').textContent = 'never';"#,
    );
    let (mut engine, first) = Engine::new(&p);
    assert!(!first.errors.is_empty());
    assert!(shown(&first).contains(">0</p>"));
    // Nothing outside the page is reachable.
    let probe = r#"<p id="x"></p>"#;
    let p = page(
        probe,
        r#"document.getElementById('x').textContent =
             [typeof fetch, typeof XMLHttpRequest, typeof require, typeof WebSocket].join();"#,
    );
    let (_, sandboxed) = Engine::new(&p);
    let page = shown(&sandboxed);
    assert!(
        page.contains("undefined,undefined,undefined,undefined"),
        "{page}"
    );
    assert_eq!(engine.ask(&Ask::Click(9999)).page, None);
}

#[test]
fn the_runtime_answers_from_its_own_thread() {
    let p = page(
        r#"<button id="b" onclick="this.textContent = 'clicked'">go</button>"#,
        "",
    );
    let (woke, wakes) = mpsc::channel();
    let scripts = Scripts::start(vec![p], move || {
        let _ = woke.send(());
    })
    .expect("the runtime starts");
    wakes.recv_timeout(Duration::from_secs(20)).expect("loaded");
    let first = scripts.changes();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].chapter, 3);
    scripts.ask(3, Ask::Pick("#b".into()));
    wakes
        .recv_timeout(Duration::from_secs(20))
        .expect("answered");
    let change = scripts.changes();
    assert!(shown(&change[0]).contains(">clicked</button>"));
    assert!(!scripts.stuck());
}
