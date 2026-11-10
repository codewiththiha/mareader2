# Review batch: slice ownership

The review at https://01a1c3c9-4aea-7de4-b8d1-c9eaba76880a.arena.site/
defines the detailed briefs. Branch parallel slices from the S0 commit. Do not
merge or run CI until the owner requests it. The owner coordinates assignment.

| Slice | Exclusive write-set |
| --- | --- |
| S1 | `crates/ui/src/icons/**` |
| S2 | `crates/animated-icons/**` |
| S3 | `crates/settings/src/load.rs` |
| S4 | `crates/nn-core/**`, `crates/kokoro/**`, `crates/vits/**`, root `Cargo.toml`, `Cargo.lock`, `xtask/src/{layers,unsafety}.rs`, `docs/architecture.md` |
| S5 | `tools/{parity-kit,kitten-parity,piper-parity,phonon-parity}/**` |
| S6 | `crates/ui/tests/**`, `crates/ui/Cargo.toml`, `crates/epub/tests/**`, `crates/epub/Cargo.toml` |
| S7 | `crates/library/src/**` |
| S8 | `crates/ui/src/app.rs`, `crates/ui/src/app/**` |
| S9 | `crates/ui/src/pdf_view.rs`, `crates/ui/src/pdf_view/**` |
| S10 | `crates/ui/src/{lib,stable_hash,library,appearance_menu,dial_deck,bubble,find_bar,motion,transcript,trigger}.rs`, `library/{books,organizing,review,view,counted}.rs`, `dial_deck/{dials,labels}.rs`, `page_ai/mod.rs` (all under `crates/ui/src/`) |
| S11 | `crates/epub/src/xhtml.rs`, `crates/epub/src/xhtml/**`, `crates/epub/src/{calc,query}.rs` |
| S12 | `crates/reflow/src/flow.rs`, `crates/reflow/src/flow/**` |

Each parallel slice also writes its own `docs/slices/<id>.md`. S0 owns AGENTS,
CLAUDE, the shape and comment tools, hooks, workflows, toolchain, reuse index,
this batch plan and verification docs. SZ owns the baseline, ROADMAP, reuse
index, slice reports, verification docs, CI workflow and pre-push hook after
parallel work. No slice writes the parity ledger. S4 needs owner approval for a
new L0 crate and must inspect any pre-existing related branches.

The review also calls for S0 to add module placeholders but its explicit
write-set excludes those source files. Parallel slices own their module lists as
shown above; no placeholders are added by S0. Any unexpected overlap must be
resolved before those slices begin.
