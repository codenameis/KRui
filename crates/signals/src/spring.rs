//! Closed-form analytical spring physics (`step_analytical_spring`) for 120Hz+ UI animations.

pub(crate) const SPRING_SETTLE_EPSILON: f32 = 0.001;
const CRITICAL_ZETA_TOLERANCE: f64 = 1e-5;
const MAX_DECAY_EXPONENT: f64 = 80.0;

/// Exact closed-form analytical step of a unit-mass damped harmonic oscillator (`x'' + c x' + k x = 0`).
///
/// Unconditionally stable for arbitrarily large `dt` (e.g. `dt = 5.0` second lag spikes) across
/// underdamped (`zeta < 1`), critically damped (`zeta == 1`), and overdamped (`zeta > 1`) regimes.
pub fn step_analytical_spring(
    pos: f32,
    vel: f32,
    target: f32,
    stiffness: f32,
    damping: f32,
    dt: f32,
) -> (f32, f32) {
    if !target.is_finite() {
        return (0.0, 0.0);
    }
    if !pos.is_finite() || !vel.is_finite() {
        return (target, 0.0);
    }
    if !dt.is_finite() || dt <= 0.0 {
        return (pos, vel);
    }
    let k = stiffness as f64;
    if !k.is_finite() || k <= 1e-6 {
        return (target, 0.0);
    }
    let c = (damping as f64).max(0.0);
    let t = dt as f64;
    let x0 = (pos as f64) - (target as f64);
    let v0 = vel as f64;

    if x0.abs() < SPRING_SETTLE_EPSILON as f64 && v0.abs() < SPRING_SETTLE_EPSILON as f64 {
        return (target, 0.0);
    }

    let omega0 = k.sqrt();
    let zeta = c / (2.0 * omega0);
    let (x_next, v_next) = eval_oscillator_regime(x0, v0, omega0, zeta, t);

    if !x_next.is_finite()
        || !v_next.is_finite()
        || (x_next.abs() < SPRING_SETTLE_EPSILON as f64
            && v_next.abs() < SPRING_SETTLE_EPSILON as f64)
    {
        (target, 0.0)
    } else {
        (((target as f64) + x_next) as f32, v_next as f32)
    }
}

fn eval_oscillator_regime(x0: f64, v0: f64, omega0: f64, zeta: f64, t: f64) -> (f64, f64) {
    if zeta < 1.0 - CRITICAL_ZETA_TOLERANCE {
        let decay_arg = zeta * omega0 * t;
        if decay_arg > MAX_DECAY_EXPONENT {
            return (0.0, 0.0);
        }
        let omega_d = omega0 * (1.0 - zeta * zeta).sqrt();
        let env = (-decay_arg).exp();
        let (sin_wt, cos_wt) = (omega_d * t).sin_cos();
        let b = (v0 + zeta * omega0 * x0) / omega_d;
        let x = env * (x0 * cos_wt + b * sin_wt);
        let v = env * (v0 * cos_wt - (zeta * omega0 * b + omega_d * x0) * sin_wt);
        (x, v)
    } else if zeta <= 1.0 + CRITICAL_ZETA_TOLERANCE {
        let decay_arg = omega0 * t;
        if decay_arg > MAX_DECAY_EXPONENT {
            return (0.0, 0.0);
        }
        let env = (-decay_arg).exp();
        let b = v0 + omega0 * x0;
        let x = env * (x0 + b * t);
        let v = env * (v0 - omega0 * b * t);
        (x, v)
    } else {
        let root_term = (zeta * zeta - 1.0).sqrt();
        let r1 = -omega0 * (zeta - root_term);
        let r2 = -omega0 * (zeta + root_term);
        let denom = r1 - r2;
        let c1 = (v0 - r2 * x0) / denom;
        let c2 = x0 - c1;
        let e1 = if -r1 * t > MAX_DECAY_EXPONENT {
            0.0
        } else {
            (r1 * t).exp()
        };
        let e2 = if -r2 * t > MAX_DECAY_EXPONENT {
            0.0
        } else {
            (r2 * t).exp()
        };
        (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
    }
}
