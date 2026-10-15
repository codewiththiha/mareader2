//! Book images: sized from their header up front, decoded when drawn.
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use image::{DynamicImage, ImageReader, Limits};
use vello_cpu::peniko::ImageAlphaType;
use vello_cpu::{PixelMetadata, Pixmap};

/// Larger images are refused before any pixel is decoded.
pub const MAX_PICTURE_SIDE: u32 = 16_384;
pub const MAX_PICTURE_PIXELS: u64 = 50_000_000;
/// Decoded images kept for redraws, in bytes of pixels.
const CACHE_BYTES: usize = 64 << 20;
/// Decoding never allocates more than this.
const DECODE_BYTES: u64 = 256 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PictureError {
    #[error("not an image format the reader knows")]
    Unknown,
    #[error("the image is too large")]
    TooLarge,
}

/// An encoded image and its pixel size; cheap to probe, decoded lazily.
pub struct Picture {
    data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Picture({}×{})", self.width, self.height)
    }
}

/// Pictures compare by identity: one image file, one picture.
impl PartialEq for Picture {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Picture {
    /// Reads only the header: format, then size within the caps.
    pub fn probe(data: Vec<u8>) -> Result<Self, PictureError> {
        let reader = ImageReader::new(Cursor::new(&data))
            .with_guessed_format()
            .map_err(|_| PictureError::Unknown)?;
        if reader.format().is_none() {
            return Err(PictureError::Unknown);
        }
        let (width, height) = reader
            .into_dimensions()
            .map_err(|_| PictureError::Unknown)?;
        let pixels = u64::from(width) * u64::from(height);
        if width == 0 || height == 0 {
            return Err(PictureError::Unknown);
        }
        if width > MAX_PICTURE_SIDE || height > MAX_PICTURE_SIDE || pixels > MAX_PICTURE_PIXELS {
            return Err(PictureError::TooLarge);
        }
        Ok(Self {
            data,
            width,
            height,
        })
    }

    /// A 1 × 1 stand-in that never decodes, for tests.
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self {
            data: Vec::new(),
            width: 1,
            height: 1,
        }
    }

    /// Height over width.
    pub fn aspect(&self) -> f32 {
        self.height as f32 / self.width as f32
    }

    /// Straight-alpha RGBA at most `side` on its long edge: width, height,
    /// pixels.
    pub fn thumbnail(&self, side: u32) -> Option<(u32, u32, Vec<u8>)> {
        let rgba = self.rgba(side)?;
        Some((rgba.width(), rgba.height(), rgba.into_raw()))
    }

    /// Decodes and shrinks to fit `side`, never enlarging.
    fn rgba(&self, side: u32) -> Option<image::RgbaImage> {
        let mut reader = ImageReader::new(Cursor::new(&self.data))
            .with_guessed_format()
            .ok()?;
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_PICTURE_SIDE);
        limits.max_image_height = Some(MAX_PICTURE_SIDE);
        limits.max_alloc = Some(DECODE_BYTES);
        reader.limits(limits);
        let mut img = reader.decode().ok()?;
        if img.width().max(img.height()) > side {
            img = img.resize(side, side, image::imageops::FilterType::Triangle);
        }
        Some(DynamicImage::into_rgba8(img))
    }

    /// Decodes and shrinks to fit `side` pixels; `None` if it won't decode.
    fn decode(&self, side: u32) -> Option<Pixmap> {
        let rgba = self.rgba(side)?;
        let (w, h) = (
            u16::try_from(rgba.width()).ok()?,
            u16::try_from(rgba.height()).ok()?,
        );
        let meta = PixelMetadata {
            may_have_transparency: true,
            alpha_type: ImageAlphaType::Alpha,
        };
        Some(Pixmap::from_parts(rgba.into_raw(), w, h, meta))
    }
}

/// A decode for one size bucket; `None` marks an image that won't decode.
struct Entry {
    picture: Arc<Picture>,
    side: u32,
    pixmap: Option<Arc<Pixmap>>,
}

impl Entry {
    fn bytes(&self) -> usize {
        self.pixmap
            .as_ref()
            .map_or(0, |p| usize::from(p.width()) * usize::from(p.height()) * 4)
    }
}

/// Recently drawn decodes, least recent first; shared by the workers.
#[derive(Default)]
pub(crate) struct Decoded(Mutex<Vec<Entry>>);

impl Decoded {
    /// `picture` decoded to at least `need` pixels on its long side.
    pub(crate) fn get(&self, picture: &Arc<Picture>, need: f32) -> Option<Arc<Pixmap>> {
        let full = picture.width.max(picture.height);
        // Power-of-two buckets: zooming reuses a decode, sampling stays sharp.
        let side = (need.max(1.0).ceil() as u32)
            .next_power_of_two()
            .clamp(64, 4096)
            .min(full);
        let found = |e: &Entry| Arc::ptr_eq(&e.picture, picture) && e.side == side;
        if let Ok(mut cache) = self.0.lock()
            && let Some(at) = cache.iter().position(found)
        {
            let entry = cache.remove(at);
            let pixmap = entry.pixmap.clone();
            cache.push(entry);
            return pixmap;
        }
        // Decode unlocked, so other pages keep drawing meanwhile.
        let pixmap = picture.decode(side).map(Arc::new);
        if let Ok(mut cache) = self.0.lock() {
            cache.retain(|e| !found(e));
            cache.push(Entry {
                picture: picture.clone(),
                side,
                pixmap: pixmap.clone(),
            });
            let mut total: usize = cache.iter().map(Entry::bytes).sum();
            while total > CACHE_BYTES && cache.len() > 1 {
                total -= cache.first().map_or(0, Entry::bytes);
                cache.remove(0);
            }
        }
        pixmap
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A `w × h` PNG, red fading to blue.
    pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, _| {
            let t = (x * 255 / w.max(1)) as u8;
            image::Rgb([255 - t, 40, t])
        });
        let mut out = Cursor::new(Vec::new());
        let _ = img.write_to(&mut out, image::ImageFormat::Png);
        out.into_inner()
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
            }
        }
        !crc
    }

    #[test]
    fn probing_reads_the_size_without_decoding() {
        let p = Picture::probe(png(30, 20));
        assert_eq!(p.as_ref().map(|p| (p.width, p.height)).ok(), Some((30, 20)));
    }

    #[test]
    fn junk_and_giants_are_refused() {
        assert_eq!(
            Picture::probe(b"not an image".to_vec()).err(),
            Some(PictureError::Unknown)
        );
        // A PNG header claiming 20000 × 20000 pixels, CRC fixed up.
        let mut giant = png(1, 1);
        if let Some(ihdr) = giant.get_mut(16..24) {
            ihdr.copy_from_slice(&[0, 0, 0x4e, 0x20, 0, 0, 0x4e, 0x20]);
        }
        let crc = crc32(giant.get(12..29).unwrap_or_default());
        if let Some(at) = giant.get_mut(29..33) {
            at.copy_from_slice(&crc.to_be_bytes());
        }
        assert_eq!(Picture::probe(giant).err(), Some(PictureError::TooLarge));
    }

    #[test]
    fn decodes_shrink_to_their_bucket_and_are_reused() {
        let p = Picture::probe(png(600, 300)).map(Arc::new);
        let Ok(p) = p else {
            return assert!(p.is_ok(), "probe");
        };
        let cache = Decoded::default();
        let a = cache.get(&p, 100.0);
        assert_eq!(a.as_ref().map(|a| (a.width(), a.height())), Some((128, 64)));
        let b = cache.get(&p, 120.0);
        assert!(matches!((&a, &b), (Some(a), Some(b)) if Arc::ptr_eq(a, b)));
        // Never upscaled past the file's own size.
        let c = cache.get(&p, 5000.0);
        assert_eq!(c.map(|c| c.width()), Some(600));
    }
}
