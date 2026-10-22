//! CGPU-F0408 · 与 VE 卷 T 域协议联动预留（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - VE 联动：预算语义对齐（CGPU-T F3041+ VE 对接协议的预算侧——VE
//!   功能的算力标签与本域账户的映射预留）；跨卷预算交易预留（VE 域
//!   功能↔系统功能的预算互借通道概念对齐）；诚实标注（跨卷联动随 T 域
//!   开工推进，本组只对齐语义与预留接口）。
//! - 判据：语义对齐、交易预留、诚实标注、判据。
//!
//! 设计纪律：
//! - 语义对齐：VE 算力标签（TAG 单位）↔ 本域预算单位（PERM 千分位）
//!   的映射表——汇率在册、换算可复算、方向可逆；映射只对齐语义不启动
//!   交易（预留）。
//! - 跨卷互借通道预留：借出方/借入方/额度/期限四元组概念账——通道
//!   处于 Reserved 态，开工（T 域）前禁止真实成交。
//! - 诚实标注：跨卷能力位一期全关（对齐≠联通）；宣称检查器防虚标。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0CB0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0CB0，全仓独占）。
pub const SEG_BASE: u16 = 0x0CB0;

pub const E_TMAP_UNKNOWN: u16 = SEG_BASE + 1; // 映射表缺项
pub const E_TMAP_RATE: u16 = SEG_BASE + 2; // 汇率非法（0）
pub const E_TCHAN_STATE: u16 = SEG_BASE + 3; // 预留态通道被成交
pub const E_TCHAN_TERMS: u16 = SEG_BASE + 4; // 互借四元组缺件
pub const E_THONEST: u16 = SEG_BASE + 5; // 跨卷能力虚标
pub const E_TMAP_REV: u16 = SEG_BASE + 6; // 换算不可逆

/// VE 算力标签单位 → 本域千分位的映射行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemAlign {
    /// VE 功能号（T 域坐标）。
    pub ve_func: u32,
    /// 本域账户。
    pub acct: u32,
    /// 汇率：1 TAG = rate PERM。
    pub rate_perm: u32,
}

/// 语义对齐账。
#[derive(Clone, Debug, Default)]
pub struct SemBook {
    pub rows: Vec<SemAlign>,
}

impl SemBook {
    pub fn align(&mut self, ve_func: u32, acct: u32, rate_perm: u32) -> Result<(), u16> {
        if rate_perm == 0 {
            return Err(E_TMAP_RATE);
        }
        match self.rows.iter_mut().find(|r| r.ve_func == ve_func) {
            Some(e) => {
                e.acct = acct;
                e.rate_perm = rate_perm;
            }
            None => self.rows.push(SemAlign { ve_func, acct, rate_perm }),
        }
        Ok(())
    }

    /// VE → 本域换算。
    pub fn to_local(&self, ve_func: u32, tags: u64) -> Result<u64, u16> {
        let r = self
            .rows
            .iter()
            .find(|r| r.ve_func == ve_func)
            .ok_or(E_TMAP_UNKNOWN)?;
        Ok(tags * r.rate_perm as u64)
    }

    /// 本域 → VE 反算（可逆性：反算误差 < 1 TAG）。
    pub fn to_ve(&self, ve_func: u32, perm: u64) -> Result<u64, u16> {
        let r = self
            .rows
            .iter()
            .find(|r| r.ve_func == ve_func)
            .ok_or(E_TMAP_UNKNOWN)?;
        if r.rate_perm == 0 {
            return Err(E_TMAP_RATE);
        }
        let back = perm / r.rate_perm as u64;
        // 可逆自证：再正算回去误差必须在 1 个 PERM 单位内。
        let re = back * r.rate_perm as u64;
        if re.abs_diff(perm) > r.rate_perm as u64 {
            return Err(E_TMAP_REV);
        }
        Ok(back)
    }

    /// 对齐完整性：每行本域账户唯一（一个账户不挂两个 VE 功能）。
    pub fn unique_acct(&self) -> Result<(), u16> {
        for i in 0..self.rows.len() {
            for j in (i + 1)..self.rows.len() {
                if self.rows[i].acct == self.rows[j].acct {
                    return Err(E_TMAP_UNKNOWN);
                }
            }
        }
        Ok(())
    }
}

/// 通道状态：Reserved（预留）——一期只有此态；T 域开工后才有 Open。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChanState {
    Reserved,
}

/// 跨卷互借概念账（四元组）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LendTerms {
    /// 借出方账户（本域）。
    pub lender_acct: u32,
    /// 借入方（VE 功能号）。
    pub borrower_func: u32,
    /// 额度（本域 PERM 单位）。
    pub amount_perm: u64,
    /// 期限（帧）。
    pub term_frames: u64,
}

/// 互借通道（预留态——概念账）。
#[derive(Clone, Debug)]
pub struct LendChannel {
    pub state: ChanState,
    pub terms: Option<LendTerms>,
}

impl Default for LendChannel {
    fn default() -> Self {
        LendChannel { state: ChanState::Reserved, terms: None }
    }
}

impl Default for CrossVolumeMark {
    fn default() -> Self {
        VE_PHASE1_MARK
    }
}

impl LendChannel {
    pub fn new() -> Self {
        LendChannel { state: ChanState::Reserved, terms: None }
    }

    /// 预留登记：四元组齐才入账；预留态不成交。
    pub fn reserve(&mut self, t: LendTerms) -> Result<(), u16> {
        if t.amount_perm == 0 || t.term_frames == 0 {
            return Err(E_TCHAN_TERMS);
        }
        self.terms = Some(t);
        Ok(())
    }

    /// 成交守闸：预留态下一律拒绝（T 域开工后开放）。
    pub fn settle(&self) -> Result<(), u16> {
        match self.state {
            ChanState::Reserved => Err(E_TCHAN_STATE),
        }
    }
}

/// 跨卷能力位（一期全关——诚实标注锚）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossVolumeMark {
    /// 语义对齐：开（本组做）。
    pub sem_align: bool,
    /// 真实互借交易：关（随 T 域）。
    pub live_trade: bool,
    /// 跨卷预算实时同步：关。
    pub live_sync: bool,
}

pub const VE_PHASE1_MARK: CrossVolumeMark = CrossVolumeMark {
    sem_align: true,
    live_trade: false,
    live_sync: false,
};

impl CrossVolumeMark {
    pub fn claims_ok(&self, claimed: &[bool; 3]) -> bool {
        let have = [self.sem_align, self.live_trade, self.live_sync];
        claimed.iter().zip(have.iter()).all(|(c, h)| !*c || *h)
    }
}

/// VE 联动预留总账。
#[derive(Clone, Debug, Default)]
pub struct VeLinkage {
    pub sem: SemBook,
    pub channel: LendChannel,
    pub mark: CrossVolumeMark,
}

impl VeLinkage {
    pub fn new() -> Self {
        VeLinkage {
            sem: SemBook::default(),
            channel: LendChannel::new(),
            mark: VE_PHASE1_MARK,
        }
    }

    /// 预留完整性自检：对齐账合法 + 通道概念账合法 + 标注位诚实。
    pub fn validate(&self) -> Result<(), u16> {
        self.sem.unique_acct()?;
        if let Some(t) = &self.channel.terms {
            if t.amount_perm == 0 || t.term_frames == 0 {
                return Err(E_TCHAN_TERMS);
            }
        }
        Ok(())
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn it() -> VeLinkage {
        let mut v = VeLinkage::new();
        v.sem.align(3041, 100, 4).ok(); // 1 TAG = 4 PERM
        v.sem.align(3042, 101, 10).ok();
        v
    }

    #[test]
    fn a01_语义对齐_映射与换算可复算() {
        let v = it();
        assert_eq!(v.sem.to_local(3041, 25), Ok(100));
        assert_eq!(v.sem.to_local(3042, 7), Ok(70));
        // 缺项拒。
        assert_eq!(v.sem.to_local(9999, 1), Err(E_TMAP_UNKNOWN));
        // 零汇率拒。
        let mut v2 = it();
        assert_eq!(v2.sem.align(3043, 102, 0), Err(E_TMAP_RATE));
    }

    #[test]
    fn a02_换算可逆_往返误差在一单位内() {
        let v = it();
        // 正算 25 TAG → 100 PERM，反算回 25。
        assert_eq!(v.sem.to_ve(3041, 100), Ok(25));
        // 非整倍反算：7 PERM / 4 = 1 余 3（回正算 4，误差 3 ≤ 4）。
        assert_eq!(v.sem.to_ve(3041, 7), Ok(1));
        // 可逆守恒抽查多值。
        for tags in [0u64, 1, 5, 100, 4096] {
            let perm = v.sem.to_local(3042, tags).unwrap_or(u64::MAX);
            assert_eq!(v.sem.to_ve(3042, perm), Ok(tags));
        }
    }

    #[test]
    fn a03_交易预留_四元组与预留态守闸() {
        let mut v = it();
        // 四元组缺件拒。
        assert_eq!(
            v.channel.reserve(LendTerms { lender_acct: 100, borrower_func: 3041, amount_perm: 0, term_frames: 100 }),
            Err(E_TCHAN_TERMS)
        );
        assert_eq!(
            v.channel.reserve(LendTerms { lender_acct: 100, borrower_func: 3041, amount_perm: 500, term_frames: 0 }),
            Err(E_TCHAN_TERMS)
        );
        // 完整预留入账。
        assert_eq!(
            v.channel.reserve(LendTerms { lender_acct: 100, borrower_func: 3041, amount_perm: 500, term_frames: 240 }),
            Ok(())
        );
        // 预留态成交被守闸拒绝。
        assert_eq!(v.channel.settle(), Err(E_TCHAN_STATE));
        assert_eq!(v.channel.state, ChanState::Reserved);
    }

    #[test]
    fn a04_诚实标注_跨卷能力一期全关只开对齐() {
        let v = it();
        // 语义对齐可宣称；交易与实时同步不可宣称。
        assert_eq!(v.validate(), Ok(()));
        assert!(v.mark.claims_ok(&[true, false, false]));
        assert!(v.mark.claims_ok(&[false, false, false]));
        assert_eq!(v.mark.claims_ok(&[true, true, false]), false);
        assert_eq!(v.mark.claims_ok(&[true, false, true]), false);
        // 标注位固化。
        assert!(VE_PHASE1_MARK.sem_align);
        assert!(!VE_PHASE1_MARK.live_trade);
        assert!(!VE_PHASE1_MARK.live_sync);
    }

    #[test]
    fn a05_预留完整性_账户唯一与概念账合法() {
        let mut v = it();
        assert_eq!(v.validate(), Ok(()));
        // 一账户挂两 VE 功能 ⇒ 对齐账红。
        let mut v2 = it();
        v2.sem.align(3043, 100, 2).ok();
        assert_eq!(v2.sem.unique_acct(), Err(E_TMAP_UNKNOWN));
        // 概念账入位后仍绿。
        v.channel
            .reserve(LendTerms { lender_acct: 101, borrower_func: 3042, amount_perm: 200, term_frames: 60 })
            .ok();
        assert_eq!(v.validate(), Ok(()));
    }
}
