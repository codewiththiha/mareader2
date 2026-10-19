//! Results set as pages: shrunk to fit, drawn off the frame, cached.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{Context, Rect, Vec2, pos2, vec2};
use mareader_reflow::{Block, Flow, Fonts, MAX_SIDE, PAD, Typography};
use mareader_session::{Document, Raster};

use crate::gpu::Gpu;
use crate::pages::PageTexture;

/// The smallest share of the reader's size a result shrinks to.
const MIN_SHARE: f32 = 0.4;
/// Never smaller than this, in layout units, however it is shared.
const MIN_SIZE: f32 = 6.0;
/// Seconds an unseen card is kept before its texture goes.
const KEEP: f64 = 3.0;

/// What to set: the blocks, their type, and the room on the page.
pub struct Job {
    pub blocks: Arc<Vec<Block>>,
    pub typo: Typography,
    /// The column's width and height, in page units.
    pub room: (f32, f32),
    /// Pixels per page unit.
    pub scale: f32,
    /// The book, whose pictures the result shows.
    pub book: Option<Arc<Document>>,
}

/// A set result, ready to draw.
#[derive(Clone)]
pub struct Card {
    pub texture: PageTexture,
    /// Its width and depth, in page units.
    pub size: Vec2,
    /// Pictures and emoji, in page units from the card's corner.
    pub pictures: Vec<Rect>,
    pub emojis: Vec<Rect>,
}

/// A job for a slot, keyed by what it sets.
type Ask = (u64, u64, Job);

struct Done {
    slot: u64,
    key: u64,
    set: Option<Set>,
}

struct Set {
    pixels: mareader_reflow::Pixels,
    size: Vec2,
    pictures: Vec<Rect>,
    emojis: Vec<Rect>,
    /// A PDF's: put over white for its filters.
    pdf: bool,
}

struct Held {
    key: u64,
    card: Option<Card>,
    seen: f64,
}

/// Each slot's latest card; a stale one shows until its next is set.
pub struct Cards {
    tx: Option<Sender<Ask>>,
    rx: Option<Receiver<Done>>,
    held: HashMap<u64, Held>,
    /// Slots being set now, by key.
    busy: HashMap<u64, u64>,
    /// Cards the frame set, without a worker.
    ready: Vec<Done>,
}

impl Default for Cards {
    fn default() -> Self {
        let (tx, rx) = worker().unzip();
        Self {
            tx,
            rx,
            held: HashMap::new(),
            busy: HashMap::new(),
            ready: Vec::new(),
        }
    }
}

/// A thread that sets cards; none where threads can't start.
fn worker() -> Option<(Sender<Ask>, Receiver<Done>)> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let (jobs, inbox) = channel::<Ask>();
    let (done, news) = channel();
    let spawned = std::thread::Builder::new()
        .name("page-ai-cards".into())
        .spawn(move || {
            while let Ok(first) = inbox.recv() {
                // Only each slot's newest job is worth setting.
                let mut queue = vec![first];
                queue.extend(inbox.try_iter());
                let mut newest: Vec<Ask> = Vec::new();
                for job in queue {
                    newest.retain(|j| j.0 != job.0);
                    newest.push(job);
                }
                for (slot, key, job) in newest {
                    let set = set(&job);
                    if done.send(Done { slot, key, set }).is_err() {
                        return;
                    }
                }
            }
        });
    spawned.ok().map(|_| (jobs, news))
}

impl Cards {
    /// `slot`'s card for `key`; else its last, while `job` is set.
    pub fn get(
        &mut self,
        slot: u64,
        key: u64,
        now: f64,
        job: impl FnOnce() -> Job,
    ) -> Option<Card> {
        let fresh = self.held.get(&slot).is_some_and(|h| h.key == key);
        if !fresh && self.busy.get(&slot) != Some(&key) {
            let unsent = match &self.tx {
                Some(tx) => tx.send((slot, key, job())).err().map(|e| e.0.2),
                None => Some(job()),
            };
            match unsent {
                Some(job) => self.inline(slot, key, &job),
                None => {
                    self.busy.insert(slot, key);
                }
            }
        }
        let held = self.held.get_mut(&slot)?;
        held.seen = now;
        held.card.clone()
    }

    /// Without a worker the frame sets it, for the next receive.
    fn inline(&mut self, slot: u64, key: u64, job: &Job) {
        let set = set(job);
        self.ready.push(Done { slot, key, set });
        self.busy.insert(slot, key);
    }

    /// Sets cards on the frame; else a thread sets them.
    pub fn set_inline(&mut self, inline: bool) {
        if inline {
            (self.tx, self.rx) = (None, None);
        } else if self.tx.is_none() {
            (self.tx, self.rx) = worker().unzip();
        }
    }

    /// Takes finished cards to the GPU; drops ones long unseen.
    pub fn receive(&mut self, ctx: &Context, gpu: Option<&Gpu>, now: f64) -> bool {
        let mut any = false;
        let mut news = std::mem::take(&mut self.ready);
        news.extend(self.rx.iter().flat_map(Receiver::try_iter));
        for done in news {
            if self.busy.get(&done.slot) == Some(&done.key) {
                self.busy.remove(&done.slot);
            }
            let card = done.set.and_then(|s| upload(ctx, gpu, done.slot, s));
            // A failed set keeps the last card rather than none.
            let last = self.held.remove(&done.slot).and_then(|h| h.card);
            self.held.insert(
                done.slot,
                Held {
                    key: done.key,
                    card: card.or(last),
                    seen: now,
                },
            );
            any = true;
        }
        self.held.retain(|_, h| now - h.seen < KEEP);
        // A card being set wakes the frame that shows it.
        if !self.busy.is_empty() {
            ctx.request_repaint_after_secs(0.05);
        }
        any
    }
}

fn upload(ctx: &Context, gpu: Option<&Gpu>, slot: u64, s: Set) -> Option<Card> {
    let raster = Raster {
        width: s.pixels.width,
        height: s.pixels.height,
        pixels: s.pixels.rgba,
    };
    let texture = match gpu {
        Some(gpu) if s.pdf => PageTexture::Gpu(gpu.upload(&on_white(raster))?),
        Some(gpu) => PageTexture::Gpu(gpu.upload(&raster)?),
        None => {
            let size = [raster.width as usize, raster.height as usize];
            let image = egui::ColorImage::from_rgba_premultiplied(size, &raster.pixels);
            let name = format!("page-ai-{slot:x}");
            PageTexture::Egui(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
        }
    };
    Some(Card {
        texture,
        size: s.size,
        pictures: s.pictures,
        emojis: s.emojis,
    })
}

/// Over white, so a PDF's filters theme it as they theme its page.
fn on_white(mut raster: Raster) -> Raster {
    for px in raster.pixels.as_chunks_mut::<4>().0 {
        let [.., alpha] = *px;
        let under = 255 - u16::from(alpha);
        for c in px.iter_mut() {
            *c = (u16::from(*c) + under).min(255) as u8;
        }
    }
    raster
}

/// The largest size that fits one page; past the least, it is drawn smaller.
fn fit(job: &Job) -> Flow {
    let base = job.typo.font_size.max(1.0);
    let least = (base * MIN_SHARE).max(MIN_SIZE.min(base));
    let set = |size: f32, tall: bool| {
        let typo = Typography {
            font_size: size,
            ..job.typo.clone()
        };
        Flow::sized(
            Arc::clone(&job.blocks),
            &typo,
            Fonts::system(),
            job.room,
            tall,
        )
    };
    let whole = set(base, false);
    if whole.page_count() <= 1 {
        return whole;
    }
    if set(least, false).page_count() > 1 {
        return set(least, true);
    }
    // Eight halvings land within a fifth of a unit at any reading size.
    let (mut lo, mut hi) = (least, base);
    for _ in 0..8 {
        let mid = (lo + hi) / 2.0;
        if set(mid, false).page_count() <= 1 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    set(lo, false)
}

fn set(job: &Job) -> Option<Set> {
    let flow = fit(job);
    let flow = match job.book.as_deref() {
        Some(Document::Epub { book, .. }) => book.with_pictures(flow),
        _ => flow,
    };
    let width = job.room.0;
    let depth = flow.depth(0).max(1.0);
    let max = MAX_SIDE as f32;
    let scale = job
        .scale
        .min(max / width.max(1.0))
        .min(max / depth)
        .max(0.05);
    let px = |v: f32| ((v * scale).ceil() as u32).clamp(1, MAX_SIDE);
    let region = [px(PAD), px(PAD), px(width), px(depth)];
    let pixels = flow.render(0, scale, region)?;
    let rect = |r: [f32; 4]| Rect::from_min_size(pos2(r[0] - PAD, r[1] - PAD), vec2(r[2], r[3]));
    Some(Set {
        pixels,
        size: vec2(width, depth),
        pictures: flow.pictures(0).into_iter().map(rect).collect(),
        emojis: flow.emojis(0).into_iter().map(rect).collect(),
        pdf: job.book.is_none(),
    })
}
