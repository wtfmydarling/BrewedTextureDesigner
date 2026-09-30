//! WGSL code generation: compiles an expression into a shader expression so the Pixel Math
//! node can evaluate it per pixel, and FX-Map per splat, on the GPU.
//!
//! The helpers in [`PRELUDE`] reproduce the CPU semantics (floored modulo, pow with a
//! negative base, rounding halves away from zero, the PCG hash) so that the same expression
//! gives the same result on both sides; each for numbers and for vectors of 2, 3 and 4
//! (WGSL has no functions of either), `fx_mod` and `fx_mod3` and so on.

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
fn fx_pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}
// Whose random numbers `rand(k)` gives: the splat's, the pixel's (set before they are read).
var<private> fx_seed: u32;
fn fx_rand(k: f32) -> f32 {
    return f32(fx_pcg(fx_seed ^ fx_pcg(bitcast<u32>(k))) >> 8u) / 16777216.0;
}
fn fx_mod2(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> { return a - b * floor(a / b); }
fn fx_mod3(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> { return a - b * floor(a / b); }
fn fx_mod4(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> { return a - b * floor(a / b); }
fn fx_pow2(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> { return vec2<f32>(fx_pow(a.x, b.x), fx_pow(a.y, b.y)); }
fn fx_pow3(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> { return vec3<f32>(fx_pow(a.x, b.x), fx_pow(a.y, b.y), fx_pow(a.z, b.z)); }
fn fx_pow4(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(fx_pow(a.x, b.x), fx_pow(a.y, b.y), fx_pow(a.z, b.z), fx_pow(a.w, b.w));
}
fn fx_round2(x: vec2<f32>) -> vec2<f32> { return sign(x) * floor(abs(x) + 0.5); }
fn fx_round3(x: vec3<f32>) -> vec3<f32> { return sign(x) * floor(abs(x) + 0.5); }
fn fx_round4(x: vec4<f32>) -> vec4<f32> { return sign(x) * floor(abs(x) + 0.5); }
fn fx_fract2(x: vec2<f32>) -> vec2<f32> { return x - floor(x); }
fn fx_fract3(x: vec3<f32>) -> vec3<f32> { return x - floor(x); }
fn fx_fract4(x: vec4<f32>) -> vec4<f32> { return x - floor(x); }
fn fx_lerp2(a: vec2<f32>, b: vec2<f32>, t: vec2<f32>) -> vec2<f32> { return a + (b - a) * t; }
fn fx_lerp3(a: vec3<f32>, b: vec3<f32>, t: vec3<f32>) -> vec3<f32> { return a + (b - a) * t; }
fn fx_lerp4(a: vec4<f32>, b: vec4<f32>, t: vec4<f32>) -> vec4<f32> { return a + (b - a) * t; }
fn fx_cbrt2(x: vec2<f32>) -> vec2<f32> { return sign(x) * pow(abs(x), vec2<f32>(1.0 / 3.0)); }
fn fx_cbrt3(x: vec3<f32>) -> vec3<f32> { return sign(x) * pow(abs(x), vec3<f32>(1.0 / 3.0)); }
fn fx_cbrt4(x: vec4<f32>) -> vec4<f32> { return sign(x) * pow(abs(x), vec4<f32>(1.0 / 3.0)); }
fn fx_smoothstep2(e0: vec2<f32>, e1: vec2<f32>, x: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(fx_smoothstep(e0.x, e1.x, x.x), fx_smoothstep(e0.y, e1.y, x.y));
}
fn fx_smoothstep3(e0: vec3<f32>, e1: vec3<f32>, x: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(fx_smoothstep(e0.x, e1.x, x.x), fx_smoothstep(e0.y, e1.y, x.y), fx_smoothstep(e0.z, e1.z, x.z));
}
fn fx_smoothstep4(e0: vec4<f32>, e1: vec4<f32>, x: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(fx_smoothstep(e0.x, e1.x, x.x), fx_smoothstep(e0.y, e1.y, x.y), fx_smoothstep(e0.z, e1.z, x.z), fx_smoothstep(e0.w, e1.w, x.w));
}
"#;

/// The WGSL type of a value of `dim` components.
pub fn wgsl_type(dim: u8) -> &'static str {
    match dim {
        1 => "f32",
        2 => "vec2<f32>",
        3 => "vec3<f32>",
        _ => "vec4<f32>",
    }
}

/// The suffix of the helpers of [`PRELUDE`] for values of `dim` components.
fn suffix(dim: u8) -> &'static str {
    match dim {
        1 => "",
        2 => "2",
        3 => "3",
        _ => "4",
    }
}

/// `code`, of `from` components, as a value of `to`: a number as a vector of it.
fn widen(code: &str, from: u8, to: u8) -> String {
    if from == to { code.to_string() } else { format!("{}({code})", wgsl_type(to)) }
}

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

/// How the WGSL an expression is made into reads what it reads.
pub struct Env<'a> {
    /// Variable `i`: an input (as a number: the channel of an image a node reads) or a local.
    pub var: &'a dyn Fn(usize) -> String,
    /// How many components variable `i` has.
    pub dim: &'a dyn Fn(usize) -> u8,
    /// The four channels of input `i` (`img.rgb`): of its image, or its value in red, green and
    /// blue, opaque.
    pub texel: &'a dyn Fn(usize) -> String,
    /// `sample(image, x, y)`: input `i` at `x`, `y` (WGSL numbers), as a number.
    pub sample: &'a dyn Fn(usize, &str, &str) -> String,
    /// Its four channels (`sample(image, x, y).rgb`).
    pub sample_texel: &'a dyn Fn(usize, &str, &str) -> String,
}

/// Translate the value of `program` (the first of a list) to a WGSL expression. `var` maps a
/// variable index to the WGSL expression that provides its value; `sample(image, x, y)` reads
/// the variable's value.
pub fn to_wgsl(program: &Program, var: &dyn Fn(usize) -> String) -> String {
    to_wgsl_sampling(program, var, &|i, _, _| var(i))
}

/// [`to_wgsl`], with `sample` giving the WGSL expression that reads variable `i` at the uv
/// coordinates `x` and `y` (WGSL expressions): `sample(image, x, y)` of Pixel Math.
pub fn to_wgsl_sampling(program: &Program, var: &dyn Fn(usize) -> String, sample: &dyn Fn(usize, &str, &str) -> String) -> String {
    let dim = |i: usize| program.var_dim(i);
    let texel = |i: usize| format!("vec4<f32>(vec3<f32>({}), 1.0)", var(i));
    let sample_texel = |i: usize, x: &str, y: &str| format!("vec4<f32>(vec3<f32>({}), 1.0)", sample(i, x, y));
    emit(&program.root, &Env { var, dim: &dim, texel: &texel, sample, sample_texel: &sample_texel }).0
}

/// The statements of `program` as WGSL, its locals declared first (`local(k)` names local `k`,
/// which `env.var` reads), and the WGSL of each component of its value, in order.
pub fn program_wgsl(program: &Program, env: &Env, local: &dyn Fn(usize) -> String) -> (String, Vec<String>) {
    let mut body = String::new();
    for (k, dim) in program.local_dims.iter().enumerate() {
        body.push_str(&format!("    var {}: {} = {}(0.0);\n", local(k), wgsl_type(*dim), wgsl_type(*dim)));
    }
    for s in &program.statements {
        let (code, dim) = emit(&s.values[0], env);
        body.push_str(&format!("    {} = {};\n", local(s.local), widen(&code, dim, program.local_dims[s.local])));
    }
    let mut components = Vec::new();
    for (k, node) in std::iter::once(&program.root).chain(&program.more).enumerate() {
        let (code, dim) = emit(node, env);
        if dim == 1 {
            components.push(code);
        } else {
            body.push_str(&format!("    let fx_value{k} = {code};\n"));
            components.extend(["x", "y", "z", "w"].iter().take(usize::from(dim)).map(|c| format!("fx_value{k}.{c}")));
        }
    }
    (body, components)
}

/// Each statement of a script: the local it sets, and the WGSL of its value or values, each
/// with its components.
pub fn statements_wgsl(script: &super::script::Script, env: &Env) -> Vec<(usize, Vec<(String, u8)>)> {
    script.statements.iter().map(|s| (s.local, s.values.iter().map(|node| emit(node, env)).collect())).collect()
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

/// The channels at `places`, as WGSL swizzle letters.
fn letters(places: &[u8]) -> String {
    places.iter().map(|p| ['x', 'y', 'z', 'w'][usize::from(*p).min(3)]).collect()
}

/// The WGSL of `node`, and how many components it has.
fn emit(node: &Node, env: &Env) -> (String, u8) {
    match node {
        Node::Num(v) => (literal(*v), 1),
        Node::Var(i) => (format!("({})", (env.var)(*i)), (env.dim)(*i)),
        Node::Unary(UnOp::Neg, a) => {
            let (a, d) = emit(a, env);
            (format!("(-{a})"), d)
        }
        Node::Unary(UnOp::Not, a) => (format!("fx_bool({} == 0.0)", emit(a, env).0), 1),
        Node::Binary(op, a, b) => {
            let ((a, da), (b, db)) = (emit(a, env), emit(b, env));
            let d = da.max(db);
            let code = match op {
                // (WGSL takes a number with a vector for these.)
                BinOp::Add => format!("({a} + {b})"),
                BinOp::Sub => format!("({a} - {b})"),
                BinOp::Mul => format!("({a} * {b})"),
                BinOp::Div => format!("({a} / {b})"),
                BinOp::Mod => format!("fx_mod{}({}, {})", suffix(d), widen(&a, da, d), widen(&b, db, d)),
                BinOp::Pow => format!("fx_pow{}({}, {})", suffix(d), widen(&a, da, d), widen(&b, db, d)),
                BinOp::Lt => format!("fx_bool({a} < {b})"),
                BinOp::Le => format!("fx_bool({a} <= {b})"),
                BinOp::Gt => format!("fx_bool({a} > {b})"),
                BinOp::Ge => format!("fx_bool({a} >= {b})"),
                BinOp::Eq => format!("fx_bool({a} == {b})"),
                BinOp::Ne => format!("fx_bool({a} != {b})"),
                BinOp::And => format!("fx_bool(({a} != 0.0) && ({b} != 0.0))"),
                BinOp::Or => format!("fx_bool(({a} != 0.0) || ({b} != 0.0))"),
            };
            (code, if op.is_logical() { 1 } else { d })
        }
        Node::Cond(c, a, b) => {
            let ((a, da), (b, db)) = (emit(a, env), emit(b, env));
            let d = da.max(db);
            (format!("select({}, {}, {} != 0.0)", widen(&b, db, d), widen(&a, da, d), emit(c, env).0), d)
        }
        Node::Swizzle(base, places) => {
            let picked = letters(places);
            let n = places.len() as u8;
            match base.as_ref() {
                // The channels of an image input (a number the program sets is its own in
                // three, opaque: its .x is itself), and of what `sample` reads of one.
                Node::Var(i) if (env.dim)(*i) == 1 => (format!("({}).{picked}", (env.texel)(*i)), n),
                Node::Call(Func::Sample, args) if matches!(args.as_slice(), [Node::Var(_), _, _]) => {
                    let Node::Var(i) = &args[0] else { unreachable!("sample reads a name") };
                    let (x, y) = (emit(&args[1], env).0, emit(&args[2], env).0);
                    (format!("({}).{picked}", (env.sample_texel)(*i, &x, &y)), n)
                }
                _ => {
                    let (code, d) = emit(base, env);
                    if d == 1 {
                        // Of a number (only .x, .xx…): itself, or a vector of it.
                        (widen(&code, 1, n), n)
                    } else {
                        (format!("({code}).{picked}"), n)
                    }
                }
            }
        }
        Node::Call(func, args) => {
            // The image `sample` reads is named, not computed (see the parser).
            if *func == Func::Sample
                && let [Node::Var(i), x, y] = args.as_slice()
            {
                return (format!("({})", (env.sample)(*i, &emit(x, env).0, &emit(y, env).0)), 1);
            }
            let emitted: Vec<(String, u8)> = args.iter().map(|n| emit(n, env)).collect();
            let d = emitted.iter().map(|(_, d)| *d).max().unwrap_or(1);
            // Each argument as a value of as many components as the widest.
            let a: Vec<String> = emitted.iter().map(|(code, dim)| widen(code, *dim, d)).collect();
            let x = a.first().cloned().unwrap_or_else(|| "0.0".into());
            let y = a.get(1).cloned().unwrap_or_else(|| "0.0".into());
            let z = a.get(2).cloned().unwrap_or_else(|| "0.0".into());
            let s = suffix(d);
            let code = match func {
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
                Func::Cbrt => format!("fx_cbrt{s}({x})"),
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
                Func::Pow => format!("fx_pow{s}({x}, {y})"),
                Func::Abs => format!("abs({x})"),
                Func::Sign => format!("sign({x})"),
                Func::Floor => format!("floor({x})"),
                Func::Ceil => format!("ceil({x})"),
                Func::Round => format!("fx_round{s}({x})"),
                Func::Trunc => format!("trunc({x})"),
                Func::Fract => format!("fx_fract{s}({x})"),
                Func::Min | Func::Max => pairs(if *func == Func::Min { "min" } else { "max" }, &a),
                Func::Clamp => format!("min(max({x}, {y}), {z})"),
                Func::Saturate => format!("saturate({x})"),
                Func::Lerp => format!("fx_lerp{s}({x}, {y}, {z})"),
                Func::Step => format!("select({}(1.0), {}(0.0), {y} < {x})", wgsl_type(d), wgsl_type(d)),
                Func::Smoothstep => format!("fx_smoothstep{s}({x}, {y}, {z})"),
                Func::Mod => format!("fx_mod{s}({x}, {y})"),
                Func::Hypot => format!("length(vec2<f32>({x}, {y}))"),
                Func::Degrees => format!("degrees({x})"),
                Func::Radians => format!("radians({x})"),
                Func::If => {
                    // The condition is a number; the values as wide as the wider.
                    let d = emitted[1].1.max(emitted[2].1);
                    let (c, t, f) = (&emitted[0].0, widen(&emitted[1].0, emitted[1].1, d), widen(&emitted[2].0, emitted[2].1, d));
                    return (format!("select({f}, {t}, {c} != 0.0)"), d);
                }
                Func::Hash => format!("fx_hash({x})"),
                Func::Rand => format!("fx_rand({x})"),
                Func::Sample => x,
                Func::Vec2 | Func::Vec3 | Func::Vec4 => {
                    let n = match func {
                        Func::Vec2 => 2,
                        Func::Vec3 => 3,
                        _ => 4,
                    };
                    let parts: Vec<&str> = emitted.iter().map(|(code, _)| code.as_str()).collect();
                    return (format!("{}({})", wgsl_type(n), parts.join(", ")), n);
                }
                Func::Dot if d == 1 => format!("({x} * {y})"),
                Func::Dot => format!("dot({x}, {y})"),
                Func::Length => format!("length({x})"),
                Func::Distance => format!("distance({x}, {y})"),
                Func::Normalize if d == 1 => format!("sign({x})"),
                Func::Normalize => format!("normalize({x})"),
                Func::Cross => format!("cross({x}, {y})"),
            };
            let dim = match func {
                Func::Dot | Func::Length | Func::Distance | Func::Hash | Func::Rand | Func::Hypot => 1,
                _ => d,
            };
            (code, dim)
        }
    }
}
