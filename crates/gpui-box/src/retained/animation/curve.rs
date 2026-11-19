use super::AnimationError;
use std::{sync::Arc, time::Duration};

/// Physical spring solved analytically. Duration is a hard end, not a numerical
/// convergence threshold. Sampling is bounded even after a long scheduling gap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub mass: f64,
    pub stiffness: f64,
    pub damping: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationCurve {
    Linear,
    /// Cubic Bezier easing; x and y controls must be within 0..=1.
    CubicBezier {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
    /// Ordered progress/eased-progress pairs. Exactly (0,0) to (1,1).
    Keyframes(Arc<[[f64; 2]]>),
    Spring(Spring),
}
impl AnimationCurve {
    pub(super) fn validate(&self) -> Result<(), AnimationError> {
        let unit = |x: f64| x.is_finite() && (0.0..=1.0).contains(&x);
        let ok = match self {
            Self::Linear => true,
            Self::CubicBezier { x1, y1, x2, y2 } => [*x1, *y1, *x2, *y2].into_iter().all(unit),
            Self::Keyframes(k) => {
                k.len() >= 2
                    && k.len() <= 256
                    && k.first() == Some(&[0.0, 0.0])
                    && k.last() == Some(&[1.0, 1.0])
                    && k.iter().flatten().copied().all(unit)
                    && k.windows(2).all(|k| k[0][0] < k[1][0])
            }
            Self::Spring(s) => {
                s.mass.is_finite()
                    && s.stiffness.is_finite()
                    && s.damping.is_finite()
                    && s.mass > 0.0
                    && s.stiffness > 0.0
                    && s.damping >= 0.0
                    && {
                        let w = (s.stiffness / s.mass).sqrt();
                        let a = s.damping / (2.0 * s.mass);
                        w.is_finite() && w > 0.0 && a.is_finite() && a <= 1e6 && w <= 1e6
                    }
            }
        };
        if ok {
            Ok(())
        } else {
            Err(AnimationError::InvalidSpec)
        }
    }
    pub(super) fn sample(
        &self,
        from: [f64; 2],
        to: [f64; 2],
        velocity: [f64; 2],
        elapsed: Duration,
        duration: Duration,
    ) -> Result<([f64; 2], [f64; 2]), AnimationError> {
        let t = elapsed.as_secs_f64();
        let d = duration.as_secs_f64();
        let p = (t / d).clamp(0.0, 1.0);
        let (value, speed) = match self {
            Self::Spring(s) => {
                let w = (s.stiffness / s.mass).sqrt();
                let a = s.damping / (2.0 * s.mass);
                let mut values = [0.0; 2];
                let mut speeds = [0.0; 2];
                for i in 0..2 {
                    let x = from[i] - to[i];
                    let v = velocity[i];
                    let (y, dy) = if (a - w).abs() <= 1e-6 * w.max(1.0) {
                        let b = v + a * x;
                        let e = (-a * t).exp();
                        (e * (x + b * t), e * (b - a * (x + b * t)))
                    } else if a < w {
                        let b = (w * w - a * a).sqrt();
                        let c = (v + a * x) / b;
                        let e = (-a * t).exp();
                        let (sin, cos) = (b * t).sin_cos();
                        let y = x * cos + c * sin;
                        (e * y, e * (-a * y - x * b * sin + c * b * cos))
                    } else {
                        let q = (a * a - w * w).sqrt();
                        // Stable roots avoid subtractive loss at high damping.
                        let r2 = -a - q;
                        let r1 = -(w * w) / (a + q);
                        let c1 = (v - r2 * x) / (r1 - r2);
                        let c2 = x - c1;
                        let e1 = (r1 * t).exp();
                        let e2 = (r2 * t).exp();
                        (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
                    };
                    values[i] = to[i] + y;
                    speeds[i] = dy;
                }
                (values, speeds)
            }
            _ => {
                let (progress, slope) = self.ease(p);
                (
                    std::array::from_fn(|i| {
                        if p == 0.0 {
                            from[i]
                        } else {
                            from[i] + (to[i] - from[i]) * progress
                        }
                    }),
                    std::array::from_fn(|i| (to[i] - from[i]) * slope / d),
                )
            }
        };
        if value.iter().chain(&speed).any(|v| !v.is_finite()) {
            Err(AnimationError::InvalidSpec)
        } else {
            Ok((value, speed))
        }
    }
    fn ease(&self, p: f64) -> (f64, f64) {
        match self {
            Self::Linear => (p, 1.0),
            Self::Keyframes(keys) => {
                let upper = keys.partition_point(|k| k[0] <= p).clamp(1, keys.len() - 1);
                let [a, b] = keys[upper - 1];
                let [c, d] = keys[upper];
                let slope = (d - b) / (c - a);
                (b + (p - a) * slope, slope)
            }
            Self::CubicBezier { x1, y1, x2, y2 } => {
                let b = |t: f64, a: f64, c: f64| {
                    3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * c + t.powi(3)
                };
                let derivative = |t: f64, a: f64, c: f64| {
                    3.0 * (1.0 - t).powi(2) * a
                        + 6.0 * (1.0 - t) * t * (c - a)
                        + 3.0 * t * t * (1.0 - c)
                };
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..40 {
                    let mid = (lo + hi) * 0.5;
                    if b(mid, *x1, *x2) < p {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let t = (lo + hi) * 0.5;
                let dx = derivative(t, *x1, *x2);
                let dy = derivative(t, *y1, *y2);
                (b(t, *y1, *y2), if dx > 1e-12 { dy / dx } else { 0.0 })
            }
            Self::Spring(_) => unreachable!(),
        }
    }
}
