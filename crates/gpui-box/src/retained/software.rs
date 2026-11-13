//! Experimental CPU consumer. Run construction, stepping and destruction on a
//! worker; a work quota bounds algorithmic work, not OS allocation/scheduling.
//! Only a completed image may replace the host's shown image and hit snapshot.
use super::{pacing::SoftwareEffects, *};

const TILE: u32 = 32;
const PIXELS: usize = (TILE * TILE) as usize;
const MAX_DEPTH: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoftwareError {
    InvalidSurface,
    InvalidFrame,
    ResourceLimit,
    UnsupportedShadows,
    InvalidTextScale,
    Cancelled,
}

#[derive(Clone, Copy, Debug)]
pub struct SoftwareOptions {
    pub size: [u32; 2],
    pub scale: f32,
    pub background: u32,
    pub effects: SoftwareEffects,
    /// Output, fixed tile scratch, plan capacity and conservative effect-index
    /// storage. Input packet/resources and allocator overhead are excluded.
    pub max_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SoftwareStats {
    pub prepared_items: usize,
    pub work_units: u64,
    pub raster_samples: u64,
    pub omitted_shadows: usize,
    pub tiles: usize,
}

/// Premultiplied RGBA8 in the current encoded-channel blending convention.
/// AA/sampling equivalence with WGPU is not yet qualified for fractional edges.
pub struct SoftwareFrame {
    packet: FramePacket,
    pub size: [u32; 2],
    pub scale: f32,
    pixels: Vec<u8>,
    pub stats: SoftwareStats,
}
impl SoftwareFrame {
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    pub fn packet(&self) -> &FramePacket {
        &self.packet
    }
    /// Geometry belongs to this completed image, even if a newer job is pending.
    /// The host must install this frame only after its native upload succeeds.
    pub fn hit_test(&self, physical: [f32; 2]) -> Option<NodeId> {
        if physical.iter().any(|n| !n.is_finite())
            || physical[0] < 0.0
            || physical[1] < 0.0
            || physical[0] >= self.size[0] as f32
            || physical[1] >= self.size[1] as f32
        {
            return None;
        }
        self.packet
            .hit_test(physical[0] / self.scale, physical[1] / self.scale)
    }
}

#[derive(Clone, Copy)]
enum Op {
    Begin(f32),
    End,
    Quad(usize, usize),
    Image(usize),
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    Glyphs(usize),
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Prepare,
    Clear,
    Draw,
    Copy,
    Complete,
    Cancelled,
}

/// Resumable complete-frame raster job. `step` never publishes partial pixels.
/// A unit is one bounded item preparation (<=32 effects, <=64 commands), one
    /// dispatch/skip, or one pixel operation. Resource allocation is worker-only.
/// Hosts choose quotas, yield to input, and may abandon obsolete visual jobs;
/// lossless state transactions must already have committed before this point.
pub struct SoftwareJob {
    packet: FramePacket,
    options: SoftwareOptions,
    output: Vec<u8>,
    layers: Vec<[f32; 4]>,
    opacities: [f32; MAX_DEPTH + 1],
    ops: Vec<Op>,
    path: Vec<NodeId>,
    closed: std::collections::BTreeSet<NodeId>,
    stage: Stage,
    item: usize,
    tile: [u32; 2],
    depth: usize,
    op: usize,
    cursor: usize,
    area: Option<[u32; 4]>,
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    glyph: usize,
    stats: SoftwareStats,
}

impl SoftwareJob {
    pub fn new(packet: FramePacket, options: SoftwareOptions) -> Result<Self, SoftwareError> {
        if options.size.contains(&0)
            || !options.scale.is_finite()
            || options.scale <= 0.0
            || !(1.0 / options.scale).is_finite()
        {
            return Err(SoftwareError::InvalidSurface);
        }
        let bytes = u64::from(options.size[0])
            .checked_mul(u64::from(options.size[1]))
            .and_then(|n| n.checked_mul(4))
            .ok_or(SoftwareError::ResourceLimit)?;
        let scratch = (MAX_DEPTH + 1) * PIXELS * std::mem::size_of::<[f32; 4]>();
        if bytes > options.max_bytes as u64
            || scratch > options.max_bytes.saturating_sub(bytes as usize)
        {
            return Err(SoftwareError::ResourceLimit);
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(bytes as usize)
            .map_err(|_| SoftwareError::ResourceLimit)?;
        // Output is initialized incrementally; no full-surface memset per slice.
        let mut layers = Vec::new();
        layers
            .try_reserve_exact((MAX_DEPTH + 1) * PIXELS)
            .map_err(|_| SoftwareError::ResourceLimit)?;
        layers.resize((MAX_DEPTH + 1) * PIXELS, [0.0; 4]);
        Ok(Self {
            packet,
            options,
            output,
            layers,
            opacities: [1.0; MAX_DEPTH + 1],
            ops: vec![],
            path: vec![],
            closed: Default::default(),
            stage: Stage::Prepare,
            item: 0,
            tile: [0, 0],
            depth: 0,
            op: 0,
            cursor: 0,
            area: None,
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            glyph: 0,
            stats: SoftwareStats::default(),
        })
    }
    pub fn stats(&self) -> SoftwareStats {
        self.stats
    }
    pub fn is_complete(&self) -> bool {
        self.stage == Stage::Complete
    }
    pub fn cancel(&mut self) {
        self.stage = Stage::Cancelled;
    }
    pub fn into_frame(self) -> Result<SoftwareFrame, SoftwareError> {
        if self.stage == Stage::Cancelled {
            return Err(SoftwareError::Cancelled);
        }
        if !self.is_complete() {
            return Err(SoftwareError::InvalidFrame);
        }
        Ok(SoftwareFrame {
            packet: self.packet,
            size: self.options.size,
            scale: self.options.scale,
            pixels: self.output,
            stats: self.stats,
        })
    }
    /// Returns true only after every tile is complete. Zero work does nothing.
    /// Any error poisons this job: a later call cannot publish an incomplete plan.
    pub fn step(&mut self, quota: usize) -> Result<bool, SoftwareError> {
        if self.stage == Stage::Cancelled {
            return Err(SoftwareError::Cancelled);
        }
        for _ in 0..quota {
            if self.is_complete() {
                return Ok(true);
            }
            if let Err(error) = self.unit() {
                self.stage = Stage::Cancelled;
                return Err(error);
            }
            self.stats.work_units += 1;
        }
        Ok(self.is_complete())
    }
    fn plan_push(&mut self, op: Op) -> Result<(), SoftwareError> {
        // Charge actual reserved capacity, including growth; input resources are
        // already owned by the packet. Growth is bounded and fallible.
        if self.ops.len() == self.ops.capacity() {
            let add = 64.max(self.ops.len());
            let base = self.output.capacity()
                + self.layers.capacity() * std::mem::size_of::<[f32; 4]>()
                + self.closed.len().saturating_mul(128);
            let next = self
                .ops
                .capacity()
                .checked_add(add)
                .and_then(|n| n.checked_mul(std::mem::size_of::<Op>()));
            if next.is_none_or(|n| n > self.options.max_bytes.saturating_sub(base)) {
                return Err(SoftwareError::ResourceLimit);
            }
            self.ops
                .try_reserve_exact(add)
                .map_err(|_| SoftwareError::ResourceLimit)?;
        }
        self.ops.push(op);
        Ok(())
    }
    fn prepare(&mut self) -> Result<(), SoftwareError> {
        if self.item == self.packet.items.len() {
            if self.path.pop().is_some() {
                self.plan_push(Op::End)?;
                return Ok(());
            }
            self.stage = Stage::Clear;
            return Ok(());
        }
        let item = &self.packet.items[self.item];
        if item.node.tree != self.packet.tree
            || !item.bounds.valid()
            || !item.content_bounds.valid()
            || item.clip.is_some_and(|r| !r.valid())
            || item.commands.len() > 64
        {
            return Err(SoftwareError::InvalidFrame);
        }
        if !item.shadows.is_empty() && self.options.effects == SoftwareEffects::Exact {
            return Err(SoftwareError::UnsupportedShadows);
        }
        #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
        if item
            .text
            .as_ref()
            .is_some_and(|text| text.resource.scale() != self.options.scale)
            || item.text.is_some() != item.glyphs.is_some()
        {
            return Err(SoftwareError::InvalidTextScale);
        }
        let mut path = Vec::with_capacity(MAX_DEPTH);
        let mut cursor = item.properties.effect;
        while let Some(id) = cursor {
            if path.len() == MAX_DEPTH {
                return Err(SoftwareError::ResourceLimit);
            }
            let effect = self
                .packet
                .properties
                .effects
                .get(&id)
                .ok_or(SoftwareError::InvalidFrame)?;
            if !effect.value.is_finite() || !(0.0..=1.0).contains(&effect.value) {
                return Err(SoftwareError::InvalidFrame);
            }
            path.push(id);
            cursor = effect.parent;
        }
        path.reverse();
        let common = path
            .iter()
            .zip(&self.path)
            .take_while(|(a, b)| a == b)
            .count();
        while self.path.len() > common {
            let base = self.output.capacity()
                + self.layers.capacity() * std::mem::size_of::<[f32; 4]>()
                + self.ops.capacity() * std::mem::size_of::<Op>();
            if (self.closed.len() + 1).saturating_mul(128)
                > self.options.max_bytes.saturating_sub(base)
            {
                return Err(SoftwareError::ResourceLimit);
            }
            self.closed.insert(self.path.pop().unwrap());
            self.plan_push(Op::End)?;
        }
        for id in path.into_iter().skip(common) {
            if self.closed.contains(&id) {
                return Err(SoftwareError::InvalidFrame);
            }
            self.plan_push(Op::Begin(self.packet.properties.effects[&id].value))?;
            self.path.push(id);
        }
        self.stats.omitted_shadows += self.packet.items[self.item].shadows.len();
        for command in 0..self.packet.items[self.item].commands.len() {
            let (_, decoration) = self.packet.items[self.item].commands[command].quad();
            if decoration.validate().is_err() {
                return Err(SoftwareError::InvalidFrame);
            }
            self.plan_push(Op::Quad(self.item, command))?;
        }
        if self.packet.items[self.item].image.is_some() {
            self.plan_push(Op::Image(self.item))?;
        }
        #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
        if self.packet.items[self.item].glyphs.is_some() {
            self.plan_push(Op::Glyphs(self.item))?;
        }
        self.item += 1;
        self.stats.prepared_items += 1;
        Ok(())
    }
    fn unit(&mut self) -> Result<(), SoftwareError> {
        match self.stage {
            Stage::Prepare => self.prepare()?,
            Stage::Clear => {
                // Extend at most 4 KiB in a unit. Full-surface storage remains
                // private, and no raster work starts before initialization.
                let length = self.options.size[0] as usize * self.options.size[1] as usize * 4;
                if self.output.len() < length {
                    self.output
                        .resize((self.output.len() + 4096).min(length), 0);
                } else {
                    self.layers[self.cursor] = color(self.options.background);
                    self.cursor += 1;
                    if self.cursor == PIXELS {
                        self.cursor = 0;
                        self.stage = Stage::Draw;
                    }
                }
            }
            Stage::Draw => self.draw()?,
            Stage::Copy => {
                let x = self.tile[0] + (self.cursor % TILE as usize) as u32;
                let y = self.tile[1] + (self.cursor / TILE as usize) as u32;
                if x < self.options.size[0] && y < self.options.size[1] {
                    let dst = (y as usize * self.options.size[0] as usize + x as usize) * 4;
                    for c in 0..4 {
                        self.output[dst + c] =
                            (self.layers[self.cursor][c].clamp(0.0, 1.0) * 255.0).round() as u8;
                    }
                }
                self.cursor += 1;
                if self.cursor == PIXELS {
                    self.stats.tiles += 1;
                    self.tile[0] += TILE;
                    if self.tile[0] >= self.options.size[0] {
                        self.tile[0] = 0;
                        self.tile[1] += TILE;
                    }
                    self.op = 0;
                    self.cursor = 0;
                    self.stage = if self.tile[1] >= self.options.size[1] {
                        Stage::Complete
                    } else {
                        Stage::Clear
                    };
                }
            }
            Stage::Complete | Stage::Cancelled => (),
        }
        Ok(())
    }
    fn advance(&mut self) {
        self.op += 1;
        self.cursor = 0;
        self.area = None;
        #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
        {
            self.glyph = 0;
        }
    }
    fn draw(&mut self) -> Result<(), SoftwareError> {
        let Some(op) = self.ops.get(self.op).copied() else {
            if self.depth != 0 {
                return Err(SoftwareError::InvalidFrame);
            }
            self.stage = Stage::Copy;
            return Ok(());
        };
        match op {
            Op::Begin(opacity) => {
                self.layers[(self.depth + 1) * PIXELS + self.cursor] = [0.0; 4];
                self.cursor += 1;
                if self.cursor == PIXELS {
                    self.depth += 1;
                    self.opacities[self.depth] = opacity;
                    self.advance();
                }
            }
            Op::End => {
                if self.depth == 0 {
                    return Err(SoftwareError::InvalidFrame);
                }
                let src = self.layers[self.depth * PIXELS + self.cursor]
                    .map(|v| v * self.opacities[self.depth]);
                over(
                    &mut self.layers[(self.depth - 1) * PIXELS + self.cursor],
                    src,
                );
                self.cursor += 1;
                if self.cursor == PIXELS {
                    self.depth -= 1;
                    self.advance();
                }
            }
            _ => self.primitive(op)?,
        }
        Ok(())
    }
    fn primitive(&mut self, op: Op) -> Result<(), SoftwareError> {
        let (index, bounds) = match op {
            Op::Quad(i, _) => (i, scaled(self.packet.items[i].bounds, self.options.scale)),
            Op::Image(i) => (
                i,
                scaled(self.packet.items[i].content_bounds, self.options.scale),
            ),
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            Op::Glyphs(i) => {
                let item = &self.packet.items[i];
                let glyphs = &item.glyphs.as_ref().unwrap().glyphs;
                let Some(glyph) = glyphs.get(self.glyph) else {
                    self.advance();
                    return Ok(());
                };
                if glyph.bitmap.mask().len() as u64
                    != u64::from(glyph.bitmap.size[0]) * u64::from(glyph.bitmap.size[1])
                {
                    return Err(SoftwareError::InvalidFrame);
                }
                (i, item.glyph_bounds(glyph, self.options.scale))
            }
            _ => unreachable!(),
        };
        if !bounds.valid() {
            return Err(SoftwareError::InvalidFrame);
        }
        let item = &self.packet.items[index];
        let clip = item.clip.map(|r| scaled(r, self.options.scale));
        if self.area.is_none() {
            let mut ink = bounds;
            if let Some(clip) = clip {
                ink = ink.intersect(clip);
            }
            let ink = ink.intersect(Rect {
                x: self.tile[0] as f32,
                y: self.tile[1] as f32,
                width: TILE.min(self.options.size[0] - self.tile[0]) as f32,
                height: TILE.min(self.options.size[1] - self.tile[1]) as f32,
            });
            self.area = Some([
                ink.x.floor() as u32,
                ink.y.floor() as u32,
                (ink.x + ink.width).ceil() as u32,
                (ink.y + ink.height).ceil() as u32,
            ]);
            if ink.is_empty() {
                self.finish_primitive(op);
                return Ok(());
            }
        }
        let [left, top, right, bottom] = self.area.unwrap();
        let width = (right - left) as usize;
        if width == 0 || bottom <= top {
            self.finish_primitive(op);
            return Ok(());
        }
        let x = left + (self.cursor % width) as u32;
        let y = top + (self.cursor / width) as u32;
        let point = [x as f32 + 0.5, y as f32 + 0.5];
        let inclusive = matches!(op, Op::Quad(_, _));
        let contains = |r: Rect| {
            !r.is_empty()
                && point[0] >= r.x
                && point[1] >= r.y
                && if inclusive {
                    point[0] <= r.x + r.width && point[1] <= r.y + r.height
                } else {
                    point[0] < r.x + r.width && point[1] < r.y + r.height
                }
        };
        let clip_coverage = if contains(bounds) && clip.is_none_or(contains) {
            1.0
        } else {
            0.0
        };
        let src = match op {
            Op::Quad(_, command) => {
                let (fill, d) = item.resolved_quad(&item.commands[command]);
                let d = d.resolved(item.bounds);
                let outer = coverage(bounds, d.radius * self.options.scale, point);
                let border = d.border_width * self.options.scale;
                let inner = Rect {
                    x: bounds.x + border,
                    y: bounds.y + border,
                    width: (bounds.width - 2.0 * border).max(0.0),
                    height: (bounds.height - 2.0 * border).max(0.0),
                };
                let inside = if border == 0.0 {
                    outer
                } else {
                    coverage(
                        inner,
                        (d.radius * self.options.scale - border).max(0.0),
                        point,
                    )
                    .min(outer)
                };
                let f = color(fill);
                let b = color(d.border_color);
                std::array::from_fn(|c| (f[c] * inside + b[c] * (outer - inside)) * clip_coverage)
            }
            Op::Image(_) => {
                let image = item.image.as_ref().unwrap();
                let u = ((point[0] - bounds.x) / bounds.width * image.size()[0] as f32)
                    .floor()
                    .clamp(0.0, image.size()[0] as f32 - 1.0) as usize;
                let v = ((point[1] - bounds.y) / bounds.height * image.size()[1] as f32)
                    .floor()
                    .clamp(0.0, image.size()[1] as f32 - 1.0) as usize;
                let n = (v * image.size()[0] as usize + u) * 4;
                let rgba = u32::from_be_bytes(image.pixels()[n..n + 4].try_into().unwrap());
                color(rgba).map(|c| c * clip_coverage)
            }
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            Op::Glyphs(_) => {
                let glyph = &item.glyphs.as_ref().unwrap().glyphs[self.glyph];
                let gx = (point[0] - bounds.x).floor() as usize;
                let gy = (point[1] - bounds.y).floor() as usize;
                let mask = glyph
                    .bitmap
                    .mask()
                    .get(gy * glyph.bitmap.size[0] as usize + gx)
                    .copied()
                    .unwrap_or(0) as f32
                    / 255.0;
                color(item.text.as_ref().unwrap().color).map(|c| c * mask * clip_coverage)
            }
            _ => unreachable!(),
        };
        let target = (y - self.tile[1]) as usize * TILE as usize + (x - self.tile[0]) as usize;
        over(&mut self.layers[self.depth * PIXELS + target], src);
        self.stats.raster_samples += 1;
        self.cursor += 1;
        if self.cursor >= width * (bottom - top) as usize {
            self.finish_primitive(op);
        }
        Ok(())
    }
    fn finish_primitive(&mut self, op: Op) {
        #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
        if matches!(op, Op::Glyphs(_)) {
            self.glyph += 1;
            self.cursor = 0;
            self.area = None;
            return;
        }
        let _ = op;
        self.advance();
    }
}

fn color(rgba: u32) -> [f32; 4] {
    let [r, g, b, a] = rgba.to_be_bytes();
    let a = a as f32 / 255.0;
    [
        r as f32 / 255.0 * a,
        g as f32 / 255.0 * a,
        b as f32 / 255.0 * a,
        a,
    ]
}
fn over(dst: &mut [f32; 4], src: [f32; 4]) {
    for c in 0..4 {
        dst[c] = src[c] + dst[c] * (1.0 - src[3]);
    }
}
fn scaled(r: Rect, s: f32) -> Rect {
    Rect {
        x: r.x * s,
        y: r.y * s,
        width: r.width * s,
        height: r.height * s,
    }
}
fn coverage(r: Rect, radius: f32, p: [f32; 2]) -> f32 {
    if r.is_empty() {
        return 0.0;
    }
    let q = [
        (p[0] - r.x - r.width / 2.0).abs() - r.width / 2.0 + radius,
        (p[1] - r.y - r.height / 2.0).abs() - r.height / 2.0 + radius,
    ];
    let distance = q[0].max(0.0).hypot(q[1].max(0.0)) + q[0].max(q[1]).min(0.0) - radius;
    (0.5 - distance).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests;
