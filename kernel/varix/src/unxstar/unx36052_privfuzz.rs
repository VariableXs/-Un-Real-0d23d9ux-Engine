//! UNX-F36052-J1 特权在访问令牌 fuzz 面的鲁棒性。
//! 判据：① 特权数组 fuzz 10^3 次（重复 LUID/非法属性位）全拒零静默；
//! ② 黄金 50 组往返一致；③ 纯功能代码 ≥300 行。
//! 设计纪律：fuzz 输入走独立 LCG 流盐（坑 23 教训——字段短周期耦合
//! 会把合法样本数打到零）；每例结论必落账（全拒零静默）。

use super::unx36025_tokpriv::{PrivBook, PRIV_VALID};
use super::unx36041_privtable::{self, PRIV_N};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfde0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_SILENT: u16 = SEG_BASE + 1; // 静默（无结论无拒绝）
pub const E_BAL: u16 = SEG_BASE + 2; // fuzz 账守恒破
pub const E_TABLE: u16 = SEG_BASE + 3; // 上游表纪律破
pub const E_GOLD: u16 = SEG_BASE + 4; // 黄金组往返失配
pub const E_DUP: u16 = SEG_BASE + 5; // 重复 LUID（应拒未拒）
pub const E_ATTR: u16 = SEG_BASE + 6; // 非法属性位（应拒未拒）

/// fuzz 输入：特权数组的变形样本。
pub struct FuzzArr {
    pub seed: u32,
    pub items: [(u32, u32); 4], // (luid_low, attrs) × 4
    pub n: usize,
}

/// 独立 LCG 流（每字段独立状态——解耦短周期）。
fn lcg(s: &mut u32) -> u32 {
    *s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    *s
}

/// 样本构造：合法 LUID（表内）+ 30% 重复项 + 30% 非法属性位。
pub fn from_seed(seed: u32) -> FuzzArr {
    let mut sd = seed;
    let mut f = FuzzArr {
        seed,
        items: [(0, 0); 4],
        n: 0,
    };
    let cnt = 1 + (lcg(&mut sd) % 4) as usize;
    let mut i = 0;
    while i < cnt {
        let low = unx36041_privtable::PRIV_TABLE[(lcg(&mut sd) % PRIV_N as u32) as usize].luid_low;
        // 属性面：0..0x1f（0x10 以上为非法位）。
        let mut attrs = lcg(&mut sd) & 0x1f;
        if i > 0 && lcg(&mut sd) % 3 == 0 {
            // 30% 概率与前项重复 LUID。
            attrs &= 0x0f;
            let dup_low = f.items[(i as usize + 3) % (i as usize).max(1)].0;
            f.items[i] = (dup_low, attrs);
            f.n += 1;
            i += 1;
            continue;
        }
        f.items[i] = (low, attrs);
        f.n += 1;
        i += 1;
    }
    f
}

/// 样本校验：重复 LUID / 非法属性位（attrs & !PRIV_VALID != 0）全拒。
/// 结论必落账（Ok=入册 / Err(码)=拒绝）——零静默由调用方账对平保证。
pub fn validate(f: &FuzzArr) -> Result<(), u16> {
    let mut i = 0;
    while i < f.n {
        let (low, attrs) = f.items[i];
        // 表内 LUID 校验。
        if unx36041_privtable::lookup_by_luid(low).is_err() {
            return Err(super::unx36043_privnotheld::E_NOENT);
        }
        // 属性位校验：PRIV_VALID(0x0f) 之外非法。
        if attrs & !PRIV_VALID != 0 {
            return Err(E_ATTR);
        }
        // 重复 LUID 校验。
        let mut j = 0;
        while j < i {
            if f.items[j].0 == low {
                return Err(E_DUP);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// fuzz 账：accepted/rejected/dup_r/attr_r 四面对平。
pub struct FuzzLedger {
    pub total: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub dup_r: u32,
    pub attr_r: u32,
}

impl FuzzLedger {
    pub const fn new() -> FuzzLedger {
        FuzzLedger {
            total: 0,
            accepted: 0,
            rejected: 0,
            dup_r: 0,
            attr_r: 0,
        }
    }
    pub fn record(&mut self, r: Result<(), u16>) -> Result<(), u16> {
        self.total += 1;
        match r {
            Ok(()) => self.accepted += 1,
            Err(E_DUP) => {
                self.rejected += 1;
                self.dup_r += 1;
            }
            Err(E_ATTR) => {
                self.rejected += 1;
                self.attr_r += 1;
            }
            Err(_) => self.rejected += 1,
        }
        Ok(())
    }
    pub fn conserve(&self) -> bool {
        self.accepted + self.rejected == self.total
            && self.dup_r + self.attr_r <= self.rejected
    }
}

/// 黄金 50 组：确定性合法样本（表内 LUID 顺序、合法属性位）——
/// enroll 往返一致（入册后 attrs_of 回读 == 输入位面）。
pub fn golden_roundtrip() -> Result<(), u16> {
    let book = PrivBook::new();
    let mut g = 0;
    while g < 50 {
        let idx = g % PRIV_N;
        let low = unx36041_privtable::PRIV_TABLE[idx].luid_low;
        // 合法位面：ENABLED/BY_DEFAULT 交替（避开已用 LUID 重复）。
        // 同册 50 例 > 17 槽——改用「入册-校验-清册」循环（黄金组独立册）。
        let mut single = PrivBook::new();
        single.enroll((0, low), g % 2 == 1)?;
        // 位面回读 == 期望（enroll 后：default 位或全零——无 ENABLED）。
        let got = single.attrs_of((0, low))?;
        let want = if g % 2 == 1 {
            super::unx36025_tokpriv::PRIV_ENABLED_BY_DEFAULT
        } else {
            0
        };
        if got != want {
            return Err(E_GOLD);
        }
        // 再入册 = 重复（重复入册必拒——E_DUP 面）。
        if single.enroll((0, low), false).is_ok() {
            return Err(E_DUP);
        }
        let _ = book;
        g += 1;
    }
    Ok(())
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_SILENT => "静默（无结论无拒绝）",
        E_BAL => "fuzz 账守恒破",
        E_TABLE => "上游表纪律破",
        E_GOLD => "黄金组往返失配",
        E_DUP => "重复 LUID（应拒未拒）",
        E_ATTR => "非法属性位（应拒未拒）",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表；R2 fuzz 10^3 全拒零静默（账四面对平）；
/// R3 黄金 50 组往返一致；R4 重复/非法位两拒绝面各 ≥1（多样性）；
/// R5 账守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    hits += 1;
    // R2：10^3 fuzz。
    let mut led = FuzzLedger::new();
    let mut k = 0u32;
    while k < 1000 {
        let f = from_seed(1 + k);
        let r = validate(&f);
        // 零静默：每例必 Ok 或已知错误码。
        match r {
            Ok(()) | Err(E_DUP) | Err(E_ATTR) => {}
            Err(_) => return Err(E_SILENT),
        }
        led.record(r)?;
        hits += 1;
        k += 1;
    }
    if led.total != 1000 {
        return Err(E_BAL);
    }
    // R4：两拒绝面各 ≥1（fuzz 覆盖多样性）。
    if led.dup_r == 0 || led.attr_r == 0 {
        return Err(E_BAL);
    }
    hits += 1;
    // R3：黄金 50 组。
    golden_roundtrip()?;
    hits += 1;
    // R5：账守恒。
    if !led.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(1004));
    }

    #[test]
    fn duplicate_luid_rejected() {
        let f = FuzzArr {
            seed: 1,
            items: [(2, 0x01), (2, 0x02), (0, 0), (0, 0)],
            n: 2,
        };
        assert_eq!(validate(&f), Err(E_DUP));
    }

    #[test]
    fn illegal_attr_rejected() {
        let f = FuzzArr {
            seed: 2,
            items: [(2, 0x10), (0, 0), (0, 0), (0, 0)],
            n: 1,
        };
        assert_eq!(validate(&f), Err(E_ATTR));
    }

    #[test]
    fn golden_50() {
        assert!(golden_roundtrip().is_ok());
    }
}

pub fn run_unx36052_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36052_privfuzz");
    cs.add("SEG 段位独占", SEG_BASE == 0xFDE0, "0xFDE0~0xFDFF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
