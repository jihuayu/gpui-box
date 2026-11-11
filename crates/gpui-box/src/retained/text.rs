//! Owned grayscale glyph resources and width-dependent local paint chunks.
use super::{paragraph::ParagraphSnapshot, *};
use std::{
    collections::VecDeque,
    sync::{Mutex, Weak},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GlyphKey {
    epoch: usize,
    font: usize,
    glyph: u32,
    size: u32,
    scale: u32,
}
#[derive(Debug)]
pub struct GlyphBitmap {
    id: u64,
    pub offset: [i32; 2],
    pub size: [u32; 2],
    mask: Arc<[u8]>,
}
impl GlyphBitmap {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn mask(&self) -> &Arc<[u8]> {
        &self.mask
    }
}

/// Owner-side raster cache. Weak entries never retain pixels after resources
/// and packets are released. IDs are scoped by real text system and font epoch.
pub struct GlyphRasterizer {
    system: Arc<gpui::TextSystem>,
    glyphs: BTreeMap<GlyphKey, Weak<GlyphBitmap>>,
    rasterized: u64,
}
impl GlyphRasterizer {
    pub fn new(system: Arc<gpui::TextSystem>) -> Self {
        Self {
            system,
            glyphs: BTreeMap::new(),
            rasterized: 0,
        }
    }
    pub fn rasterized_glyphs(&self) -> u64 {
        self.rasterized
    }
    pub fn prepare(
        &mut self,
        paragraph: ParagraphSnapshot,
        scale: f32,
    ) -> Result<TextResource, Error> {
        if !scale.is_finite()
            || scale <= 0.0
            || !(1.0 / scale).is_finite()
            || paragraph.text_system_id() != self.system.identity()
            || paragraph.font_epoch() != self.system.font_epoch()
        {
            return Err(Error::InvalidTextResource);
        }
        self.glyphs.retain(|_, glyph| glyph.strong_count() > 0);
        let mut glyphs = BTreeMap::new();
        let mut mask_bytes = 0usize;
        for line in paragraph.shaped_lines() {
            let device_em = f32::from(line.font_size) * scale;
            if !device_em.is_finite() || device_em > 1024.0 {
                return Err(Error::UnsupportedText);
            }
            for run in &line.runs {
                for glyph in &run.glyphs {
                    if glyph.is_emoji {
                        return Err(Error::UnsupportedText);
                    }
                    let key = GlyphKey {
                        epoch: paragraph.font_epoch(),
                        font: run.font_id.0,
                        glyph: glyph.id.0,
                        size: f32::from(line.font_size).to_bits(),
                        scale: scale.to_bits(),
                    };
                    if glyphs.contains_key(&key) {
                        continue;
                    }
                    let bitmap = if let Some(cached) = self.glyphs.get(&key).and_then(Weak::upgrade)
                    {
                        cached
                    } else {
                        let params = gpui::RenderGlyphParams {
                            font_id: run.font_id,
                            glyph_id: glyph.id,
                            font_size: line.font_size,
                            subpixel_variant: Default::default(),
                            scale_factor: scale,
                            is_emoji: false,
                            subpixel_rendering: false,
                            dilation: 0,
                        };
                        let (bounds, bytes) = self
                            .system
                            .rasterize_glyph_owned(&params)
                            .map_err(|error| Error::Gpu(error.to_string()))?;
                        let width = bounds.size.width.0;
                        let height = bounds.size.height.0;
                        if width < 0
                            || height < 0
                            || width > 1024
                            || height > 1024
                            || bytes.len() as u64 != width as u64 * height as u64
                        {
                            return Err(Error::UnsupportedText);
                        }
                        static NEXT: AtomicU64 = AtomicU64::new(1);
                        let id = NEXT
                            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                                n.checked_add(1)
                            })
                            .map_err(|_| Error::RevisionExhausted)?;
                        let bitmap = Arc::new(GlyphBitmap {
                            id,
                            offset: [bounds.origin.x.0, bounds.origin.y.0],
                            size: [width as u32, height as u32],
                            mask: bytes.into(),
                        });
                        self.glyphs.insert(key, Arc::downgrade(&bitmap));
                        self.rasterized += 1;
                        bitmap
                    };
                    mask_bytes += bitmap.mask.len();
                    if mask_bytes > 32 * 1024 * 1024 {
                        return Err(Error::UnsupportedText);
                    }
                    glyphs.insert(key, bitmap);
                }
            }
        }
        if paragraph.font_epoch() != self.system.font_epoch() {
            return Err(Error::InvalidTextResource);
        }
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::RevisionExhausted)?;
        Ok(TextResource(Arc::new(Resource {
            id,
            paragraph,
            scale,
            glyphs,
            chunks: Mutex::new(VecDeque::new()),
        })))
    }
}
type ChunkCache = VecDeque<(u32, Arc<GlyphChunk>)>;
#[derive(Debug)]
struct Resource {
    id: u64,
    paragraph: ParagraphSnapshot,
    scale: f32,
    glyphs: BTreeMap<GlyphKey, Arc<GlyphBitmap>>,
    chunks: Mutex<ChunkCache>,
}
#[derive(Clone, Debug)]
pub struct TextResource(Arc<Resource>);
impl PartialEq for TextResource {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}
impl TextResource {
    pub fn paragraph(&self) -> &ParagraphSnapshot {
        &self.0.paragraph
    }
    pub fn scale(&self) -> f32 {
        self.0.scale
    }
    pub fn chunk(&self, width: f32) -> Result<Arc<GlyphChunk>, Error> {
        if !width.is_finite() || width < 0.0 {
            return Err(Error::InvalidMeasure);
        }
        let key = if width == 0.0 { 0 } else { width.to_bits() };
        let mut cache = self
            .0
            .chunks
            .lock()
            .map_err(|_| Error::ParagraphCachePoisoned)?;
        if let Some(index) = cache.iter().position(|(cached, _)| *cached == key) {
            let entry = cache.remove(index).unwrap();
            let result = entry.1.clone();
            cache.push_back(entry);
            return Ok(result);
        }
        let layout = self.0.paragraph.layout(Some(width))?;
        let mut glyphs = vec![];
        let mut ink: Option<Rect> = None;
        let mut hard_y = 0.0;
        for line in &layout.lines {
            let baseline = (self.0.paragraph.line_height()
                - f32::from(line.ascent())
                - f32::from(line.descent()))
                / 2.0
                + f32::from(line.ascent());
            let mut boundaries = line.wrap_boundaries().iter().peekable();
            let mut line_x = 0.0;
            let mut line_y = hard_y;
            for (run_ix, run) in line.runs().iter().enumerate() {
                for (glyph_ix, glyph) in run.glyphs.iter().enumerate() {
                    if boundaries.peek().is_some_and(|boundary| {
                        **boundary == gpui::WrapBoundary { run_ix, glyph_ix }
                    }) {
                        boundaries.next();
                        line_x = f32::from(glyph.position.x);
                        line_y += self.0.paragraph.line_height();
                    }
                    let key = GlyphKey {
                        epoch: self.paragraph().font_epoch(),
                        font: run.font_id.0,
                        glyph: glyph.id.0,
                        size: f32::from(line.font_size()).to_bits(),
                        scale: self.scale().to_bits(),
                    };
                    let bitmap = self.0.glyphs.get(&key).ok_or(Error::InvalidTextResource)?;
                    if bitmap.size.contains(&0) {
                        continue;
                    }
                    let origin = [
                        f32::from(glyph.position.x) - line_x,
                        line_y + baseline + f32::from(glyph.position.y),
                    ];
                    let bounds = Rect {
                        x: origin[0] + bitmap.offset[0] as f32 / self.scale() - 1.0 / self.scale(),
                        y: origin[1] + bitmap.offset[1] as f32 / self.scale() - 1.0 / self.scale(),
                        width: (bitmap.size[0] + 2) as f32 / self.scale(),
                        height: (bitmap.size[1] + 2) as f32 / self.scale(),
                    };
                    ink = Some(ink.map_or(bounds, |ink| ink.union(bounds)));
                    glyphs.push(PositionedGlyph {
                        origin,
                        bitmap: bitmap.clone(),
                        font: run.font_id,
                        glyph: glyph.id,
                        font_size: line.font_size(),
                    });
                }
            }
            hard_y += f32::from(line.size(gpui::px(self.paragraph().line_height())).height);
        }
        let chunk = Arc::new(GlyphChunk {
            glyphs: glyphs.into(),
            ink,
        });
        if cache.len() == 4 {
            cache.pop_front();
        }
        cache.push_back((key, chunk.clone()));
        Ok(chunk)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct TextPaint {
    pub resource: TextResource,
    pub color: u32,
}
#[derive(Debug)]
pub struct PositionedGlyph {
    pub origin: [f32; 2],
    pub bitmap: Arc<GlyphBitmap>,
    pub font: gpui::FontId,
    pub glyph: gpui::GlyphId,
    pub font_size: gpui::Pixels,
}
#[derive(Debug)]
pub struct GlyphChunk {
    pub glyphs: Arc<[PositionedGlyph]>,
    pub ink: Option<Rect>,
}

impl PaintItem {
    pub(super) fn same_text(&self, other: &Self) -> bool {
        self.text == other.text
            && match (&self.glyphs, &other.glyphs) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
    /// Physical bounds under the retained integer-origin grayscale contract.
    pub fn glyph_bounds(&self, glyph: &PositionedGlyph, scale: f32) -> Rect {
        Rect {
            x: ((self.content_bounds.x + glyph.origin[0]) * scale).round()
                + glyph.bitmap.offset[0] as f32,
            y: ((self.content_bounds.y + glyph.origin[1]) * scale).round()
                + glyph.bitmap.offset[1] as f32,
            width: glyph.bitmap.size[0] as f32,
            height: glyph.bitmap.size[1] as f32,
        }
    }
}
