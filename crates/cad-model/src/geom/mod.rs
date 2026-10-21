//! Geometry primitives: vectors, affine transforms, the object coordinate
//! system (OCS), bounding boxes and curve helpers.

pub mod curve;

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

pub const EPS: f64 = 1e-12;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    pub const fn new(x: f64, y: f64) -> Self {
        Vec2 { x, y }
    }
    pub fn from_array(a: [f64; 2]) -> Self {
        Vec2 { x: a[0], y: a[1] }
    }
    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    /// z component of the 3D cross product.
    pub fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn normalized(self) -> Vec2 {
        let l = self.length();
        if l < EPS {
            Vec2::ZERO
        } else {
            self / l
        }
    }
    /// Rotated 90° counter-clockwise.
    pub fn perp(self) -> Vec2 {
        Vec2 {
            x: -self.y,
            y: self.x,
        }
    }
    pub fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }
    pub fn from_angle(a: f64) -> Vec2 {
        Vec2 {
            x: a.cos(),
            y: a.sin(),
        }
    }
    pub fn distance(self, o: Vec2) -> f64 {
        (self - o).length()
    }
    pub fn lerp(self, o: Vec2, t: f64) -> Vec2 {
        self + (o - self) * t
    }
    pub fn extend(self, z: f64) -> Vec3 {
        Vec3 {
            x: self.x,
            y: self.y,
            z,
        }
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    pub const X: Vec3 = Vec3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    pub const Y: Vec3 = Vec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    pub const Z: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }
    pub fn from_array(a: [f64; 3]) -> Self {
        Vec3 {
            x: a[0],
            y: a[1],
            z: a[2],
        }
    }
    pub fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3 {
            x: self.y * o.z - self.z * o.y,
            y: self.z * o.x - self.x * o.z,
            z: self.x * o.y - self.y * o.x,
        }
    }
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Vec3 {
        let l = self.length();
        if l < EPS {
            Vec3::ZERO
        } else {
            self / l
        }
    }
    pub fn xy(self) -> Vec2 {
        Vec2 {
            x: self.x,
            y: self.y,
        }
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

macro_rules! impl_vec_ops {
    ($t:ident, $($f:ident),+) => {
        impl Add for $t { type Output = $t; fn add(self, o: $t) -> $t { $t { $($f: self.$f + o.$f),+ } } }
        impl Sub for $t { type Output = $t; fn sub(self, o: $t) -> $t { $t { $($f: self.$f - o.$f),+ } } }
        impl Mul<f64> for $t { type Output = $t; fn mul(self, s: f64) -> $t { $t { $($f: self.$f * s),+ } } }
        impl Div<f64> for $t { type Output = $t; fn div(self, s: f64) -> $t { $t { $($f: self.$f / s),+ } } }
        impl Neg for $t { type Output = $t; fn neg(self) -> $t { $t { $($f: -self.$f),+ } } }
        impl AddAssign for $t { fn add_assign(&mut self, o: $t) { $(self.$f += o.$f;)+ } }
    };
}
impl_vec_ops!(Vec2, x, y);
impl_vec_ops!(Vec3, x, y, z);

/// Affine transform in 3D: `p' = M * [x y z 1]ᵀ` with a 3×4 matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub m: [[f64; 4]; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Transform::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    };

    pub fn translation(v: Vec3) -> Transform {
        let mut t = Transform::IDENTITY;
        t.m[0][3] = v.x;
        t.m[1][3] = v.y;
        t.m[2][3] = v.z;
        t
    }

    pub fn scale(s: Vec3) -> Transform {
        Transform {
            m: [
                [s.x, 0.0, 0.0, 0.0],
                [0.0, s.y, 0.0, 0.0],
                [0.0, 0.0, s.z, 0.0],
            ],
        }
    }

    pub fn rotation_z(angle: f64) -> Transform {
        let (s, c) = angle.sin_cos();
        Transform {
            m: [[c, -s, 0.0, 0.0], [s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
        }
    }

    /// Columns are the images of the x, y and z axes; `origin` is the
    /// translation.
    pub fn from_axes(x: Vec3, y: Vec3, z: Vec3, origin: Vec3) -> Transform {
        Transform {
            m: [
                [x.x, y.x, z.x, origin.x],
                [x.y, y.y, z.y, origin.y],
                [x.z, y.z, z.z, origin.z],
            ],
        }
    }

    /// OCS → WCS for an extrusion (normal) vector, via the arbitrary axis
    /// algorithm.
    pub fn ocs_to_wcs(normal: Vec3) -> Transform {
        let (ax, ay, az) = arbitrary_axes(normal);
        Transform::from_axes(ax, ay, az, Vec3::ZERO)
    }

    /// `self` applied after `first`: (self ∘ first)(p) = self(first(p)).
    pub fn after(&self, first: &Transform) -> Transform {
        let a = &self.m;
        let b = &first.m;
        let mut r = [[0.0; 4]; 3];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                let mut v = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
                if j == 3 {
                    v += a[i][3];
                }
                *cell = v;
            }
        }
        Transform { m: r }
    }

    pub fn apply(&self, p: Vec3) -> Vec3 {
        let m = &self.m;
        Vec3 {
            x: m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3],
            y: m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3],
            z: m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3],
        }
    }

    pub fn apply_vector(&self, v: Vec3) -> Vec3 {
        let m = &self.m;
        Vec3 {
            x: m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
            y: m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
            z: m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
        }
    }

    /// Apply to a 2D point lying at elevation `z` and project to plan (XY).
    pub fn apply_xy(&self, p: Vec2, z: f64) -> Vec2 {
        self.apply(p.extend(z)).xy()
    }

    /// If the plan projection of a plane spanned by `u`,`v` (unit,
    /// orthogonal, in the source space) is a similarity, return
    /// (uniform scale, mirrored). Circles map to circles exactly then.
    pub fn plane_similarity(&self, u: Vec3, v: Vec3) -> Option<(f64, bool)> {
        let a = self.apply_vector(u).xy();
        let b = self.apply_vector(v).xy();
        let la = a.length();
        let lb = b.length();
        if la < EPS || lb < EPS {
            return None;
        }
        let rel = 1e-6;
        if (la - lb).abs() > rel * la.max(lb) || a.dot(b).abs() > rel * la * lb {
            return None;
        }
        Some((la, a.cross(b) < 0.0))
    }
}

/// The arbitrary axis algorithm used by AutoCAD for object coordinate
/// systems: returns the OCS x, y and z axes in WCS for a normal vector.
pub fn arbitrary_axes(normal: Vec3) -> (Vec3, Vec3, Vec3) {
    let n = {
        let l = normal.length();
        if l < EPS || !l.is_finite() {
            Vec3::Z
        } else {
            normal / l
        }
    };
    let limit = 1.0 / 64.0;
    let ax = if n.x.abs() < limit && n.y.abs() < limit {
        Vec3::Y.cross(n)
    } else {
        Vec3::Z.cross(n)
    }
    .normalized();
    let ay = n.cross(ax).normalized();
    (ax, ay, n)
}

/// True when the normal is the default +Z (within tolerance).
pub fn is_default_normal(n: Vec3) -> bool {
    (n.x.abs() < 1e-12) && (n.y.abs() < 1e-12) && (n.z - 1.0).abs() < 1e-12
}

/// Axis-aligned bounding box in plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub min: Vec2,
    pub max: Vec2,
}

impl Default for BBox {
    fn default() -> Self {
        BBox::EMPTY
    }
}

impl BBox {
    pub const EMPTY: BBox = BBox {
        min: Vec2 {
            x: f64::INFINITY,
            y: f64::INFINITY,
        },
        max: Vec2 {
            x: f64::NEG_INFINITY,
            y: f64::NEG_INFINITY,
        },
    };

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }
    pub fn add(&mut self, p: Vec2) {
        if !p.is_finite() {
            return;
        }
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }
    pub fn union(&mut self, o: &BBox) {
        if !o.is_empty() {
            self.add(o.min);
            self.add(o.max);
        }
    }
    pub fn width(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            self.max.x - self.min.x
        }
    }
    pub fn height(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            self.max.y - self.min.y
        }
    }
    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn arbitrary_axis_default_and_flipped() {
        let (ax, ay, az) = arbitrary_axes(Vec3::Z);
        assert!(close(ax.x, 1.0) && close(ay.y, 1.0) && close(az.z, 1.0));
        // Normal (0,0,-1): mirrored plan, x axis becomes -X.
        let (ax, ay, _) = arbitrary_axes(Vec3::new(0.0, 0.0, -1.0));
        assert!(close(ax.x, -1.0) && close(ay.y, 1.0));
    }

    #[test]
    fn compose() {
        let t = Transform::translation(Vec3::new(10.0, 0.0, 0.0))
            .after(&Transform::rotation_z(std::f64::consts::FRAC_PI_2));
        let p = t.apply(Vec3::new(1.0, 0.0, 0.0));
        assert!(close(p.x, 10.0) && close(p.y, 1.0));
    }

    #[test]
    fn similarity_detection() {
        let t = Transform::scale(Vec3::new(2.0, 2.0, 1.0));
        assert_eq!(t.plane_similarity(Vec3::X, Vec3::Y), Some((2.0, false)));
        let t = Transform::scale(Vec3::new(-2.0, 2.0, 1.0));
        assert_eq!(t.plane_similarity(Vec3::X, Vec3::Y), Some((2.0, true)));
        let t = Transform::scale(Vec3::new(2.0, 1.0, 1.0));
        assert_eq!(t.plane_similarity(Vec3::X, Vec3::Y), None);
    }
}
