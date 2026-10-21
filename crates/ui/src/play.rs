//! Hands a book's sound or video to the system's player.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use mareader_session::Document;

/// Copies older than this are cleared when another clip plays.
const KEEP: Duration = Duration::from_secs(24 * 60 * 60);
/// Bytes of a clip that name its copy, with its path and length.
const NAMED_BY: usize = 64 << 10;

/// Plays `path` of `doc` on a helper thread; false if it couldn't start.
pub fn play(doc: Arc<Document>, path: String) -> bool {
    let spawned = std::thread::Builder::new()
        .name("mareader-play".into())
        .spawn(move || {
            if let Some(file) = copy_out(&doc, &path) {
                launch(&file);
            }
        });
    spawned.is_ok()
}

/// The clip copied out, named by its declared type, never the book's name.
fn copy_out(doc: &Document, path: &str) -> Option<PathBuf> {
    let media = doc.media(path)?;
    let folder = std::env::temp_dir().join("mareader-media");
    std::fs::create_dir_all(&folder).ok()?;
    prune(&folder);
    let file = folder.join(name(path, &media.data, media.extension));
    let fresh = std::fs::metadata(&file).is_ok_and(|m| m.len() == media.data.len() as u64);
    if !fresh {
        std::fs::write(&file, &media.data).ok()?;
    }
    Some(file)
}

/// `<hash>.<extension>`: the same clip keeps one name, others never share it.
fn name(path: &str, data: &[u8], extension: &str) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    data.len().hash(&mut hasher);
    data.get(..NAMED_BY).unwrap_or(data).hash(&mut hasher);
    format!("{:016x}.{extension}", hasher.finish())
}

/// Removes copies left from earlier days; ones in use may stay.
fn prune(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten().take(256) {
        let modified = entry.metadata().and_then(|m| m.modified());
        let old = modified.is_ok_and(|m| now.duration_since(m).is_ok_and(|age| age > KEEP));
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(target_os = "macos")]
fn launch(file: &Path) {
    let _ = std::process::Command::new("open").arg(file).status();
}

#[cfg(windows)]
fn launch(file: &Path) {
    let _ = std::process::Command::new("explorer").arg(file).status();
}

#[cfg(all(unix, not(target_os = "macos")))]
fn launch(file: &Path) {
    let _ = std::process::Command::new("xdg-open").arg(file).status();
}

#[cfg(target_arch = "wasm32")]
fn launch(_file: &Path) {}

#[cfg(test)]
mod tests {
    use super::name;

    #[test]
    fn copies_are_named_by_clip_and_type() {
        let a = name("/OPS/a.mp4", b"clip", "mp4");
        assert_eq!(a, name("/OPS/a.mp4", b"clip", "mp4"));
        assert_ne!(a, name("/OPS/b.mp4", b"clip", "mp4"));
        assert_ne!(a, name("/OPS/a.mp4", b"clap", "mp4"));
        assert!(a.ends_with(".mp4") && !a.contains('/'), "{a}");
    }
}
