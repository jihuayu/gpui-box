//! Ordinary logical Text component; owner preparation precedes pure layout.
use super::*;
use crate::retained::{logical::*, reactive::Property, text_style::*};
use std::{
    cell::{Cell, RefCell},
    fmt,
    rc::Rc,
};
mod cache;
use cache::PreparationCache;
pub use cache::{TextCacheLimits, TextCacheStats};

/// Owner-local resource adapter, never invoked by layout or a renderer. It must
/// return immutable resources from these explicit inputs; memoization is allowed.
/// External font changes require new input/preparer identity to invalidate use.
pub trait TextPreparer: 'static {
    fn prepare(
        &self,
        text: &str,
        typography: &TextTypography,
        scale: f32,
        font_revision: u64,
    ) -> Result<TextResource, Error>;
}
/// Shared owner-local adapter identity. Does not carry a Window borrow or cross
/// the immutable frame boundary; only its prepared resource leaves the owner.
#[derive(Clone)]
pub struct TextPreparation(Rc<dyn TextPreparer>);
impl TextPreparation {
    pub fn new(preparer: impl TextPreparer) -> Self {
        Self(Rc::new(preparer))
    }
}
impl PartialEq for TextPreparation {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
impl fmt::Debug for TextPreparation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextPreparation").finish_non_exhaustive()
    }
}
/// Existing platform shaping/grayscale glyph adapter. Text and typography use
/// the original ParagraphSnapshot/GlyphRasterizer contracts and restrictions.
pub struct GpuiTextPreparer {
    system: Rc<gpui::WindowTextSystem>,
    raster: RefCell<GlyphRasterizer>,
    prepared: Cell<u64>,
    cache: RefCell<PreparationCache>,
}
impl GpuiTextPreparer {
    pub fn new(system: Rc<gpui::WindowTextSystem>) -> Self {
        let raster = GlyphRasterizer::new(Arc::clone(&system));
        let cache = PreparationCache::new(system.font_epoch());
        Self {
            system,
            raster: RefCell::new(raster),
            prepared: Cell::new(0),
            cache: RefCell::new(cache),
        }
    }
    pub fn prepared_resources(&self) -> u64 {
        self.prepared.get()
    }
    pub fn rasterized_glyphs(&self) -> u64 {
        self.raster.borrow().rasterized_glyphs()
    }
    pub fn cache_stats(&self) -> TextCacheStats {
        self.cache.borrow().stats()
    }
    /// Limit retained preparation payloads. Zero entries or bytes disables
    /// storage; current frame resources remain owned and usable after eviction.
    pub fn set_cache_limits(&self, limits: TextCacheLimits) {
        self.cache.borrow_mut().set_limits(limits);
    }
    pub fn clear_cache(&self) {
        self.cache.borrow_mut().clear();
    }
}
impl TextPreparer for GpuiTextPreparer {
    fn prepare(
        &self,
        text: &str,
        typography: &TextTypography,
        scale: f32,
        font_revision: u64,
    ) -> Result<TextResource, Error> {
        typography.validate()?;
        if !scale.is_finite() || scale <= 0.0 || !(1.0 / scale).is_finite() {
            return Err(Error::InvalidTextResource);
        }
        let epoch = self.system.font_epoch();
        if let Some(resource) =
            self.cache
                .borrow_mut()
                .get(text, typography, scale, font_revision, epoch)
        {
            if self.system.font_epoch() != epoch {
                return Err(Error::InvalidTextResource);
            }
            return Ok(resource);
        }
        let mut font = gpui::font(typography.font.family.to_string());
        font.weight = gpui::FontWeight(typography.font.weight as f32);
        font.style = match typography.font.slant {
            FontSlant::Normal => gpui::FontStyle::Normal,
            FontSlant::Italic => gpui::FontStyle::Italic,
            FontSlant::Oblique => gpui::FontStyle::Oblique,
        };
        let paragraph = ParagraphSnapshot::shape(
            &self.system,
            text,
            font,
            typography.font_size,
            typography.line_height,
        )?;
        let resource = self.raster.borrow_mut().prepare(paragraph, scale)?;
        self.cache.borrow_mut().insert(
            text,
            typography,
            scale,
            font_revision,
            epoch,
            resource.clone(),
        );
        self.prepared.set(self.prepared.get().saturating_add(1));
        Ok(resource)
    }
}
impl TextPreparer for Rc<GpuiTextPreparer> {
    fn prepare(
        &self,
        text: &str,
        typography: &TextTypography,
        scale: f32,
        font_revision: u64,
    ) -> Result<TextResource, Error> {
        (**self).prepare(text, typography, scale, font_revision)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct TextProps {
    pub content: Arc<str>,
    pub style: TextStyle,
    pub layout: LayoutStyle,
    /// Logical-to-device scale for glyph resource preparation, not text zoom.
    pub scale: f32,
    /// Explicit host invalidation after font database/fallback changes. It is
    /// input data, not an automatic OS subscription or renderer resource epoch.
    pub font_revision: u64,
    pub preparation: TextPreparation,
}
impl TextProps {
    pub fn new(content: impl Into<Arc<str>>, preparation: TextPreparation) -> Self {
        Self {
            content: content.into(),
            style: TextStyle::default(),
            layout: LayoutStyle::column(),
            scale: 1.0,
            font_revision: 0,
            preparation,
        }
    }
}
#[derive(Clone, PartialEq)]
struct Request {
    content: Arc<str>,
    typography: TextTypography,
    scale: f32,
    font_revision: u64,
    preparation: TextPreparation,
}
/// A mount-once ordinary leaf. Logical text inheritance is automatic; Flex
/// sizing, wrap, baseline, overflow, immutable frames and sinks remain ordinary.
pub struct Text;
impl Component for Text {
    type Props = TextProps;
    fn mount(
        p: ReadProp<TextProps>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let style = cx.computed(0, &[p.dependency()], move |i| {
            Ok(i.read_prop(p)?.style.clone())
        })?;
        let resolved = cx.resolve_text_style(1, crate::retained::text_style::prop(style))?;
        let request = cx.computed(2, &[p.dependency(), resolved.dependency()], move |i| {
            let p = i.read_prop(p)?;
            if !p.scale.is_finite() || p.scale <= 0.0 || !(1.0 / p.scale).is_finite() {
                return Err(Error::InvalidTextResource);
            }
            Ok(Request {
                content: p.content.clone(),
                typography: i.read_prop(resolved)?.typography()?,
                scale: p.scale,
                font_revision: p.font_revision,
                preparation: p.preparation.clone(),
            })
        })?;
        let resource = cx.computed(3, &[request.dependency()], move |i| {
            let r = i.read(request)?;
            let prepared =
                r.preparation
                    .0
                    .prepare(&r.content, &r.typography, r.scale, r.font_revision)?;
            if prepared.scale() != r.scale {
                return Err(Error::InvalidTextResource);
            }
            Ok(prepared)
        })?;
        let paint = cx.computed(
            4,
            &[resource.dependency(), resolved.dependency()],
            move |i| {
                Ok(Some(TextPaint {
                    resource: i.read(resource)?.clone(),
                    color: i.read_prop(resolved)?.color,
                }))
            },
        )?;
        let layout = cx.computed(5, &[p.dependency()], move |i| {
            Ok(i.read_prop(p)?.layout.clone())
        })?;
        let node = cx.append_layout(LayoutStyle::column(), 0)?;
        cx.bind(layout, Property::layout_style(node))?;
        cx.bind(paint, Property::text(node))?;
        Ok(())
    }
}
