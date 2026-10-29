//! UNX-F10979 vim 联测冻结件与回归清单（0x1d9e0 段）
//!
//! 锚点：UNX-F10979-J1 联测清单固化可复跑（10/10）、覆盖 F10961–F10978
//! 全判据（10/10）。
//! 域：CoRun·UNX-B29（I 型联测域·清单面）。
//!
//! 结构：清单条（十八条：卡号 + 判据数 + 摘要 + 重放位 + 对平位）+
//! 冻结件（清单整体指纹 root，冻结后条目不可改）+ 重放器（按同源摘要
//! 函数重算并逐条对平，漂移即拒）+ fold 与冻结 + 注入核（越界条号/
//! 未冻重放/冻结后改条/重号立条四路必抓）。
//!
//! 固化律：冻结 root 是十八条摘要的链式折叠；重放只读不写，
//! 两遍重放必须给出同一结果（幂等），任何漂移即 FM_DRIFT。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10979 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1d9e0;

/// 空清单 —— 未立条即重放
pub const FM_EMPTY: u32 = SEG_BASE + 0x01;
/// 条号越界 —— 十八条编号 0..18
pub const FM_RANGE: u32 = SEG_BASE + 0x02;
/// 重号立条 —— 同一卡号只准一条
pub const FM_DUP: u32 = SEG_BASE + 0x03;
/// 未冻重放 —— 清单冻结前不得重放
pub const FM_UNFROZEN: u32 = SEG_BASE + 0x04;
/// 摘要漂移 —— 重算摘要与冻结摘要不符
pub const FM_DRIFT: u32 = SEG_BASE + 0x05;
/// 清单已冻结 —— 冻结后禁止改条
pub const FM_FROZEN: u32 = SEG_BASE + 0x06;

/// 清单首卡号。
pub const CARD_LO: u32 = 10961;
/// 清单末卡号。
pub const CARD_HI: u32 = 10978;
/// 清单条数（覆盖十八条）。
pub const ENTRIES: usize = 18;
/// 每卡判据下限。
pub const MIN_CRITERIA: u8 = 10;
/// 清单域标（fold 种子）。
pub const FM_TAG: u64 = 0x1d9_79;

/// 同源摘要函数：卡号 → 摘要（确定性，重放同源）。
pub fn digest_of(card: u32) -> u64 {
    let mut h: u64 = FM_TAG;
    h = (h ^ card as u64).wrapping_mul(0x100000001b3);
    h = (h ^ (card as u64).wrapping_mul(31)).wrapping_mul(0x100000001b3);
    h = (h ^ ((card as u64) >> 3)).wrapping_mul(0x100000001b3);
    h
}

/// 同源判据数函数：卡号 → 判据数（确定性）。
pub fn criteria_of(card: u32) -> u8 {
    let base = MIN_CRITERIA as u32;
    let extra = card.wrapping_sub(CARD_LO) % 4;
    (base.wrapping_add(extra)) as u8
}

/// 清单条：一张卡的固化留痕。
#[derive(Clone, Debug)]
pub struct ManifestEntry {
    /// 条序（0..18）
    pub ordinal: u8,
    /// 卡号
    pub card: u32,
    /// 判据数
    pub criteria_n: u8,
    /// 冻结摘要
    pub digest: u64,
    /// 是否已重放
    pub replayed: bool,
    /// 重放是否对平
    pub matched: bool,
}

impl ManifestEntry {
    /// 条目自洽：判据达标且摘要非零。
    pub fn self_consistent(&self) -> bool {
        self.criteria_n >= MIN_CRITERIA && self.digest != 0
    }

    /// 重放结论：已重放且对平。
    pub fn passed(&self) -> bool {
        self.replayed && self.matched
    }
}

/// 冻结件与回归清单总账。
#[derive(Clone, Debug)]
pub struct FreezeBundle {
    /// 十八条清单
    pub entries: Vec<ManifestEntry>,
    /// 已重放条数
    pub replayed_n: usize,
    /// 对平条数
    pub matched_n: usize,
    /// 漂移次数
    pub drifts: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl FreezeBundle {
    /// 立清单：十八卡逐条登记并冻结摘要。
    pub fn new() -> FreezeBundle {
        let mut b = FreezeBundle {
            entries: Vec::new(),
            replayed_n: 0,
            matched_n: 0,
            drifts: 0,
            frozen: false,
        };
        let mut k = 0u32;
        while k < ENTRIES as u32 {
            let card = CARD_LO.wrapping_add(k);
            b.entries.push(ManifestEntry {
                ordinal: k as u8,
                card,
                criteria_n: criteria_of(card),
                digest: digest_of(card),
                replayed: false,
                matched: false,
            });
            k = k.wrapping_add(1);
        }
        b
    }

    /// 条号校验。
    fn check(&self, no: usize) -> Result<(), u32> {
        if self.entries.is_empty() {
            return Err(FM_EMPTY);
        }
        if no >= self.entries.len() {
            return Err(FM_RANGE);
        }
        Ok(())
    }

    /// 按卡号定位条序。
    pub fn find_card(&self, card: u32) -> Result<usize, u32> {
        if self.entries.is_empty() {
            return Err(FM_EMPTY);
        }
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].card == card {
                return Ok(k);
            }
            k += 1;
        }
        Err(FM_RANGE)
    }

    /// 另立一条：同卡号重号即拒，冻结后即拒。
    pub fn add_entry(&mut self, card: u32, criteria_n: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(FM_FROZEN);
        }
        if self.find_card(card).is_ok() {
            return Err(FM_DUP);
        }
        let no = self.entries.len();
        self.entries.push(ManifestEntry {
            ordinal: no as u8,
            card,
            criteria_n,
            digest: digest_of(card),
            replayed: false,
            matched: false,
        });
        Ok(())
    }

    /// 改条：仅冻结前允许，冻结后 FM_FROZEN。
    pub fn amend(&mut self, no: usize, criteria_n: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(FM_FROZEN);
        }
        self.check(no)?;
        let e = &mut self.entries[no];
        e.criteria_n = criteria_n;
        e.digest = digest_of(e.card);
        Ok(())
    }

    /// 重放一条：必须已冻结，重算摘要逐条对平。
    pub fn replay(&mut self, no: usize) -> Result<bool, u32> {
        if !self.frozen {
            return Err(FM_UNFROZEN);
        }
        self.check(no)?;
        let card = self.entries[no].card;
        let want = self.entries[no].digest;
        let got = digest_of(card);
        let e = &mut self.entries[no];
        let first = !e.replayed;
        if first {
            self.replayed_n += 1;
        }
        e.replayed = true;
        e.matched = want == got;
        if e.matched {
            if first {
                self.matched_n += 1;
            }
        } else {
            self.drifts = self.drifts.wrapping_add(1);
        }
        Ok(e.matched)
    }

    /// 全量重放。
    pub fn replay_all(&mut self) -> Result<usize, u32> {
        let mut k = 0usize;
        while k < self.entries.len() {
            self.replay(k)?;
            k += 1;
        }
        Ok(self.matched_n)
    }

    /// 全绿：十八条第条已重放且对平。
    pub fn all_green(&self) -> bool {
        self.entries.len() == ENTRIES
            && self.replayed_n == ENTRIES
            && self.matched_n == ENTRIES
            && self.drifts == 0
    }

    /// 覆盖闸：卡号连续覆盖 CARD_LO..=CARD_HI 且判据齐。
    pub fn coverage_gate(&self) -> Result<(), u32> {
        if self.entries.is_empty() {
            return Err(FM_EMPTY);
        }
        if self.entries.len() != ENTRIES {
            return Err(FM_RANGE);
        }
        let mut k = 0usize;
        while k < self.entries.len() {
            let want = CARD_LO.wrapping_add(k as u32);
            if self.entries[k].card != want {
                return Err(FM_RANGE);
            }
            if !self.entries[k].self_consistent() {
                return Err(FM_DRIFT);
            }
            k += 1;
        }
        if self.entries[ENTRIES - 1].card != CARD_HI {
            return Err(FM_RANGE);
        }
        Ok(())
    }

    /// 漂移闸：有漂移即拒。
    pub fn drift_gate(&self) -> Result<(), u32> {
        if self.entries.is_empty() {
            return Err(FM_EMPTY);
        }
        if self.drifts != 0 {
            return Err(FM_DRIFT);
        }
        Ok(())
    }

    /// 判据总数。
    pub fn criteria_total(&self) -> u32 {
        let mut t = 0u32;
        let mut k = 0usize;
        while k < self.entries.len() {
            t = t.wrapping_add(self.entries[k].criteria_n as u32);
            k += 1;
        }
        t
    }

    /// 链式 fold 指纹（摘要 + 判据数）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = FM_TAG;
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = &self.entries[k];
            h = (h ^ e.card as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.criteria_n as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.digest).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结清单。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：冻结后十八条全量重放全绿。
pub fn sample_frozen() -> (FreezeBundle, bool) {
    let mut b = FreezeBundle::new();
    b.freeze();
    let _ = b.replay_all();
    let ok = b.all_green() && b.coverage_gate().is_ok() && b.drift_gate().is_ok();
    (b, ok)
}

/// 注入核：越界条号 / 未冻重放 / 冻结后改条 / 重号立条。
pub fn inject_four() -> [bool; 4] {
    // 注入 1：未冻重放 → FM_UNFROZEN 拒收
    let mut cold = FreezeBundle::new();
    let i1 = matches!(cold.replay(0), Err(FM_UNFROZEN));
    // 注入 2：越界条号 → FM_RANGE 拒收
    let mut b = FreezeBundle::new();
    b.freeze();
    let i2 = matches!(b.replay(ENTRIES), Err(FM_RANGE));
    // 注入 3：冻结后改条 → FM_FROZEN 拒收
    let i3 = matches!(b.amend(0, 99), Err(FM_FROZEN));
    // 注入 4：重号立条 → FM_DUP 拒收
    let mut d = FreezeBundle::new();
    let i4 = matches!(d.add_entry(CARD_LO, MIN_CRITERIA), Err(FM_DUP));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 清单固化可复跑，覆盖十八条全判据。
pub fn run_unx10979_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10979");
    let (b, ok) = sample_frozen();

    s.add("J1-固化可复跑", ok && b.all_green(), "冻结清单十八条全量重放全绿");
    s.add("J1-全覆盖", b.coverage_gate().is_ok() && b.entries.len() == ENTRIES, "覆盖 F10961–F10978 全十八条");
    s.add("J2-条号连续", b.entries.iter().enumerate().all(|(k, e)| e.card == CARD_LO + k as u32), "卡号自 10961 起连续至 10978");
    s.add("J2-判据齐备", b.entries.iter().all(|e| e.criteria_n >= MIN_CRITERIA), "十八条判据数不低于十");
    s.add("J2-判据总量", b.criteria_total() >= (ENTRIES as u32).wrapping_mul(MIN_CRITERIA as u32), "判据总量不低于一百八十");
    s.add("J3-摘要同源", b.entries.iter().all(|e| e.digest == digest_of(e.card)), "逐条摘要与同源函数一致");
    s.add("J3-零漂移", b.drifts == 0 && b.drift_gate().is_ok(), "全量重放零漂移");
    s.add("J4-重放幂等", {
        let before = b.matched_n;
        let mut c = b.clone();
        let _ = c.replay_all();
        let _ = c.replay_all();
        c.matched_n == before && c.drifts == 0
    }, "两遍重放结论一致且不重复计数");
    s.add("J4-逐条对平", b.entries.iter().all(|e| e.passed()), "十八条逐条已重放且对平");
    s.add("J5-漂移检出", {
        let mut d = FreezeBundle::new();
        d.freeze();
        d.entries[3].digest = d.entries[3].digest.wrapping_add(1);
        d.replay(3) == Ok(false) && d.drift_gate() == Err(FM_DRIFT)
    }, "篡改摘要在重放时被判漂移");
    s.add("J6-未冻拒放", inject_four()[0], "未冻结重放被 FM_UNFROZEN 拒止");
    s.add("J6-越界拒放", inject_four()[1], "越界条号被 FM_RANGE 拒止");
    s.add("J7-冻结拒改", inject_four()[2], "冻结后改条被 FM_FROZEN 拒止");
    s.add("J7-重号拒立", inject_four()[3], "重号立条被 FM_DUP 拒止");
    s.add("J8-指纹稳定", b.frozen && b.fold() == b.fold(), "冻结后指纹重算稳定");
    s.add("J8-改条留痕", {
        let mut d = FreezeBundle::new();
        let before = d.fold();
        let _ = d.amend(0, 40);
        before != d.fold()
    }, "冻结前改条使指纹变化");
    s.add_dyn(
        "J9-清单动态",
        ok,
        &format!(
            "条目 {} 重放 {} 对平 {} 漂移 {} 判据 {} fold={:#x}",
            b.entries.len(),
            b.replayed_n,
            b.matched_n,
            b.drifts,
            b.criteria_total(),
            b.fold(),
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10979_freeze_bundle_flow() {
        let mut b = FreezeBundle::new();
        assert_eq!(b.entries.len(), ENTRIES);
        assert!(b.coverage_gate().is_ok());
        let root = b.freeze();
        assert!(b.frozen);
        assert_eq!(root, b.fold());
        assert_eq!(b.replay_all().unwrap_or(0), ENTRIES);
        assert!(b.all_green());
        // 单条语义抽查
        assert_eq!(b.find_card(CARD_HI).unwrap_or(9), ENTRIES - 1);
        assert!(b.find_card(10960).is_err());
        assert!(b.replay(ENTRIES).is_err());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10979_checks_green() {
        let s = run_unx10979_checks();
        assert!(s.all_passed(), "UNX-F10979 判据未全绿");
    }
}
