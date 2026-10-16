//! One pack's download: mirrors in turn, resumable, checked before
//! it is kept.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};

/// A parquet starts and ends with this; an error page does neither.
const MAGIC: &[u8; 4] = b"PAR1";
/// No pack comes near this; a bigger body is something else.
const MAX_BYTES: u64 = 256 * 1024 * 1024;
/// Tries per address before the next mirror.
const TRIES: usize = 2;

/// What a download is told to do, between chunks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Halt {
    Go = 0,
    /// Stop and keep the part, to resume.
    Pause = 1,
    /// Stop and delete the part.
    Cancel = 2,
}

impl Halt {
    pub fn read(flag: &AtomicU8) -> Self {
        match flag.load(Ordering::Relaxed) {
            1 => Self::Pause,
            2 => Self::Cancel,
            _ => Self::Go,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stopped {
    /// Told to stop; the part is kept on a pause.
    Halted(Halt),
    /// Every address failed; the last reason.
    Failed(String),
}

/// Fetches the first parquet `urls` serve into `part`, with progress.
pub fn fetch(
    urls: &[String],
    part: &Path,
    halt: &AtomicU8,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), Stopped> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30 * 60))
        .user_agent(concat!("mareader2/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| Stopped::Failed(e.to_string()))?;
    let mut why = String::from("no address to fetch from");
    for url in urls {
        for _ in 0..TRIES {
            match one(&client, url, part, halt, progress) {
                Ok(()) => match check(part) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        // A whole body that isn't a parquet: try the next.
                        let _ = std::fs::remove_file(part);
                        why = e;
                        break;
                    }
                },
                Err(Stopped::Halted(h)) => {
                    if h == Halt::Cancel {
                        let _ = std::fs::remove_file(part);
                    }
                    return Err(Stopped::Halted(h));
                }
                Err(Stopped::Failed(e)) => why = e,
            }
        }
    }
    Err(Stopped::Failed(why))
}

/// One request: the rest of the body after what `part` holds.
fn one(
    client: &reqwest::blocking::Client,
    url: &str,
    part: &Path,
    halt: &AtomicU8,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), Stopped> {
    let failed = |e: &dyn std::fmt::Display| Stopped::Failed(e.to_string());
    let have = std::fs::metadata(part).map_or(0, |m| m.len());
    let mut request = client.get(url);
    if have > 0 {
        request = request.header(RANGE, format!("bytes={have}-"));
    }
    let mut response = request.send().map_err(|e| failed(&e))?;
    let status = response.status();
    let (mut got, total) = match status {
        StatusCode::PARTIAL_CONTENT if starts_at(&response) == Some(have) => {
            (have, response.content_length().map(|n| n + have))
        }
        StatusCode::RANGE_NOT_SATISFIABLE if have > 0 => {
            // Already whole, or the part is longer than the file.
            return match check(part) {
                Ok(()) => Ok(()),
                Err(_) => {
                    let _ = std::fs::remove_file(part);
                    Err(Stopped::Failed("the partial download was stale".into()))
                }
            };
        }
        s if s.is_success() => (0, response.content_length()),
        s => return Err(Stopped::Failed(format!("the server answered {s}"))),
    };
    if total.is_some_and(|t| t > MAX_BYTES) {
        return Err(Stopped::Failed("the file is far too large".into()));
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(got > 0)
        .write(true)
        .truncate(got == 0)
        .open(part)
        .map_err(|e| failed(&e))?;
    progress(got, total);
    let mut buf = vec![0; 64 * 1024];
    loop {
        match Halt::read(halt) {
            Halt::Go => {}
            h => return Err(Stopped::Halted(h)),
        }
        let n = response.read(&mut buf).map_err(|e| failed(&e))?;
        if n == 0 {
            break;
        }
        let chunk = buf.get(..n).unwrap_or_default();
        file.write_all(chunk).map_err(|e| failed(&e))?;
        got += n as u64;
        if got > MAX_BYTES {
            return Err(Stopped::Failed("the file is far too large".into()));
        }
        progress(got, total);
    }
    file.flush().map_err(|e| failed(&e))?;
    match total {
        Some(t) if got < t => Err(Stopped::Failed("the download was cut short".into())),
        _ => Ok(()),
    }
}

/// Where a 206 body starts, from `Content-Range: bytes N-M/T`.
fn starts_at(response: &reqwest::blocking::Response) -> Option<u64> {
    let range = response.headers().get(CONTENT_RANGE)?.to_str().ok()?;
    let rest = range.strip_prefix("bytes ")?;
    rest.split('-').next()?.trim().parse().ok()
}

/// A parquet at both ends; anything else isn't kept.
pub fn check(path: &Path) -> Result<(), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let mut head = [0; 4];
    let mut tail = [0; 4];
    file.read_exact(&mut head)
        .map_err(|_| "the file is empty")?;
    file.seek(SeekFrom::End(-4)).map_err(|e| e.to_string())?;
    file.read_exact(&mut tail).map_err(|e| e.to_string())?;
    match (&head == MAGIC, &tail == MAGIC && len >= 8) {
        (true, true) => Ok(()),
        (true, false) => Err("the file is cut short".into()),
        (false, _) => Err("the server sent something other than a dictionary".into()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use super::*;

    /// A body that is a parquet at both ends.
    fn body() -> Vec<u8> {
        let mut b = b"PAR1".to_vec();
        b.extend((0..200_000u32).map(|i| (i % 251) as u8));
        b.extend_from_slice(b"PAR1");
        b
    }

    /// `/pack` takes ranges, `/cut` drops once halfway, `/page` is HTML,
    /// else 404.
    fn serve() -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let resumes = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&resumes);
        std::thread::spawn(move || {
            let mut cuts = 0;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split(' ').nth(1).unwrap_or("/").to_owned();
                let mut from = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap() <= 2 {
                        break;
                    }
                    if let Some(r) = h.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        from = r.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                if from > 0 {
                    counted.fetch_add(1, Ordering::Relaxed);
                }
                let full = body();
                let (status, bytes, extra) = match path.as_str() {
                    "/pack" | "/cut" if from > 0 => {
                        let range = format!(
                            "Content-Range: bytes {from}-{}/{}\r\n",
                            full.len() - 1,
                            full.len()
                        );
                        ("206 Partial Content", full[from..].to_vec(), range)
                    }
                    "/pack" | "/cut" => ("200 OK", full, String::new()),
                    "/page" => ("200 OK", b"<html>Not here</html>".to_vec(), String::new()),
                    _ => ("404 Not Found", b"no".to_vec(), String::new()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    bytes.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let send = if path == "/cut" && from == 0 && cuts == 0 {
                    cuts += 1;
                    bytes.len() / 2
                } else {
                    bytes.len()
                };
                let _ = stream.write_all(&bytes[..send]);
            }
        });
        (format!("http://{addr}"), resumes)
    }

    #[test]
    fn mirrors_are_tried_in_turn_and_an_error_page_is_never_kept() {
        let (base, _) = serve();
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("p.part");
        let go = AtomicU8::new(0);
        let urls = [
            format!("{base}/gone"),
            format!("{base}/page"),
            format!("{base}/pack"),
        ];
        let mut seen = Vec::new();
        fetch(&urls, &part, &go, &mut |got, total| seen.push((got, total))).unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), body());
        assert_eq!(
            seen.last(),
            Some(&(body().len() as u64, Some(body().len() as u64)))
        );
        let only_bad = [format!("{base}/page")];
        let err = fetch(&only_bad, &part, &go, &mut |_, _| {});
        assert_eq!(
            err,
            Err(Stopped::Failed(
                "the server sent something other than a dictionary".into()
            ))
        );
        assert!(!part.exists());
    }

    #[test]
    fn a_dropped_connection_resumes_where_it_stopped() {
        let (base, resumes) = serve();
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("p.part");
        let go = AtomicU8::new(0);
        let urls = [format!("{base}/cut")];
        fetch(&urls, &part, &go, &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), body());
        assert_eq!(resumes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_pause_keeps_the_part_and_a_cancel_drops_it() {
        let (base, _) = serve();
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("p.part");
        let urls = [format!("{base}/pack")];
        let pause = AtomicU8::new(Halt::Pause as u8);
        let err = fetch(&urls, &part, &pause, &mut |_, _| {});
        assert_eq!(err, Err(Stopped::Halted(Halt::Pause)));
        assert!(part.exists());
        let cancel = AtomicU8::new(Halt::Cancel as u8);
        let err = fetch(&urls, &part, &cancel, &mut |_, _| {});
        assert_eq!(err, Err(Stopped::Halted(Halt::Cancel)));
        assert!(!part.exists());
    }

    #[test]
    fn a_parquet_is_checked_at_both_ends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        std::fs::write(&path, b"PAR1 half").unwrap();
        assert_eq!(check(&path), Err("the file is cut short".into()));
        std::fs::write(&path, b"").unwrap();
        assert!(check(&path).is_err());
        std::fs::write(&path, b"PAR1PAR1").unwrap();
        assert_eq!(check(&path), Ok(()));
    }
}
