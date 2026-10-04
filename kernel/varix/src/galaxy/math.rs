//! no_std 浮点辅助（内核无 libm）：round/sqrt/sin/cos/ln/exp/pow 自带实现。
//!
//! 精度：泰勒/牛顿迭代，双精度下误差 ~1e-9 量级，满足定点渲染与
//! 自检断言（对比度、gamma、ITD 等）需求。输入量级有限，循环归约即可。

pub fn round32(x: f32) -> f32 {
    let t = x as i64 as f32;
    let frac = x - t;
    if x >= 0.0 && frac >= 0.5 {
        t + 1.0
    } else if x < 0.0 && frac <= -0.5 {
        t - 1.0
    } else {
        t
    }
}

pub fn round64(x: f64) -> f64 {
    let t = x as i64 as f64;
    let frac = x - t;
    if x >= 0.0 && frac >= 0.5 {
        t + 1.0
    } else if x < 0.0 && frac <= -0.5 {
        t - 1.0
    } else {
        t
    }
}

pub fn sqrt32(x: f32) -> f32 {
    sqrt64(x as f64) as f32
}

pub fn sqrt64(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut r = if x < 1.0 { 1.0 } else { x };
    let mut i = 0;
    while i < 40 {
        r = 0.5 * (r + x / r);
        i += 1;
    }
    r
}

const PI: f64 = core::f64::consts::PI;
const TWO_PI: f64 = 2.0 * PI;

/// sin，输入先归一到 [-π, π]（调用方量级有限）。
pub fn sin64(x: f64) -> f64 {
    let mut x = x;
    while x > PI {
        x -= TWO_PI;
    }
    while x < -PI {
        x += TWO_PI;
    }
    let x2 = x * x;
    // 泰勒 Horner 至 x^23 项（x∈[-π,π] 截断误差 <1e-12）：
    // x·(1 - x²/6·(1 - x²/20·(1 - … - x²/506)))
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0 * (1.0 - x2 / 156.0 * (1.0 - x2 / 210.0 * (1.0 - x2 / 272.0 * (1.0 - x2 / 342.0 * (1.0 - x2 / 420.0 * (1.0 - x2 / 506.0)))))))))))
}

pub fn cos64(x: f64) -> f64 {
    sin64(x + PI * 0.5)
}

pub fn sin32(x: f32) -> f32 {
    sin64(x as f64) as f32
}

pub fn cos32(x: f32) -> f32 {
    cos64(x as f64) as f32
}

pub fn ln64(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut m = x;
    let mut e: i64 = 0;
    while m >= 2.0 {
        m *= 0.5;
        e += 1;
    }
    while m < 1.0 {
        m *= 2.0;
        e -= 1;
    }
    // ln(m) = 2·atanh((m-1)/(m+1))，m∈[1,2) → |t|<1/3
    let t = (m - 1.0) / (m + 1.0);
    let t2 = t * t;
    let mut sum = 0.0f64;
    let mut tk = t;
    let mut k = 1u32;
    while k <= 25 {
        sum += tk / k as f64;
        tk *= t2;
        k += 2;
    }
    e as f64 * core::f64::consts::LN_2 + 2.0 * sum
}

pub fn exp64(x: f64) -> f64 {
    let k = round64(x / core::f64::consts::LN_2) as i64;
    let r = x - k as f64 * core::f64::consts::LN_2;
    let mut sum = 1.0f64;
    let mut term = 1.0f64;
    let mut i = 1;
    while i <= 18 {
        term *= r / i as f64;
        sum += term;
        i += 1;
    }
    let mut p = 1.0f64;
    let mut j = 0;
    let ak = k.abs();
    while j < ak {
        p *= 2.0;
        j += 1;
    }
    if k >= 0 {
        sum * p
    } else {
        sum / p
    }
}

pub fn pow64(x: f64, p: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    exp64(p * ln64(x))
}

pub fn pow32(x: f32, p: f32) -> f32 {
    pow64(x as f64, p as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_helpers_accuracy() {
        assert!((sqrt64(2.0) - 1.414_213_562_373_095_1).abs() < 1e-9);
        assert!((sin64(PI * 0.5) - 1.0).abs() < 1e-9);
        assert!((cos64(0.0) - 1.0).abs() < 1e-9);
        assert!((ln64(core::f64::consts::E) - 1.0).abs() < 1e-9);
        assert!((exp64(1.0) - core::f64::consts::E).abs() < 1e-9);
        assert!((pow64(2.0, 10.0) - 1024.0).abs() < 1e-6);
        assert_eq!(round64(2.5), 3.0);
        assert_eq!(round64(-2.5), -3.0);
        assert_eq!(round32(0.454_5 * 255.0) as u64, 116);
    }
}
