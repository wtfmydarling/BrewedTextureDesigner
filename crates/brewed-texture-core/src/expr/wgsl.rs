//! WGSL code generation: compiles an expression into a shader expression so the
//! Pixel Math node can evaluate it per pixel on the GPU.
//!
//! The helpers in [`PRELUDE`] reproduce the CPU semantics (floored modulo, pow with a
//! negative base, rounding halves away from zero, the PCG hash) so that the same
//! expression gives the same result on both sides.

use super::Program;
use super::builtins::Func;
use super::parser::{BinOp, Node, UnOp};

pub const PRELUDE: &str = r#"
fn fx_bool(b: bool) -> f32 { return select(0.0, 1.0, b); }
fn fx_mod(a: f32, b: f32) -> f32 { return a - b * floor(a / b); }
fn fx_pow(a: f32, b: f32) -> f32 {
    if (a >= 0.0) { return pow(a, b); }
    if (fract(b) == 0.0) {
        let r = pow(-a, b);
        return select(r, -r, (i32(abs(b)) & 1) == 1);
    }
    return bitcast<f32>(0x7fc00000u);
}
fn fx_round(x: f32) -> f32 { return sign(x) * floor(abs(x) + 0.5); }
fn fx_fract(x: f32) -> f32 { return x - floor(x); }
fn fx_lerp(a: f32, b: f32, t: f32) -> f32 { return a + (b - a) * t; }
fn fx_cbrt(x: f32) -> f32 { return sign(x) * pow(abs(x), 1.0 / 3.0); }
fn fx_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if (e0 == e1) { return select(1.0, 0.0, x < e0); }
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}
fn fx_hash(x: f32) -> f32 {
    let state = bitcast<u32>(x) * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return f32((word >> 22u) ^ word) / 4294967296.0;
}
"#;

fn literal(v: f64) -> String {
    let v = v as f32;
    if v.is_nan() {
        "bitcast<f32>(0x7fc00000u)".to_string()
    } else if v == f32::INFINITY {
        "bitcast<f32>(0x7f800000u)".to_string()
    } else if v == f32::NEG_INFINITY {
        "bitcast<f32>(0xff800000u)".to_string()
    } else {
        // `{:?}` always prints a decimal point or an exponent, e.g. `1.0`, `1e-7`.
        format!("f32({v:?})")
    }
}

/// Translate `program` to a WGSL `f32` expression. `var` maps a variable index to the
/// WGSL expression that provides its value; `sample(image, x, y)` reads the variable's value.
pub fn to_wgsl(program: &Program, var: &dyn Fn(usize) -> String) -> String {
    to_wgsl_sampling(program, var, &|i, _, _| var(i))
}

/// [`to_wgsl`], with `sample` giving the WGSL expression that reads variable `i` at the uv
/// coordinates `x` and `y` (WGSL expressions): `sample(image, x, y)` of Pixel Math.
pub fn to_wgsl_sampling(program: &Program, var: &dyn Fn(usize) -> String, sample: &dyn Fn(usize, &str, &str) -> String) -> String {
    emit(&program.root, &Env { var, sample })
}

/// [`to_wgsl_sampling`] of each value of the expression: one, or each of a list.
pub fn components_wgsl(program: &Program, var: &dyn Fn(usize) -> String, sample: &dyn Fn(usize, &str, &str) -> String) -> Vec<String> {
    let env = Env { var, sample };
    std::iter::once(&program.root).chain(&program.more).map(|node| emit(node, &env)).collect()
}

struct Env<'a> {
    var: &'a dyn Fn(usize) -> String,
    sample: &'a dyn Fn(usize, &str, &str) -> String,
}

/// `name` (`min` or `max`) of all of `values`, taken in pairs, and the pairs in pairs: as
/// deep as the logarithm of how many there are, not as many.
fn pairs(name: &str, values: &[String]) -> String {
    match values {
        [] => "0.0".into(),
        [one] => one.clone(),
        _ => {
            let (a, b) = values.split_at(values.len() / 2);
            format!("{name}({}, {})", pairs(name, a), pairs(name, b))
        }
    }
}

fn emit(node: &Node, env: &Env) -> String {
    let var = env.var;
    match node {
        Node::Num(v) => literal(*v),
        Node::Var(i) => format!("({})", var(*i)),
        Node::Unary(UnOp::Neg, a) => format!("(-{})", emit(a, env)),
        Node::Unary(UnOp::Not, a) => format!("fx_bool({} == 0.0)", emit(a, env)),
        Node::Binary(op, a, b) => {
            let (a, b) = (emit(a, env), emit(b, env));
            match op {
                BinOp::Add => format!("({a} + {b})"),
                BinOp::Sub => format!("({a} - {b})"),
                BinOp::Mul => format!("({a} * {b})"),
                BinOp::Div => format!("({a} / {b})"),
                BinOp::Mod => format!("fx_mod({a}, {b})"),
                BinOp::Pow => format!("fx_pow({a}, {b})"),
                BinOp::Lt => format!("fx_bool({a} < {b})"),
                BinOp::Le => format!("fx_bool({a} <= {b})"),
                BinOp::Gt => format!("fx_bool({a} > {b})"),
                BinOp::Ge => format!("fx_bool({a} >= {b})"),
                BinOp::Eq => format!("fx_bool({a} == {b})"),
                BinOp::Ne => format!("fx_bool({a} != {b})"),
                BinOp::And => format!("fx_bool(({a} != 0.0) && ({b} != 0.0))"),
                BinOp::Or => format!("fx_bool(({a} != 0.0) || ({b} != 0.0))"),
            }
        }
        Node::Cond(c, a, b) => {
            format!("select({}, {}, {} != 0.0)", emit(b, env), emit(a, env), emit(c, env))
        }
        Node::Call(func, args) => {
            // The image `sample` reads is named, not computed (see the parser).
            if *func == Func::Sample
                && let [Node::Var(i), x, y] = args.as_slice()
            {
                return format!("({})", (env.sample)(*i, &emit(x, env), &emit(y, env)));
            }
            let a: Vec<String> = args.iter().map(|n| emit(n, env)).collect();
            let x = a.first().cloned().unwrap_or_else(|| "0.0".into());
            let y = a.get(1).cloned().unwrap_or_else(|| "0.0".into());
            let z = a.get(2).cloned().unwrap_or_else(|| "0.0".into());
            match func {
                Func::Sin => format!("sin({x})"),
                Func::Cos => format!("cos({x})"),
                Func::Tan => format!("tan({x})"),
                Func::Asin => format!("asin({x})"),
                Func::Acos => format!("acos({x})"),
                Func::Atan => format!("atan({x})"),
                Func::Atan2 => format!("atan2({x}, {y})"),
                Func::Sinh => format!("sinh({x})"),
                Func::Cosh => format!("cosh({x})"),
                Func::Tanh => format!("tanh({x})"),
                Func::Sqrt => format!("sqrt({x})"),
                Func::Cbrt => format!("fx_cbrt({x})"),
                Func::Exp => format!("exp({x})"),
                Func::Exp2 => format!("exp2({x})"),
                Func::Ln => format!("log({x})"),
                Func::Log => {
                    if a.len() >= 2 {
                        format!("(log({x}) / log({y}))")
                    } else {
                        format!("(log({x}) * 0.4342944819)")
                    }
                }
                Func::Log2 => format!("log2({x})"),
                Func::Log10 => format!("(log({x}) * 0.4342944819)"),
                Func::Pow => format!("fx_pow({x}, {y})"),
                Func::Abs => format!("abs({x})"),
                Func::Sign => format!("sign({x})"),
                Func::Floor => format!("floor({x})"),
                Func::Ceil => format!("ceil({x})"),
                Func::Round => format!("fx_round({x})"),
                Func::Trunc => format!("trunc({x})"),
                Func::Fract => format!("fx_fract({x})"),
                Func::Min | Func::Max => pairs(if *func == Func::Min { "min" } else { "max" }, &a),
                Func::Clamp => format!("min(max({x}, {y}), {z})"),
                Func::Saturate => format!("saturate({x})"),
                Func::Lerp => format!("fx_lerp({x}, {y}, {z})"),
                Func::Step => format!("select(1.0, 0.0, {y} < {x})"),
                Func::Smoothstep => format!("fx_smoothstep({x}, {y}, {z})"),
                Func::Mod => format!("fx_mod({x}, {y})"),
                Func::Hypot => format!("length(vec2<f32>({x}, {y}))"),
                Func::Degrees => format!("degrees({x})"),
                Func::Radians => format!("radians({x})"),
                Func::If => format!("select({z}, {y}, {x} != 0.0)"),
                Func::Hash => format!("fx_hash({x})"),
                Func::Sample => x,
            }
        }
    }
}
