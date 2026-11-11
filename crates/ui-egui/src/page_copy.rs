//! Copies whole pages' text, read off the frame when it may take a while.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::Context;
use mareader_session::Document;

use crate::toast::Toast;

/// Pages this few, or a book's, are read in the frame.
const INLINE: usize = 4;

/// The one copy being read; a newer one replaces it.
#[derive(Debug, Default)]
pub struct PageCopy {
    reading: Option<Receiver<Copied>>,
}

#[derive(Debug)]
struct Copied {
    text: String,
    pages: usize,
}

impl PageCopy {
    /// Starts reading `pages` of `document`, to copy them once read.
    pub fn start(&mut self, ctx: &Context, document: Arc<Document>, pages: Vec<usize>) {
        let (tx, rx) = channel();
        let inline = pages.len() <= INLINE || document.is_reflowable();
        let ctx = ctx.clone();
        let work = move || {
            let copied = Copied {
                text: joined(&document, &pages),
                pages: pages.len(),
            };
            let _ = tx.send(copied);
            ctx.request_repaint();
        };
        if inline || cfg!(target_arch = "wasm32") {
            work();
        } else {
            let named = std::thread::Builder::new().name("page-copy".into());
            // Unspawned, the copy just never lands.
            let _ = named.spawn(work);
        }
        self.reading = Some(rx);
    }

    /// Puts read text on the clipboard; the toast to say so.
    pub fn poll(&mut self, ctx: &Context) -> Option<Toast> {
        let copied = match self.reading.as_ref()?.try_recv() {
            Ok(copied) => copied,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                self.reading = None;
                return Some(Toast::info("Couldn't read those pages"));
            }
        };
        self.reading = None;
        if copied.text.is_empty() {
            return Some(Toast::info("No text to copy on these pages"));
        }
        ctx.copy_text(copied.text);
        Some(Toast::info(match copied.pages {
            1 => "Copied the page's text".to_owned(),
            n => format!("Copied the text of {n} pages"),
        }))
    }
}

/// The pages' text in order, a blank line between pages.
fn joined(document: &Document, pages: &[usize]) -> String {
    let texts = pages.iter().filter_map(|&page| {
        let layer = document.text_layer(page).ok()?;
        let text = layer.text.text(0..layer.text.len());
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    });
    texts.collect::<Vec<_>>().join("\n\n")
}
