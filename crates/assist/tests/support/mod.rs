//! A loopback HTTP server replaying scripted answers, and fixtures.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One scripted answer.
#[derive(Clone)]
pub struct Answer {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl Answer {
    pub fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: body.to_owned(),
        }
    }

    /// A chat stream sending `text` in `pieces` content deltas.
    pub fn stream(pieces: &[&str]) -> Self {
        let mut body = String::new();
        for p in pieces {
            let chunk = serde_json::json!({
                "id": "c1", "model": "m",
                "choices": [{"index": 0, "delta": {"content": p}, "finish_reason": null}]
            });
            body.push_str(&format!("data: {chunk}\n\n"));
        }
        let end =
            r#"{"id":"c1","model":"m","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        body.push_str(&format!("data: {end}\n\ndata: [DONE]\n\n"));
        Self {
            status: 200,
            content_type: "text/event-stream",
            body,
        }
    }
}

/// A request as the server saw it.
#[derive(Clone, Debug)]
pub struct Seen {
    pub line: String,
    pub auth: Option<String>,
    pub body: serde_json::Value,
}

pub struct Server {
    pub url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    /// Answers in order, repeating the last; runs on the current runtime.
    pub async fn start(answers: Vec<Answer>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/", listener.local_addr().unwrap());
        let script = Arc::new(Mutex::new(VecDeque::from(answers)));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let (script, log) = (Arc::clone(&script), Arc::clone(&log));
                tokio::spawn(async move {
                    let Some(seen) = read(&mut sock).await else {
                        return;
                    };
                    log.lock().unwrap().push(seen);
                    let answer = {
                        let mut q = script.lock().unwrap();
                        if q.len() > 1 {
                            q.pop_front()
                        } else {
                            q.front().cloned()
                        }
                    };
                    let a = answer.unwrap_or_else(|| Answer::json(500, "{}"));
                    let head = format!(
                        "HTTP/1.1 {} X\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        a.status,
                        a.content_type,
                        a.body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(a.body.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Self { url, seen }
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

async fn read(sock: &mut tokio::net::TcpStream) -> Option<Seen> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let end = loop {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).to_string();
    let header = |name: &str| {
        head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
        })
    };
    let length: usize = header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    while buf.len() < end + length {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let body = buf.get(end..).unwrap_or_default();
    Some(Seen {
        line: head.lines().next().unwrap_or_default().to_owned(),
        auth: header("authorization"),
        body: serde_json::from_slice(body).unwrap_or(serde_json::Value::Null),
    })
}

/// The answer `quick` gets, in JSON.
pub const QUICK: &str = r#"{"pos":"adjective","meaning":"Fast.","synonyms":["swift","rapid"],"usages":["Be quick.","A quick look."]}"#;
