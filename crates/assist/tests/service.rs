//! The app's service end to end: settings in, answers and key jobs out.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mareader_assist::{Assist, Board, Job, KeyState, Load};
use mareader_settings::{AiSettings, Provider};
use mareader_study::{Explainer, Why, WordInfo};
use support::{Answer, QUICK, Server};

/// A runtime for the loopback server, apart from the service's own.
fn server(answers: Vec<Answer>) -> (tokio::runtime::Runtime, Server) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(Server::start(answers));
    (rt, server)
}

fn assist(dir: &tempfile::TempDir) -> Assist {
    let assist = Assist::new(dir.path().to_owned(), Arc::new(|| {})).unwrap();
    assist.without_keychain()
}

/// Polls until `done` holds for the board, or fails after ten seconds.
fn wait(a: &mut Assist, done: impl Fn(&Board) -> bool) {
    let start = Instant::now();
    while !done(a.board()) {
        assert!(start.elapsed() < Duration::from_secs(10), "{:?}", a.board());
        std::thread::sleep(Duration::from_millis(10));
        a.poll();
    }
}

fn explain(e: &dyn Explainer) -> (Result<WordInfo, Why>, Vec<WordInfo>) {
    let seen = Mutex::new(Vec::new());
    let push = |i| seen.lock().unwrap().push(i);
    let got = e
        .explain("quick", "The quick fox", &push)
        .map_err(|f| f.why);
    (got, seen.into_inner().unwrap())
}

fn settings(p: Provider, url: &str) -> AiSettings {
    let mut ai = AiSettings {
        provider: p,
        ..AiSettings::default()
    };
    if let Some(e) = ai.endpoint_mut(p) {
        e.url = url.to_owned();
        e.model = "m".into();
    }
    ai
}

#[test]
fn a_local_server_answers_without_keys() {
    let models = r#"{"data":[{"id":"llama3.2"}]}"#;
    let pieces = [&QUICK[..30], &QUICK[30..]];
    let (_rt, server) = server(vec![Answer::stream(&pieces), Answer::json(200, models)]);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let explainer = a.configure(&settings(Provider::Ollama, &server.url));
    let (got, seen) = explain(explainer.as_ref());
    assert_eq!(got.map(|i| i.meaning).as_deref(), Ok("Fast."));
    assert!(!seen.is_empty(), "partials reach the card");
    assert_eq!(server.seen()[0].auth, None);
    a.send(Job::Models(Provider::Ollama));
    wait(&mut a, |b| b.models(Provider::Ollama).ready().is_some());
    let listed = a.board().models(Provider::Ollama).ready().cloned();
    assert_eq!(listed, Some(vec!["llama3.2".to_owned()]));
}

#[test]
fn keys_are_added_used_banned_recovered_and_removed() {
    let (_rt, server) = server(vec![Answer::stream(&[QUICK])]);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let p = Provider::Custom;
    let explainer = a.configure(&settings(p, &server.url));
    let keys = |b: &Board| b.keys(p).ready().cloned().unwrap_or_default();

    a.send(Job::AddKey(p, "  sk-test-0123456789  ".into()));
    wait(&mut a, |b| keys(b).len() == 1);
    let row = keys(a.board())[0].clone();
    assert!(!row.shown.contains("0123456789"), "{}", row.shown);
    assert!(dir.path().join("master.key").exists(), "no keychain asked");
    let (got, _) = explain(explainer.as_ref());
    assert!(got.is_ok(), "{got:?}");
    let auth = server.seen().last().and_then(|s| s.auth.clone());
    assert_eq!(auth.as_deref(), Some("Bearer sk-test-0123456789"));

    a.send(Job::BanKey(p, row.id.clone()));
    wait(&mut a, |b| {
        matches!(
            keys(b).first().map(|r| &r.state),
            Some(KeyState::Banned { .. })
        )
    });
    assert_eq!(explain(explainer.as_ref()).0.err(), Some(Why::KeysRejected));
    a.send(Job::RecoverKey(p, row.id.clone()));
    wait(&mut a, |b| {
        keys(b).first().map(|r| &r.state) == Some(&KeyState::Ready)
    });

    a.send(Job::AddKey(p, "two words".into()));
    wait(&mut a, |b| b.problems.contains_key(&p));
    a.send(Job::RemoveKey(p, row.id));
    wait(&mut a, |b| b.keys(p).ready().is_some_and(Vec::is_empty));
    // With no keys left, a custom server is asked without one.
    let (got, _) = explain(explainer.as_ref());
    assert!(got.is_ok(), "{got:?}");
    assert_eq!(server.seen().last().and_then(|s| s.auth.clone()), None);
}

#[test]
fn providers_that_cannot_answer_say_why() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let (_rt, server) = server(vec![Answer::stream(&[QUICK])]);
    let openai = a.configure(&settings(Provider::OpenAi, &server.url));
    assert_eq!(explain(openai.as_ref()).0.err(), Some(Why::NoKey));
    assert!(server.seen().is_empty(), "nothing is sent without a key");
    let mut blank = settings(Provider::Custom, "");
    blank.custom.url = "localhost:1234".into();
    let custom = a.configure(&blank);
    assert_eq!(explain(custom.as_ref()).0.err(), Some(Why::Setup));
    if !cfg!(target_os = "macos") {
        let auto = a.configure(&AiSettings::default());
        assert_eq!(explain(auto.as_ref()).0.err(), Some(Why::NotHere));
        let apple = a.configure(&settings(Provider::Apple, ""));
        assert_eq!(explain(apple.as_ref()).0.err(), Some(Why::NotHere));
        a.send(Job::Probe);
        wait(&mut a, |b| matches!(b.apple, Load::Ready(_)));
    }
    a.send(Job::Try(Provider::OpenAi));
    wait(&mut a, |b| {
        matches!(b.tried(Provider::OpenAi), Load::Failed(_))
    });
}

#[cfg(unix)]
mod apple {
    use std::os::unix::fs::PermissionsExt;

    use fm_bridge::Bridge;
    use mareader_assist::{Apple, AppleState};

    use super::*;

    /// A stand-in for the Swift helper: probes ready, streams `quick`.
    const HELPER: &str = r#"#!/usr/bin/env bash
if [ "${1:-}" = "--probe" ]; then
  printf '%s\n' '{"ready":{"available":true,"contextSize":4096,"supportedLanguages":["en-US"]}}'
  exit 0
fi
read -r _request || true
printf '%s\n' '{"snapshot":{"pos":"adjective"}}'
printf '%s\n' '{"snapshot":{"pos":"adjective","meaning":"Fa"}}'
printf '%s\n' '{"structured":{"pos":"adjective","meaning":"Fast.","synonyms":["swift","rapid"],"usages":["Be quick."]}}'
printf '%s\n' '{"done":true,"usage":{"promptTokens":9,"completionTokens":9}}'
"#;

    #[test]
    fn apple_intelligence_streams_through_the_helper() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("FMBridge");
        std::fs::write(&path, HELPER).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut a = assist(&dir).with_apple(Apple::new(Bridge::new(&path)));
        for p in [Provider::Apple, Provider::Auto] {
            let explainer = a.configure(&settings(p, ""));
            let (got, seen) = explain(explainer.as_ref());
            assert_eq!(got.map(|i| i.synonyms.len()), Ok(2));
            let meanings: Vec<&str> = seen.iter().map(|i| i.meaning.as_str()).collect();
            assert_eq!(meanings, ["", "Fa"]);
        }
        a.send(Job::Probe);
        wait(&mut a, |b| b.apple == Load::Ready(AppleState::Ready));
    }
}
