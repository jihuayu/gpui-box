//! Render-owned animation data and sampling over an immutable authored scene.
//! Hosts own scheduling, resource preparation and actual image installation.
use super::{local::LocalFrame, *};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

mod curve;
pub use curve::{AnimationCurve, Spring};
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationError {
    InvalidSpec,
    InvalidTarget,
    WrongScene,
    StaleScene,
    StaleSequence,
    ClockReversed,
    Capacity,
    UnsupportedPixelSnap,
    InvalidGeometry,
    Exhausted,
}
impl std::fmt::Display for AnimationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "animation error: {self:?}")
    }
}
impl std::error::Error for AnimationError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnimatedProperty {
    Translation,
    Opacity,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimationValue {
    Translation([f32; 2]),
    Opacity(f32),
}
impl AnimationValue {
    pub fn property(self) -> AnimatedProperty {
        match self {
            Self::Translation(_) => AnimatedProperty::Translation,
            Self::Opacity(_) => AnimatedProperty::Opacity,
        }
    }
    fn channels(self) -> [f64; 2] {
        match self {
            Self::Translation(v) => v.map(f64::from),
            Self::Opacity(v) => [f64::from(v), 0.0],
        }
    }
    fn validate(self) -> Result<(), AnimationError> {
        match self {
            Self::Translation(v) if v.iter().all(|v| v.is_finite()) => Ok(()),
            Self::Opacity(v) if v.is_finite() && (0.0..=1.0).contains(&v) => Ok(()),
            _ => Err(AnimationError::InvalidSpec),
        }
    }
    fn from_channels(property: AnimatedProperty, v: [f64; 2]) -> Result<Self, AnimationError> {
        let value = match property {
            AnimatedProperty::Translation => Self::Translation(v.map(|v| v as f32)),
            // Opacity's finite physical range is independent of spring overshoot.
            AnimatedProperty::Opacity => Self::Opacity(v[0].clamp(0.0, 1.0) as f32),
        };
        value.validate()?;
        Ok(value)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationScene {
    pub tree: u64,
    pub revision: u64,
}
impl AnimationScene {
    pub fn of(frame: &LocalFrame) -> Self {
        Self {
            tree: frame.tree(),
            revision: frame.revision(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationStart {
    Now,
    At(Instant),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetargetVelocity {
    Reset,
    Preserve,
}
/// One controller per node/property. Finished values hold until Release or a new
/// controller, so an unrelated UI scene update cannot revive an old base value.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationSpec {
    pub id: u64,
    pub node: NodeId,
    pub destination: AnimationValue,
    pub curve: AnimationCurve,
    pub duration: Duration,
    pub start: AnimationStart,
    pub velocity: RetargetVelocity,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelBehavior {
    Hold,
    Release,
}
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationControl {
    Animate(AnimationSpec),
    Cancel {
        id: u64,
        behavior: CancelBehavior,
    },
    Finish {
        id: u64,
    },
    /// Give a property back to the latest authored scene. If running, emits
    /// Released. A finished controller has already emitted its terminal result.
    Release {
        node: NodeId,
        property: AnimatedProperty,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationBatch {
    pub scene: AnimationScene,
    pub sequence: u64,
    pub controls: Vec<AnimationControl>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationOutcome {
    Finished,
    Replaced,
    Cancelled,
    Released,
    TargetRemoved,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationNotice {
    pub id: u64,
    pub node: NodeId,
    pub property: AnimatedProperty,
    pub outcome: AnimationOutcome,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationLimits {
    pub max_tracks: usize,
    pub max_notices: usize,
    pub max_controls_per_batch: usize,
}
impl Default for AnimationLimits {
    fn default() -> Self {
        Self {
            max_tracks: 4096,
            max_notices: 8192,
            max_controls_per_batch: 4096,
        }
    }
}
#[derive(Clone)]
struct Track {
    spec: AnimationSpec,
    from: [f64; 2],
    velocity: [f64; 2],
    start: Instant,
    held: Option<AnimationValue>,
}
impl Track {
    fn value(&self, now: Instant) -> Result<(AnimationValue, [f64; 2], bool), AnimationError> {
        if let Some(value) = self.held {
            return Ok((value, [0.0; 2], false));
        }
        let Some(elapsed) = now.checked_duration_since(self.start) else {
            return Ok((
                AnimationValue::from_channels(self.spec.destination.property(), self.from)?,
                [0.0; 2],
                false,
            ));
        };
        if elapsed >= self.spec.duration {
            return Ok((self.spec.destination, [0.0; 2], true));
        }
        let (v, mut velocity) = self.spec.curve.sample(
            self.from,
            self.spec.destination.channels(),
            self.velocity,
            elapsed,
            self.spec.duration,
        )?;
        if self.spec.destination.property() == AnimatedProperty::Opacity
            && !(0.0..=1.0).contains(&v[0])
        {
            velocity = [0.0; 2];
        }
        Ok((
            AnimationValue::from_channels(self.spec.destination.property(), v)?,
            velocity,
            false,
        ))
    }
    fn notice(&self, outcome: AnimationOutcome) -> AnimationNotice {
        AnimationNotice {
            id: self.spec.id,
            node: self.spec.node,
            property: self.spec.destination.property(),
            outcome,
        }
    }
}
/// A complete sampled output. Its geometry and resource leases survive runtime
/// or UI destruction. Input may be projected only from this exact source frame.
#[derive(Clone, Debug)]
pub struct AnimationFrame {
    source: LocalFrame,
    frame: LocalFrame,
    pub sampled_at: Instant,
}
impl AnimationFrame {
    pub fn frame(&self) -> &LocalFrame {
        &self.frame
    }
    pub fn source(&self) -> &LocalFrame {
        &self.source
    }
    pub fn input_scene(
        &self,
        source: &input::UiInputScene,
    ) -> Result<input::UiInputScene, AnimationError> {
        if !source.frame.same(&self.source) {
            return Err(AnimationError::WrongScene);
        }
        let mut scene = source.clone();
        scene.id.presentation = Some(input::UiPresentationId {
            stream: self.frame.stream(),
            revision: self.frame.revision(),
        });
        scene.frame = self.frame.clone();
        Ok(scene)
    }
}
/// One render owner, movable to a worker with no Rc UI objects or callbacks.
/// Calls are atomic on failure. Terminal notices reserve capacity on admission;
/// visual samples can be skipped, accepted controls and outcomes cannot.
pub struct AnimationRuntime {
    source: LocalFrame,
    stream: u64,
    revision: u64,
    next_sequence: u64,
    last_id: u64,
    last_time: Option<Instant>,
    tracks: BTreeMap<(NodeId, AnimatedProperty), Track>,
    notices: VecDeque<AnimationNotice>,
    limits: AnimationLimits,
    previous: Option<AnimationFrame>,
}
impl AnimationRuntime {
    pub fn new(source: LocalFrame, limits: AnimationLimits) -> Result<Self, AnimationError> {
        if source.stream() != source.tree() {
            return Err(AnimationError::WrongScene);
        }
        if source.pixel_snap() != PixelSnap::None {
            return Err(AnimationError::UnsupportedPixelSnap);
        }
        Ok(Self {
            source,
            stream: next_frame_stream().map_err(|_| AnimationError::Exhausted)?,
            revision: 0,
            next_sequence: 1,
            last_id: 0,
            last_time: None,
            tracks: BTreeMap::new(),
            notices: VecDeque::new(),
            limits,
            previous: None,
        })
    }
    pub fn scene(&self) -> AnimationScene {
        AnimationScene::of(&self.source)
    }
    fn candidate(&self) -> Self {
        Self {
            source: self.source.clone(),
            stream: self.stream,
            revision: self.revision,
            next_sequence: self.next_sequence,
            last_id: self.last_id,
            last_time: self.last_time,
            tracks: self.tracks.clone(),
            notices: self.notices.clone(),
            limits: self.limits,
            previous: self.previous.clone(),
        }
    }
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }
    pub fn is_animating(&self) -> bool {
        self.tracks.values().any(|t| t.held.is_none())
    }
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }
    pub fn drain_notices(&mut self) -> impl Iterator<Item = AnimationNotice> + '_ {
        self.notices.drain(..)
    }
    fn check_time(&self, now: Instant) -> Result<(), AnimationError> {
        if self.last_time.is_some_and(|old| now < old) {
            Err(AnimationError::ClockReversed)
        } else {
            Ok(())
        }
    }
    fn check_capacity(&self) -> Result<(), AnimationError> {
        if self.tracks.len() > self.limits.max_tracks
            || self
                .notices
                .len()
                .saturating_add(self.tracks.values().filter(|t| t.held.is_none()).count())
                > self.limits.max_notices
        {
            Err(AnimationError::Capacity)
        } else {
            Ok(())
        }
    }
    fn settle(&mut self, now: Instant) -> Result<(), AnimationError> {
        for track in self.tracks.values_mut() {
            let (value, _, finished) = track.value(now)?;
            if finished {
                self.notices
                    .push_back(track.notice(AnimationOutcome::Finished));
                track.held = Some(value);
            }
        }
        Ok(())
    }
    /// Rebase onto the latest complete authored scene. Existing controllers
    /// keep their sampled trajectory; removal retires only the exact generation.
    pub fn set_scene(&mut self, source: LocalFrame, now: Instant) -> Result<(), AnimationError> {
        self.check_time(now)?;
        if source.tree() != self.source.tree() || source.stream() != source.tree() {
            return Err(AnimationError::WrongScene);
        }
        if source.revision() < self.source.revision() {
            return Err(AnimationError::StaleScene);
        }
        if source.pixel_snap() != PixelSnap::None {
            return Err(AnimationError::UnsupportedPixelSnap);
        }
        if source.same(&self.source) {
            self.last_time = Some(now);
            return Ok(());
        }
        let mut next = self.candidate();
        next.settle(now)?;
        next.tracks.retain(|(node, _), track| {
            if source.presentation_value(*node).is_some() {
                return true;
            }
            if track.held.is_none() {
                next.notices
                    .push_back(track.notice(AnimationOutcome::TargetRemoved));
            }
            false
        });
        next.source = source;
        next.last_time = Some(now);
        next.validate_sample(now)?;
        *self = next;
        Ok(())
    }
    pub fn apply(&mut self, batch: AnimationBatch, now: Instant) -> Result<(), AnimationError> {
        self.check_time(now)?;
        if batch.scene != self.scene() {
            return Err(AnimationError::WrongScene);
        }
        if batch.sequence != self.next_sequence {
            return Err(AnimationError::StaleSequence);
        }
        if batch.controls.len() > self.limits.max_controls_per_batch {
            return Err(AnimationError::Capacity);
        }
        let mut next = self.candidate();
        next.settle(now)?;
        for control in batch.controls {
            next.control(control, now)?;
        }
        next.check_capacity()?;
        next.validate_sample(now)?;
        next.next_sequence = next
            .next_sequence
            .checked_add(1)
            .ok_or(AnimationError::Exhausted)?;
        next.last_time = Some(now);
        *self = next;
        Ok(())
    }
    /// Adopt a complete authored source and its causally associated controls
    /// together. A rejected control cannot leave a half-adopted source behind.
    pub fn activate(
        &mut self,
        source: LocalFrame,
        batch: AnimationBatch,
        now: Instant,
    ) -> Result<(), AnimationError> {
        let mut candidate = self.candidate();
        candidate.set_scene(source, now)?;
        candidate.apply(batch, now)?;
        *self = candidate;
        Ok(())
    }
    fn control(&mut self, control: AnimationControl, now: Instant) -> Result<(), AnimationError> {
        match control {
            AnimationControl::Animate(spec) => {
                spec.destination.validate()?;
                spec.curve.validate()?;
                if spec.id <= self.last_id
                    || spec.duration.is_zero()
                    || spec.duration.as_secs_f64() > 86400.0
                {
                    return Err(AnimationError::InvalidSpec);
                }
                self.last_id = spec.id;
                let (translation, opacity) = self
                    .source
                    .presentation_value(spec.node)
                    .ok_or(AnimationError::InvalidTarget)?;
                let key = (spec.node, spec.destination.property());
                let (from, velocity) = if let Some(old) = self.tracks.remove(&key) {
                    let (value, velocity, _) = old.value(now)?;
                    if old.held.is_none() {
                        self.notices
                            .push_back(old.notice(AnimationOutcome::Replaced));
                    }
                    (
                        value.channels(),
                        if spec.velocity == RetargetVelocity::Preserve {
                            velocity
                        } else {
                            [0.0; 2]
                        },
                    )
                } else {
                    (
                        match spec.destination {
                            AnimationValue::Translation(_) => {
                                AnimationValue::Translation(translation)
                            }
                            AnimationValue::Opacity(_) => AnimationValue::Opacity(opacity),
                        }
                        .channels(),
                        [0.0; 2],
                    )
                };
                let start = match spec.start {
                    AnimationStart::Now => now,
                    AnimationStart::At(at) => at,
                };
                self.tracks.insert(
                    key,
                    Track {
                        spec,
                        from,
                        velocity,
                        start,
                        held: None,
                    },
                );
            }
            AnimationControl::Cancel { id, behavior } => {
                let key = self
                    .tracks
                    .iter()
                    .find(|(_, t)| t.spec.id == id)
                    .map(|(k, _)| *k)
                    .ok_or(AnimationError::InvalidTarget)?;
                let mut track = self.tracks.remove(&key).unwrap();
                let (value, _, _) = track.value(now)?;
                if track.held.is_none() {
                    self.notices
                        .push_back(track.notice(AnimationOutcome::Cancelled));
                }
                if behavior == CancelBehavior::Hold {
                    track.held = Some(value);
                    self.tracks.insert(key, track);
                }
            }
            AnimationControl::Finish { id } => {
                let track = self
                    .tracks
                    .values_mut()
                    .find(|t| t.spec.id == id)
                    .ok_or(AnimationError::InvalidTarget)?;
                if track.held.is_none() {
                    self.notices
                        .push_back(track.notice(AnimationOutcome::Finished));
                }
                track.held = Some(track.spec.destination);
            }
            AnimationControl::Release { node, property } => {
                if self.source.presentation_value(node).is_none() {
                    return Err(AnimationError::InvalidTarget);
                }
                if let Some(track) = self.tracks.remove(&(node, property))
                    && track.held.is_none()
                {
                    self.notices
                        .push_back(track.notice(AnimationOutcome::Released));
                }
            }
        }
        Ok(())
    }
    fn overrides(&self, now: Instant) -> Result<Vec<local::PresentationOverride>, AnimationError> {
        let mut values: Vec<local::PresentationOverride> = vec![];
        for (&(node, _), track) in &self.tracks {
            if values.last().is_none_or(|v| v.node != node) {
                values.push(local::PresentationOverride {
                    node,
                    translation: None,
                    opacity: None,
                });
            }
            let last = values.last_mut().unwrap();
            match track.value(now)?.0 {
                AnimationValue::Translation(v) => last.translation = Some(v),
                AnimationValue::Opacity(v) => last.opacity = Some(v),
            }
        }
        Ok(values)
    }
    fn validate_sample(&self, now: Instant) -> Result<LocalFrame, AnimationError> {
        self.source
            .sample_placements(
                self.previous.as_ref().map(|p| &p.frame),
                self.stream,
                self.revision
                    .checked_add(1)
                    .ok_or(AnimationError::Exhausted)?,
                &self.overrides(now)?,
            )
            .map_err(|_| AnimationError::InvalidGeometry)
    }
    /// Sample the requested monotonic presentation time once, skipping obsolete
    /// ticks. No timer, per-frame UI callback or business mutation is performed.
    pub fn sample(&mut self, now: Instant) -> Result<AnimationFrame, AnimationError> {
        self.check_time(now)?;
        let frame = self.validate_sample(now)?;
        // All track values and composed geometry were checked before mutation.
        // Avoid cloning the whole track/outcome table on every visual sample.
        self.settle(now)?;
        self.revision = frame.revision();
        self.last_time = Some(now);
        let output = AnimationFrame {
            source: self.source.clone(),
            frame,
            sampled_at: now,
        };
        self.previous = Some(output.clone());
        Ok(output)
    }
}
