//! Why a file won't open, and whether one that did looks cut short.

use hayro::hayro_syntax::{DecryptionError, LoadPdfError};

/// Where a PDF's header may sit: the specification allows junk before it.
const HEAD: usize = 1024;
/// Where `%%EOF` should sit: writers may leave a little after it.
const TAIL: usize = 1024;

/// Why a file didn't open, after PDFCraft's error kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    #[error("the file is empty")]
    EmptyFile,
    #[error("the file isn't a PDF")]
    NotPdf,
    #[error("the PDF is cut short")]
    CutShort,
    #[error("the PDF is damaged")]
    Damaged,
    #[error("the PDF is locked with a password")]
    Locked,
    #[error("the PDF's encryption can't be read")]
    Encryption,
    #[error("the PDF has no pages")]
    NoPages,
    #[error("the PDF has too many pages")]
    TooManyPages,
}

impl OpenError {
    /// What the reader can do about it, as a sentence.
    pub fn hint(self) -> Option<&'static str> {
        Some(match self {
            Self::EmptyFile => "Copy or download it again.",
            Self::NotPdf => "It may be a web page or another file named .pdf.",
            Self::CutShort => "It may not have finished downloading; download it again.",
            Self::Damaged => "Another app may open it and save a readable copy.",
            Self::Locked | Self::Encryption => {
                "An app that can unlock it may save an unlocked copy."
            }
            Self::NoPages | Self::TooManyPages => return None,
        })
    }
}

/// The reason a file hayro refused gives, from what its bytes show.
pub(crate) fn diagnose(bytes: &[u8], error: &LoadPdfError) -> OpenError {
    match error {
        LoadPdfError::Decryption(DecryptionError::PasswordProtected) => OpenError::Locked,
        LoadPdfError::Decryption(_) => OpenError::Encryption,
        LoadPdfError::Invalid if bytes.iter().all(u8::is_ascii_whitespace) => OpenError::EmptyFile,
        LoadPdfError::Invalid if !has_header(bytes) => OpenError::NotPdf,
        LoadPdfError::Invalid if cut_short(bytes) => OpenError::CutShort,
        LoadPdfError::Invalid => OpenError::Damaged,
    }
}

/// `%PDF-` within the first kilobyte.
fn has_header(bytes: &[u8]) -> bool {
    let head = bytes.get(..HEAD.min(bytes.len())).unwrap_or_default();
    head.windows(5).any(|w| w == b"%PDF-")
}

/// No `%%EOF` near the end, as when a download stopped early.
pub(crate) fn cut_short(bytes: &[u8]) -> bool {
    let tail = bytes
        .get(bytes.len().saturating_sub(TAIL)..)
        .unwrap_or_default();
    !tail.windows(5).any(|w| w == b"%%EOF")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_names_its_cause() {
        let invalid = LoadPdfError::Invalid;
        assert_eq!(diagnose(b"", &invalid), OpenError::EmptyFile);
        assert_eq!(diagnose(b" \n\t", &invalid), OpenError::EmptyFile);
        let page = b"<!DOCTYPE html><html><body>Sign in</body></html>";
        assert_eq!(diagnose(page, &invalid), OpenError::NotPdf);
        let cut = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog";
        assert_eq!(diagnose(cut, &invalid), OpenError::CutShort);
        let broken = b"%PDF-1.7\ngarbage\n%%EOF\n";
        assert_eq!(diagnose(broken, &invalid), OpenError::Damaged);
        let locked = LoadPdfError::Decryption(DecryptionError::PasswordProtected);
        assert_eq!(diagnose(broken, &locked), OpenError::Locked);
        let odd = LoadPdfError::Decryption(DecryptionError::UnsupportedAlgorithm);
        assert_eq!(diagnose(broken, &odd), OpenError::Encryption);
    }

    #[test]
    fn a_header_after_junk_still_counts_but_not_past_a_kilobyte() {
        let mut late = vec![b' '; 600];
        late.extend_from_slice(b"%PDF-1.4\n");
        assert!(has_header(&late));
        let mut too_late = vec![b'x'; HEAD];
        too_late.extend_from_slice(b"%PDF-1.4\n");
        assert!(!has_header(&too_late));
    }

    #[test]
    fn an_end_marker_with_a_little_after_it_is_whole() {
        let mut whole = b"%PDF-1.7\n...%%EOF".to_vec();
        whole.extend_from_slice(&[b'\n'; 200]);
        assert!(!cut_short(&whole));
        assert!(cut_short(b"%PDF-1.7\n1 0 obj"));
        assert!(OpenError::NoPages.hint().is_none());
        assert!(OpenError::Locked.hint().is_some());
    }
}
