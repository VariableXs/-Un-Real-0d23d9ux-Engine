//! unx0355_apucode —— UNX-F0355 AP 微码修订一致性断言。
//! 锚点（CoRun Unxreal 主册）：UNX-F0355 · AP 微码修订一致性 · 目标 200 行
//! 判据原文：UNX-F0355-J1 全 AP 修订号与 BSP 三方一致（8 核
//!           8/8），不一致隔离有账（只读）。
//! 组件：微码修订采样账（每 AP 一行：AP ID + 修订号 + 采签
//!       FNV，只读采样不改微码）+ 一致性断言（全 AP 修订号 ==
//!       BSP 修订号：8 核要求 8/8 全对齐，一票不一致即整批不齐）
//!       + 不一致隔离账（失配 AP 进隔离建议清单——只读账面，
//!       实际隔离走 F0347）+ 三方一致性（BSP 快照/账面/断言
//!       三处修订号同源：任两处不等 = 账面破）。
//! 职责定位：微码一致性是「同构 SMP 假设」的地基——修订号不
//!   齐 = 不同核可能对同一指令有不同行为，补丁级差异会造成
//!   难以归因的偶发错。本模块只采样只断言不写 MSR（只读红线），
//!   失配处置权完全交 F0347。
//! 性能逐项分解：采样 O(1)/AP；断言 O(AP 数)；三方核 O(1)。
//! 诊断码段：0xE4A0（0xE4A1~0xE4A9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const UC_VERSION: &str = "unx0355_apucode-v1";

/// 诊断段基址（0xE4A0 段 · 本模块独占）。
pub const UC_SEG_BASE: u16 = 0xE4A0;

/// XcCode 闭集（0xE4A1~0xE4A9）。
pub const XC_REV_MISMATCH: u32 = 0xE4A1;
pub const XC_DUP_AP: u32 = 0xE4A2;
pub const XC_BSP_MISSING: u32 = 0xE4A3;
pub const XC_THREEWAY_DRIFT: u32 = 0xE4A4;
pub const XC_REV_ZERO: u32 = 0xE4A5;
pub const XC_SNAP_MUTATED: u32 = 0xE4A6;
pub const XC_ISO_SUGGEST_FULL: u32 = 0xE4A7;
pub const XC_SAMPLE_OOR: u32 = 0xE4A8;
pub const XC_WRITTEN: u32 = 0xE4A9;

/// 演练矩阵基准核数（8 核 8/8 判据）。
pub const DRILL_CORES: u32 = 8;

/// 单 AP 微码修订采样行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UcodeSample {
    pub ap_id: u32,
    /// 微码修订号（MSR 0x8B 只读采样）。
    pub revision: u32,
    /// 采样签（FNV(revision bytes)，防账面被改）。
    pub sample_sig: u64,
}

impl UcodeSample {
    /// 构造：修订号 0 = 采样失败账（不许进一致断言）。
    pub fn new(ap_id: u32, revision: u32) -> Result<UcodeSample, u16> {
        if revision == 0 {
            return Err(XC_REV_ZERO as u16);
        }
        Ok(UcodeSample { ap_id, revision, sample_sig: fnv64(&revision.to_le_bytes()) })
    }

    /// 账行完整性：签与修订号复算一致（被改 = 断言拒）。
    pub fn sig_ok(&self) -> bool {
        self.sample_sig == fnv64(&self.revision.to_le_bytes())
    }
}

/// FNV-1a 64 位（本模块局部实现，避免跨模块依赖）。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 微码一致性断言账。
pub struct UcodeConsistency {
    samples: Vec<UcodeSample>,
    /// BSP 快照修订号（三方之一）。
    bsp_snapshot_rev: u32,
}

impl UcodeConsistency {
    /// 新建：必须先落 BSP 快照（三方同源锚点）。
    pub fn new(bsp_rev: u32) -> Result<UcodeConsistency, u16> {
        if bsp_rev == 0 {
            return Err(XC_REV_ZERO as u16);
        }
        Ok(UcodeConsistency { samples: Vec::new(), bsp_snapshot_rev: bsp_rev })
    }

    /// 采样落账：同 AP 重复采样拒。
    pub fn sample(&mut self, s: UcodeSample) -> Result<(), u16> {
        if self.samples.iter().any(|e| e.ap_id == s.ap_id) {
            return Err(XC_DUP_AP as u16);
        }
        self.samples.push(s);
        Ok(())
    }

    /// 一致性断言：全 AP 修订号 == BSP 快照号；返回失配 AP 清单
    /// （空清单 = 全对齐）。零修订号采样不进断言（采样失败 ≠ 失配）。
    pub fn assert_all(&self) -> Result<Vec<u32>, u16> {
        if self.samples.is_empty() {
            return Err(XC_BSP_MISSING as u16);
        }
        let mut mismatched = Vec::new();
        for s in self.samples.iter() {
            if !s.sig_ok() {
                return Err(XC_SNAP_MUTATED as u16);
            }
            if s.revision != self.bsp_snapshot_rev {
                mismatched.push(s.ap_id);
            }
        }
        Ok(mismatched)
    }

    /// 对齐率分母/分子（8 核 8/8 口径：含 BSP 的全核数）。
    pub fn aligned_ratio(&self, total_cores: u32) -> Result<(u32, u32), u16> {
        if total_cores == 0 {
            return Err(XC_SAMPLE_OOR as u16);
        }
        let mismatched = self.assert_all()?;
        let aligned = total_cores - mismatched.len() as u32;
        Ok((aligned, total_cores))
    }

    /// 隔离建议清单（只读账面——实际隔离走 F0347，本模块不执行）。
    pub fn iso_suggestions(&self) -> Result<Vec<u32>, u16> {
        self.assert_all()
    }

    /// 三方一致性核：BSP 快照 / 账面 BSP 行 / 断言基准 三处同源。
    pub fn threeway_ok(&self, ledger_bsp_rev: u32) -> Result<(), u16> {
        if ledger_bsp_rev != self.bsp_snapshot_rev {
            return Err(XC_THREEWAY_DRIFT as u16);
        }
        Ok(())
    }

    /// 采样数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }
}

/// 域自检入口：采样/断言/三方/只读 四族。
pub fn run_unx0355_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0355-微码一致");
    // 族一：采样账（8 核 8/8 夹具）。
    let mut uc = match UcodeConsistency::new(0x2B) {
        Ok(u) => u,
        Err(_) => {
            cs.add("账构造", false, "BSP 快照被拒");
            return cs;
        }
    };
    cs.add("零修订快照拒", UcodeConsistency::new(0).is_err(), "采样失败不许当锚");
    let mut all_ok = true;
    for ap in 1..DRILL_CORES {
        match UcodeSample::new(ap, 0x2B) {
            Ok(s) => {
                if uc.sample(s).is_err() {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
    }
    cs.add("8 核采样齐", all_ok && uc.len() == 7, "AP1..7 各一行");
    cs.add("重复采样拒", {
        match UcodeSample::new(1, 0x2B) {
            Ok(s) => uc.sample(s).is_err(),
            Err(_) => false,
        }
    }, "同 AP 二采即拒");
    cs.add("零修订采样拒", UcodeSample::new(9, 0).is_err(), "修订号 0 拒收");

    // 族二：一致性断言（8/8 全对齐）。
    let mis = uc.assert_all();
    cs.add("全对齐零失配", matches!(&mis, Ok(v) if v.is_empty()), "8/8 判据");
    let ratio = uc.aligned_ratio(DRILL_CORES);
    cs.add("对齐率 8/8", matches!(ratio, Ok((8, 8))), "含 BSP 全核数");

    // 族三：失配检出与隔离建议。
    let mut bad = match UcodeConsistency::new(0x2C) {
        Ok(u) => u,
        Err(_) => {
            cs.add("失配账构造", false, "被拒");
            return cs;
        }
    };
    if let Ok(s1) = UcodeSample::new(1, 0x2C) {
        let _ = bad.sample(s1);
    }
    if let Ok(s2) = UcodeSample::new(2, 0x2D) {
        let _ = bad.sample(s2);
    }
    let bmis = bad.assert_all();
    cs.add("失配检出", matches!(&bmis, Ok(v) if v.as_slice() == &[2u32]), "AP2 修订漂移");
    cs.add("隔离建议同源", matches!(bad.iso_suggestions(), Ok(ref v) if v.as_slice() == &[2u32]),
           "只读账面交 F0347");

    // 族四：账行防篡改 + 三方同源。
    cs.add("签复算过", UcodeSample::new(3, 0x2B).map(|s| s.sig_ok()).unwrap_or(false), "FNV 复算一致");
    cs.add("三方同源", uc.threeway_ok(0x2B).is_ok(), "快照=账面=断言基准");
    cs.add("三方漂移检出", uc.threeway_ok(0x99).is_err(), "任两处不等即破");
    cs
}
