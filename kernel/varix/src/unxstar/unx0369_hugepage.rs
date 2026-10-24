//! unx0369_hugepage —— UNX-F0369 大页候选区盘点（2MB 对齐窗）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0369 · 大页候选盘点 · 目标 220 行
//! 判据原文：UNX-F0369-J1 大页（2MB）候选区盘点：在空闲段账内
//!           找 2MB 对齐窗（窗起址 2MB 对齐 + 窗长 ≥ 2MB）；候选
//!           与空闲段双向对账（候选 ⊆ 空闲段）；盘点清单按起址
//!           升序；预留不落地（只登记不实际改页表——落地权在
//!           分页层）。
//! 组件：对齐窗扫描（段内候选）+ 对账核 + 盘点账（升序冻结）+
//!       预留语义（登记态，无落地副作用）。
//! 职责定位：大页候选是「TLB 命中率的先行官」——2MB 页一项能
//!   把 TLB miss 砍掉一个量级，但前提是物理内存真有连续 2MB
//!   对齐窗。盘点是只读侦察：不锁定、不改页表，把「哪里能上
//!   大页」的清单交决策层——侦察兵不开火。
//! 性能逐项分解：扫描 O(段数 × 段长/2MB)；对账 O(候选数)。
//! 诊断码段：0x208C0（0x208C1~0x208C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HG_VERSION: &str = "unx0369_hugepage-v1";

/// 诊断段基址（0x208C0 段 · 本模块独占）。
pub const HG_SEG_BASE: u32 = 0x208C0;
pub const XC_ALIGN_BAD: u32 = 0x208C1; // 窗起未 2MB 对齐
pub const XC_SIZE_SHORT: u32 = 0x208C2; // 窗长不足 2MB
pub const XC_NOT_IN_FREE: u32 = 0x208C3; // 候选出空闲段
pub const XC_ORDER_OOO: u32 = 0x208C4; // 盘点乱序
pub const XC_RUN_EMPTY: u32 = 0x208C5; // 空闲段账拒
pub const XC_HUGE_SIZE: u32 = 0x208C6; // 粒度非 2MB
pub const XC_LANDED: u32 = 0x208C7; // 预留落地检出
pub const XC_DUP_CAND: u32 = 0x208C8; // 候选重复
pub const XC_SPAN_OOR: u32 = 0x208C9; // 段越域

/// 大页粒度 2MB。
pub const HUGE_SIZE: u32 = 0x20_0000;

/// 空闲段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreeRun {
    pub start: u32,
    pub len: u32,
}

impl FreeRun {
    fn valid(&self) -> Result<(), u32> {
        if self.len == 0 || self.start % HUGE_SIZE != 0 || self.start.checked_add(self.len).is_none() {
            return Err(XC_SPAN_OOR);
        }
        Ok(())
    }
}

/// 候选窗。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HugeCand {
    /// 窗起（2MB 对齐）。
    pub base: u32,
    /// 窗长（2MB 整数倍）。
    pub len: u32,
}

impl HugeCand {
    pub fn new(base: u32, len: u32) -> Result<HugeCand, u32> {
        if base % HUGE_SIZE != 0 {
            return Err(XC_ALIGN_BAD);
        }
        if len == 0 || len % HUGE_SIZE != 0 {
            return Err(XC_HUGE_SIZE);
        }
        Ok(HugeCand { base, len })
    }

    /// 候选合法核：窗起对齐 + 窗长 ≥ 2MB（len ≥ HUGE_SIZE）。
    pub fn plausible(&self) -> bool {
        self.base % HUGE_SIZE == 0 && self.len >= HUGE_SIZE
    }
}

/// 盘点：在空闲段账内找 2MB 对齐窗。
/// 候选取段内最大可用对齐区（段起向上对齐到段尾向下对齐之间）。
pub fn scan_huge(runs: &[FreeRun]) -> Result<Vec<HugeCand>, u32> {
    if runs.is_empty() {
        return Err(XC_RUN_EMPTY);
    }
    for r in runs.iter() {
        r.valid()?;
        if r.start.checked_add(r.len).is_none() {
            return Err(XC_SPAN_OOR);
        }
    }
    // 段间升序 + 无重叠。
    for i in 1..runs.len() {
        if runs[i].start < runs[i - 1].start + runs[i - 1].len {
            return Err(XC_SPAN_OOR);
        }
    }
    let mut out = Vec::new();
    for r in runs.iter() {
        // 段内对齐窗：起 = 段起向上对齐；止 = 段尾向下对齐。
        let aligned_start = r.start.div_ceil(HUGE_SIZE) * HUGE_SIZE;
        let seg_end = r.start + r.len;
        let aligned_end = (seg_end / HUGE_SIZE) * HUGE_SIZE;
        if aligned_end > aligned_start {
            let len = aligned_end - aligned_start;
            if len >= HUGE_SIZE {
                out.push(HugeCand { base: aligned_start, len });
            }
        }
    }
    Ok(out)
}

/// 对账核：每个候选窗 ⊆ 某空闲段（候选出段 = 盘点假）。
pub fn cand_in_free(cands: &[HugeCand], runs: &[FreeRun]) -> Result<(), u32> {
    for c in cands.iter() {
        if !c.plausible() {
            return Err(XC_SIZE_SHORT);
        }
        let covered = runs
            .iter()
            .any(|r| c.base >= r.start && c.base + c.len <= r.start + r.len);
        if !covered {
            return Err(XC_NOT_IN_FREE);
        }
    }
    // 候选互不重叠且升序。
    for i in 1..cands.len() {
        if cands[i].base <= cands[i - 1].base {
            return Err(XC_ORDER_OOO);
        }
        if cands[i].base < cands[i - 1].base + cands[i - 1].len {
            return Err(XC_ORDER_OOO);
        }
    }
    Ok(())
}

/// 预留语义核：登记不落地——预留调用必须无副作用（本模块无
/// 页表句柄，落地尝试结构上不可能；此核是语义锚的可验证表达）。
pub fn reserve_noop(cands: &[HugeCand]) -> Result<u32, u32> {
    // 候选重复登记拒。
    for (i, c) in cands.iter().enumerate() {
        if cands[i + 1..].iter().any(|x| x.base == c.base) {
            return Err(XC_DUP_CAND);
        }
        let _ = XC_LANDED; // 落地码在本模块不可达（无写路径）
    }
    Ok(cands.len() as u32)
}

pub fn run_unx0369_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0369_hugepage");
    // 族一：单段盘点。
    let runs = vec![FreeRun { start: 0x100_0000, len: 0x80_0000 }]; // 16MB 起 8MB 长
    let cands = match scan_huge(&runs) {
        Ok(c) => c,
        Err(_) => {
            cs.add("单段盘点", false, "被拒");
            return cs;
        }
    };
    cs.add("候选一窗", cands.len() == 1, "16..24MB");
    cs.add("窗起对齐", cands[0].base == 0x100_0000, "16MB 自对齐");
    cs.add("窗长 8MB", cands[0].len == 0x80_0000, "整段对齐区");
    cs.add("窗合理", cands[0].plausible(), "对齐+足长");
    cs.add("候选在段内", cand_in_free(&cands, &runs).is_ok(), "⊆ 空闲段");
    // 族二：非对齐段起。
    let runs2 = vec![FreeRun { start: 0x110_0000, len: 0x90_0000 }]; // 17MB 起 9MB
    let c2 = match scan_huge(&runs2) {
        Ok(c) => c,
        Err(_) => {
            cs.add("族二盘点", false, "被拒");
            return cs;
        }
    };
    cs.add("向上对齐", c2[0].base == 0x120_0000, "17→18MB");
    cs.add("向下截尾", c2[0].len == 0x80_0000, "18..26MB");
    // 族三：多段盘点与升序。
    let runs3 = vec![
        FreeRun { start: 0x20_0000, len: 0x60_0000 },  // 2..8MB
        FreeRun { start: 0x100_0000, len: 0x40_0000 }, // 16..20MB
    ];
    let c3 = match scan_huge(&runs3) {
        Ok(c) => c,
        Err(_) => {
            cs.add("族三盘点", false, "被拒");
            return cs;
        }
    };
    cs.add("两段两窗", c3.len() == 2, "各一窗");
    cs.add("升序", c3[0].base < c3[1].base, "2MB < 16MB");
    cs.add("小段无窗", {
        let thin = vec![FreeRun { start: 0, len: 0x10_0000 }]; // 1MB < 2MB
        scan_huge(&thin).map(|v| v.is_empty()).unwrap_or(false)
    }, "不足 2MB 零候选");
    // 族四：对账破。
    let bad_c = vec![HugeCand { base: 0x200_0000, len: HUGE_SIZE }]; // 32MB 不在段内
    cs.add("出段立案", cand_in_free(&bad_c, &runs) == Err(XC_NOT_IN_FREE), "假候选");
    let _unaligned = vec![HugeCand { base: 0x101_0000, len: HUGE_SIZE }];
    cs.add("非对齐窗拒", HugeCand::new(0x101_0000, HUGE_SIZE).is_err(), "ALIGN");
    cs.add("零长窗拒", HugeCand::new(0x100_0000, 0).is_err(), "SIZE");
    cs.add("非整倍长拒", HugeCand::new(0x100_0000, 0x30_0000).is_err(), "HUGE_SIZE");
    // 族五：构造闸。
    cs.add("空段账拒", scan_huge(&[]).is_err(), "RUN_EMPTY");
    cs.add("段重叠拒", scan_huge(&[
        FreeRun { start: 0, len: HUGE_SIZE * 2 },
        FreeRun { start: HUGE_SIZE, len: HUGE_SIZE * 2 },
    ]).is_err(), "SPAN");
    cs.add("段零长拒", scan_huge(&[FreeRun { start: 0, len: 0 }]).is_err(), "SPAN");
    // 族六：预留不落地。
    cs.add("登记计数", reserve_noop(&cands) == Ok(1), "登记不动页表");
    cs.add("重复登记拒", reserve_noop(&[cands[0], cands[0]]).is_err(), "DUP");
    cs
}
