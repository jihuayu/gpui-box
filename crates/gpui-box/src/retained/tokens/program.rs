use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenProjection {
    Left,
    Right,
    Top,
    Bottom,
    Width,
    Height,
}
/// Flat owned expression data. Operand indices must refer to earlier operations;
/// every operation is validated/evaluated, with the final operation the result.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenOp {
    Value(TokenValue),
    Reference(TokenId),
    Add { left: u32, right: u32 },
    Scale { value: u32, factor: u32 },
    MixColor { left: u32, right: u32, factor: u32 },
    Dimension { length: u32 },
    Project { value: u32, field: TokenProjection },
}
#[derive(Clone, Debug, PartialEq)]
pub struct TokenProgram(Arc<[TokenOp]>);
impl TokenProgram {
    pub(crate) fn projected(program: &Self, field: TokenProjection) -> Result<Self, TokenError> {
        Self::appended(&[program], |r| TokenOp::Project { value: r[0], field })
    }
    pub fn new(operations: Vec<TokenOp>) -> Result<Self, TokenError> {
        let p = Self(operations.into());
        p.validate(TokenLimits::default())?;
        Ok(p)
    }
    pub fn operations(&self) -> &[TokenOp] {
        &self.0
    }
    pub(crate) fn references(&self) -> BTreeSet<TokenId> {
        self.0
            .iter()
            .filter_map(|op| {
                if let TokenOp::Reference(id) = op {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect()
    }
    pub(crate) fn validate(&self, limits: TokenLimits) -> Result<TokenKind, TokenError> {
        if self.0.is_empty() || self.0.len() > limits.max_operations_per_expression {
            return Err(TokenError::LimitExceeded);
        }
        let mut kinds = vec![];
        let mut depths: Vec<usize> = vec![];
        for op in self.0.iter() {
            let get = |i: u32| {
                kinds
                    .get(i as usize)
                    .copied()
                    .ok_or(TokenError::InvalidProgram)
            };
            let depth = |i: u32| {
                depths
                    .get(i as usize)
                    .copied()
                    .ok_or(TokenError::InvalidProgram)
            };
            let (kind, d) = match op {
                TokenOp::Value(v) => {
                    v.validate()?;
                    (v.kind(), 1usize)
                }
                TokenOp::Reference(id) => {
                    id.validate()?;
                    (id.kind, 1)
                }
                TokenOp::Add { left, right } => {
                    let k = get(*left)?;
                    if get(*right)? != k || !matches!(k, TokenKind::Number | TokenKind::Length) {
                        return Err(TokenError::InvalidProgram);
                    }
                    (k, depth(*left)?.max(depth(*right)?) + 1)
                }
                TokenOp::Scale { value, factor } => {
                    let k = get(*value)?;
                    if get(*factor)? != TokenKind::Number
                        || !matches!(k, TokenKind::Number | TokenKind::Length)
                    {
                        return Err(TokenError::InvalidProgram);
                    }
                    (k, depth(*value)?.max(depth(*factor)?) + 1)
                }
                TokenOp::MixColor {
                    left,
                    right,
                    factor,
                } => {
                    if get(*left)? != TokenKind::Color
                        || get(*right)? != TokenKind::Color
                        || get(*factor)? != TokenKind::Number
                    {
                        return Err(TokenError::InvalidProgram);
                    }
                    (
                        TokenKind::Color,
                        depth(*left)?.max(depth(*right)?).max(depth(*factor)?) + 1,
                    )
                }
                TokenOp::Dimension { length } => {
                    if get(*length)? != TokenKind::Length {
                        return Err(TokenError::InvalidProgram);
                    }
                    (TokenKind::Dimension, depth(*length)? + 1)
                }
                TokenOp::Project { value, field } => {
                    let k = match (get(*value)?, field) {
                        (
                            TokenKind::Edges,
                            TokenProjection::Left
                            | TokenProjection::Right
                            | TokenProjection::Top
                            | TokenProjection::Bottom,
                        ) => TokenKind::Length,
                        (
                            TokenKind::Dimensions,
                            TokenProjection::Width | TokenProjection::Height,
                        ) => TokenKind::Dimension,
                        (TokenKind::Lengths, TokenProjection::Width | TokenProjection::Height) => {
                            TokenKind::Length
                        }
                        _ => return Err(TokenError::InvalidProgram),
                    };
                    (k, depth(*value)? + 1)
                }
            };
            if d > limits.max_expression_depth {
                return Err(TokenError::LimitExceeded);
            }
            kinds.push(kind);
            depths.push(d);
        }
        Ok(*kinds.last().unwrap())
    }
    pub fn evaluate(&self, snapshot: &TokenSnapshot) -> Result<TokenValue, TokenError> {
        // TokenExpr::value deliberately permits fallible literals so callers
        // can build data before validation. Never index operands before proof.
        self.validate(TokenLimits::default())?;
        let mut values: Vec<TokenValue> = vec![];
        for op in self.0.iter() {
            let v = match op {
                TokenOp::Value(v) => v.clone(),
                TokenOp::Reference(id) => snapshot.get(id)?.clone(),
                TokenOp::Add { left, right } => {
                    match (&values[*left as usize], &values[*right as usize]) {
                        (TokenValue::Number(a), TokenValue::Number(b)) => TokenValue::Number(a + b),
                        (TokenValue::Length(a), TokenValue::Length(b)) => {
                            TokenValue::Length(add_length(*a, *b)?)
                        }
                        _ => return Err(TokenError::InvalidProgram),
                    }
                }
                TokenOp::Scale { value, factor } => {
                    let TokenValue::Number(f) = values[*factor as usize] else {
                        return Err(TokenError::InvalidProgram);
                    };
                    match values[*value as usize] {
                        TokenValue::Number(n) => TokenValue::Number(n * f),
                        TokenValue::Length(l) => TokenValue::Length(scale_length(l, f)?),
                        _ => return Err(TokenError::InvalidProgram),
                    }
                }
                TokenOp::MixColor {
                    left,
                    right,
                    factor,
                } => {
                    let TokenValue::Color(a) = values[*left as usize] else {
                        return Err(TokenError::InvalidProgram);
                    };
                    let TokenValue::Color(b) = values[*right as usize] else {
                        return Err(TokenError::InvalidProgram);
                    };
                    let TokenValue::Number(t) = values[*factor as usize] else {
                        return Err(TokenError::InvalidProgram);
                    };
                    if !(0.0..=1.0).contains(&t) {
                        return Err(TokenError::InvalidValue);
                    }
                    let mut color = 0;
                    for shift in [24, 16, 8, 0] {
                        let x = ((a >> shift) & 255) as f32;
                        let y = ((b >> shift) & 255) as f32;
                        color |= ((x + (y - x) * t).round() as u32) << shift;
                    }
                    TokenValue::Color(color)
                }
                TokenOp::Dimension { length } => {
                    let TokenValue::Length(l) = values[*length as usize] else {
                        return Err(TokenError::InvalidProgram);
                    };
                    TokenValue::Dimension(l.into())
                }
                TokenOp::Project { value, field } => match (&values[*value as usize], field) {
                    (TokenValue::Edges(e), TokenProjection::Left) => TokenValue::Length(e.left),
                    (TokenValue::Edges(e), TokenProjection::Right) => TokenValue::Length(e.right),
                    (TokenValue::Edges(e), TokenProjection::Top) => TokenValue::Length(e.top),
                    (TokenValue::Edges(e), TokenProjection::Bottom) => TokenValue::Length(e.bottom),
                    (TokenValue::Dimensions(s), TokenProjection::Width) => {
                        TokenValue::Dimension(s.width)
                    }
                    (TokenValue::Dimensions(s), TokenProjection::Height) => {
                        TokenValue::Dimension(s.height)
                    }
                    (TokenValue::Lengths(s), TokenProjection::Width) => TokenValue::Length(s.width),
                    (TokenValue::Lengths(s), TokenProjection::Height) => {
                        TokenValue::Length(s.height)
                    }
                    _ => return Err(TokenError::InvalidProgram),
                },
            };
            v.validate()?;
            values.push(v);
        }
        values.pop().ok_or(TokenError::InvalidProgram)
    }
    fn appended(
        programs: &[&Self],
        op: impl FnOnce(&[u32]) -> TokenOp,
    ) -> Result<Self, TokenError> {
        if programs
            .iter()
            .try_fold(1usize, |n, p| n.checked_add(p.0.len()))
            .is_none_or(|n| n > TokenLimits::default().max_operations_per_expression)
        {
            return Err(TokenError::LimitExceeded);
        }
        let mut operations = vec![];
        let mut roots = vec![];
        for p in programs {
            let offset = operations.len() as u32;
            for value in p.0.iter() {
                let mut v = value.clone();
                match &mut v {
                    TokenOp::Add { left, right } => {
                        *left += offset;
                        *right += offset
                    }
                    TokenOp::Scale { value, factor } => {
                        *value += offset;
                        *factor += offset
                    }
                    TokenOp::MixColor {
                        left,
                        right,
                        factor,
                    } => {
                        *left += offset;
                        *right += offset;
                        *factor += offset
                    }
                    TokenOp::Dimension { length } => *length += offset,
                    TokenOp::Project { value, .. } => *value += offset,
                    _ => {}
                }
                operations.push(v);
            }
            roots.push(operations.len() as u32 - 1);
        }
        operations.push(op(&roots));
        Self::new(operations)
    }
}
fn components(l: LayoutLength) -> Result<(f32, f32), TokenError> {
    match l {
        LayoutLength::Px(v) => Ok((v, 0.0)),
        LayoutLength::Percent(v) => Ok((0.0, v)),
        LayoutLength::Calc { px, percent } => Ok((px, percent)),
        LayoutLength::Auto => Err(TokenError::InvalidValue),
    }
}
fn add_length(a: LayoutLength, b: LayoutLength) -> Result<LayoutLength, TokenError> {
    match (a, b) {
        (LayoutLength::Px(a), LayoutLength::Px(b)) => Ok(LayoutLength::Px(a + b)),
        (LayoutLength::Percent(a), LayoutLength::Percent(b)) => Ok(LayoutLength::Percent(a + b)),
        _ => {
            let (apx, apct) = components(a)?;
            let (bpx, bpct) = components(b)?;
            Ok(LayoutLength::Calc {
                px: apx + bpx,
                percent: apct + bpct,
            })
        }
    }
}
fn scale_length(l: LayoutLength, f: f32) -> Result<LayoutLength, TokenError> {
    match l {
        LayoutLength::Px(v) => Ok(LayoutLength::Px(v * f)),
        LayoutLength::Percent(v) => Ok(LayoutLength::Percent(v * f)),
        LayoutLength::Calc { px, percent } => Ok(LayoutLength::Calc {
            px: px * f,
            percent: percent * f,
        }),
        LayoutLength::Auto => Err(TokenError::InvalidValue),
    }
}
pub struct TokenExpr<T> {
    program: TokenProgram,
    marker: PhantomData<fn() -> T>,
}
impl<T> Clone for TokenExpr<T> {
    fn clone(&self) -> Self {
        Self {
            program: self.program.clone(),
            marker: PhantomData,
        }
    }
}
impl<T> std::fmt::Debug for TokenExpr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.program.fmt(f)
    }
}
impl<T> PartialEq for TokenExpr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.program == other.program
    }
}
impl<T> TokenExpr<T> {
    pub fn program(&self) -> &TokenProgram {
        &self.program
    }
}
impl<T: TokenType> TokenExpr<T> {
    pub fn from_program(program: TokenProgram) -> Result<Self, TokenError> {
        let kind = program.validate(TokenLimits::default())?;
        if kind != T::KIND {
            return Err(TokenError::TypeMismatch {
                expected: T::KIND,
                actual: kind,
            });
        }
        Ok(Self {
            program,
            marker: PhantomData,
        })
    }
    pub fn value(value: T) -> Self {
        Self {
            program: TokenProgram(Arc::from([TokenOp::Value(value.into_token())])),
            marker: PhantomData,
        }
    }
    pub fn reference(key: TokenKey<T>) -> Self {
        Self {
            program: TokenProgram(Arc::from([TokenOp::Reference(key.id)])),
            marker: PhantomData,
        }
    }
    pub fn evaluate(&self, snapshot: &TokenSnapshot) -> Result<T, TokenError> {
        T::from_token(&self.program.evaluate(snapshot)?)
    }
}
impl TokenExpr<f32> {
    pub fn plus(&self, other: &Self) -> Result<Self, TokenError> {
        Self::from_program(TokenProgram::appended(
            &[&self.program, &other.program],
            |r| TokenOp::Add {
                left: r[0],
                right: r[1],
            },
        )?)
    }
    pub fn times(&self, other: &Self) -> Result<Self, TokenError> {
        Self::from_program(TokenProgram::appended(
            &[&self.program, &other.program],
            |r| TokenOp::Scale {
                value: r[0],
                factor: r[1],
            },
        )?)
    }
}
impl TokenExpr<LayoutLength> {
    pub fn plus(&self, other: &Self) -> Result<Self, TokenError> {
        Self::from_program(TokenProgram::appended(
            &[&self.program, &other.program],
            |r| TokenOp::Add {
                left: r[0],
                right: r[1],
            },
        )?)
    }
    pub fn scale(&self, factor: &TokenExpr<f32>) -> Result<Self, TokenError> {
        Self::from_program(TokenProgram::appended(
            &[&self.program, &factor.program],
            |r| TokenOp::Scale {
                value: r[0],
                factor: r[1],
            },
        )?)
    }
    pub fn dimension(&self) -> Result<TokenExpr<LayoutDimension>, TokenError> {
        TokenExpr::from_program(TokenProgram::appended(&[&self.program], |r| {
            TokenOp::Dimension { length: r[0] }
        })?)
    }
}
impl TokenExpr<u32> {
    /// Component-wise interpolation of encoded straight RGBA bytes, round nearest.
    /// This is not linear-light or premultiplied color interpolation.
    pub fn mix(&self, other: &Self, factor: &TokenExpr<f32>) -> Result<Self, TokenError> {
        Self::from_program(TokenProgram::appended(
            &[&self.program, &other.program, &factor.program],
            |r| TokenOp::MixColor {
                left: r[0],
                right: r[1],
                factor: r[2],
            },
        )?)
    }
}
