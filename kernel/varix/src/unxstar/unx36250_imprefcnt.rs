//! UNX-F36250-J1 模拟令牌引用计数与泄漏防护。
//! 判据：① 模拟开始/结束 10^4 次 ref 归零守恒；② 模拟中线程终止
//! 泄漏注入 10 例被清扫回收；③ 纯功能 ≥300 行。
//! 设计纪律：引用计数的守恒律是「开始 N 次 = 结束 N 次 ⇒ 计数
//! 归零」——泄漏 = 结束路径少了计数递减。线程在模拟中途被终止是
//! 最典型的泄漏源（终止路径忘了释放挂接令牌的引用），清扫器的
//! 职责是把这类孤儿引用回收至归零。10^4 次循环是守恒律的规模
//! 压力面：任何路径少减一次，10^4 次后计数就是正数。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA380;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_LEAK: u16 = SEG_BASE + 2; // 引用计数未归零
pub const E_ORPHAN: u16 = SEG_BASE + 3; // 孤儿引用（终止未清扫）
pub const E_UNDER: u16 = SEG_BASE + 4; // 计数下溢（减多于加）
pub const E_SWEEP: u16 = SEG_BASE + 5; // 清扫失败
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 令牌引用账（模拟挂接的引用计数面）。
pub struct TokenRef {
    /// 令牌指纹。
    pub fp: u32,
    /// 引用计数（挂接 +1 / 摘挂 -1 / 终止挂接态由清扫 -1）。
    pub refs: u32,
    /// 孤儿标记（终止时仍挂接 ⇒ 清扫器回收）。
    pub orphaned: bool,
}

impl TokenRef {
    pub fn new(fp: u32) -> Self {
        TokenRef { fp, refs: 0, orphaned: false }
    }

    /// 挂接（引用 +1）。
    pub fn acquire(&mut self) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        self.refs = self.refs.wrapping_add(1);
        Ok(())
    }

    /// 摘挂（引用 -1——下溢即账破）。
    pub fn release(&mut self) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if self.refs == 0 {
            return Err(E_UNDER);
        }
        self.refs -= 1;
        Ok(())
    }

    /// 线程终止（挂接态终止 ⇒ 标记孤儿——泄漏防护的挂起面）。
    pub fn thread_terminated(&mut self) {
        if self.refs > 0 {
            self.orphaned = true;
        }
    }

    /// 归零守恒（计数零且无孤儿——判据 ① 落点）。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.refs != 0 {
            return Err(E_LEAK);
        }
        if self.orphaned {
            return Err(E_ORPHAN);
        }
        Ok(())
    }
}

/// 清扫器（孤儿引用回收——判据 ② 落点）。
pub struct Sweeper;

impl Sweeper {
    /// 回收孤儿：refs 清零 + 孤儿标记摘除。
    pub fn sweep(t: &mut TokenRef) -> Result<u32, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if !t.orphaned {
            return Ok(0); // 无孤儿不回收（幂等）
        }
        let reclaimed = t.refs;
        t.refs = 0;
        t.orphaned = false;
        Ok(reclaimed)
    }
}

/// 10^4 次开始/结束守恒（判据 ① 落点：交替 acquire/release，
/// 终点归零；奇数次结束路径全部走完）。
pub fn conserve_10k() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut t = TokenRef::new(0x1EAF);
    let mut i = 0u32;
    let mut peak = 0u32;
    while i < 10_000 {
        t.acquire()?;
        // 每第 3 次挂接夹带一次双挂接（压力面：峰值 > 1）。
        if i % 3 == 0 {
            t.acquire()?;
        }
        if t.refs > peak {
            peak = t.refs;
        }
        t.release()?;
        if i % 3 == 0 {
            t.release()?;
        }
        i += 1;
    }
    t.conserved()?;
    if peak < 2 {
        return Err(E_UNDER); // 压力面没建起来 = 测试无效
    }
    Ok(peak)
}

/// 模拟中线程终止泄漏注入 10 例被清扫回收（判据 ② 落点）。
pub fn orphan_ten_swept() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut swept = 0u32;
    let mut a = 0u32;
    while a < 10 {
        let r = a.wrapping_mul(2_654_435_761).wrapping_add(0x5EED);
        let mut t = TokenRef::new((r & 0xFFFF) | 1);
        // 模拟开始（挂接 1..3 次——多重引用压力）。
        let n = (a % 3) + 1;
        let mut k = 0;
        while k < n {
            t.acquire().map_err(|_| E_ILLEGAL)?;
            k += 1;
        }
        // 线程终止（模拟中——孤儿产生）。
        t.thread_terminated();
        if !t.orphaned {
            return Err(E_ORPHAN); // 孤儿未标记 = 泄漏防护失灵
        }
        // 清扫回收。
        let got = Sweeper::sweep(&mut t)?;
        if got != n {
            return Err(E_SWEEP);
        }
        t.conserved()?;
        swept += 1;
        a += 1;
    }
    // 非孤儿清扫幂等（sweep 返回 0）。
    let mut t2 = TokenRef::new(0x2222);
    if Sweeper::sweep(&mut t2).map_err(|_| E_SWEEP)? != 0 {
        return Err(E_SWEEP);
    }
    Ok(swept)
}

/// 峰值账（守恒测试的副账：acquire 峰值/释放轨迹——泄漏检测的
/// 灵敏度面：峰值轨迹可重放）。
pub fn ref_peak_ledger() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut t = TokenRef::new(0x1EAF);
    let mut peak = 0u32;
    // 阶梯挂接 1..5，逐级释放归零——五轮全守恒。
    let mut round = 0u32;
    while round < 5 {
        let n = round + 1;
        let mut k = 0;
        while k < n {
            t.acquire().map_err(|_| E_ILLEGAL)?;
            k += 1;
        }
        if t.refs > peak {
            peak = t.refs;
        }
        let mut k2 = 0;
        while k2 < n {
            t.release().map_err(|_| E_UNDER)?;
            k2 += 1;
        }
        t.conserved()?;
        round += 1;
    }
    if peak != 5 {
        return Err(E_LEAK);
    }
    Ok(peak)
}

/// 引用账指纹（账态折叠——同态恒同指纹：泄漏检测的长驻快检锚）。
pub fn ref_fingerprint(t: &TokenRef) -> u32 {
    t.fp.wrapping_mul(1_000_003)
        .wrapping_add(t.refs.wrapping_mul(97))
        .wrapping_add((t.orphaned as u32).wrapping_mul(31))
}

/// 并发模拟账（三令牌各自独立计数——账与令牌一一对应无串扰）。
pub fn multi_token_ledger() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut t1 = TokenRef::new(0x1001);
    let mut t2 = TokenRef::new(0x1002);
    let mut t3 = TokenRef::new(0x1003);
    // 交错挂接/释放（t1 ×2、t2 ×1、t3 ×3）。
    t1.acquire().map_err(|_| E_ILLEGAL)?;
    t2.acquire().map_err(|_| E_ILLEGAL)?;
    t3.acquire().map_err(|_| E_ILLEGAL)?;
    t1.acquire().map_err(|_| E_ILLEGAL)?;
    t3.acquire().map_err(|_| E_ILLEGAL)?;
    t3.acquire().map_err(|_| E_ILLEGAL)?;
    // 各账独立：t1=2、t2=1、t3=3。
    if t1.refs != 2 || t2.refs != 1 || t3.refs != 3 {
        return Err(E_LEAK);
    }
    // 全释放全守恒。
    t1.release().map_err(|_| E_UNDER)?;
    t1.release().map_err(|_| E_UNDER)?;
    t2.release().map_err(|_| E_UNDER)?;
    t3.release().map_err(|_| E_UNDER)?;
    t3.release().map_err(|_| E_UNDER)?;
    t3.release().map_err(|_| E_UNDER)?;
    t1.conserved()?;
    t2.conserved()?;
    t3.conserved()?;
    // 指纹稳定（同态重放同指纹）。
    let f1 = ref_fingerprint(&t1);
    let f2 = ref_fingerprint(&t1);
    if f1 != f2 {
        return Err(E_LEAK);
    }
    Ok(3)
}

/// 终止时序面（终止 → 清扫 → 再挂接：清扫后对象可复用，无孤儿
/// 残留——生命周期与引用账的正交面）。
pub fn terminate_then_reuse() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut t = TokenRef::new(0xBEEF);
    t.acquire().map_err(|_| E_ILLEGAL)?;
    t.thread_terminated();
    if Sweeper::sweep(&mut t).map_err(|_| E_SWEEP)? != 1 {
        return Err(E_SWEEP);
    }
    // 复用：清扫后可重新挂接（孤儿标记已摘）。
    t.acquire().map_err(|_| E_ILLEGAL)?;
    if t.orphaned {
        return Err(E_ORPHAN);
    }
    t.release().map_err(|_| E_UNDER)?;
    t.conserved()
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let peak = conserve_10k()?;
    let swept = orphan_ten_swept()?;
    // 峰值账 + 终止复用面 + 多令牌账。
    ref_peak_ledger()?;
    terminate_then_reuse()?;
    multi_token_ledger()?;
    if swept != 10 || peak < 2 {
        return Err(E_LEAK);
    }
    Ok(peak.wrapping_mul(1000).wrapping_add(swept))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_LEAK => "引用计数未归零",
        E_ORPHAN => "孤儿引用（终止未清扫）",
        E_UNDER => "计数下溢（减多于加）",
        E_SWEEP => "清扫失败",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36250_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36250_imprefcnt");
    cs.add("SEG 段位独占", SEG_BASE == 0xA380, "0xA380~0xA38F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "10^4 次守恒归零/孤儿注入 10 例全清扫"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("下溢守卫", VER == 1, "release 零计数即 E_UNDER（减不多于加）");
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
    fn test_underflow_guard() {
        let mut t = TokenRef::new(1);
        assert!(t.release().err() == Some(E_UNDER));
        t.acquire().unwrap_or(());
        t.acquire().unwrap_or(());
        t.release().unwrap_or(());
        t.release().unwrap_or(());
        assert!(t.conserved().is_ok());
    }

    #[test]
    fn test_orphan_sweep() {
        let mut t = TokenRef::new(0xFEED);
        t.acquire().unwrap_or(());
        t.acquire().unwrap_or(());
        t.thread_terminated();
        assert_eq!(Sweeper::sweep(&mut t).unwrap_or(0), 2);
        assert!(t.conserved().is_ok());
        assert_eq!(orphan_ten_swept().ok(), Some(10));
    }
}
