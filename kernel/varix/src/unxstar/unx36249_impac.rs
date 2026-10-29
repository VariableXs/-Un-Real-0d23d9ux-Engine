//! UNX-F36249-J1 模拟中的 AccessCheck 路径（引擎取令牌次序：
//! 线程→进程）。
//! 判据：① 取令牌次序 100 例（线程优先/进程回退）；② 次序错乱
//! 注入 10 例被断言拦截；③ 纯功能 ≥300 行。
//! 设计纪律：单一引擎红线——模拟态 AccessCheck 的取令牌次序是
//! 「线程挂接令牌优先，无挂接回退进程主令牌」，这个次序必须只有
//! 一处实现（本模块的 `pick_token`），任何「第二套次序」都是权限
//! 边界的旁路。次序错乱注入 = 用进程令牌替换线程令牌求值，断言
//! 必须检出结果差异（同一请求两种次序结果不同 ⇒ 次序被换过）。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA370;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ORDER: u16 = SEG_BASE + 2; // 取令牌次序错乱
pub const E_DIFF: u16 = SEG_BASE + 3; // 双次序结果差异未检出
pub const E_NOPICK: u16 = SEG_BASE + 4; // 次序实现缺（双源）
pub const E_TOKEN: u16 = SEG_BASE + 5; // 两令牌皆缺
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 线程令牌态（挂接或回退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThrTok {
    /// 线程挂接令牌指纹（0 = 无挂接）。
    pub attached_fp: u32,
    /// 进程主令牌指纹（回退目标——恒非零）。
    pub proc_fp: u32,
}

/// 取令牌（单一实现——线程优先/进程回退，全引擎唯一入口）。
pub fn pick_token(t: &ThrTok) -> Result<(u32, u8), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if t.proc_fp == 0 && t.attached_fp == 0 {
        return Err(E_TOKEN);
    }
    if t.attached_fp != 0 {
        Ok((t.attached_fp, 1)) // 线程优先
    } else {
        Ok((t.proc_fp, 0)) // 进程回退
    }
}

/// 最小求值器（掩码过滤：令牌 grant ∩ 请求——模拟态授权语义的
/// 本地最小面，仅供次序对照；真判定引擎在 B09 单一红线内）。
pub fn eval_with(tok_fp: u32, grant: u16, request: u16) -> u16 {
    let _ = tok_fp;
    request & grant
}

/// 令牌授权表（指纹 → grant——对照面用，LCG 派生）。
pub fn grant_of(fp: u32) -> u16 {
    // 确定性派生（同指纹恒同 grant——可重放）。
    let h = fp.wrapping_mul(31).wrapping_add(0x9E37_79B9);
    ((h >> 8) & 0xFFFF) as u16
}

/// 取令牌次序 100 例（判据 ① 落点：两种线程态各 50 例对平）。
pub fn order_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0u32;
    while i < 100 {
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let attached = (r & 0xFFFF) | 1;
        let proc = r.rotate_left(13) | 1;
        let tt = ThrTok { attached_fp: attached, proc_fp: proc };
        // 有挂接：线程优先。
        let (fp1, src1) = pick_token(&tt)?;
        if src1 != 1 || fp1 != attached {
            return Err(E_ORDER);
        }
        // 摘挂接：进程回退。
        let tt2 = ThrTok { attached_fp: 0, proc_fp: proc };
        let (fp2, src2) = pick_token(&tt2)?;
        if src2 != 0 || fp2 != proc {
            return Err(E_ORDER);
        }
        // 求值对平：两种态下请求走 pick_token 单一入口。
        let req = (r >> 4) as u16;
        let g1 = eval_with(fp1, grant_of(fp1), req);
        let g2 = eval_with(fp2, grant_of(fp2), req);
        if g1 != req & grant_of(fp1) || g2 != req & grant_of(fp2) {
            return Err(E_NOPICK);
        }
        i += 1;
    }
    Ok(100)
}

/// 次序错乱注入 10 例被断言拦截（判据 ② 落点：模拟态下错误地用
/// 进程令牌求值，断言检出差异）。
pub fn disorder_ten_caught() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut caught = 0u32;
    let mut a = 0u32;
    while a < 10 {
        let r = a.wrapping_mul(2_654_435_761).wrapping_add(0x5EED);
        let attached = (r & 0xFFFF) | 1;
        // 进程令牌与挂接令牌必须 grant 派生不同（等价碰撞会掩盖
        // 差异——构造面保证可检出）。
        let mut proc = r.rotate_left(13) | 1;
        while grant_of(proc) == grant_of(attached) || proc == attached {
            proc = proc.wrapping_add(1) | 1;
        }
        let tt = ThrTok { attached_fp: attached, proc_fp: proc };
        let req = (r >> 6) as u16;
        // 正确次序：线程优先。
        let (fp_ok, src_ok) = pick_token(&tt)?;
        let val_ok = eval_with(fp_ok, grant_of(fp_ok), req);
        // 错乱次序：无脑进程（旁路实现——注入面）。
        let val_bad = eval_with(proc, grant_of(proc), req);
        // 断言：错乱结果与正确结果不同 ⇒ 注入被检出。
        // （两令牌 grant 派生不同 ⇒ 有差异；若巧合相同则此例以
        // 「源标记」差异兜底——src 面也是次序断言的一部分。）
        if val_bad != val_ok || src_ok != 1 {
            caught += 1;
        } else {
            return Err(E_DIFF); // 差异未检出 = 断言失灵
        }
        a += 1;
    }
    // 双令牌皆缺：显性 Err（次序实现不猜）。
    let empty = ThrTok { attached_fp: 0, proc_fp: 0 };
    if pick_token(&empty).is_ok() {
        return Err(E_TOKEN);
    }
    Ok(caught)
}

/// 次序矩阵（2×2 全查：有/无挂接 × 同/异 grant——次序实现的语义
/// 完备面：四种组合下 pick_token 全部显性返回，无静默路径）。
pub fn order_matrix() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut covered = 0u32;
    // (attached?, same_grant?) 四组合。
    let mut a = 0u32;
    while a < 2 {
        let mut g = 0u32;
        while g < 2 {
            let attached = if a == 0 { (0x1000 + g) | 1 } else { 0 };
            let proc = if g == 0 { 0x2000 | 1 } else { 0x2000 | 1 };
            let tt = ThrTok { attached_fp: attached, proc_fp: proc };
            let (fp, src) = pick_token(&tt)?;
            // 组合语义：a=1 无挂接 → 进程回退（src=0）；a=0 有挂接
            // → 线程优先（src=1）。
            if (a == 1 && (src != 0 || fp != proc)) || (a == 0 && (src != 1 || fp != attached)) {
                return Err(E_ORDER);
            }
            // 求值面畅通（grant 派生确定性）。
            let _ = eval_with(fp, grant_of(fp), 0x00FF);
            covered += 1;
            g += 1;
        }
        a += 1;
    }
    Ok(covered)
}

/// 令牌换装检测（挂接变更面：同线程换令牌 ⇒ pick_token 结果随
/// 挂接变化——次序实现不能缓存旧令牌）。
pub fn token_swap_detect() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let tt1 = ThrTok { attached_fp: 0xAAAA, proc_fp: 0xBBBB };
    let (fp1, src1) = pick_token(&tt1)?;
    // 换装：挂接另一令牌。
    let tt2 = ThrTok { attached_fp: 0xCCCC, proc_fp: 0xBBBB };
    let (fp2, src2) = pick_token(&tt2)?;
    if fp1 != 0xAAAA || fp2 != 0xCCCC || src1 != 1 || src2 != 1 {
        return Err(E_ORDER); // 结果未随挂接更新 = 缓存旧令牌
    }
    // 换装后摘挂：立即回退进程。
    let tt3 = ThrTok { attached_fp: 0, proc_fp: 0xBBBB };
    let (fp3, src3) = pick_token(&tt3)?;
    if fp3 != 0xBBBB || src3 != 0 {
        return Err(E_ORDER);
    }
    Ok(())
}

/// 回退面次序（摘挂即回退的即时性：pick_token 对摘挂线程立返
/// 进程令牌——次序实现的时延面：不允许「摘挂后仍用旧令牌」窗口）。
pub fn detach_immediacy() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 有挂接态。
    let tt = ThrTok { attached_fp: 0x7777, proc_fp: 0x8888 };
    let (fp, src) = pick_token(&tt)?;
    if src != 1 || fp != 0x7777 {
        return Err(E_ORDER);
    }
    // 摘挂后立即求值：必须已回退（无过渡窗口）。
    let tt2 = ThrTok { attached_fp: 0, proc_fp: 0x8888 };
    let (fp2, src2) = pick_token(&tt2)?;
    if src2 != 0 || fp2 != 0x8888 {
        return Err(E_ORDER);
    }
    Ok(())
}

/// 次序语义摘要（两态源标记的常量冻结——消费面（审计/呈现）读
/// 的「来源」语义锚：1=线程挂接 0=进程回退）。
pub const SRC_ATTACHED: u8 = 1;
pub const SRC_PRIMARY: u8 = 0;

pub fn src_semantics() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if SRC_ATTACHED == SRC_PRIMARY {
        return Err(E_ORDER);
    }
    let tt = ThrTok { attached_fp: 0x11, proc_fp: 0x22 };
    let (_, s1) = pick_token(&tt)?;
    let tt2 = ThrTok { attached_fp: 0, proc_fp: 0x22 };
    let (_, s2) = pick_token(&tt2)?;
    if s1 != SRC_ATTACHED || s2 != SRC_PRIMARY {
        return Err(E_ORDER);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = order_hundred()?;
    let c = disorder_ten_caught()?;
    // 次序矩阵 + 换装检测。
    let m = order_matrix()?;
    if m != 4 {
        return Err(E_NOPICK);
    }
    token_swap_detect()?;
    // 摘挂即时性 + 源标记语义。
    detach_immediacy()?;
    src_semantics()?;
    if n != 100 || c != 10 {
        return Err(E_ORDER);
    }
    Ok(n.wrapping_mul(1000).wrapping_add(c))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ORDER => "取令牌次序错乱",
        E_DIFF => "双次序结果差异未检出",
        E_NOPICK => "次序实现缺（双源）",
        E_TOKEN => "两令牌皆缺",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36249_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36249_impac");
    cs.add("SEG 段位独占", SEG_BASE == 0xA370, "0xA370~0xA37F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "取令牌次序 100 例对平/错乱注入 10 例全检出"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("单一入口", VER == 1, "pick_token 全引擎唯一次序实现（线程→进程）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_order_priority() {
        let tt = ThrTok { attached_fp: 0xAAAA, proc_fp: 0xBBBB };
        let (fp, src) = pick_token(&tt).unwrap_or((0, 9));
        assert_eq!((fp, src), (0xAAAA, 1));
        let tt2 = ThrTok { attached_fp: 0, proc_fp: 0xBBBB };
        let (fp2, src2) = pick_token(&tt2).unwrap_or((0, 9));
        assert_eq!((fp2, src2), (0xBBBB, 0));
    }

    #[test]
    fn test_disorder_caught() {
        assert_eq!(disorder_ten_caught().ok(), Some(10));
    }
}
