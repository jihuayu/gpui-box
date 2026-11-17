//! Framework-owned numeric Grid tracks and item placement.
use super::*;

/// A Grid track's minimum contribution. Percentages are fractions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridMinTrack {
    Px(f32),
    Percent(f32),
    Calc { px: f32, percent: f32 },
    Auto,
    MinContent,
    MaxContent,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridMaxTrack {
    Px(f32),
    Percent(f32),
    Calc { px: f32, percent: f32 },
    Fr(f32),
    Auto,
    MinContent,
    MaxContent,
    FitContentPx(f32),
    FitContentPercent(f32),
    FitContentCalc { px: f32, percent: f32 },
}
/// Separate min/max types prevent a fractional minimum or fit-content minimum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridTrack {
    pub min: GridMinTrack,
    pub max: GridMaxTrack,
}
impl GridTrack {
    pub const fn minmax(min: GridMinTrack, max: GridMaxTrack) -> Self {
        Self { min, max }
    }
    pub const fn px(value: f32) -> Self {
        Self::minmax(GridMinTrack::Px(value), GridMaxTrack::Px(value))
    }
    pub const fn percent(value: f32) -> Self {
        Self::minmax(GridMinTrack::Percent(value), GridMaxTrack::Percent(value))
    }
    pub const fn calc(px: f32, percent: f32) -> Self {
        Self::minmax(
            GridMinTrack::Calc { px, percent },
            GridMaxTrack::Calc { px, percent },
        )
    }
    pub const fn fit_content_calc(px: f32, percent: f32) -> Self {
        Self::minmax(
            GridMinTrack::Auto,
            GridMaxTrack::FitContentCalc { px, percent },
        )
    }
    /// CSS fr has an automatic minimum; use minmax(Px(0), Fr(n)) to allow shrink.
    pub const fn fr(value: f32) -> Self {
        Self::minmax(GridMinTrack::Auto, GridMaxTrack::Fr(value))
    }
    pub const fn auto() -> Self {
        Self::minmax(GridMinTrack::Auto, GridMaxTrack::Auto)
    }
    pub const fn min_content() -> Self {
        Self::minmax(GridMinTrack::MinContent, GridMaxTrack::MinContent)
    }
    pub const fn max_content() -> Self {
        Self::minmax(GridMinTrack::MaxContent, GridMaxTrack::MaxContent)
    }
    pub const fn fit_content_px(limit: f32) -> Self {
        Self::minmax(GridMinTrack::Auto, GridMaxTrack::FitContentPx(limit))
    }
    pub const fn fit_content_percent(limit: f32) -> Self {
        Self::minmax(GridMinTrack::Auto, GridMaxTrack::FitContentPercent(limit))
    }
    fn validate(self) -> Result<(), Error> {
        if matches!(
            self.max,
            GridMaxTrack::FitContentPx(_)
                | GridMaxTrack::FitContentPercent(_)
                | GridMaxTrack::FitContentCalc { .. }
        ) && self.min != GridMinTrack::Auto
        {
            // fit-content() is a complete track function, not a minmax() bound.
            return Err(Error::InvalidStyle);
        }
        let min_calc = if let GridMinTrack::Calc { px, percent } = self.min {
            Some((px, percent))
        } else {
            None
        };
        let max_calc = match self.max {
            GridMaxTrack::Calc { px, percent } | GridMaxTrack::FitContentCalc { px, percent } => {
                Some((px, percent))
            }
            _ => None,
        };
        if [min_calc, max_calc]
            .into_iter()
            .flatten()
            .any(|(px, percent)| !px.is_finite() || !percent.is_finite())
        {
            return Err(Error::InvalidStyle);
        }
        let min = match self.min {
            GridMinTrack::Px(v) | GridMinTrack::Percent(v) => Some(v),
            _ => None,
        };
        let max = match self.max {
            GridMaxTrack::Px(v)
            | GridMaxTrack::Percent(v)
            | GridMaxTrack::Fr(v)
            | GridMaxTrack::FitContentPx(v)
            | GridMaxTrack::FitContentPercent(v) => Some(v),
            _ => None,
        };
        if [min, max]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || v < 0.0)
        {
            return Err(Error::InvalidStyle);
        }
        Ok(())
    }
    fn fixed_component(self) -> bool {
        matches!(
            self.min,
            GridMinTrack::Px(_) | GridMinTrack::Percent(_) | GridMinTrack::Calc { .. }
        ) || matches!(
            self.max,
            GridMaxTrack::Px(_) | GridMaxTrack::Percent(_) | GridMaxTrack::Calc { .. }
        )
    }
    fn to_taffy(self) -> taffy::TrackSizingFunction {
        use taffy::{MaxTrackSizingFunction as Max, MinTrackSizingFunction as Min};
        taffy::MinMax {
            min: match self.min {
                GridMinTrack::Px(v) => Min::length(v),
                GridMinTrack::Percent(v) => Min::percent(v),
                GridMinTrack::Calc { px, percent } => Min::affine(px, percent),
                GridMinTrack::Auto => Min::auto(),
                GridMinTrack::MinContent => Min::min_content(),
                GridMinTrack::MaxContent => Min::max_content(),
            },
            max: match self.max {
                GridMaxTrack::Px(v) => Max::length(v),
                GridMaxTrack::Percent(v) => Max::percent(v),
                GridMaxTrack::Calc { px, percent } => Max::affine(px, percent),
                GridMaxTrack::Fr(v) => Max::fr(v),
                GridMaxTrack::Auto => Max::auto(),
                GridMaxTrack::MinContent => Max::min_content(),
                GridMaxTrack::MaxContent => Max::max_content(),
                GridMaxTrack::FitContentPx(v) => Max::fit_content_px(v),
                GridMaxTrack::FitContentPercent(v) => Max::fit_content_percent(v),
                GridMaxTrack::FitContentCalc { px, percent } => {
                    Max::fit_content_affine(px, percent)
                }
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridRepetition {
    Count(u16),
    AutoFill,
    AutoFit,
}
#[derive(Clone, Debug, PartialEq)]
pub enum GridTemplate {
    Track(GridTrack),
    /// Flat repeated fragment; nested repeats are intentionally not representable.
    Repeat {
        count: GridRepetition,
        tracks: Vec<GridTrack>,
    },
}
impl From<GridTrack> for GridTemplate {
    fn from(track: GridTrack) -> Self {
        Self::Track(track)
    }
}
impl GridTemplate {
    pub fn repeat(count: GridRepetition, tracks: Vec<GridTrack>) -> Self {
        Self::Repeat { count, tracks }
    }
    fn to_taffy(&self) -> taffy::GridTemplateComponent<String> {
        match self {
            Self::Track(track) => taffy::GridTemplateComponent::Single(track.to_taffy()),
            Self::Repeat { count, tracks } => {
                taffy::GridTemplateComponent::Repeat(taffy::GridTemplateRepetition {
                    count: match count {
                        GridRepetition::Count(n) => taffy::RepetitionCount::Count(*n),
                        GridRepetition::AutoFill => taffy::RepetitionCount::AutoFill,
                        GridRepetition::AutoFit => taffy::RepetitionCount::AutoFit,
                    },
                    tracks: tracks.iter().map(|t| t.to_taffy()).collect(),
                    line_names: vec![],
                })
            }
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridAutoFlow {
    #[default]
    Row,
    Column,
    RowDense,
    ColumnDense,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridStyle {
    pub columns: Vec<GridTemplate>,
    pub rows: Vec<GridTemplate>,
    /// Repeating pattern for implicit tracks. Empty uses Auto.
    pub auto_columns: Vec<GridTrack>,
    pub auto_rows: Vec<GridTrack>,
    pub auto_flow: GridAutoFlow,
}
impl GridStyle {
    pub fn columns(mut self, columns: Vec<GridTemplate>) -> Self {
        self.columns = columns;
        self
    }
    pub fn rows(mut self, rows: Vec<GridTemplate>) -> Self {
        self.rows = rows;
        self
    }
    pub fn auto_columns(mut self, columns: Vec<GridTrack>) -> Self {
        self.auto_columns = columns;
        self
    }
    pub fn auto_rows(mut self, rows: Vec<GridTrack>) -> Self {
        self.auto_rows = rows;
        self
    }
    pub fn auto_flow(mut self, flow: GridAutoFlow) -> Self {
        self.auto_flow = flow;
        self
    }
    pub(super) fn validate(&self) -> Result<(), Error> {
        for tracks in [&self.columns, &self.rows] {
            let mut auto = 0;
            let mut all_fixed = true;
            for component in tracks {
                match component {
                    GridTemplate::Track(track) => {
                        track.validate()?;
                        all_fixed &= track.fixed_component();
                    }
                    GridTemplate::Repeat { count, tracks } => {
                        if tracks.is_empty()
                            || matches!(count, GridRepetition::Count(0))
                            || tracks.len() > u16::MAX as usize
                        {
                            return Err(Error::InvalidStyle);
                        }
                        if !matches!(count, GridRepetition::Count(_)) {
                            auto += 1;
                        }
                        for track in tracks {
                            track.validate()?;
                            all_fixed &= track.fixed_component();
                        }
                    }
                }
            }
            // Match the kernel's accepted auto-repeat grammar, but report an
            // invalid public style rather than silently dropping the template.
            if auto > 1 || (auto == 1 && !all_fixed) {
                return Err(Error::InvalidStyle);
            }
        }
        for track in self.auto_columns.iter().chain(&self.auto_rows) {
            track.validate()?;
        }
        Ok(())
    }
    pub(super) fn lower(&self, style: &mut taffy::Style) {
        style.grid_template_columns = self.columns.iter().map(GridTemplate::to_taffy).collect();
        style.grid_template_rows = self.rows.iter().map(GridTemplate::to_taffy).collect();
        style.grid_auto_columns = self.auto_columns.iter().map(|t| t.to_taffy()).collect();
        style.grid_auto_rows = self.auto_rows.iter().map(|t| t.to_taffy()).collect();
        style.grid_auto_flow = match self.auto_flow {
            GridAutoFlow::Row => taffy::GridAutoFlow::Row,
            GridAutoFlow::Column => taffy::GridAutoFlow::Column,
            GridAutoFlow::RowDense => taffy::GridAutoFlow::RowDense,
            GridAutoFlow::ColumnDense => taffy::GridAutoFlow::ColumnDense,
        };
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridPlacement {
    #[default]
    Auto,
    /// One-based line; negative indices count from the explicit grid's end.
    Line(i16),
    Span(u16),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridAxisPlacement {
    pub start: GridPlacement,
    pub end: GridPlacement,
}
impl GridAxisPlacement {
    pub const fn new(start: GridPlacement, end: GridPlacement) -> Self {
        Self { start, end }
    }
    pub(super) fn validate(self) -> Result<(), Error> {
        if [self.start, self.end]
            .iter()
            .any(|v| matches!(v, GridPlacement::Line(0) | GridPlacement::Span(0)))
        {
            return Err(Error::InvalidStyle);
        }
        Ok(())
    }
    pub(super) fn to_taffy(self) -> taffy::Line<taffy::GridPlacement> {
        use taffy::style_helpers::TaffyGridLine;
        let lower = |v| match v {
            GridPlacement::Auto => taffy::GridPlacement::Auto,
            GridPlacement::Line(n) => taffy::GridPlacement::from_line_index(n),
            GridPlacement::Span(n) => taffy::GridPlacement::Span(n),
        };
        taffy::Line {
            start: lower(self.start),
            end: lower(self.end),
        }
    }
}
