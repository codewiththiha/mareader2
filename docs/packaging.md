# Packaging and opening files from the system

Mareader opens a PDF from the system file manager (Finder, Explorer, a
Linux file manager) the way mareader did: as the registered PDF viewer,
reusing the window that is already running.

## Installers

[cargo-packager](https://github.com/crabnebula-dev/cargo-packager) reads
`[package.metadata.packager]` in `apps/mareader/Cargo.toml`:

```sh
cargo install cargo-packager --locked
cd apps/mareader && cargo packager --release
```

Each platform gets its default formats: `.app` and `.dmg` on macOS, NSIS
and MSI on Windows, `.deb`, AppImage and pacman on Linux. Each one
registers `.pdf` (`application/pdf`) with Mareader as a viewer, as in
mareader's `tauri.conf.json`. EPUB joins the list when it can be read (M6).

The icons in `apps/mareader/icons` are mareader's.

## How a file arrives

| Where | How it arrives | Code |
| --- | --- | --- |
| Command line, Windows, Linux | the path is an argument | `main.rs` |
| macOS Finder and Dock | an "open documents" Apple event | `open_files.rs` |
| Any, while Mareader runs | the new launch forwards it | `single_instance.rs` |

Every route posts to one `Inbox`. The window reads it each frame and
opens the newest path in the focused pane, as mareader's `queue_pending`
did. A file given at launch takes the place of resuming the last book.

### macOS

AppKit delivers Finder's files as a `kAEOpenDocuments` Apple event, not
as arguments. winit installs its own `NSApplicationDelegate`, so
Mareader can't add `application:openURLs:` to it. Instead, `open_files.rs`
watches for `NSApplicationWillFinishLaunchingNotification` and then
installs its own handler with `NSAppleEventManager`. That is the moment
Apple documents for replacing AppKit's default handlers, and it comes
before the launch's own event, so a double-clicked file opens on the
first frame. With the app running, macOS sends the event to it directly.

This is the workspace's second `unsafe` module, approved by the owner and
listed in `xtask/src/unsafety.rs`. The `open-files` Cargo feature (on by
default) turns it off; without it, only the command line opens files.

### One window

On Windows and Linux, opening a file starts a new process. As mareader's
single-instance plugin did, the first launch listens on loopback (port 0)
and writes the port and a random token to `instance` in eframe's data
folder. A later launch sends the token and its absolute paths, waits for
`ok`, and exits; the first window opens the file and comes forward.

If the recorded reader is gone, the connection is refused at once. If the
port belongs to something else now, there is no `ok` within half a
second. Either way, the new launch runs as the primary and records itself.

## Checking it on a Mac

1. `cd apps/mareader && cargo packager --release --formats app`.
2. Move `Mareader.app` to `/Applications` and open it once.
3. In Finder, choose a PDF › Get Info › Open with: Mareader, then
   double-click the PDF. It opens on the first frame.
4. With Mareader running, double-click another PDF. It replaces the book
   in the focused pane, in the same window.
5. `open -a Mareader book.pdf` does the same from a terminal.

## The Apple Intelligence helper

On a Mac, Explain can use the on-device model through `FMBridge`, a small
Swift program from [fm-bridge](https://github.com/codewiththiha/fm-bridge).
Only macOS 26 with Xcode 26 can build it, so it is not part of
`cargo build`:

```sh
SIGN_IDENTITY="Developer ID Application: …" tools/fm-helper.sh
cd apps/mareader && cargo packager --release
```

- **Which version.** The script builds the fm-bridge commit that
  `Cargo.lock` pins, so the helper and the crate speak the same protocol.
  Without `SIGN_IDENTITY` it signs ad hoc, enough for this Mac only.
- **Where it goes.** The script writes `apps/mareader/helper/FMBridge`
  (ignored by git). The packager's `resources` entry copies it to
  `Mareader.app/Contents/Resources/`, where the app looks for it. Without
  the file, packaging still works and Settings › AI says Apple
  Intelligence is not available.
- **While developing.** Debug builds also honour
  `FM_BRIDGE_BIN=/path/to/FMBridge`, so `cargo run` can use the helper
  without packaging.
- **Other platforms.** The helper is never built or shipped; the AI tab
  hides Apple Intelligence and offers the servers instead.
