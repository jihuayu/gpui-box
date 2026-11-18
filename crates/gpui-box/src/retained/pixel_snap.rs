//! Optional output projection; layout, intrinsic queries and text stay logical.
use super::*;

/// Presentation geometry policy. Scale is physical pixels per logical unit,
/// including any host zoom. It does not change Flex, shaping or query inputs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PixelSnap {
    #[default]
    None,
    /// Snap absolute border/content/clip edges to nearest device pixels, with
    /// ties away from zero. Glyph geometry, radii and effect lengths stay logical.
    DevicePixels { scale: f32 },
}

struct ProjectedAncestor {
    node: NodeId,
    origin: [f32; 2],
    clip: Option<Rect>,
    clip_property: Option<NodeId>,
}

impl PixelSnap {
    pub fn validate(self) -> Result<(), Error> {
        if let Self::DevicePixels { scale } = self
            && (!scale.is_finite() || scale <= 0.0 || !(1.0 / scale).is_finite())
        {
            return Err(Error::InvalidEnvironment);
        }
        Ok(())
    }

    pub(in crate::retained) fn accepts_scale(self, actual: f32) -> bool {
        match self {
            Self::None => true,
            Self::DevicePixels { scale } => scale == actual && self.validate().is_ok(),
        }
    }

    pub(super) fn apply(
        self,
        items: &mut [PaintItem],
        properties: &mut PropertyTrees,
    ) -> Result<(), Error> {
        if self == Self::None {
            return Ok(());
        }
        self.validate()?;
        let rect = |value| self.project_rect(value);
        // Projection bakes visual translations/scroll into parent-local output
        // placement. The mutable owner's authored properties are untouched.
        let mut builder = properties::PropertyBuilder::default();
        let mut stack: Vec<ProjectedAncestor> = vec![];
        for item in items {
            spatial::validate_geometry(item.bounds, item.content_bounds, item.clip, &item.spatial)?;
            while stack.last().map(|entry| entry.node) != item.spatial.parent {
                stack.pop().ok_or(Error::InvalidFrame)?;
            }
            let mut bounds = rect(item.bounds)?;
            let mut content = rect(item.content_bounds)?;
            let mut clip = item.clip.map(rect).transpose()?;
            let content_clip = item.content_clip.map(rect).transpose()?;
            let root_clip = item.spatial.root_clip.map(rect).transpose()?;
            let inherited = stack.last().map_or(root_clip, |entry| entry.clip);
            if let Some(parent_clip) = inherited {
                clip = clip.map(|value| value.intersect(parent_clip));
            }
            let parent_origin = stack.last().map_or([0.0; 2], |entry| entry.origin);
            let local = [bounds.x - parent_origin[0], bounds.y - parent_origin[1]];
            // Canonicalize through the consumer's exact f32 operation order.
            // Arbitrary device grids are not exactly representable in f32.
            let origin = [parent_origin[0] + local[0], parent_origin[1] + local[1]];
            bounds.x = origin[0];
            bounds.y = origin[1];
            let offsets = [content.x - bounds.x, content.y - bounds.y, 0.0, 0.0];
            content.x = bounds.x + offsets[0];
            content.y = bounds.y + offsets[1];
            let own_clip = clip.filter(|_| clip != inherited).map(|r| Rect {
                x: r.x - bounds.x,
                y: r.y - bounds.y,
                ..r
            });
            clip = match (inherited, own_clip.map(|r| r.translated(origin))) {
                (Some(a), Some(b)) => Some(a.intersect(b)),
                (a, b) => a.or(b),
            };
            // Projection bakes the effective axis clip into a finite local
            // content viewport, just as it bakes inherited visual placement.
            let overflow_clip =
                item.spatial
                    .overflow_clip
                    .and(content_clip)
                    .map(|r| OverflowClip {
                        bounds: Rect {
                            x: r.x - origin[0],
                            y: r.y - origin[1],
                            ..r
                        },
                        axes: [true; 2],
                    });
            item.spatial = SpatialState {
                parent: item.spatial.parent,
                layout_origin: local,
                translation: [0.0; 2],
                scroll: [0.0; 2],
                clip: own_clip,
                overflow_clip,
                root_clip,
            };
            item.bounds = bounds;
            item.content_bounds = content;
            item.content_offsets = offsets;
            item.clip = clip;
            item.content_clip = item.spatial.content_clip(origin, clip);
            item.properties.transform = None;
            item.properties.scroll = None;
            // Reuse equal inherited clips; a stricter local clip includes the
            // effective intersection, while retaining its ancestor identity.
            item.properties.clip = own_clip
                .map(|_| item.node)
                .or_else(|| stack.last().and_then(|entry| entry.clip_property));
            use properties::PropertyOutput;
            builder.record(
                item.node,
                None,
                own_clip.map(|value| PropertyNode {
                    parent: stack.last().and_then(|entry| entry.clip_property),
                    value,
                }),
                None,
                properties.effects.get(&item.node).cloned(),
            );
            spatial::validate_geometry(item.bounds, item.content_bounds, item.clip, &item.spatial)?;
            stack.push(ProjectedAncestor {
                node: item.node,
                origin,
                clip: item.content_clip,
                clip_property: item.properties.clip,
            });
        }
        *properties = builder.finish();
        Ok(())
    }

    pub(super) fn project_rect(self, r: Rect) -> Result<Rect, Error> {
        let Self::DevicePixels { scale } = self else {
            return Ok(r);
        };
        let snap = |value: f32| -> Result<f32, Error> {
            let physical = f64::from(value) * f64::from(scale);
            let value = (physical.round() / f64::from(scale)) as f32;
            if !physical.is_finite()
                || physical.abs() > f64::from(f32::MAX)
                || !value.is_finite()
                || !(value * scale).is_finite()
            {
                return Err(Error::InvalidFrame);
            }
            Ok(value)
        };
        if !r.valid() {
            return Err(Error::InvalidFrame);
        }
        let x = snap(r.x)?;
        let y = snap(r.y)?;
        let right = snap(r.x + r.width)?;
        let bottom = snap(r.y + r.height)?;
        let result = Rect {
            x,
            y,
            width: right - x,
            height: bottom - y,
        };
        if !result.valid() {
            return Err(Error::InvalidFrame);
        }
        Ok(result)
    }
}

impl UiTree {
    pub fn pixel_snap(&self) -> PixelSnap {
        self.pixel_snap
    }

    /// Change future output projection without invalidating logical layout.
    /// For atomic sources/lifetimes/frame output use `LogicalEdit::pixel_snap`.
    pub fn set_pixel_snap(&mut self, policy: PixelSnap) -> Result<Commit, Error> {
        policy.validate()?;
        if self.pixel_snap == policy {
            return Ok(Commit {
                revision: self.revision,
                ..Default::default()
            });
        }
        self.advance()?;
        self.apply_pixel_snap(policy);
        Ok(Commit {
            revision: self.revision,
            request_frame: true,
            ..Default::default()
        })
    }

    pub(super) fn apply_pixel_snap(&mut self, policy: PixelSnap) {
        self.pixel_snap = policy;
        self.full_extract_required = true;
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests;
