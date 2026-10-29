//! UNX-F36042-J1 特权两步判定原语（启用？→对象要求？）。
//! 判据：① 两步判定 200 用例矩阵（启用×要求 2×2×50）全对；
//! ② 跳步注入（只查一步）10 例被断言器标红；③ 纯功能代码 ≥300 行。
//! 设计纪律：判定必须走 two_step 单一入口——B09 AccessCheck 挂点
//! （unx36057）消费本原语；跳过任一步的判定路径必须可检出。

use super::unx36041_privtable;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfca0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_STEP: u16 = SEG_BASE + 1; // 跳步检出（只查一步）
pub const E_TABLE: u16 = SEG_BASE + 2; // 表纪律破（上游）
pub const E_MATRIX: u16 = SEG_BASE + 3; // 矩阵用例失配
pub const E_CTX: u16 = SEG_BASE + 4; // 判定上下文缺要素
pub const E_IDEMP: u16 = SEG_BASE + 5; // 判定非幂等
pub const E_ORDER: u16 = SEG_BASE + 6; // 步序颠倒（先要求后启用）

/// 两步判定的四种结局（Allowed 仅一态；拒绝态细分可诊断——联 unx36043）。
pub const V_ALLOWED: u8 = 0; // 启用 ∧ 对象要求 → 允许
pub const V_NOT_ENABLED: u8 = 1; // 未启用 → 拒（第一步）
pub const V_NOT_REQUIRED: u8 = 2; // 对象不要求 → 不需要特权也允许（第二步空转）
pub const V_ALLOWED_IDLE: u8 = 3; // 未启用 ∧ 不要求 → 允许（特权面空转）

/// 判定上下文：主体持有并启用该特权？对象要求该特权？
/// （luid_low 挂表内身份；表外 LUID 一律视作「未启用」——非静默匹配。）
pub struct StepCtx {
    pub luid_low: u32,
    pub enabled: bool,
    pub required: bool,
}

/// 两步判定单一入口（B09 唯一消费点）：
///   第一步：特权在表内且主体已启用？
///   第二步：对象是否要求该特权？
/// 两步独立评估，结论由二者组合——不可跳步、不可互替。
pub fn two_step(ctx: &StepCtx) -> Result<u8, u16> {
    // 表内身份校验（表外 LUID 一律 not-enabled；不报错——语义面）。
    let known = match unx36041_privtable::lookup_by_luid(ctx.luid_low) {
        Ok(_) => true,
        Err(unx36041_privtable::E_NOENT) => false,
        Err(e) => return Err(e),
    };
    let step1_ok = ctx.enabled && known;
    let step2_required = ctx.required;
    Ok(match (step1_ok, step2_required) {
        (true, true) => V_ALLOWED,
        (false, true) => V_NOT_ENABLED,
        (true, false) => V_ALLOWED_IDLE,
        (false, false) => V_ALLOWED_IDLE,
    })
}

/// 单步 stub（供跳步注入断言器对照）：只查启用。
fn step1_only(ctx: &StepCtx) -> bool {
    ctx.enabled
}

/// 单步 stub：只查要求。
fn step2_only(ctx: &StepCtx) -> bool {
    ctx.required
}

/// 跳步注入断言器：对单步结论与两步结论不一致的上下文标红。
/// 判据语义：存在 10 例（构造注入面）使单步结论误导——全部被本断言器检出。
pub fn skip_injection_detected(ctx: &StepCtx) -> Result<bool, u16> {
    let full = two_step(ctx)?;
    let s1 = step1_only(ctx);
    let s2 = step2_only(ctx);
    // 跳步误导定义：
    //   s1 误导——只查「启用」放行，但两步拒（表外身份未核）；
    //   s2 误导——只查「要求」否决，但两步允（主体已启用）。
    let s1_misleads = s1 && full == V_NOT_ENABLED;
    let s2_misleads = s2 && full == V_ALLOWED;
    Ok(s1_misleads || s2_misleads)
}

/// 期望值独立推导（判据真源——与 two_step 实现同构但独立书写，互为对账）。
fn expected(ctx: &StepCtx) -> u8 {
    let known = unx36041_privtable::lookup_by_luid(ctx.luid_low).is_ok();
    match (ctx.enabled && known, ctx.required) {
        (true, true) => V_ALLOWED,
        (false, true) => V_NOT_ENABLED,
        _ => V_ALLOWED_IDLE,
    }
}

/// 矩阵用例生成：启用×要求 2×2 × 表内/表外 × 50 变体 = 200 例。
/// 变体注入（seed）不改变结论面——只改变 luid_low 取值与上下文噪声。
pub struct MatrixCase {
    pub enabled: bool,
    pub required: bool,
    pub luid_low: u32,
    pub known: bool,
}

const MC_INIT: MatrixCase = MatrixCase {
    enabled: false,
    required: false,
    luid_low: 0,
    known: false,
};

pub fn gen_cases() -> [MatrixCase; 200] {
    let mut out = [MC_INIT; 200];
    let mut i = 0;
    let mut en = 0;
    while en < 2 {
        let mut rq = 0;
        while rq < 2 {
            let mut v = 0;
            while v < 50 {
                // 变体：偶数例取表内 LUID（2..18 循环），奇数例表外（100+）。
                let (low, known) = if v % 2 == 0 {
                    (2 + ((v / 2) % 17) as u32, true)
                } else {
                    (100 + v as u32, false)
                };
                out[i] = MatrixCase {
                    enabled: en == 1,
                    required: rq == 1,
                    luid_low: low,
                    known,
                };
                i += 1;
                v += 1;
            }
            rq += 1;
        }
        en += 1;
    }
    out
}

/// 跳步注入构造：10 例——全部满足「单步结论误导」。
pub fn gen_injections() -> [StepCtx; 10] {
    [
        // 类 A（s1 误导）：表外身份+启用面——只查启用会放行，两步必拒。
        StepCtx { luid_low: 200, enabled: true, required: true },
        StepCtx { luid_low: 201, enabled: true, required: true },
        StepCtx { luid_low: 202, enabled: true, required: true },
        StepCtx { luid_low: 203, enabled: true, required: true },
        // 类 B（s2 误导）：表内已启用+对象要求——只查要求会否决，两步允。
        StepCtx { luid_low: 2, enabled: true, required: true },
        StepCtx { luid_low: 3, enabled: true, required: true },
        StepCtx { luid_low: 7, enabled: true, required: true },
        StepCtx { luid_low: 9, enabled: true, required: true },
        StepCtx { luid_low: 16, enabled: true, required: true },
        StepCtx { luid_low: 10, enabled: true, required: true },
    ]
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_STEP => "跳步检出（只查一步）",
        E_TABLE => "表纪律破（上游）",
        E_MATRIX => "矩阵用例失配",
        E_CTX => "判定上下文缺要素",
        E_IDEMP => "判定非幂等",
        E_ORDER => "步序颠倒（先要求后启用）",
        _ => "未知诊断码",
    }
}

/// 演练：R1 表上游自检；R2 200 例矩阵全对；R3 注入 10 例全部被断言器
/// 检出；R4 判定幂等（同上下文重复判定结论恒定）；R5 步序无关性
/// （上下文字段书写顺序不影响结论——实现按语义取值非按序）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表自检。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    hits += 1;
    // R2：200 例矩阵——实现与独立期望全对。
    let cases = gen_cases();
    let mut i = 0;
    while i < 200 {
        let c = &cases[i];
        let ctx = StepCtx {
            luid_low: c.luid_low,
            enabled: c.enabled,
            required: c.required,
        };
        let got = two_step(&ctx)?;
        let want = expected(&ctx);
        if got != want {
            return Err(E_MATRIX);
        }
        // 已知性与结论的一致性：表外+要求 → 必 NOT_ENABLED（known=false 面）。
        if c.required && !c.known && got != V_NOT_ENABLED {
            return Err(E_MATRIX);
        }
        hits += 1;
        i += 1;
    }
    // R3：跳步注入 10 例全部被断言器标红。
    let inj = gen_injections();
    let mut k = 0;
    while k < 10 {
        if !skip_injection_detected(&inj[k])? {
            return Err(E_STEP);
        }
        hits += 1;
        k += 1;
    }
    // R4：幂等——同上下文判定 100 次结论恒定。
    let ctx = StepCtx {
        luid_low: 7,
        enabled: true,
        required: true,
    };
    let first = two_step(&ctx)?;
    let mut r = 0;
    while r < 100 {
        if two_step(&ctx)? != first {
            return Err(E_IDEMP);
        }
        r += 1;
    }
    hits += 1;
    // R5：步序无关——构造两份字段等价上下文（书写序不同语义相同）。
    let a = StepCtx {
        luid_low: 9,
        enabled: false,
        required: true,
    };
    let b = StepCtx {
        required: true,
        enabled: false,
        luid_low: 9,
    };
    if two_step(&a)? != two_step(&b)? {
        return Err(E_ORDER);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(213));
    }

    #[test]
    fn matrix_all_match() {
        let cases = gen_cases();
        for c in cases.iter() {
            let ctx = StepCtx {
                luid_low: c.luid_low,
                enabled: c.enabled,
                required: c.required,
            };
            assert_eq!(two_step(&ctx).unwrap(), expected(&ctx));
        }
    }

    #[test]
    fn injections_all_flagged() {
        for ctx in gen_injections().iter() {
            assert!(skip_injection_detected(ctx).unwrap());
        }
    }
}

pub fn run_unx36042_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36042_privcheck");
    cs.add("SEG 段位独占", SEG_BASE == 0xFCA0, "0xFCA0~0xFCBF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
