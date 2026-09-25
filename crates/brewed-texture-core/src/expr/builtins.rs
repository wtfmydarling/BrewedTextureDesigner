//! Built-in functions and constants of the expression language.

/// Every function the expression language understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Func {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Sinh,
    Cosh,
    Tanh,
    Sqrt,
    Cbrt,
    Exp,
    Exp2,
    Ln,
    Log,
    Log2,
    Log10,
    Pow,
    Abs,
    Sign,
    Floor,
    Ceil,
    Round,
    Trunc,
    Fract,
    Min,
    Max,
    Clamp,
    Saturate,
    Lerp,
    Step,
    Smoothstep,
    Mod,
    Hypot,
    Degrees,
    Radians,
    If,
    Hash,
    Sample,
}

pub const VARIADIC: usize = usize::MAX;

pub struct FuncInfo {
    pub name: &'static str,
    pub func: Func,
    pub min_args: usize,
    pub max_args: usize,
    pub signature: &'static str,
    pub doc: &'static str,
}

const fn f(
    name: &'static str,
    func: Func,
    min_args: usize,
    max_args: usize,
    signature: &'static str,
    doc: &'static str,
) -> FuncInfo {
    FuncInfo { name, func, min_args, max_args, signature, doc }
}

pub static FUNCTIONS: &[FuncInfo] = &[
    f("sin", Func::Sin, 1, 1, "sin(x)", "Sine (radians)"),
    f("cos", Func::Cos, 1, 1, "cos(x)", "Cosine (radians)"),
    f("tan", Func::Tan, 1, 1, "tan(x)", "Tangent (radians)"),
    f("asin", Func::Asin, 1, 1, "asin(x)", "Arc sine"),
    f("acos", Func::Acos, 1, 1, "acos(x)", "Arc cosine"),
    f("atan", Func::Atan, 1, 1, "atan(x)", "Arc tangent"),
    f("atan2", Func::Atan2, 2, 2, "atan2(y, x)", "Angle of the vector (x, y)"),
    f("sinh", Func::Sinh, 1, 1, "sinh(x)", "Hyperbolic sine"),
    f("cosh", Func::Cosh, 1, 1, "cosh(x)", "Hyperbolic cosine"),
    f("tanh", Func::Tanh, 1, 1, "tanh(x)", "Hyperbolic tangent"),
    f("sqrt", Func::Sqrt, 1, 1, "sqrt(x)", "Square root"),
    f("cbrt", Func::Cbrt, 1, 1, "cbrt(x)", "Cube root"),
    f("exp", Func::Exp, 1, 1, "exp(x)", "e raised to x"),
    f("exp2", Func::Exp2, 1, 1, "exp2(x)", "2 raised to x"),
    f("ln", Func::Ln, 1, 1, "ln(x)", "Natural logarithm"),
    f("log", Func::Log, 1, 2, "log(x) / log(x, base)", "Base-10 logarithm, or logarithm in a custom base"),
    f("log2", Func::Log2, 1, 1, "log2(x)", "Base-2 logarithm"),
    f("log10", Func::Log10, 1, 1, "log10(x)", "Base-10 logarithm"),
    f("pow", Func::Pow, 2, 2, "pow(x, y)", "x raised to y (same as x ^ y)"),
    f("abs", Func::Abs, 1, 1, "abs(x)", "Absolute value"),
    f("sign", Func::Sign, 1, 1, "sign(x)", "-1, 0 or 1"),
    f("floor", Func::Floor, 1, 1, "floor(x)", "Round down"),
    f("ceil", Func::Ceil, 1, 1, "ceil(x)", "Round up"),
    f("round", Func::Round, 1, 1, "round(x)", "Round to nearest (halves away from zero)"),
    f("trunc", Func::Trunc, 1, 1, "trunc(x)", "Drop the fractional part"),
    f("fract", Func::Fract, 1, 1, "fract(x)", "x - floor(x)"),
    f("min", Func::Min, 2, VARIADIC, "min(a, b, ...)", "Smallest argument"),
    f("max", Func::Max, 2, VARIADIC, "max(a, b, ...)", "Largest argument"),
    f("clamp", Func::Clamp, 3, 3, "clamp(x, lo, hi)", "Limit x to [lo, hi]"),
    f("saturate", Func::Saturate, 1, 1, "saturate(x)", "Limit x to [0, 1]"),
    f("lerp", Func::Lerp, 3, 3, "lerp(a, b, t)", "Linear interpolation a + (b - a)·t"),
    f("mix", Func::Lerp, 3, 3, "mix(a, b, t)", "Same as lerp"),
    f("step", Func::Step, 2, 2, "step(edge, x)", "0 if x < edge, otherwise 1"),
    f("smoothstep", Func::Smoothstep, 3, 3, "smoothstep(e0, e1, x)", "Smooth Hermite step between e0 and e1"),
    f("mod", Func::Mod, 2, 2, "mod(x, y)", "Floored modulo (same as x % y)"),
    f("hypot", Func::Hypot, 2, 2, "hypot(x, y)", "sqrt(x² + y²)"),
    f("degrees", Func::Degrees, 1, 1, "degrees(rad)", "Radians to degrees"),
    f("deg", Func::Degrees, 1, 1, "deg(rad)", "Radians to degrees"),
    f("radians", Func::Radians, 1, 1, "radians(deg)", "Degrees to radians"),
    f("rad", Func::Radians, 1, 1, "rad(deg)", "Degrees to radians"),
    f("if", Func::If, 3, 3, "if(cond, a, b)", "a when cond ≠ 0, otherwise b"),
    f("hash", Func::Hash, 1, 1, "hash(x)", "Deterministic pseudo-random number in [0, 1)"),
    f(
        "sample",
        Func::Sample,
        3,
        3,
        "sample(image, x, y)",
        "Pixel Math: the image input at (x, y), from 0 to 1 like u and v, between pixels; a value is the same everywhere",
    ),
];

pub struct ConstInfo {
    pub name: &'static str,
    pub value: f64,
    pub doc: &'static str,
}

pub static CONSTANTS: &[ConstInfo] = &[
    ConstInfo { name: "pi", value: std::f64::consts::PI, doc: "π ≈ 3.14159" },
    ConstInfo { name: "tau", value: std::f64::consts::TAU, doc: "τ = 2π ≈ 6.28319" },
    ConstInfo { name: "e", value: std::f64::consts::E, doc: "Euler's number ≈ 2.71828" },
    ConstInfo { name: "phi", value: 1.618_033_988_749_895, doc: "Golden ratio ≈ 1.61803" },
    ConstInfo { name: "true", value: 1.0, doc: "1" },
    ConstInfo { name: "false", value: 0.0, doc: "0" },
];

pub fn find_function(name: &str) -> Option<&'static FuncInfo> {
    FUNCTIONS.iter().find(|f| f.name == name)
}

pub fn find_constant(name: &str) -> Option<f64> {
    CONSTANTS.iter().find(|c| c.name == name).map(|c| c.value)
}

/// Is `name` reserved by the language (a function or a constant)?
pub fn is_builtin(name: &str) -> bool {
    find_function(name).is_some() || find_constant(name).is_some()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Suggest the closest known function name for a typo like `Sin` or `sqr`.
pub fn suggest_function(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if let Some(f) = find_function(&lower) {
        return Some(f.name);
    }
    FUNCTIONS
        .iter()
        .map(|f| (levenshtein(&lower, f.name), f.name))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| n)
}

/// PCG hash, shared bit-for-bit by the CPU evaluator and the generated WGSL.
pub fn pcg_hash(v: u32) -> u32 {
    let state = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// Evaluate a function on already evaluated arguments.
pub fn call(func: Func, a: &[f64]) -> f64 {
    let x = a.first().copied().unwrap_or(f64::NAN);
    let y = a.get(1).copied().unwrap_or(f64::NAN);
    let z = a.get(2).copied().unwrap_or(f64::NAN);
    match func {
        Func::Sin => x.sin(),
        Func::Cos => x.cos(),
        Func::Tan => x.tan(),
        Func::Asin => x.asin(),
        Func::Acos => x.acos(),
        Func::Atan => x.atan(),
        Func::Atan2 => x.atan2(y),
        Func::Sinh => x.sinh(),
        Func::Cosh => x.cosh(),
        Func::Tanh => x.tanh(),
        Func::Sqrt => x.sqrt(),
        Func::Cbrt => x.cbrt(),
        Func::Exp => x.exp(),
        Func::Exp2 => x.exp2(),
        Func::Ln => x.ln(),
        Func::Log => {
            if a.len() >= 2 {
                x.ln() / y.ln()
            } else {
                x.log10()
            }
        }
        Func::Log2 => x.log2(),
        Func::Log10 => x.log10(),
        Func::Pow => x.powf(y),
        Func::Abs => x.abs(),
        Func::Sign => {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                x // keeps 0 and NaN
            }
        }
        Func::Floor => x.floor(),
        Func::Ceil => x.ceil(),
        Func::Round => x.round(),
        Func::Trunc => x.trunc(),
        Func::Fract => x - x.floor(),
        Func::Min => a.iter().copied().fold(f64::INFINITY, f64::min),
        Func::Max => a.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        Func::Clamp => x.max(y).min(z),
        Func::Saturate => x.clamp(0.0, 1.0),
        Func::Lerp => x + (y - x) * z,
        Func::Step => {
            if y < x {
                0.0
            } else {
                1.0
            }
        }
        Func::Smoothstep => {
            if x == y {
                if z < x { 0.0 } else { 1.0 }
            } else {
                let t = ((z - x) / (y - x)).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            }
        }
        Func::Mod => x - y * (x / y).floor(),
        Func::Hypot => x.hypot(y),
        Func::Degrees => x.to_degrees(),
        Func::Radians => x.to_radians(),
        Func::If => {
            if x != 0.0 {
                y
            } else {
                z
            }
        }
        Func::Hash => pcg_hash((x as f32).to_bits()) as f64 / 4_294_967_296.0,
        // A value is the same wherever it is read: only an image of Pixel Math differs.
        Func::Sample => x,
    }
}
