//! The dataset end to end: parquet to table, then hard words.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mareader_cefr::{Part, Span, State, Store, Vocabulary, build};
use parquet::basic::{Compression, ZstdLevel};
use parquet::data_type::{ByteArray, ByteArrayType, FloatType};
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::parser::parse_message_type;

type Row = (&'static str, Option<&'static str>, f32);

const ROWS: &[Row] = &[
    ("the", Some("DT"), 1.0),
    ("run", Some("NN"), 2.0),
    ("run", Some("VB"), 4.0),
    ("Ephemeral", Some("JJ"), 5.6),
    ("known", None, 2.2),
    ("", Some("NN"), 3.0),
];

/// A parquet shaped like the real one: zstd, nullable tags, a float level.
fn dataset(path: &Path, rows: &[Row]) {
    let message = "message schema {
        REQUIRED BYTE_ARRAY word (UTF8);
        OPTIONAL BYTE_ARRAY pos_tag (UTF8);
        REQUIRED FLOAT level;
        OPTIONAL BYTE_ARRAY lemma (UTF8);
    }";
    let schema = Arc::new(parse_message_type(message).unwrap());
    let zstd = Compression::ZSTD(ZstdLevel::default());
    let props = Arc::new(WriterProperties::builder().set_compression(zstd).build());
    let mut writer = SerializedFileWriter::new(File::create(path).unwrap(), schema, props).unwrap();
    let mut group = writer.next_row_group().unwrap();
    let words: Vec<ByteArray> = rows.iter().map(|r| r.0.into()).collect();
    let tags: Vec<ByteArray> = rows.iter().filter_map(|r| r.1).map(Into::into).collect();
    let tagged: Vec<i16> = rows.iter().map(|r| i16::from(r.1.is_some())).collect();
    let levels: Vec<f32> = rows.iter().map(|r| r.2).collect();
    let none = vec![0i16; rows.len()];
    let mut col = group.next_column().unwrap().unwrap();
    col.typed::<ByteArrayType>()
        .write_batch(&words, None, None)
        .unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    col.typed::<ByteArrayType>()
        .write_batch(&tags, Some(&tagged), None)
        .unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    col.typed::<FloatType>()
        .write_batch(&levels, None, None)
        .unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    col.typed::<ByteArrayType>()
        .write_batch(&[], Some(&none), None)
        .unwrap();
    col.close().unwrap();
    group.close().unwrap();
    writer.close().unwrap();
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn the_dataset_becomes_a_table_that_answers() {
    let dir = tempfile::tempdir().unwrap();
    let (parquet, db) = (dir.path().join("cefr.parquet"), dir.path().join("cefr.db"));
    dataset(&parquet, ROWS);
    // The blank word is skipped.
    assert_eq!(build(&parquet, &db), Ok(5));
    let store = Store::open(&db).unwrap();
    assert_eq!(store.words(), Ok(4));
    let keys = ["run", "ephemeral", "nope"].map(String::from);
    let levels = store.levels(&keys).unwrap();
    assert!(close(levels["run"], 3.0));
    assert!(close(levels["ephemeral"], 5.6));
    assert!(!levels.contains_key("nope"));
    let senses = store.senses("run").unwrap();
    let tags: Vec<&str> = senses.iter().map(|s| s.pos.as_str()).collect();
    assert_eq!(tags, ["NN", "VB"]);
    // A tag missing from the table falls to its family, then the first.
    let past = store.sense("run", "VBD").unwrap().unwrap();
    assert_eq!((past.pos.as_str(), past.level), ("VB", 4.0));
    let adjective = store.sense("run", "JJ").unwrap().unwrap();
    assert_eq!(adjective.pos, "NN");
    assert_eq!(store.sense("known", "NN"), Ok(None));
}

#[test]
fn a_file_that_is_not_the_dataset_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("page.parquet");
    std::fs::write(&page, b"<html>Not Found</html>").unwrap();
    let err = build(&page, &dir.path().join("x.db")).unwrap_err();
    assert!(err.starts_with("not the dataset"), "{err}");
    // A table never finished is not opened as one.
    let half = dir.path().join("half.db");
    let conn = rusqlite::Connection::open(&half).unwrap();
    conn.execute_batch("CREATE TABLE cefr (word TEXT, level REAL);")
        .unwrap();
    drop(conn);
    assert!(Store::open(&half).is_err());
}

/// Polls until `done` holds, or fails after a while.
fn wait(vocab: &mut Vocabulary, done: impl Fn(&Vocabulary) -> bool) {
    let until = Instant::now() + Duration::from_secs(20);
    while !done(vocab) {
        assert!(Instant::now() < until, "timed out");
        std::thread::sleep(Duration::from_millis(20));
        vocab.poll();
    }
}

#[test]
fn a_downloaded_dataset_converts_then_shades_hard_words() {
    let dir = tempfile::tempdir().unwrap();
    dataset(&dir.path().join("cefr.parquet.part"), ROWS);
    let mut vocab = Vocabulary::default();
    let text = "The ephemeral run";
    // Not attached: nothing is hard and nothing is asked.
    assert!(vocab.plan(text, 4, 99).is_empty());
    vocab.attach(dir.path().into(), Arc::new(|| {})).unwrap();
    assert_eq!(vocab.state(Part::Dataset), &State::Converting);
    assert_eq!(vocab.state(Part::Tagger), &State::Absent);
    wait(&mut vocab, |v| v.state(Part::Dataset) == &State::Ready);
    assert_eq!(vocab.words(), Some(4));
    assert!(!dir.path().join("cefr.parquet.part").exists());
    vocab.set_inline(true);
    let before = vocab.generation();
    assert!(vocab.plan(text, 4, 99).is_empty());
    assert!(vocab.poll().changed);
    assert!(vocab.generation() > before);
    let ephemeral = Span { start: 4, end: 13 };
    assert_eq!(vocab.plan(text, 4, 99), [ephemeral]);
    // At A2, run's average of B1 is above too; C2 leaves nothing.
    assert_eq!(vocab.plan(text, 2, 99).len(), 2);
    assert!(vocab.plan(text, 6, 99).is_empty());
    // Nothing more to ask: a poll brings no change.
    assert!(!vocab.poll().changed);
    // No tagger, no role.
    assert_eq!(vocab.role("run", text), None);
}

#[test]
fn removing_drops_the_files_and_the_shading() {
    let dir = tempfile::tempdir().unwrap();
    let parquet = dir.path().join("src.parquet");
    dataset(&parquet, ROWS);
    build(&parquet, &dir.path().join("cefr.db")).unwrap();
    std::fs::write(dir.path().join("en_tokenizer.bin.zst.part"), b"half").unwrap();
    let mut vocab = Vocabulary::default();
    vocab.attach(dir.path().into(), Arc::new(|| {})).unwrap();
    vocab.set_inline(true);
    assert_eq!(vocab.state(Part::Dataset), &State::Ready);
    let paused = State::Paused {
        got: 4,
        total: None,
    };
    assert_eq!(vocab.state(Part::Tagger), &paused);
    vocab.plan("ephemeral", 4, 9);
    vocab.poll();
    assert_eq!(vocab.plan("ephemeral", 4, 9).len(), 1);
    vocab.remove();
    assert_eq!(vocab.state(Part::Dataset), &State::Absent);
    assert!(vocab.plan("ephemeral", 4, 9).is_empty());
    let until = Instant::now() + Duration::from_secs(20);
    let left = || std::fs::read_dir(dir.path()).unwrap().count();
    while left() > 1 {
        assert!(Instant::now() < until, "files stayed");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(parquet.exists());
}
