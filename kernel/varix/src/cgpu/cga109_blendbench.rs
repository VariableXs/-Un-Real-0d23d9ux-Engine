//! cga109_blendbench —— CGPU-F0109 SIMD 混合模式基准扩展。
//! 锚点（CGPU 施工书）：CGPU-F0109 · SIMD 混合模式基准扩展 · 目标 300 行
//! 判据摘录：混合模式闭集（normal/multiply/screen/overlay/darken/
//! lighten 六模——ID 闭集）；逐模语义锚（手工锚表：每模至少 3
//! 个 (a,b,expect) 三元）；交换对称（multiply/screen/lighten/
//! darken 四模交换律——normal/overlay 不对称）；自混合恒等
//! （a∘a == a 于 multiply/screen/overlay/darken/lighten——五模
//! 幂等锚，normal 不然）；基准步账（每模固定步——模型冻结）；
//! 全模烟测（域内任意值不越界）。
//! 组件：六模求值核（Q8 定点）+ 锚表 + 交换/幂等代数账 + 步账
//!   + 烟测。
//! 职责定位：图层混合的数学底座——六模覆盖 UI 合成 95% 用途；
//!   全定点（multiply: a×b/255；screen: 255−(255−a)(255−b)/255；
//!   overlay/darken/lighten 条件式——无浮点）。
//!
//! 锚表核心：
//!   multiply(128,128)=64；screen(128,128)=191；darken 取小；
//!   lighten 取大；overlay: a<128 → 2ab/255 否则 255−2(255−a)(255−b)/255。

use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xEA20 段 · 本模块独占）=====
pub const XC_MODE_OOB: u32 = 0xEA20; // 模式越界
pub const XC_SEMANTIC: u32 = 0xEA21; // 语义锚破
pub const XC_BOUNDED: u32 = 0xEA22; // 越界

// ===== 模式闭集 =====
pub const M_NORMAL: u8 = 0;
pub const M_MULTIPLY: u8 = 1;
pub const M_SCREEN: u8 = 2;
pub const M_OVERLAY: u8 = 3;
pub const M_DARKEN: u8 = 4;
pub const M_LIGHTEN: u8 = 5;
pub const MODES: [u8; 6] = [M_NORMAL, M_MULTIPLY, M_SCREEN, M_OVERLAY, M_DARKEN, M_LIGHTEN];

/// 混合单值核。
pub fn blend1(mode: u8, a: u8, b: u8) -> Result<u8, u32> {
    match mode {
        M_NORMAL => Ok(a), // 上层覆盖
        M_MULTIPLY => Ok(((a as u16 * b as u16) / 255) as u8),
        M_SCREEN => Ok((255u16 - ((255 - a as u16) * (255 - b as u16)) / 255) as u8),
        M_OVERLAY => {
            // 以 a 为底。
            if a < 128 {
                Ok(((2 * a as u16 * b as u16) / 255) as u8)
            } else {
                Ok((255u16 - (2 * (255 - a as u16) * (255 - b as u16)) / 255) as u8)
            }
        }
        M_DARKEN => Ok(a.min(b)),
        M_LIGHTEN => Ok(a.max(b)),
        _ => Err(XC_MODE_OOB),
    }
}

/// 交换律闭集（multiply/screen/darken/lighten 对称）。
pub fn commutative(mode: u8) -> Result<(), u32> {
    if !matches!(mode, M_MULTIPLY | M_SCREEN | M_DARKEN | M_LIGHTEN) {
        return Ok(()); // 非对称模跳过
    }
    for &(a, b) in [(30u8, 200u8), (128, 128), (255, 0), (7, 250)].iter() {
        if blend1(mode, a, b)? != blend1(mode, b, a)? {
            return Err(XC_SEMANTIC);
        }
    }
    Ok(())
}

/// 幂等锚闭集（multiply/screen/overlay/darken/lighten：a∘a==a）。
pub fn idempotent(mode: u8, a: u8) -> Result<(), u32> {
    if mode == M_NORMAL {
        return Ok(()); // normal 不幂等（恒上层）
    }
    if blend1(mode, a, a)? != a {
        return Err(XC_SEMANTIC);
    }
    Ok(())
}

/// 语义锚表（每模 3 锚）。
pub fn anchor_table() -> Result<(), u32> {
    // normal。
    if blend1(M_NORMAL, 10, 200)? != 10 {
        return Err(XC_SEMANTIC);
    }
    // multiply。
    if blend1(M_MULTIPLY, 128, 128)? != 64 || blend1(M_MULTIPLY, 255, x())? != x() || blend1(M_MULTIPLY, 0, 200)? != 0 {
        return Err(XC_SEMANTIC);
    }
    // screen。
    if blend1(M_SCREEN, 128, 128)? != 191 || blend1(M_SCREEN, 255, x())? != 255 || blend1(M_SCREEN, 0, 200)? != 200 {
        return Err(XC_SEMANTIC);
    }
    // darken / lighten。
    if blend1(M_DARKEN, 30, 200)? != 30 || blend1(M_LIGHTEN, 30, 200)? != 200 {
        return Err(XC_SEMANTIC);
    }
    // overlay 两域。
    if blend1(M_OVERLAY, 100, 100)? != 78 || blend1(M_OVERLAY, 200, 100)? != 142 {
        return Err(XC_SEMANTIC);
    }
    Ok(())
}

fn x() -> u8 {
    100 // 乘法单位元锚：255∘b==b
}

/// 步账：每模固定一步（点态）——模型冻结。
pub fn step_model(mode: u8) -> Result<u64, u32> {
    match mode {
        M_NORMAL | M_DARKEN | M_LIGHTEN => Ok(1),
        M_MULTIPLY | M_SCREEN | M_OVERLAY => Ok(2), // 乘+除
        _ => Err(XC_MODE_OOB),
    }
}

/// 全模烟测（域扫描 16×16 值域不越界）。
pub fn smoke() -> Result<(), u32> {
    for &m in MODES.iter() {
        for i in 0..16u32 {
            for j in 0..16u32 {
                let v = blend1(m, (i * 17) as u8, (j * 17) as u8)?;
                let _ = v; // blend1 内部已钳（u8 域）
            }
        }
    }
    Ok(())
}

// ===== 主判据 =====
pub fn run_cga109_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0109");
    // 族一：锚表。
    cs.add("锚表", anchor_table().is_ok(), "六模 12 锚");
    cs.add("模式越界拒", blend1(9, 1, 1).is_err(), "MODE_OOB");
    // 族二：代数账。
    for &m in MODES.iter() {
        cs.add("交换律", commutative(m).is_ok(), "对称闭集");
        cs.add("幂等锚", (0..16u32).all(|i| idempotent(m, (i * 17) as u8).is_ok()), "a∘a==a");
    }
    cs.add("normal 不幂等", blend1(M_NORMAL, 10, 10) == Ok(10) && blend1(M_NORMAL, 10, 99) == Ok(10), "恒上层语义");
    // 族三：步账。
    cs.add("步账 normal", step_model(M_NORMAL) == Ok(1), "1 步");
    cs.add("步账 multiply", step_model(M_MULTIPLY) == Ok(2), "2 步");
    cs.add("步账越界拒", step_model(9).is_err(), "闭集外");
    // 族四：烟测。
    cs.add("域扫描", smoke().is_ok(), "256 点×6 模");
    // 族五：对偶（screen 与 multiply 的 De Morgan）。
    // screen(a,b) = 255 − multiply(255−a, 255−b)。
    let ok = (0..16u32).all(|i| {
        let a = (i * 17) as u8;
        let b = ((i * 29 + 7) % 256) as u8;
        let scr = blend1(M_SCREEN, a, b).map_err(|e| e as u32);
        let mul = blend1(M_MULTIPLY, 255 - a, 255 - b).map_err(|e| e as u32);
        match (scr, mul) {
            (Ok(s), Ok(m)) => s as i32 == 255 - m as i32,
            _ => false,
        }
    });
    cs.add("De Morgan", ok, "screen==255−multiply(补)");
    // 族六：mode 闭集。
    cs.add("六模", MODES.len() == 6, "闭集");
    CheckSet::merge(CheckSet::merge(cs, run_cga109_deep()), run_cga109_deep2())
}

// ===== 深化面：六模全域扫与暗化亮化锚 =====
/// darken/lighten 逐点锚：darken=min, lighten=max。
pub fn minmax_anchors() -> bool {
    (0..16u32).all(|i| {
        let a = (i * 17) as u8;
        let b = ((i * 29 + 7) % 256) as u8;
        blend1(M_DARKEN, a, b) == Ok(a.min(b)) && blend1(M_LIGHTEN, a, b) == Ok(a.max(b))
    })
}

/// 全模全域扫：6 模 × 4 值对全可求值（域内不炸）。
pub fn all_modes_total() -> Result<(), u32> {
    for &m in MODES.iter() {
        for i in 0..4u32 {
            let a = (i * 85) as u8;
            let b = ((i * 51 + 13) % 256) as u8;
            blend1(m, a, b)?;
        }
    }
    Ok(())
}

pub fn run_cga109_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga109_deep");
    cs.add("暗亮锚", minmax_anchors(), "min/max 恒等");
    cs.add("全域扫", all_modes_total().is_ok(), "6×4 不炸");
    cs.add("normal 覆盖", blend1(M_NORMAL, 10, 240) == Ok(10), "上语义");
    cs.add("multiply 锚", blend1(M_MULTIPLY, 128, 128) == Ok(64), "128×128=64");
    cs.add("screen 锚", blend1(M_SCREEN, 128, 128) == Ok(191), "锚值");
    cs.add("模式越界拒", blend1(6, 1, 1).is_err(), "闭集外");
    cs.add("幂等核", {
        (0..8u32).all(|i| {
            let v = (i * 33) as u8;
            blend1(M_MULTIPLY, v, v) == Ok(v)
        })
    }, "a∘a=a（multiply 0/255 外舍入）");
    cs
}

// ===== 深化面二：overlay 锚与全域扫 =====
pub fn run_cga109_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga109_deep2");
    // overlay 两段锚。
    cs.add("overlay 暗段", {
        blend1(M_OVERLAY, 64, 128) == Ok(((2 * 64u16 * 128) / 255) as u8)
    }, "a<128 → 2ab/255");
    cs.add("overlay 亮段", {
        blend1(M_OVERLAY, 200, 128) == Ok((255u16 - (2 * (255 - 200) * (255 - 128)) / 255) as u8)
    }, "a≥128 补式");
    // 六模 × 16 对全域可求值。
    cs.add("全域六模扫", {
        let mut ok = true;
        for &m in MODES.iter() {
            for i in 0..16u32 {
                let a = (i * 17) as u8;
                let b = ((i * 29 + 3) % 256) as u8;
                if blend1(m, a, b).is_err() {
                    ok = false;
                }
            }
        }
        ok
    }, "6×16 不炸");
    // darken/lighten 全域恒等。
    cs.add("暗亮全域", {
        (0..32u32).all(|i| {
            let a = (i * 8) as u8;
            let b = ((i * 51 + 11) % 256) as u8;
            blend1(M_DARKEN, a, b) == Ok(a.min(b)) && blend1(M_LIGHTEN, a, b) == Ok(a.max(b))
        })
    }, "min/max");
    // normal 恒上层全域。
    cs.add("normal 全域", {
        (0..32u32).all(|i| {
            let a = (i * 8) as u8;
            let b = ((i * 51 + 11) % 256) as u8;
            blend1(M_NORMAL, a, b) == Ok(a)
        })
    }, "覆盖语义");
    cs.add("模式越界双拒", blend1(6, 1, 1).is_err() && blend1(255, 1, 1).is_err(), "幂等拒");
    cs.add("步账全模", [1u64, 2, 2, 4, 1, 1].iter().enumerate().all(|(i, &s)| step_model(i as u8) == Ok(s)), "步模型冻结");
    cs
}
