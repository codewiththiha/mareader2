//! A second launch hands its files to the first, and exits.
//! See docs/packaging.md.

use std::hash::{BuildHasher, RandomState};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::inbox::Inbox;

/// The file in the app's data folder naming the running reader.
const FILE: &str = "instance";
/// Opens every message, so a stray connection is refused.
const HELLO: &str = "mareader-open-1";
/// Loopback refuses at once; a stale port must not stall launch.
const CONNECT: Duration = Duration::from_millis(200);
const REPLY: Duration = Duration::from_millis(500);
/// A message is a few paths; anything longer is not ours.
const MAX_MESSAGE: u64 = 64 * 1024;

#[derive(Debug, PartialEq)]
pub enum Claim {
    /// No reader answered: this one runs and now listens.
    Primary,
    /// The running reader took the files; this launch should exit.
    Forwarded,
}

/// Forwards `paths` to a running reader, else starts listening.
pub fn claim(dir: &Path, paths: &[PathBuf], inbox: &Inbox) -> Claim {
    let file = dir.join(FILE);
    let running = std::fs::read_to_string(&file).ok();
    if running.is_some_and(|text| forward(&text, paths).is_some()) {
        return Claim::Forwarded;
    }
    // Without a listener the reader still runs, just not as the one.
    let _ = serve(&file, inbox.clone());
    Claim::Primary
}

/// Sends `paths` to the reader `record` names; None if none answered.
fn forward(record: &str, paths: &[PathBuf]) -> Option<()> {
    let (port, token) = record.trim().split_once(' ')?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port.parse().ok()?));
    let mut message = format!("{HELLO} {token}\n");
    for path in paths {
        // The other process has its own working directory.
        let path = std::path::absolute(path).ok()?;
        message.push_str(path.to_str()?);
        message.push('\n');
    }
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT).ok()?;
    stream.set_read_timeout(Some(REPLY)).ok()?;
    stream.write_all(message.as_bytes()).ok()?;
    stream.shutdown(std::net::Shutdown::Write).ok()?;
    let mut reply = String::new();
    stream.take(8).read_to_string(&mut reply).ok()?;
    (reply == "ok\n").then_some(())
}

/// Listens on loopback and records where in `file`.
fn serve(file: &Path, inbox: Inbox) -> std::io::Result<()> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let token = token();
    let port = listener.local_addr()?.port();
    write_private(file, &format!("{port} {token}\n"))?;
    std::thread::Builder::new()
        .name("mareader-instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                if let Some(paths) = receive(stream, &token) {
                    inbox.post(paths, true);
                }
            }
        })?;
    Ok(())
}

/// One message's paths, answered "ok"; None if it isn't ours.
fn receive(stream: TcpStream, token: &str) -> Option<Vec<PathBuf>> {
    stream.set_read_timeout(Some(REPLY)).ok()?;
    let mut lines = BufReader::new((&stream).take(MAX_MESSAGE)).lines();
    let hello = lines.next()?.ok()?;
    if hello.strip_prefix(HELLO)?.strip_prefix(' ')? != token {
        return None;
    }
    let mut paths = Vec::new();
    for line in lines {
        paths.push(PathBuf::from(line.ok()?));
    }
    (&stream).write_all(b"ok\n").ok()?;
    Some(paths)
}

/// 128 random bits from std's per-process hash keys.
fn token() -> String {
    let pid = std::process::id();
    let a = RandomState::new().hash_one(pid);
    let b = RandomState::new().hash_one(std::time::SystemTime::now());
    format!("{a:016x}{b:016x}")
}

/// Writes `text` readable by this user only, where that applies.
fn write_private(file: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(file)?.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mareader-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Waits for the listener thread to post.
    fn mail(inbox: &Inbox) -> crate::inbox::Mail {
        for _ in 0..200 {
            let mail = inbox.take();
            if mail.raise {
                return mail;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("nothing arrived");
    }

    #[test]
    fn a_second_launch_hands_its_files_to_the_first() {
        let dir = folder("forward");
        let first = Inbox::default();
        assert_eq!(claim(&dir, &[], &first), Claim::Primary);
        let book = std::env::temp_dir().join("book.pdf");
        assert_eq!(
            claim(&dir, std::slice::from_ref(&book), &Inbox::default()),
            Claim::Forwarded
        );
        assert_eq!(mail(&first).paths, [book]);
        // No file still counts: it raises the window.
        assert_eq!(claim(&dir, &[], &Inbox::default()), Claim::Forwarded);
        assert!(mail(&first).paths.is_empty());
    }

    #[test]
    fn a_stale_or_foreign_record_leaves_this_launch_primary() {
        let dir = folder("stale");
        let closed = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        write_private(&dir.join(FILE), &format!("{port} feed\n")).unwrap();
        assert_eq!(claim(&dir, &[], &Inbox::default()), Claim::Primary);

        // A reader answers only messages carrying its token.
        let record = std::fs::read_to_string(dir.join(FILE)).unwrap();
        let port = record.split(' ').next().unwrap();
        assert!(forward(&format!("{port} wrong"), &[]).is_none());
        assert!(forward(&record, &[]).is_some());
    }
}
