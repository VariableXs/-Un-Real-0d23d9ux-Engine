//! UNX-F11054 对照总扫与移交注账（外部对照面封存）（0x1e6c0 段）
//!
//! 外部对照面封存：对照战役收尾后，20 条移交注账
//! （8 条对照件、6 条冻结件、6 条协议件）逐项核验后封存。
//! 守恒式：sealed == verified == 总注数（逐项先验后封），
//! 封存后指纹即移交根账，冻结拒改，零悬空项。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11054 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11054 - 11000) * 0x20;

/// 注账空 —— 封存前必须先登记移交注
pub const HS_EMPTY: u32 = SEG_BASE + 0x01;
/// 注号越界 —— 20 注编号 0..20
pub const HS_RANGE: u32 = SEG_BASE + 0x02;
/// 未验先封 —— 只有 verified 态可封存
pub const HS_NOVER: u32 = SEG_BASE + 0x03;
/// 重复封存 —— 一注只封一次
pub const HS_DUP: u32 = SEG_BASE + 0x04;
/// 封存悬空 —— 总扫时仍有注未封
pub const HS_DANGLE: u32 = SEG_BASE + 0x05;
/// 面已冻结 —— 封存后禁止再登记
pub const HS_FROZEN: u32 = SEG_BASE + 0x06;

/// 类别：对照件
pub const CAT_ARTIFACT: u8 = 0;
/// 类别：冻结件
pub const CAT_FROZENUP: u8 = 1;
/// 类别：协议件
pub const CAT_PROTOCOL: u8 = 2;
/// 移交注总数
pub const NOTES_TOTAL: usize = 20;
/// 对照件数
pub const ARTIFACT_N: usize = 8;
/// 冻结件数
pub const FROZEN_N: usize = 6;
/// 协议件数
pub const PROTOCOL_N: usize = 6;
/// 移交注账标签
pub const SEAL_TAG: u64 = 0x6873_6561_6c73_3045;

/// 单条移交注
#[derive(Clone, Debug)]
pub struct SealNote {
    /// 注号（0xD000 段起编）
    pub cid: u32,
    /// 注名
    pub name: String,
    /// 类别
    pub cat: u8,
    /// 核验值
    pub verify: u64,
    /// 封存指纹（0 = 未封）
    pub seal: u64,
}

/// 移交注账总账
#[derive(Clone, Debug)]
pub struct SealLedger {
    /// 20 条移交注
    pub notes: Vec<SealNote>,
    /// 已核验注数
    pub verified_n: usize,
    /// 已封存注数
    pub sealed_n: usize,
    /// 封存指纹累计（异或根）
    pub seal_root: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl SealLedger {
    /// 立账：20 注按类别登记，核验留空
    pub fn new() -> SealLedger {
        let mut lg = SealLedger {
            notes: Vec::new(),
            verified_n: 0, sealed_n: 0, seal_root: 0, frozen: false,
        };
        for i in 0..NOTES_TOTAL {
            let cat = if i < ARTIFACT_N {
                CAT_ARTIFACT
            } else if i < ARTIFACT_N + FROZEN_N {
                CAT_FROZENUP
            } else {
                CAT_PROTOCOL
            };
            lg.notes.push(SealNote {
                cid: 0xD000 + i as u32,
                name: "seal_note".to_string(),
                cat,
                verify: 0,
                seal: 0,
            });
        }
        lg
    }

    /// 核验一注：核验值由类别闭环函数给出
    pub fn verify(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(HS_FROZEN); }
        if self.notes.is_empty() { return Err(HS_EMPTY); }
        if no >= self.notes.len() { return Err(HS_RANGE); }
        let n = &mut self.notes[no];
        if n.verify != 0 { return Err(HS_DUP); }
        // 类别闭环核验值：类别 × 注号低 8 位 + 1
        let v = ((n.cat as u64) + 1).wrapping_mul((n.cid & 0xff) as u64).wrapping_add(1);
        n.verify = v;
        self.verified_n += 1;
        Ok(v)
    }

    /// 封存一注：先验后封
    pub fn seal(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(HS_FROZEN); }
        if no >= self.notes.len() { return Err(HS_RANGE); }
        let n = &mut self.notes[no];
        if n.verify == 0 { return Err(HS_NOVER); }
        if n.seal != 0 { return Err(HS_DUP); }
        let s = n.verify.wrapping_mul(0x100000001b3) ^ (n.cid as u64);
        n.seal = s;
        self.seal_root ^= s;
        self.sealed_n += 1;
        Ok(s)
    }

    /// 验封一步到位
    pub fn verify_seal(&mut self, no: usize) -> Result<u64, u32> {
        if no >= self.notes.len() { return Err(HS_RANGE); }
        self.verify(no)?;
        self.seal(no)
    }

    /// 全量封存
    pub fn seal_all(&mut self) -> Result<usize, u32> {
        for no in 0..self.notes.len() {
            self.verify_seal(no)?;
        }
        Ok(self.sealed_n)
    }

    /// 零悬空探测：全部注已封
    pub fn zero_dangle(&self) -> bool {
        self.sealed_n == self.notes.len() && self.notes.iter().all(|n| n.seal != 0)
    }

    /// 悬空闸：存在未封注即 HS_DANGLE
    pub fn dangle_gate(&self) -> Result<(), u32> {
        if self.zero_dangle() { Ok(()) } else { Err(HS_DANGLE) }
    }

    /// 按类别统计封存数
    pub fn sealed_cat(&self, cat: u8) -> usize {
        self.notes.iter().filter(|n| n.cat == cat && n.seal != 0).count()
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SEAL_TAG;
        for n in &self.notes {
            h = (h ^ (n.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ n.verify).wrapping_mul(0x100000001b3);
            h = (h ^ n.seal).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.seal_root).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结注账（封存收尾）
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：20 注全验全封
pub fn sample_sealed() -> (SealLedger, bool) {
    let mut lg = SealLedger::new();
    let _ = lg.seal_all();
    let ok = lg.zero_dangle() && lg.verified_n == NOTES_TOTAL;
    lg.freeze();
    (lg, ok)
}

/// 注入路径：未验先封 / 重复封存 / 越界 / 冻结
pub fn inject_four() -> [bool; 4] {
    let mut lg = SealLedger::new();
    // 注入 1：未验先封 → HS_NOVER 拒止
    let i1 = matches!(lg.seal(0), Err(HS_NOVER));
    // 注入 2：验后封，再重复封 → HS_DUP 拒止
    let _ = lg.verify(1);
    let _ = lg.seal(1);
    let i2 = matches!(lg.seal(1), Err(HS_DUP));
    // 注入 3：注号越界 → HS_RANGE 拒止
    let i3 = matches!(lg.verify_seal(NOTES_TOTAL), Err(HS_RANGE));
    // 注入 4：冻结后封存 → HS_FROZEN 拒止
    lg.freeze();
    let i4 = matches!(lg.seal(2), Err(HS_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 封存逐项核销，移交注账零悬空
pub fn run_unx11054_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11054");
    let (lg, ok) = sample_sealed();

    s.add("J1-逐项核验", ok && lg.verified_n == NOTES_TOTAL, "20 注全部核验留痕");
    s.add("J1-逐项封存", lg.sealed_n == NOTES_TOTAL, "20 注全部封存注账");
    s.add("J1-零悬空", lg.zero_dangle() && lg.dangle_gate().is_ok(), "封存全覆盖零悬空项");
    s.add("J2-先验后封", lg.notes.iter().all(|n| n.seal != 0 || n.verify == 0), "未验注必未封、已封注必已验");
    s.add("J2-根账聚合", lg.seal_root == lg.notes.iter().fold(0u64, |acc, n| acc ^ n.seal), "封存根账等于各注指纹异或");
    s.add("J2-根账非零", lg.seal_root != 0, "封存根账非零可移交");
    s.add("J3-对照件", lg.sealed_cat(CAT_ARTIFACT) == ARTIFACT_N, "对照件 8 条全封");
    s.add("J3-冻结件", lg.sealed_cat(CAT_FROZENUP) == FROZEN_N, "冻结件 6 条全封");
    s.add("J3-协议件", lg.sealed_cat(CAT_PROTOCOL) == PROTOCOL_N, "协议件 6 条全封");
    s.add("J4-编号连续", lg.notes[0].cid == 0xD000 && lg.notes[NOTES_TOTAL - 1].cid == 0xD013, "注号 0xD000 段起编连续");
    s.add("J4-类别正交", lg.notes.iter().all(|n| n.cat <= CAT_PROTOCOL), "类别取值合法");
    s.add("J5-未验拒封", inject_four()[0], "未验先封被 HS_NOVER 拒止");
    s.add("J5-重复拒封", inject_four()[1], "重复封存被 HS_DUP 拒止");
    s.add("J6-越界拒封", inject_four()[2], "注号越界被 HS_RANGE 拒止");
    s.add("J6-冻结拒封", inject_four()[3], "冻结后封存被 HS_FROZEN 拒止");

    // J7 错误码分段互异：六码落在段基址页内且两两不同
    let codes = [HS_EMPTY, HS_RANGE, HS_NOVER, HS_DUP, HS_DANGLE, HS_FROZEN];
    let mut in_seg = true;
    let mut distinct = true;
    let mut a = 0usize;
    while a < codes.len() {
        if codes[a] <= SEG_BASE || codes[a] >= SEG_BASE + 0x20 {
            in_seg = false;
        }
        let mut b = a.wrapping_add(1);
        while b < codes.len() {
            if codes[a] == codes[b] {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J7-错误码分段", in_seg && distinct, "六码同段内两两互异");
    s.add_dyn(
        "J7-封存动态",
        ok,
        &format!("封存 {}/{}，根账 {:#x}，fold={:#x}", lg.sealed_n, NOTES_TOTAL, lg.seal_root, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11054_handover_seal_flow() {
        let mut lg = SealLedger::new();
        // 未封前悬空闸拒止
        assert!(matches!(lg.dangle_gate(), Err(HS_DANGLE)));
        // 全量封存
        assert_eq!(lg.seal_all().unwrap_or(0), NOTES_TOTAL);
        assert!(lg.zero_dangle());
        assert_eq!(lg.verified_n, NOTES_TOTAL);
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11054_checks_green() {
        let s = run_unx11054_checks();
        assert!(s.all_passed(), "UNX-F11054 判据未全绿");
    }
}
