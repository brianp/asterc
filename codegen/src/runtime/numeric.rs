use super::alloc::aster_class_alloc;

// ---------------------------------------------------------------------------
// Numeric operations — pow, random, int/float arithmetic, range
// ---------------------------------------------------------------------------

/// Integer exponentiation: base ** exp (exp >= 0).
pub extern "C" fn aster_pow_int(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        return 0; // integer pow with negative exp → 0 (floor)
    }
    let mut result: i64 = 1;
    let mut b = base;
    let mut e = exp as u64;
    while e > 0 {
        if e & 1 == 1 {
            result = result.wrapping_mul(b);
        }
        b = b.wrapping_mul(b);
        e >>= 1;
    }
    result
}

/// Checked integer addition. Aborts on overflow.
/// This is an interim measure until BigInt promotion is implemented (see bigint-rfc.md).
pub extern "C" fn aster_int_add(a: i64, b: i64) -> i64 {
    match a.checked_add(b) {
        Some(result) => result,
        None => {
            eprintln!("integer overflow: {} + {} exceeds 64-bit range", a, b);
            std::process::abort();
        }
    }
}

/// Checked integer subtraction. Aborts on overflow.
/// This is an interim measure until BigInt promotion is implemented (see bigint-rfc.md).
pub extern "C" fn aster_int_sub(a: i64, b: i64) -> i64 {
    match a.checked_sub(b) {
        Some(result) => result,
        None => {
            eprintln!("integer overflow: {} - {} exceeds 64-bit range", a, b);
            std::process::abort();
        }
    }
}

/// Checked integer multiplication. Aborts on overflow.
/// This is an interim measure until BigInt promotion is implemented (see bigint-rfc.md).
pub extern "C" fn aster_int_mul(a: i64, b: i64) -> i64 {
    match a.checked_mul(b) {
        Some(result) => result,
        None => {
            eprintln!("integer overflow: {} * {} exceeds 64-bit range", a, b);
            std::process::abort();
        }
    }
}

// ─── Int numeric methods ──────────────────────────────────────────────

pub extern "C" fn aster_int_is_even(n: i64) -> i8 {
    (n % 2 == 0) as i8
}

pub extern "C" fn aster_int_is_odd(n: i64) -> i8 {
    (n % 2 != 0) as i8
}

pub extern "C" fn aster_int_abs(n: i64) -> i64 {
    match n.checked_abs() {
        Some(result) => result,
        None => {
            eprintln!("integer overflow: abs({}) exceeds 64-bit range", n);
            std::process::abort();
        }
    }
}

pub extern "C" fn aster_int_clamp(n: i64, min: i64, max: i64) -> i64 {
    if min > max {
        eprintln!("invalid arguments: clamp min ({}) > max ({})", min, max);
        std::process::abort();
    }
    if n < min {
        min
    } else if n > max {
        max
    } else {
        n
    }
}

pub extern "C" fn aster_int_min(a: i64, b: i64) -> i64 {
    if a <= b { a } else { b }
}

pub extern "C" fn aster_int_max(a: i64, b: i64) -> i64 {
    if a >= b { a } else { b }
}

// ─── Float numeric methods ────────────────────────────────────────────

fn check_float_valid(val: f64, op: &str) {
    if val.is_nan() {
        eprintln!("invalid float: {}(NaN)", op);
        std::process::abort();
    }
    if val.is_infinite() {
        eprintln!("invalid float: {}({})", op, val);
        std::process::abort();
    }
}

fn check_float_fits_i64(val: f64, op: &str) {
    check_float_valid(val, op);
    if val >= i64::MAX as f64 || val < i64::MIN as f64 {
        eprintln!(
            "float out of Int range: {}({}) exceeds 64-bit integer range",
            op, val
        );
        std::process::abort();
    }
}

pub extern "C" fn aster_float_abs(n: f64) -> f64 {
    n.abs()
}

pub extern "C" fn aster_float_round(n: f64) -> i64 {
    check_float_fits_i64(n, "round");
    n.round() as i64
}

pub extern "C" fn aster_float_floor(n: f64) -> i64 {
    check_float_fits_i64(n, "floor");
    n.floor() as i64
}

pub extern "C" fn aster_float_ceil(n: f64) -> i64 {
    check_float_fits_i64(n, "ceil");
    n.ceil() as i64
}

pub extern "C" fn aster_float_clamp(n: f64, min: f64, max: f64) -> f64 {
    if min.is_nan() || max.is_nan() || n.is_nan() {
        eprintln!("invalid float: clamp with NaN argument");
        std::process::abort();
    }
    if min > max {
        eprintln!("invalid arguments: clamp min ({}) > max ({})", min, max);
        std::process::abort();
    }
    if n < min {
        min
    } else if n > max {
        max
    } else {
        n
    }
}

pub extern "C" fn aster_float_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        eprintln!("invalid float: min with NaN argument");
        std::process::abort();
    }
    if a <= b { a } else { b }
}

pub extern "C" fn aster_float_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        eprintln!("invalid float: max with NaN argument");
        std::process::abort();
    }
    if a >= b { a } else { b }
}

/// Float exponentiation: base ** exp.
pub extern "C" fn aster_pow_float(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}

// ---------------------------------------------------------------------------
// Range
// ---------------------------------------------------------------------------

/// Create a Range struct: [start: i64, end: i64, inclusive: i8]
pub extern "C" fn aster_range_new(start: i64, end: i64, inclusive: i8) -> *mut u8 {
    let ptr = aster_class_alloc(24); // 8 + 8 + 8 (padded)
    unsafe {
        *(ptr as *mut i64) = start;
        *((ptr as *mut i64).add(1)) = end;
        *((ptr as *mut i64).add(2)) = inclusive as i64;
    }
    ptr
}

/// Check if a loop variable is still within range bounds.
pub extern "C" fn aster_range_check(val: i64, end: i64, inclusive: i8) -> i8 {
    if inclusive != 0 {
        (val <= end) as i8
    } else {
        (val < end) as i8
    }
}

// ---------------------------------------------------------------------------
// Random
// ---------------------------------------------------------------------------

/// Random integer in [0, max).
/// Uses rejection sampling to avoid modulo bias.
pub extern "C" fn aster_random_int(max: i64) -> i64 {
    if max <= 0 {
        return 0;
    }
    let umax = max as u64;
    // Rejection threshold: largest multiple of umax that fits in u64.
    // Values at or above this threshold would introduce modulo bias.
    let threshold = u64::MAX - u64::MAX % umax;
    loop {
        let mut buf = [0u8; 8];
        if getrandom::getrandom(&mut buf).is_err() {
            eprintln!("aster_random_int: getrandom failed");
            std::process::abort();
        }
        let val = u64::from_le_bytes(buf);
        if val < threshold {
            return (val % umax) as i64;
        }
    }
}

/// Random float in [0.0, max).
pub extern "C" fn aster_random_float(max: f64) -> f64 {
    let mut buf = [0u8; 8];
    getrandom::getrandom(&mut buf).unwrap_or_else(|_| {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        buf = (t as u64).to_le_bytes();
    });
    let val = u64::from_le_bytes(buf);
    // Convert to [0.0, 1.0) then scale
    let unit = (val >> 11) as f64 / (1u64 << 53) as f64;
    unit * max
}

/// Random boolean.
pub extern "C" fn aster_random_bool() -> i8 {
    let mut buf = [0u8; 1];
    getrandom::getrandom(&mut buf).unwrap_or_else(|_| {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        buf = [(t & 1) as u8];
    });
    (buf[0] & 1) as i8
}
