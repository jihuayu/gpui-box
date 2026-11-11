//! Owned platform-shaped paragraphs. Wrapping/measurement borrow no UI context.
use super::*;
use gpui::{Font, ShapedLine, TextRun, WindowTextSystem, px};
use std::{collections::VecDeque, sync::Mutex};

type WrapCache = VecDeque<(Option<u32>, Arc<ParagraphLayout>)>;

#[derive(Debug)]
struct Paragraph {
    id: u64,
    font_epoch: usize,
    line_height: f32,
    lines: Arc<[ShapedLine]>,
    min_width: f32,
    max_width: f32,
    baseline: f32,
    /// Four recent widths; memoization changes no observable layout result.
    wraps: Mutex<WrapCache>,
}

/// Shape on the UI owner, then transfer the owned snapshot to pure layout.
/// This first contract is a uniform font, hard newlines and GPUI soft wrapping.
/// It stores glyph IDs/positions, not a Font/Window callback or character-width
/// approximation. The source text system's font epoch is captured at shaping.
#[derive(Clone, Debug)]
pub struct ParagraphSnapshot(Arc<Paragraph>);
impl PartialEq for ParagraphSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

#[derive(Debug)]
pub struct ParagraphLayout {
    pub lines: Vec<gpui::WrappedLine>,
    pub size: [f32; 2],
    pub first_baseline: f32,
}

impl ParagraphSnapshot {
    pub fn shape(
        text_system: &WindowTextSystem,
        text: &str,
        font: Font,
        font_size: f32,
        line_height: f32,
    ) -> Result<Self, Error> {
        if !font_size.is_finite()
            || font_size <= 0.0
            || !line_height.is_finite()
            || line_height <= 0.0
            || text.len() > 1024 * 1024
        {
            return Err(Error::InvalidMeasure);
        }
        if text.contains(['\r', '\t']) {
            return Err(Error::UnsupportedText);
        }
        let font_epoch = text_system.font_epoch();
        let mut lines = vec![];
        let mut min_width = 0.0f32;
        let mut max_width = 0.0f32;
        for text in text.split('\n') {
            let line = text_system.shape_line(
                text.to_owned().into(),
                px(font_size),
                &[TextRun {
                    len: text.len(),
                    font: font.clone(),
                    ..Default::default()
                }],
                None,
            );
            let metrics = [
                f32::from(line.width()),
                f32::from(line.ascent),
                f32::from(line.descent),
            ];
            if metrics.iter().any(|n| !n.is_finite() || *n < 0.0)
                || metrics[1] + metrics[2] > line_height
            {
                return Err(Error::InvalidMeasure);
            }
            let mut previous = None;
            for glyph in line.runs.iter().flat_map(|run| &run.glyphs) {
                let x = f32::from(glyph.position.x);
                let y = f32::from(glyph.position.y);
                if !x.is_finite()
                    || !y.is_finite()
                    || glyph.index >= text.len()
                    || !text.is_char_boundary(glyph.index)
                    || previous
                        .is_some_and(|(index, previous_x)| index > glyph.index || previous_x > x)
                {
                    return Err(Error::UnsupportedText);
                }
                previous = Some((glyph.index, x));
            }
            max_width = max_width.max(metrics[0]);
            // The existing wrapper's emergency breaks preserve shaped clusters.
            // Min-content for this initial anywhere-emergency policy is its
            // largest indivisible cluster advance, not zero available width.
            let wrapped = line.wrap_owned(Some(px(0.0)));
            let mut start = 0.0;
            for boundary in wrapped.wrap_boundaries() {
                let x = f32::from(
                    line.runs[boundary.run_ix].glyphs[boundary.glyph_ix]
                        .position
                        .x,
                );
                min_width = min_width.max((x - start).max(0.0));
                start = x;
            }
            min_width = min_width.max((metrics[0] - start).max(0.0));
            lines.push(line);
        }
        if text_system.font_epoch() != font_epoch {
            return Err(Error::InvalidMeasure);
        }
        if !(line_height * text.chars().count().max(lines.len()) as f32).is_finite() {
            return Err(Error::InvalidMeasure);
        }
        let first = &lines[0];
        let baseline = (line_height - f32::from(first.ascent) - f32::from(first.descent)) / 2.0
            + f32::from(first.ascent);
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::RevisionExhausted)?;
        Ok(Self(Arc::new(Paragraph {
            id,
            font_epoch,
            line_height,
            lines: lines.into(),
            min_width,
            max_width,
            baseline,
            wraps: Mutex::new(VecDeque::new()),
        })))
    }
    pub fn font_epoch(&self) -> usize {
        self.0.font_epoch
    }
    pub fn line_height(&self) -> f32 {
        self.0.line_height
    }
    pub fn hard_line_count(&self) -> usize {
        self.0.lines.len()
    }
    pub fn min_content_width(&self) -> f32 {
        self.0.min_width
    }
    pub fn max_content_width(&self) -> f32 {
        self.0.max_width
    }
    pub fn first_baseline(&self) -> f32 {
        self.0.baseline
    }
    /// Owns wrapped layouts sharing the original shaped runs. No shaping occurs.
    pub fn layout(&self, width: Option<f32>) -> Result<Arc<ParagraphLayout>, Error> {
        if width.is_some_and(|w| !w.is_finite() || w < 0.0) {
            return Err(Error::InvalidMeasure);
        }
        let width = width.map(|width| if width == 0.0 { 0.0 } else { width });
        let key = width.map(f32::to_bits);
        let mut cache = self
            .0
            .wraps
            .lock()
            .map_err(|_| Error::ParagraphCachePoisoned)?;
        if let Some(index) = cache.iter().position(|(cached, _)| *cached == key) {
            let entry = cache.remove(index).expect("cached width");
            let result = entry.1.clone();
            cache.push_back(entry);
            return Ok(result);
        }
        let lines: Vec<_> = self
            .0
            .lines
            .iter()
            .map(|line| line.wrap_owned(width.map(px)))
            .collect();
        let height = lines
            .iter()
            .map(|line| f32::from(line.size(px(self.0.line_height)).height))
            .sum::<f32>();
        let actual_width = width.map_or(self.0.max_width, |width| width.min(self.0.max_width));
        let result = Arc::new(ParagraphLayout {
            lines,
            size: [actual_width, height],
            first_baseline: self.0.baseline,
        });
        if cache.len() == 4 {
            cache.pop_front();
        }
        cache.push_back((key, result.clone()));
        Ok(result)
    }
    pub(super) fn measure(&self, available: AvailableSpace) -> Size<f32> {
        let width = match available {
            AvailableSpace::Definite(width) => Some(width.max(0.0)),
            AvailableSpace::MinContent => Some(self.0.min_width),
            AvailableSpace::MaxContent => None,
        };
        let layout = self.layout(width).expect("validated Taffy content width");
        Size {
            width: layout.size[0],
            height: layout.size[1],
        }
    }
}
