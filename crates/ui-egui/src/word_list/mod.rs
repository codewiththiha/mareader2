//! Word lists of picked pages: read, levelled, looked up, set out.

use egui::Context;
use mareader_dict::{Reply, Ticket};
use mareader_settings::WordOutput;

use crate::dict::Dict;
use mareader_engine::toast::Toast;
use mareader_engine::word_list::Meanings;

pub mod preview;
pub mod window;

pub use mareader_engine::word_list::{Ask, Gather, Made, Progress, WordLists, copied, file_stem};

/// The app's dictionaries answer what a list asks for.
impl Meanings for Dict {
    fn languages(&self) -> Vec<&'static str> {
        Dict::languages(self)
    }

    fn ask_words(&mut self, words: Vec<String>, to: Vec<String>, near: bool) -> Ticket {
        Dict::ask_words(self, words, to, near)
    }

    fn take_words(&mut self, ticket: Ticket) -> Option<Reply> {
        Dict::take_words(self, ticket)
    }
}

/// Copies or saves `made`; a toast for now, as saving toasts later.
pub fn deliver(
    ctx: &Context,
    files: &crate::files::Files,
    title: &str,
    made: &Made,
    output: WordOutput,
) -> Option<Toast> {
    let stem = file_stem(title);
    match output {
        WordOutput::Copy => {
            ctx.copy_text(made.markdown.clone());
            Some(Toast::info(copied(made)))
        }
        WordOutput::Markdown => {
            let name = format!("{stem}.md");
            let bytes = made.markdown.clone().into_bytes();
            files.save_as(ctx, ("Save word list", name), ("Markdown", "md"), bytes);
            None
        }
        WordOutput::Epub => {
            let name = format!("{stem}.epub");
            let none = |_: &str| None;
            let format = mareader_epub::Format::Markdown;
            match mareader_epub::convert(made.markdown.as_bytes(), format, Some(&name), &none) {
                Ok(bytes) => {
                    files.save_as(ctx, ("Save word list", name), ("EPUB", "epub"), bytes);
                    None
                }
                Err(e) => Some(Toast::error(format!("Couldn't make the EPUB: {e}"))),
            }
        }
    }
}
