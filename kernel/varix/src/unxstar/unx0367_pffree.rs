//! unx0367_pffree —— UNX-F0367 页帧释放与合并账（位图清位）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0367 · 页帧释放合并 · 目标 220 行
//! 判据原文：UNX-F0367-J1 释放 n 帧时与邻接空闲区合并：左邻空/
//!           右邻空/双侧空/孤立四种形态各成账；合并后空闲段
//!           计数下降、最大空闲段长度上升（合并收益可量化）；
//!           释放未持有帧拒（双释放防线）。
//! 组件：空闲段账（起 + 长，升序）+ 释放合并引擎（四形态）+
//!       收益核（段数/最大段）+ 持有权核。
//! 职责定位：合并是「抗碎片的核心动作」——不合并的释放会让
//!   空闲内存变成 Swiss cheese：总量够、连续段不够，大分配
//!   全卡死。四形态显式分支让每种合并路径都可单测，收益核
//!   把「合并有没有用」变成可测数字。
//! 性能逐项分解：释放 O(段数)；收益核 O(段数)；持有核 O(账深)。
//! 诊断码段：0x20840（0x20841~0x20849 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PF_VERSION: &str = "unx0367_pffree-v1";

/// 诊断段基址（0x20840 段 · 本模块独占）。
pub const PF_SEG_BASE: u32 = 0x20840;
pub const XC_DOUBLE_FREE: u32 = 0x20841; // 双释放
pub const XC_NOT_OWNED: u32 = 0x20842; // 释放未持有帧
pub const XC_ZERO_FRAMES: u32 = 0x20843; // 零帧释放
pub const XC_SPAN_OOR: u32 = 0x20844; // 释放区间越池
pub const XC_MERGE_BAD: u32 = 0x20845; // 合并结果破
pub const XC_ORDER_OOO: u32 = 0x20846; // 空闲段乱序
pub const XC_OVERLAP: u32 = 0x20847; // 空闲段重叠
pub const XC_GAIN_NEG: u32 = 0x20848; // 合并负收益
pub const XC_OWNER_ZERO: u32 = 0x20849; // 零持有方

/// 池帧数（判据规模）。
pub const POOL_FRAMES: u32 = 128;

/// 空闲段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreeRun {
    pub start: u32,
    pub len: u32,
}

/// 页帧释放合并器（空闲段账视角，与位图互为镜像）。
pub struct PfFree {
    pool_frames: u32,
    /// 持有权账：帧号 → 持有方（0 = 空闲）。
    owner: Vec<u32>,
    free_runs: Vec<FreeRun>,
}

impl PfFree {
    pub fn new(pool_frames: u32) -> Result<PfFree, u32> {
        if pool_frames == 0 || pool_frames > 4096 {
            return Err(XC_SPAN_OOR);
        }
        Ok(PfFree { pool_frames, owner: vec![0u32; pool_frames as usize], free_runs: vec![FreeRun { start: 0, len: pool_frames }] })
    }

    /// 池帧数。
    pub fn capacity(&self) -> u32 {
        self.pool_frames
    }

    /// 空闲段账。
    pub fn runs(&self) -> &[FreeRun] {
        &self.free_runs
    }

    /// 空闲段数。
    pub fn run_count(&self) -> u32 {
        self.free_runs.len() as u32
    }

    /// 最大空闲段。
    pub fn max_run(&self) -> u32 {
        self.free_runs.iter().map(|r| r.len).max().unwrap_or(0)
    }

    /// 分配（测试辅助）：从首段切 n 帧给 owner（持有权入账）。
    pub fn take(&mut self, owner: u32, n: u32) -> Result<u32, u32> {
        if owner == 0 {
            return Err(XC_OWNER_ZERO);
        }
        let idx = match self.free_runs.iter().position(|r| r.len >= n) {
            Some(i) => i,
            None => return Err(XC_SPAN_OOR),
        };
        let start = self.free_runs[idx].start;
        for f in start..start + n {
            self.owner[f as usize] = owner;
        }
        if self.free_runs[idx].len == n {
            self.free_runs.remove(idx);
        } else {
            self.free_runs[idx] = FreeRun { start: start + n, len: self.free_runs[idx].len - n };
        }
        Ok(start)
    }

    /// 释放 + 合并：四形态显式分支。
    pub fn release(&mut self, owner: u32, start: u32, n: u32) -> Result<(), u32> {
        if n == 0 {
            return Err(XC_ZERO_FRAMES);
        }
        if owner == 0 {
            return Err(XC_OWNER_ZERO);
        }
        if start >= self.pool_frames || start + n > self.pool_frames {
            return Err(XC_SPAN_OOR);
        }
        // 持有权核：整段必须属于 owner 且当前空闲段账不含（双释放防线）。
        for f in start..start + n {
            if self.owner[f as usize] != owner {
                return Err(XC_NOT_OWNED);
            }
        }
        // 双释放核：释放区间与现有空闲段不许相交。
        for r in self.free_runs.iter() {
            let r_end = r.start + r.len;
            let s_end = start + n;
            if start < r_end && r.start < s_end {
                return Err(XC_DOUBLE_FREE);
            }
        }
        // 清持有权。
        for f in start..start + n {
            self.owner[f as usize] = 0;
        }
        // 邻接判定。
        let left_free = start > 0 && self.owner[(start - 1) as usize] == 0;
        let right_free = start + n < self.pool_frames && self.owner[(start + n) as usize] == 0;
        let s_end = start + n;
        let merged = match (left_free, right_free) {
            (false, false) => {
                // 孤立：新段插入。
                let mut runs = Vec::new();
                let mut inserted = false;
                for r in self.free_runs.iter() {
                    if !inserted && r.start > start {
                        runs.push(FreeRun { start, len: n });
                        inserted = true;
                    }
                    runs.push(*r);
                }
                if !inserted {
                    runs.push(FreeRun { start, len: n });
                }
                self.free_runs = runs;
            }
            (true, false) => {
                // 左邻合并：左段扩展。
                if let Some(i) = self.free_runs.iter().position(|r| r.start + r.len == start) {
                    let l = self.free_runs[i].len;
                    self.free_runs[i] = FreeRun { start: self.free_runs[i].start, len: l + n };
                }
            }
            (false, true) => {
                // 右邻合并：右段前移。
                if let Some(i) = self.free_runs.iter().position(|r| r.start == s_end) {
                    let r_len = self.free_runs[i].len;
                    self.free_runs[i] = FreeRun { start, len: n + r_len };
                }
            }
            (true, true) => {
                // 双侧合并：三段合一。
                let li = self.free_runs.iter().position(|r| r.start + r.len == start);
                let ri = self.free_runs.iter().position(|r| r.start == s_end);
                if let (Some(li), Some(ri)) = (li, ri) {
                    let l = self.free_runs[li];
                    let r = self.free_runs[ri];
                    let total = l.len + n + r.len;
                    // 先移大的索引再移小的，防止索引漂移。
                    self.free_runs.remove(ri.max(li));
                    self.free_runs.remove(ri.min(li));
                    self.free_runs.push(FreeRun { start: l.start, len: total });
                    self.free_runs.sort_unstable_by_key(|x| x.start);
                }
            }
        };
        let _ = merged;
        // 合并后账自检：升序 + 无重叠。
        for (i, r) in self.free_runs.iter().enumerate() {
            if i > 0 {
                let prev = &self.free_runs[i - 1];
                if r.start < prev.start + prev.len {
                    return Err(XC_MERGE_BAD);
                }
                if r.start <= prev.start {
                    return Err(XC_ORDER_OOO);
                }
            }
        }
        Ok(())
    }

    /// 合并收益核：段数下降或最大段上升（至少一项改善）。
    pub fn gain_ok(before_runs: u32, before_max: u32, after_runs: u32, after_max: u32) -> Result<(), u32> {
        if after_runs <= before_runs && after_max >= before_max && (after_runs < before_runs || after_max > before_max) {
            Ok(())
        } else {
            Err(XC_GAIN_NEG)
        }
    }
}

pub fn run_unx0367_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0367_pffree");
    // 族一：分配铺开（制造四形态素材）。
    let mut pf = match PfFree::new(POOL_FRAMES) {
        Ok(p) => p,
        Err(_) => {
            cs.add("构建", false, "被拒");
            return cs;
        }
    };
    cs.add("初态单段", pf.run_count() == 1 && pf.max_run() == 128, "全池一段");
    // 占 0..8, 8..16, 16..24（三笔相邻分配，后段 24..128 空闲）。
    let _ = pf.take(1, 8);
    let _ = pf.take(1, 8);
    let _ = pf.take(1, 8);
    cs.add("占 24 帧", pf.max_run() == 104, "128-24");
    // 释放 8..16（孤立：左 7 被占、右 16 被占）。
    let r1 = pf.release(1, 8, 8);
    cs.add("孤立释放", r1.is_ok(), "新段插入");
    cs.add("孤立段数 2", pf.run_count() == 2, "(8,8)+(24,104)");
    // 释放 0..8（右邻空：与 (8,8) 合并 → (0,16)；段数不变但前段扩展）。
    let _ = pf.release(1, 0, 8);
    cs.add("右邻合并", pf.run_count() == 2 && pf.runs()[0] == FreeRun { start: 0, len: 16 }, "前段 0..16");
    // 释放 16..24（双侧空：左 (0,16) + 右 (24,104) 三合一全池）。
    let _ = pf.release(1, 16, 8);
    cs.add("双侧合并全池", pf.run_count() == 1 && pf.max_run() == 128, "三合一");
    // 族二：合并收益核。
    cs.add("收益核过", PfFree::gain_ok(3, 8, 1, 24).is_ok(), "段降+段升");
    cs.add("无收益立案", PfFree::gain_ok(2, 8, 2, 8) == Err(XC_GAIN_NEG), "无改善");
    cs.add("段升但数升", PfFree::gain_ok(2, 8, 3, 16) == Err(XC_GAIN_NEG), "段数恶化");
    // 族三：防线。
    let mut pf2 = PfFree::new(32).unwrap_or(PfFree { pool_frames: 32, owner: vec![0; 32], free_runs: vec![FreeRun { start: 0, len: 32 }] });
    let _ = pf2.take(2, 4);
    cs.add("双释放拒", {
        let _ = pf2.release(2, 0, 2);
        pf2.release(2, 0, 2).is_err()
    }, "已入空闲账");
    cs.add("非持有方拒", pf2.release(9, 2, 2).is_err(), "帧 2 归 2 号");
    cs.add("零帧拒", pf2.release(2, 4, 0).is_err(), "ZERO");
    cs.add("越池拒", pf2.release(2, 30, 4).is_err(), "SPAN");
    cs.add("零方拒", pf2.release(0, 4, 1).is_err(), "OWNER_ZERO");
    // 族四：持有权镜像。
    cs.add("占用可见", pf2.owner[0] == 0 && pf2.owner[1] == 0, "已释放清零");
    cs.add("空闲段自检", {
        // 账内所有段满足升序无重叠。
        let rs = pf2.runs();
        rs.windows(2).all(|w| w[1].start >= w[0].start + w[0].len)
    }, "MERGE_BAD 防线");
    cs
}
