//! Data types flowing through the graph and the values of node parameters.

use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

use crate::curve::Curve;
use crate::gradient::Gradient;

/// Type of a socket. Images live on the GPU, everything else is a CPU value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataType {
    Image,
    Float,
    Int,
    Bool,
    Vec2,
    Color,
}

impl DataType {
    pub const VALUE_TYPES: [DataType; 5] =
        [DataType::Float, DataType::Int, DataType::Bool, DataType::Vec2, DataType::Color];

    pub fn name(self) -> &'static str {
        match self {
            DataType::Image => "Image",
            DataType::Float => "Float",
            DataType::Int => "Integer",
            DataType::Bool => "Boolean",
            DataType::Vec2 => "Vec2",
            DataType::Color => "Color",
        }
    }

    pub fn is_scalar(self) -> bool {
        matches!(self, DataType::Float | DataType::Int | DataType::Bool)
    }
}

/// Can an output of type `from` feed an input of type `to`?
///
/// Images only connect to images. Scalars convert freely between each other
/// (float → int rounds, anything → bool tests ≠ 0) and numbers broadcast into
/// vectors and colors. Vectors and colors never silently collapse to scalars.
pub fn can_connect(from: DataType, to: DataType) -> bool {
    use DataType::*;
    match (from, to) {
        (Image, Image) => true,
        (Image, _) | (_, Image) => false,
        (a, b) if a == b => true,
        (Float | Int | Bool, Float | Int | Bool) => true,
        (Float | Int, Vec2 | Color) => true,
        _ => false,
    }
}

/// A CPU-side value flowing through a value socket. A file says it by its type:
/// `{"float": 0.5}`, `{"int": 4}`, `{"bool": true}`, `{"vec2": [x, y]}`, `{"color": [r, g, b, a]}`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Value {
    Float(f32),
    Int(i32),
    Bool(bool),
    Vec2([f32; 2]),
    Color([f32; 4]),
}

impl Value {
    pub fn default_for(ty: DataType) -> Value {
        match ty {
            DataType::Float | DataType::Image => Value::Float(0.0),
            DataType::Int => Value::Int(0),
            DataType::Bool => Value::Bool(false),
            DataType::Vec2 => Value::Vec2([0.0, 0.0]),
            DataType::Color => Value::Color([0.5, 0.5, 0.5, 1.0]),
        }
    }

    pub fn data_type(&self) -> DataType {
        match self {
            Value::Float(_) => DataType::Float,
            Value::Int(_) => DataType::Int,
            Value::Bool(_) => DataType::Bool,
            Value::Vec2(_) => DataType::Vec2,
            Value::Color(_) => DataType::Color,
        }
    }

    /// Scalar view used by expressions.
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Value::Float(v) => Some(v as f64),
            Value::Int(v) => Some(v as f64),
            Value::Bool(v) => Some(if v { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn is_finite(&self) -> bool {
        match self {
            Value::Float(v) => v.is_finite(),
            Value::Int(_) | Value::Bool(_) => true,
            Value::Vec2(v) => v.iter().all(|c| c.is_finite()),
            Value::Color(v) => v.iter().all(|c| c.is_finite()),
        }
    }

    /// Convert following the rules of [`can_connect`].
    pub fn convert(&self, to: DataType) -> Option<Value> {
        if self.data_type() == to {
            return Some(*self);
        }
        if !can_connect(self.data_type(), to) {
            return None;
        }
        let s = self.as_f64()?;
        Some(match to {
            DataType::Float => Value::Float(s as f32),
            DataType::Int => {
                let r = s.round();
                Value::Int(if r.is_nan() { 0 } else { r.clamp(i32::MIN as f64, i32::MAX as f64) as i32 })
            }
            DataType::Bool => Value::Bool(s != 0.0),
            DataType::Vec2 => Value::Vec2([s as f32; 2]),
            DataType::Color => {
                let g = s as f32;
                Value::Color([g, g, g, 1.0])
            }
            DataType::Image => return None,
        })
    }

    /// Clamp into [min, max] component-wise (no-op for bool and color).
    pub fn clamped(&self, min: &Value, max: &Value) -> Value {
        match (self, min, max) {
            (Value::Float(v), Value::Float(a), Value::Float(b)) if a <= b => Value::Float(v.clamp(*a, *b)),
            (Value::Int(v), Value::Int(a), Value::Int(b)) if a <= b => Value::Int(*v.clamp(a, b)),
            (Value::Vec2(v), Value::Vec2(a), Value::Vec2(b)) => {
                let mut out = *v;
                for i in 0..2 {
                    if a[i] <= b[i] {
                        out[i] = v[i].clamp(a[i], b[i]);
                    }
                }
                Value::Vec2(out)
            }
            _ => *self,
        }
    }

    pub fn display(&self) -> String {
        fn f(v: f32) -> String {
            if v.is_nan() {
                "NaN".into()
            } else if v.is_infinite() {
                if v > 0.0 { "∞".into() } else { "-∞".into() }
            } else if v.abs() >= 1e5 || (v != 0.0 && v.abs() < 1e-3) {
                format!("{v:.3e}")
            } else {
                let s = format!("{v:.4}");
                let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
                if s == "-0" { "0".into() } else { s }
            }
        }
        match self {
            Value::Float(v) => f(*v),
            Value::Int(v) => v.to_string(),
            Value::Bool(v) => {
                if *v {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            Value::Vec2([x, y]) => format!("({}, {})", f(*x), f(*y)),
            Value::Color([r, g, b, a]) => format!("({}, {}, {}, {})", f(*r), f(*g), f(*b), f(*a)),
        }
    }

    pub fn hash_into<H: Hasher>(&self, h: &mut H) {
        match self {
            Value::Float(v) => (0u8, v.to_bits()).hash(h),
            Value::Int(v) => (1u8, *v).hash(h),
            Value::Bool(v) => (2u8, *v).hash(h),
            Value::Vec2(v) => (3u8, v.map(f32::to_bits)).hash(h),
            Value::Color(v) => (4u8, v.map(f32::to_bits)).hash(h),
        }
    }
}

/// Tiling mode shared by all image nodes (bit 0 = horizontal, bit 1 = vertical).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tiling {
    None = 0,
    Horizontal = 1,
    Vertical = 2,
    Both = 3,
}

impl Tiling {
    pub const LABELS: [&'static str; 4] = ["No tiling", "Horizontal", "Vertical", "H and V"];
    /// What a file calls each (see [`crate::nodes::TILING_PARAM`]).
    pub const IDS: [&'static str; 4] = ["none", "horizontal", "vertical", "both"];

    pub fn from_index(i: u32) -> Tiling {
        match i {
            0 => Tiling::None,
            1 => Tiling::Horizontal,
            2 => Tiling::Vertical,
            _ => Tiling::Both,
        }
    }

    pub fn tiles_x(self) -> bool {
        (self as u32) & 1 != 0
    }

    pub fn tiles_y(self) -> bool {
        (self as u32) & 2 != 0
    }
}

/// The stored value of a node parameter (a file says it as `crate::graph::FileValue` does).
#[derive(Clone, Debug, PartialEq)]
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    Vec2([f32; 2]),
    Color([f32; 4]),
    /// One of the options of a choice, by its place (see [`crate::nodes::ParamKind::Enum`]).
    Enum(u32),
    Text(String),
    Gradient(Gradient),
    /// A tone curve (see [`crate::curve`]).
    Curve(Curve),
}

impl ParamValue {
    /// Type of the socket that appears when the parameter is exposed.
    /// Enums are driven by integers; text and gradients cannot be exposed.
    pub fn socket_type(&self) -> Option<DataType> {
        match self {
            ParamValue::Float(_) => Some(DataType::Float),
            ParamValue::Int(_) | ParamValue::Enum(_) => Some(DataType::Int),
            ParamValue::Bool(_) => Some(DataType::Bool),
            ParamValue::Vec2(_) => Some(DataType::Vec2),
            ParamValue::Color(_) => Some(DataType::Color),
            ParamValue::Text(_) | ParamValue::Gradient(_) | ParamValue::Curve(_) => None,
        }
    }

    pub fn same_kind(&self, other: &ParamValue) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    pub fn as_value(&self) -> Option<Value> {
        match self {
            ParamValue::Float(v) => Some(Value::Float(*v)),
            ParamValue::Int(v) => Some(Value::Int(*v)),
            ParamValue::Enum(v) => Some(Value::Int(*v as i32)),
            ParamValue::Bool(v) => Some(Value::Bool(*v)),
            ParamValue::Vec2(v) => Some(Value::Vec2(*v)),
            ParamValue::Color(v) => Some(Value::Color(*v)),
            ParamValue::Text(_) | ParamValue::Gradient(_) | ParamValue::Curve(_) => None,
        }
    }

    /// Build a parameter value of the same kind as `self` from an incoming value.
    /// `enum_count` bounds enum indices. Returns `None` when the value cannot be used
    /// (wrong type or not finite) so the caller falls back to the stored value.
    pub fn from_driven(&self, value: &Value, enum_count: usize) -> Option<ParamValue> {
        if !value.is_finite() {
            return None;
        }
        let ty = self.socket_type()?;
        let v = value.convert(ty)?;
        Some(match (self, v) {
            (ParamValue::Enum(_), Value::Int(i)) => {
                ParamValue::Enum(i.clamp(0, enum_count.saturating_sub(1) as i32) as u32)
            }
            (_, Value::Float(f)) => ParamValue::Float(f),
            (_, Value::Int(i)) => ParamValue::Int(i),
            (_, Value::Bool(b)) => ParamValue::Bool(b),
            (_, Value::Vec2(v)) => ParamValue::Vec2(v),
            (_, Value::Color(c)) => ParamValue::Color(c),
        })
    }

    pub fn hash_into<H: Hasher>(&self, h: &mut H) {
        match self {
            ParamValue::Float(v) => (0u8, v.to_bits()).hash(h),
            ParamValue::Int(v) => (1u8, *v).hash(h),
            ParamValue::Bool(v) => (2u8, *v).hash(h),
            ParamValue::Vec2(v) => (3u8, v.map(f32::to_bits)).hash(h),
            ParamValue::Color(v) => (4u8, v.map(f32::to_bits)).hash(h),
            ParamValue::Enum(v) => (5u8, *v).hash(h),
            ParamValue::Text(s) => (6u8, s).hash(h),
            ParamValue::Gradient(g) => {
                7u8.hash(h);
                g.hash_into(h);
            }
            ParamValue::Curve(c) => {
                8u8.hash(h);
                c.hash_into(h);
            }
        }
    }

    pub fn as_gradient(&self) -> Option<&Gradient> {
        match self {
            ParamValue::Gradient(g) => Some(g),
            _ => None,
        }
    }

    pub fn as_curve(&self) -> Option<&Curve> {
        match self {
            ParamValue::Curve(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> f32 {
        match self {
            ParamValue::Float(v) => *v,
            ParamValue::Int(v) => *v as f32,
            ParamValue::Enum(v) => *v as f32,
            ParamValue::Bool(v) => f32::from(u8::from(*v)),
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_rules() {
        use DataType::*;
        assert!(can_connect(Image, Image));
        assert!(!can_connect(Image, Float));
        assert!(!can_connect(Float, Image));
        assert!(can_connect(Float, Int));
        assert!(can_connect(Int, Float));
        assert!(can_connect(Bool, Float));
        assert!(can_connect(Float, Vec2));
        assert!(can_connect(Float, Color));
        assert!(!can_connect(Vec2, Float));
        assert!(!can_connect(Color, Vec2));
        assert!(!can_connect(Bool, Vec2));
    }

    #[test]
    fn conversions() {
        assert_eq!(Value::Float(2.6).convert(DataType::Int), Some(Value::Int(3)));
        assert_eq!(Value::Float(f32::NAN).convert(DataType::Int), Some(Value::Int(0)));
        assert_eq!(Value::Int(0).convert(DataType::Bool), Some(Value::Bool(false)));
        assert_eq!(Value::Float(0.5).convert(DataType::Color), Some(Value::Color([0.5, 0.5, 0.5, 1.0])));
        assert_eq!(Value::Vec2([1.0, 2.0]).convert(DataType::Float), None);
        let p = ParamValue::Enum(0);
        assert_eq!(p.from_driven(&Value::Float(99.0), 4), Some(ParamValue::Enum(3)));
        assert_eq!(p.from_driven(&Value::Float(f32::INFINITY), 4), None);
        assert_eq!(ParamValue::Text("a".into()).from_driven(&Value::Int(1), 0), None);
    }

    #[test]
    fn display() {
        assert_eq!(Value::Float(0.25).display(), "0.25");
        assert_eq!(Value::Float(2.0).display(), "2");
        assert_eq!(Value::Float(-0.0).display(), "0");
        assert_eq!(Value::Vec2([1.0, 0.5]).display(), "(1, 0.5)");
    }
}
