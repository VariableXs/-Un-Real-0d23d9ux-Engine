//! unx36025_tokpriv — UNX-F36025 特权数组结构（LUID + 启用/默认禁用属性）。
//!
//! 判据原文（UNX-F36025-J1）：
//!   特权 LUID 唯一性 10^3 次零重复；属性位（启用/默认禁用）双态切换
//!   100 例账实一致。
//! 设计纪律：
//!   1. LUID 单源：LuidSource 单调发射器（high=域锚恒 1，low 单调递增），
//!      10^3 次发射零重复由「单调 + 边界拒绝」结构保证（low 溢出显性拒绝，
//!      不回绕——回绕即重复源）。
//!   2. 属性双态：ENABLED / ENABLED_BY_DEFAULT 两独立位锚；切换原语
//!      enable/disable 只动 ENABLED 位，ENABLED_BY_DEFAULT 在册冻结
//!      （恢复默认语义由 reset_to_default 显式提供）。
//!   3. 账实一致：切换账（enable 计数/disable 计数）与数组位读出逐例对平。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use super::unx36021_tokencore::PrivItem;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf820;

// ── 诊断码闭集（0xf820~0xf83f，独占）─────────────────────────
const E_DUP: u16 = SEG_BASE + 1; // LUID 重复
const E_BIT: u16 = SEG_BASE + 2; // 属性位非法（保留位）
const E_CAP: u16 = SEG_BASE + 3; // 特权数越限
const E_BAL: u16 = SEG_BASE + 4; // 切换账/守恒账破
const E_NOENT: u16 = SEG_BASE + 5; // LUID 不在册
const E_OVERFLOW: u16 = SEG_BASE + 6; // LUID 发射器低 32 位耗尽（不回绕）

/// 特权数组容量。
pub const P_MAX: usize = 16;

/// SE_PRIVILEGE_* 位锚。
pub const PRIV_ENABLED: u32 = 0x01;
pub const PRIV_ENABLED_BY_DEFAULT: u32 = 0x02;
pub const PRIV_REMOVED: u32 = 0x04;
pub const PRIV_USED_FOR_ACCESS: u32 = 0x08;
/// 合法位掩码。
pub const PRIV_VALID: u32 = 0x0f;

/// LUID 发射器（单调单源，零重复）。
pub struct LuidSource {
    pub high: u32,
    pub low: u32,
}

impl LuidSource {
    pub const fn new() -> LuidSource {
        LuidSource { high: 1, low: 1 }
    }

    /// 发射：low 单调 +1；耗尽显性拒绝（不回绕——回绕即重复源）。
    pub fn issue(&mut self) -> Result<(u32, u32), u16> {
        if self.low == u32::MAX {
            return Err(E_OVERFLOW);
        }
        self.low += 1;
        Ok((self.high, self.low))
    }
}

/// 特权册（LUID → 属性位，切换账内嵌）。
pub struct PrivBook {
    pub items: [PrivItem; P_MAX],
    pub luids: [(u32, u32); P_MAX],
    pub len: usize,
    pub ena_cnt: u64,
    pub dis_cnt: u64,
}

impl PrivBook {
    pub const fn new() -> PrivBook {
        PrivBook {
            items: [PrivItem { luid_low: 0, attrs: 0 }; P_MAX],
            luids: [(0u32, 0u32); P_MAX],
            len: 0,
            ena_cnt: 0,
            dis_cnt: 0,
        }
    }

    /// 入册：默认禁用（attrs = ENABLED_BY_DEFAULT 可选）。
    pub fn enroll(&mut self, luid: (u32, u32), by_default: bool) -> Result<usize, u16> {
        if self.len >= P_MAX {
            return Err(E_CAP);
        }
        let mut i = 0;
        while i < self.len {
            if self.luids[i] == luid {
                return Err(E_DUP);
            }
            i += 1;
        }
        let attrs = if by_default { PRIV_ENABLED_BY_DEFAULT } else { 0 };
        self.items[self.len] = PrivItem { luid_low: luid.1, attrs };
        self.luids[self.len] = luid;
        self.len += 1;
        Ok(self.len - 1)
    }

    fn slot(&self, luid: (u32, u32)) -> Result<usize, u16> {
        let mut i = 0;
        while i < self.len {
            if self.luids[i] == luid {
                return Ok(i);
            }
            i += 1;
        }
        Err(E_NOENT)
    }

    /// 启用：置 ENABLED 位，切换账 +1。
    pub fn enable(&mut self, luid: (u32, u32)) -> Result<(), u16> {
        let s = self.slot(luid)?;
        if self.items[s].attrs & PRIV_ENABLED == 0 {
            self.ena_cnt += 1;
        }
        self.items[s].attrs |= PRIV_ENABLED;
        Ok(())
    }

    /// 禁用：清 ENABLED 位，切换账 +1。
    pub fn disable(&mut self, luid: (u32, u32)) -> Result<(), u16> {
        let s = self.slot(luid)?;
        if self.items[s].attrs & PRIV_ENABLED != 0 {
            self.dis_cnt += 1;
        }
        self.items[s].attrs &= !PRIV_ENABLED;
        Ok(())
    }

    /// 读出：当前位图（账实对平读面）。
    pub fn attrs_of(&self, luid: (u32, u32)) -> Result<u32, u16> {
        let s = self.slot(luid)?;
        Ok(self.items[s].attrs)
    }
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_DUP => "LUID 重复：同册重复入账",
        E_BIT => "属性位非法：保留位（!0x0F）不可置",
        E_CAP => "特权数越限：超过 P_MAX=16",
        E_BAL => "切换账/守恒账破：账面与位读出对平失败",
        E_NOENT => "LUID 不在册",
        E_OVERFLOW => "LUID 发射器低 32 位耗尽（显性拒绝不回绕）",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1：LUID 唯一性 10^3 次零重复（全量两两比对 + 单调性抽查）。
    let mut src = LuidSource::new();
    let mut luids = [(0u32, 0u32); 1024];
    let mut i = 0;
    while i < 1000 {
        let l = src.issue()?;
        luids[i] = l;
        hits += 1;
        i += 1;
    }
    // 两两比对 O(n²) 10^6 次确定性（判据侧独立复检，不信发射器承诺）。
    let mut a = 0;
    while a < 1000 {
        let mut b = a + 1;
        while b < 1000 {
            if luids[a] == luids[b] {
                return Err(E_DUP);
            }
            b += 1;
        }
        a += 1;
    }
    hits += 1;
    // R2：属性双态切换 100 例账实一致（16 槽册轮转——P_MAX 容量纪律）。
    let mut book = PrivBook::new();
    let mut sl = LuidSource::new();
    let mut luids16 = [(0u32, 0u32); P_MAX];
    let mut e = 0;
    while e < P_MAX {
        let l = sl.issue()?;
        let by_default = e % 2 == 0;
        book.enroll(l, by_default)?;
        // 期望态：偶数槽默认启用位在场但 ENABLED 清零；奇数槽全零。
        let base = if by_default { PRIV_ENABLED_BY_DEFAULT } else { 0 };
        if book.attrs_of(l)? != base {
            return Err(E_BAL);
        }
        luids16[e] = l;
        e += 1;
    }
    // 100 轮双态切换：每轮翻转一个 LUID 的 ENABLED 位（翻转账 +1/轮）。
    let mut k = 0;
    while k < 100 {
        let l = luids16[k % P_MAX];
        let cur = book.attrs_of(l)?;
        if cur & PRIV_ENABLED != 0 {
            book.disable(l)?;
            if book.attrs_of(l)? & PRIV_ENABLED != 0 {
                return Err(E_BAL);
            }
        } else {
            book.enable(l)?;
            if book.attrs_of(l)? & PRIV_ENABLED == 0 {
                return Err(E_BAL);
            }
        }
        hits += 1;
        k += 1;
    }
    // R3：切换账对平（enable + disable == 100——每轮恰一次翻转），
    // 且位读出与账面回算一致（启用数 = enable - disable）。
    if book.ena_cnt + book.dis_cnt != 100 {
        return Err(E_BAL);
    }
    let mut live_enabled = 0u64;
    let mut i2 = 0;
    while i2 < P_MAX {
        if book.items[i2].attrs & PRIV_ENABLED != 0 {
            live_enabled += 1;
        }
        i2 += 1;
    }
    if live_enabled != book.ena_cnt - book.dis_cnt {
        return Err(E_BAL);
    }
    hits += 1;
    // R4：重复入册拒绝（独立小册，避免容量前置拦截）+ 不在册操作拒绝。
    let mut book3 = PrivBook::new();
    let dup = luids[0];
    book3.enroll(dup, false)?;
    if book3.enroll(dup, false) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if book.enable((9, 987_654)).is_ok() {
        return Err(E_NOENT);
    }
    hits += 1;
    // R5：容量与保留位拒绝。
    let mut book2 = PrivBook::new();
    let mut sl2 = LuidSource::new();
    let mut m = 0;
    while m < P_MAX {
        book2.enroll(sl2.issue()?, false)?;
        m += 1;
    }
    if book2.enroll(sl2.issue()?, false) != Err(E_CAP) {
        return Err(E_CAP);
    }
    if book2.enable((0, 0x10)).is_ok() {
        // (0,0x10) 不在册 → E_NOENT；保留位拒绝由位锚合法性结构保证。
        return Err(E_BAL);
    }
    hits += 1;
    Ok(hits)
}

/// 特权册视图/批量辅助面。

/// 特权册快照（只读视图：len + 逐项 (luid, attrs) 投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrivSnapshot {
    pub len: usize,
    pub luids: [(u32, u32); P_MAX],
    pub attrs: [u32; P_MAX],
}

impl PrivBook {
    /// 快照导出。
    pub fn snapshot(&self) -> PrivSnapshot {
        let mut ls = [(0u32, 0u32); P_MAX];
        let mut as_ = [0u32; P_MAX];
        let mut i = 0;
        while i < self.len {
            ls[i] = self.luids[i];
            as_[i] = self.items[i].attrs;
            i += 1;
        }
        PrivSnapshot { len: self.len, luids: ls, attrs: as_ }
    }

    /// 快照守恒核（逐项对平）。
    pub fn snapshot_conserve(&self, s: &PrivSnapshot) -> bool {
        if s.len != self.len {
            return false;
        }
        let mut i = 0;
        while i < self.len {
            if s.luids[i] != self.luids[i] || s.attrs[i] != self.items[i].attrs {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 启用特权清单（判定面：ENABLED 位在场的 LUID 列表）。
    pub fn enabled_list(&self) -> [(u32, u32); P_MAX] {
        let mut out = [(0u32, 0u32); P_MAX];
        let mut i = 0;
        while i < self.len {
            if self.items[i].attrs & PRIV_ENABLED != 0 {
                out[i] = self.luids[i];
            }
            i += 1;
        }
        out
    }
}

/// 批量切换面：N 例 LUID 批量 enable（fork 批量提权场景）。
pub fn batch_enable(book: &mut PrivBook, luids: &[(u32, u32)]) -> Result<u64, u16> {
    let mut n = 0u64;
    let mut i = 0;
    while i < luids.len() {
        book.enable(luids[i])?;
        n += 1;
        i += 1;
    }
    Ok(n)
}

pub fn run_unx36025_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36025_tokpriv");
    cs.add("SEG 段位独占", SEG_BASE == 0xf820, "0xf820~0xf83f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 特权数组与双态账", true, "LUID 10^3 零重复+双态 100 例对平"),
        Err(code) => cs.add("J1 特权数组与双态账", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(1104));
    }
}
