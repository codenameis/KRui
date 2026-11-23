//! Ordered CSS `transform` operation list, `Mat4` 4x4 matrix math, and W3C matrix/op interpolation.

use crate::affine::Affine2;
use crate::standalone_transform::OriginAxis;
use crate::transition_lerp::lerp_f32;
use std::ops::Mul;
use std::sync::{Arc, OnceLock, RwLock};

/// Single axis length supporting fixed pixels plus percentage of the element's border-box size.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct TransformLength {
    pub px: f32,
    /// Percentage (`0.0..=100.0`, where `50.0` = `50%`).
    pub pct: f32,
}

impl TransformLength {
    pub const ZERO: Self = Self { px: 0.0, pct: 0.0 };

    #[inline]
    pub const fn px(px: f32) -> Self {
        Self { px, pct: 0.0 }
    }

    #[inline]
    pub const fn percent(pct: f32) -> Self {
        Self { px: 0.0, pct }
    }

    #[inline]
    pub fn resolve(self, box_span: f32) -> f32 {
        self.px + box_span * (self.pct * 0.01)
    }

    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            px: lerp_f32(self.px, other.px, t),
            pct: lerp_f32(self.pct, other.pct, t),
        }
    }
}

/// Ordered CSS Transform function atom evaluated sequentially around `transform-origin`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TransformOp {
    Translate3d(TransformLength, TransformLength, f32),
    Scale3d(f32, f32, f32),
    RotateZ(f32),
    RotateX(f32),
    RotateY(f32),
    Rotate3d(f32, f32, f32, f32),
    Skew(f32, f32),
    Perspective(f32),
    Matrix2d([f32; 6]),
    Matrix3d([f32; 16]),
}

impl TransformOp {
    /// Returns an identity [`TransformOp`] of matching variant for `none <-> op` interpolation.
    pub fn matching_identity(self) -> Self {
        match self {
            Self::Translate3d(..) => {
                Self::Translate3d(TransformLength::ZERO, TransformLength::ZERO, 0.0)
            }
            Self::Scale3d(..) => Self::Scale3d(1.0, 1.0, 1.0),
            Self::RotateZ(_) => Self::RotateZ(0.0),
            Self::RotateX(_) => Self::RotateX(0.0),
            Self::RotateY(_) => Self::RotateY(0.0),
            Self::Rotate3d(x, y, z, _) => Self::Rotate3d(x, y, z, 0.0),
            Self::Skew(..) => Self::Skew(0.0, 0.0),
            Self::Perspective(_) => Self::Perspective(1.0e6),
            Self::Matrix2d(_) => Self::Matrix2d([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            Self::Matrix3d(_) => Self::Matrix3d(Mat4::IDENTITY.0),
        }
    }

    /// Evaluates `self` against box dimensions `(w, h)` into a column-major 4x4 matrix.
    pub fn to_mat4(self, box_w: f32, box_h: f32) -> Mat4 {
        match self {
            Self::Translate3d(tx, ty, tz) => {
                Mat4::translation(tx.resolve(box_w), ty.resolve(box_h), tz)
            }
            Self::Scale3d(sx, sy, sz) => Mat4::scale(sx, sy, sz),
            Self::RotateZ(deg) => Mat4::rotation_z(deg),
            Self::RotateX(deg) => Mat4::rotation_x(deg),
            Self::RotateY(deg) => Mat4::rotation_y(deg),
            Self::Rotate3d(x, y, z, deg) => Mat4::rotation_axis(x, y, z, deg),
            Self::Skew(ax, ay) => Mat4::skew(ax, ay),
            Self::Perspective(d) => Mat4::perspective(d),
            Self::Matrix2d(m) => Mat4::from_matrix2d(m),
            Self::Matrix3d(m) => Mat4(m),
        }
    }
}

/// Column-major 4x4 transformation matrix (`[col0, col1, col2, col3]`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct Mat4(pub [f32; 16]);

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mul for Mat4 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self {
        let (a, b) = (self.0, rhs.0);
        let mut out = [0.0_f32; 16];
        for col in 0..4 {
            for row in 0..4 {
                out[col * 4 + row] = a[row] * b[col * 4]
                    + a[4 + row] * b[col * 4 + 1]
                    + a[8 + row] * b[col * 4 + 2]
                    + a[12 + row] * b[col * 4 + 3];
            }
        }
        Self(out)
    }
}

impl Mat4 {
    pub const IDENTITY: Self = Self([
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, //
    ]);

    pub const fn translation(tx: f32, ty: f32, tz: f32) -> Self {
        Self([
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            tx, ty, tz, 1.0, //
        ])
    }

    pub const fn scale(sx: f32, sy: f32, sz: f32) -> Self {
        Self([
            sx, 0.0, 0.0, 0.0, //
            0.0, sy, 0.0, 0.0, //
            0.0, 0.0, sz, 0.0, //
            0.0, 0.0, 0.0, 1.0, //
        ])
    }

    pub fn rotation_z(deg: f32) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Self([
            c, s, 0.0, 0.0, //
            -s, c, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, //
        ])
    }

    pub fn rotation_x(deg: f32) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Self([
            1.0, 0.0, 0.0, 0.0, //
            0.0, c, s, 0.0, //
            0.0, -s, c, 0.0, //
            0.0, 0.0, 0.0, 1.0, //
        ])
    }

    pub fn rotation_y(deg: f32) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Self([
            c, 0.0, -s, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            s, 0.0, c, 0.0, //
            0.0, 0.0, 0.0, 1.0, //
        ])
    }

    pub fn rotation_axis(x: f32, y: f32, z: f32, deg: f32) -> Self {
        let len = (x * x + y * y + z * z).sqrt();
        if len <= 1e-6 {
            return Self::IDENTITY;
        }
        let (nx, ny, nz) = (x / len, y / len, z / len);
        let (s, c) = deg.to_radians().sin_cos();
        let oc = 1.0 - c;
        Self([
            nx * nx * oc + c,
            ny * nx * oc + nz * s,
            nz * nx * oc - ny * s,
            0.0,
            nx * ny * oc - nz * s,
            ny * ny * oc + c,
            nz * ny * oc + nx * s,
            0.0,
            nx * nz * oc + ny * s,
            ny * nz * oc - nx * s,
            nz * nz * oc + c,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ])
    }

    pub fn skew(ax_deg: f32, ay_deg: f32) -> Self {
        Self([
            1.0,
            ay_deg.to_radians().tan(),
            0.0,
            0.0,
            ax_deg.to_radians().tan(),
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ])
    }

    pub const fn perspective(d: f32) -> Self {
        if d <= 1e-3 {
            return Self::IDENTITY;
        }
        Self([
            1.0,
            0.0,
            0.0,
            0.0, //
            0.0,
            1.0,
            0.0,
            0.0, //
            0.0,
            0.0,
            1.0,
            -1.0 / d, //
            0.0,
            0.0,
            0.0,
            1.0, //
        ])
    }

    pub const fn from_matrix2d([a, b, c, d, tx, ty]: [f32; 6]) -> Self {
        Self([
            a, b, 0.0, 0.0, //
            c, d, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            tx, ty, 0.0, 1.0, //
        ])
    }

    /// Projects this 4x4 matrix onto 2D screen affine `[a, b, c, d, tx, ty]` at `z = 0`.
    #[inline]
    pub fn to_affine2_projected(self) -> Affine2 {
        let m = self.0;
        let w = if m[15].abs() > 1e-6 { m[15] } else { 1.0 };
        Affine2 {
            a: m[0] / w,
            b: m[1] / w,
            c: m[4] / w,
            d: m[5] / w,
            tx: m[12] / w,
            ty: m[13] / w,
        }
    }
}

static TRANSFORM_LISTS: OnceLock<RwLock<Vec<Arc<[TransformOp]>>>> = OnceLock::new();

fn list_store() -> &'static RwLock<Vec<Arc<[TransformOp]>>> {
    TRANSFORM_LISTS.get_or_init(|| RwLock::new(Vec::new()))
}

fn intern_transform_ops(ops: Vec<TransformOp>) -> u32 {
    if let Ok(read) = list_store().read() {
        for (idx, existing) in read.iter().enumerate() {
            if existing.as_ref() == ops.as_slice() {
                return idx as u32;
            }
        }
    }
    let mut write = list_store()
        .write()
        .unwrap_or_else(|poison| poison.into_inner());
    for (idx, existing) in write.iter().enumerate() {
        if existing.as_ref() == ops.as_slice() {
            return idx as u32;
        }
    }
    let idx = write.len() as u32;
    write.push(Arc::from(ops));
    idx
}

/// `Copy` + `const`-constructible ordered transform list reference for `PaintStyle`.
#[derive(Clone, Copy, Debug, Default)]
pub enum TransformListRef {
    #[default]
    Empty,
    Static(&'static [TransformOp]),
    Interned(u32),
}

impl TransformListRef {
    /// Creates a `const` static list reference (used by `krui-macros` codegen).
    pub const fn static_ref(ops: &'static [TransformOp]) -> Self {
        if ops.is_empty() {
            Self::Empty
        } else {
            Self::Static(ops)
        }
    }

    /// Creates a `TransformListRef` from dynamic slice of ordered [`TransformOp`]s.
    pub fn from_ops(ops: &[TransformOp]) -> Self {
        if ops.is_empty() {
            Self::Empty
        } else {
            Self::Interned(intern_transform_ops(ops.to_vec()))
        }
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Passes the underlying [`TransformOp`] slice cleanly without leaking or holding nested guards.
    pub fn with_ops<R>(self, f: impl FnOnce(&[TransformOp]) -> R) -> R {
        match self {
            Self::Empty => f(&[]),
            Self::Static(s) => f(s),
            Self::Interned(idx) => {
                let arc = list_store()
                    .read()
                    .ok()
                    .and_then(|vec| vec.get(idx as usize).cloned());
                match arc {
                    Some(a) => f(&a),
                    None => f(&[]),
                }
            }
        }
    }

    /// Evaluates the full ordered 4x4 matrix around `transform-origin` relative to `(x, y, w, h)`.
    pub fn eval_mat4_at_rect(
        self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        ox: OriginAxis,
        oy: OriginAxis,
    ) -> Mat4 {
        let px = match ox {
            OriginAxis::Px(v) => x + v,
            OriginAxis::Percent(p) => x + w * (p * 0.01),
        };
        let py = match oy {
            OriginAxis::Px(v) => y + v,
            OriginAxis::Percent(p) => y + h * (p * 0.01),
        };
        self.with_ops(|ops| {
            let mut m = Mat4::translation(px, py, 0.0);
            for &op in ops {
                m = m.mul(op.to_mat4(w, h));
            }
            m.mul(Mat4::translation(-px, -py, 0.0))
        })
    }
}

impl PartialEq for TransformListRef {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,
            (Self::Static(a), Self::Static(b)) => a == b,
            (Self::Interned(a), Self::Interned(b)) if a == b => true,
            _ => {
                let copy_b: Vec<TransformOp> = other.with_ops(|s| s.to_vec());
                self.with_ops(|a| a == copy_b.as_slice())
            }
        }
    }
}
impl Eq for TransformListRef {}

#[cfg(feature = "preview")]
impl serde::Serialize for TransformListRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.with_ops(|ops| ops.serialize(s))
    }
}

#[cfg(feature = "preview")]
impl<'de> serde::Deserialize<'de> for TransformListRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let ops = Vec::<TransformOp>::deserialize(d)?;
        Ok(Self::from_ops(&ops))
    }
}

/// Interpolates two ordered [`TransformListRef`]s op-by-op when compatible or via 2D matrix fallback.
pub fn lerp_transform_list(
    a: TransformListRef,
    b: TransformListRef,
    box_w: f32,
    box_h: f32,
    t: f32,
) -> TransformListRef {
    if a.is_empty() && b.is_empty() {
        return TransformListRef::Empty;
    }
    let ob: Vec<TransformOp> = b.with_ops(|s| s.to_vec());
    let oa: Vec<TransformOp> = a.with_ops(|s| s.to_vec());
    if oa.is_empty() {
        let id_ops: Vec<_> = ob.iter().map(|op| op.matching_identity()).collect();
        return lerp_matching_op_slices(&id_ops, &ob, t);
    }
    if ob.is_empty() {
        let id_ops: Vec<_> = oa.iter().map(|op| op.matching_identity()).collect();
        return lerp_matching_op_slices(&oa, &id_ops, t);
    }
    if oa.len() == ob.len() && oa.iter().zip(ob.iter()).all(|(x, y)| same_variant(*x, *y)) {
        return lerp_matching_op_slices(&oa, &ob, t);
    }
    let ma = oa
        .iter()
        .fold(Mat4::IDENTITY, |m, op| m.mul(op.to_mat4(box_w, box_h)));
    let mb = ob
        .iter()
        .fold(Mat4::IDENTITY, |m, op| m.mul(op.to_mat4(box_w, box_h)));
    let a2 = ma.to_affine2_projected();
    let b2 = mb.to_affine2_projected();
    let pa = [a2.a, a2.b, a2.c, a2.d, a2.tx, a2.ty];
    let pb = [b2.a, b2.b, b2.c, b2.d, b2.tx, b2.ty];
    let mix = [
        lerp_f32(pa[0], pb[0], t),
        lerp_f32(pa[1], pb[1], t),
        lerp_f32(pa[2], pb[2], t),
        lerp_f32(pa[3], pb[3], t),
        lerp_f32(pa[4], pb[4], t),
        lerp_f32(pa[5], pb[5], t),
    ];
    TransformListRef::from_ops(&[TransformOp::Matrix2d(mix)])
}

fn same_variant(a: TransformOp, b: TransformOp) -> bool {
    std::mem::discriminant(&a) == std::mem::discriminant(&b)
}

fn lerp_matching_op_slices(oa: &[TransformOp], ob: &[TransformOp], t: f32) -> TransformListRef {
    let mut out = Vec::with_capacity(oa.len());
    for (&x, &y) in oa.iter().zip(ob.iter()) {
        out.push(lerp_op(x, y, t));
    }
    TransformListRef::from_ops(&out)
}

fn lerp_op(a: TransformOp, b: TransformOp, t: f32) -> TransformOp {
    match (a, b) {
        (TransformOp::Translate3d(ax, ay, az), TransformOp::Translate3d(bx, by, bz)) => {
            TransformOp::Translate3d(ax.lerp(bx, t), ay.lerp(by, t), lerp_f32(az, bz, t))
        }
        (TransformOp::Scale3d(ax, ay, az), TransformOp::Scale3d(bx, by, bz)) => {
            TransformOp::Scale3d(
                lerp_f32(ax, bx, t),
                lerp_f32(ay, by, t),
                lerp_f32(az, bz, t),
            )
        }
        (TransformOp::RotateZ(x), TransformOp::RotateZ(y)) => {
            TransformOp::RotateZ(lerp_f32(x, y, t))
        }
        (TransformOp::RotateX(x), TransformOp::RotateX(y)) => {
            TransformOp::RotateX(lerp_f32(x, y, t))
        }
        (TransformOp::RotateY(x), TransformOp::RotateY(y)) => {
            TransformOp::RotateY(lerp_f32(x, y, t))
        }
        (TransformOp::Rotate3d(ax, ay, az, aa), TransformOp::Rotate3d(bx, by, bz, ba)) => {
            TransformOp::Rotate3d(
                lerp_f32(ax, bx, t),
                lerp_f32(ay, by, t),
                lerp_f32(az, bz, t),
                lerp_f32(aa, ba, t),
            )
        }
        (TransformOp::Skew(ax, ay), TransformOp::Skew(bx, by)) => {
            TransformOp::Skew(lerp_f32(ax, bx, t), lerp_f32(ay, by, t))
        }
        (TransformOp::Perspective(x), TransformOp::Perspective(y)) => {
            TransformOp::Perspective(lerp_f32(x, y, t).max(1.0))
        }
        (TransformOp::Matrix2d(ma), TransformOp::Matrix2d(mb)) => {
            let mut m = [0.0; 6];
            for i in 0..6 {
                m[i] = lerp_f32(ma[i], mb[i], t);
            }
            TransformOp::Matrix2d(m)
        }
        (TransformOp::Matrix3d(ma), TransformOp::Matrix3d(mb)) => {
            let mut m = [0.0; 16];
            for i in 0..16 {
                m[i] = lerp_f32(ma[i], mb[i], t);
            }
            TransformOp::Matrix3d(m)
        }
        _ if t < 0.5 => a,
        _ => b,
    }
}
