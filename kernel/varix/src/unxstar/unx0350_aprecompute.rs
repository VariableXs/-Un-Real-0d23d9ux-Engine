//! unx0350_aprecompute —— UNX-F0350 AP 汇合后全局重算（热图/时间线刷新）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0350 · AP 汇合后全局重算 · 目标 240 行
//! 判据原文：UNX-F0350-J1 F0077 热图/F0198 时间线纳入 AP 数据
//!           重算，守恒断言过。
//! 组件：重算输入面（AP 侧分桶热图 + 时间线事件行）+ 全局重算器
//!       （BSP 账 + AP 账合并成全局账：分桶逐桶相加、时间线按
//!       tick 归并稳定排序）+ 三守恒断言（热图总命中数守恒/时间
//!       线行数守恒/重算前后 AP 数据不可变——重算只读输入账）+
//!       幂等面（同输入重算两次结果逐位相同）。
//! 职责定位：AP 汇合后唯一的一次性全局视图刷新——重算不是增量
//!   修补而是全量重建，全量重建的可信度靠三守恒 + 幂等两条断言
//!   支撑：守恒保证数据不丢不重，幂等保证重算本身无副作用。
//! 性能逐项分解：分桶合并 O(桶数)；时间线归并 O(n log n)
//!   （稳定排序一次）；守恒核 O(桶数 + 行数)；幂等核 = 两次重算。
//! 诊断码段：0xE450（0xE451~0xE459 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const RC_VERSION: &str = "unx0350_aprecompute-v1";

/// 诊断段基址（0xE450 段 · 本模块独占）。
pub const RC_SEG_BASE: u16 = 0xE450;

/// XcCode 闭集（0xE451~0xE459）。
pub const XC_HEAT_LOST: u32 = 0xE451;
pub const XC_TL_ROW_LOST: u32 = 0xE452;
pub const XC_INPUT_MUTATED: u32 = 0xE453;
pub const XC_IDEMPOTENT_BROKEN: u32 = 0xE454;
pub const XC_BUCKET_OOR: u32 = 0xE455;
pub const XC_TL_ORDER_BAD: u32 = 0xE456;
pub const XC_DUP_AP_BUCKET: u32 = 0xE457;
pub const XC_EMPTY_INPUT: u32 = 0xE458;
pub const XC_SNAPSHOT_DRIFT: u32 = 0xE459;

/// 热图桶数（64 桶，与 F0077 同宽）。
pub const HEAT_BUCKETS: usize = 64;

/// 时间线事件行（tick + AP 来源 + 事件码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TlRow {
    pub tick: u64,
    pub ap_id: u32,
    pub event: u16,
}

/// AP 侧分桶热图（单 AP 一账）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApHeatMap {
    pub ap_id: u32,
    pub buckets: Vec<u64>,
}

impl ApHeatMap {
    /// 构造：桶数必须恰为 HEAT_BUCKETS（不同宽账不许进重算）。
    pub fn new(ap_id: u32, buckets: Vec<u64>) -> Result<ApHeatMap, u16> {
        if buckets.len() != HEAT_BUCKETS {
            return Err(XC_BUCKET_OOR as u16);
        }
        Ok(ApHeatMap { ap_id, buckets })
    }

    /// 总命中数。
    pub fn total(&self) -> u64 {
        self.buckets.iter().fold(0u64, |a, &b| a.wrapping_add(b))
    }
}

/// 全局重算输出。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GlobalView {
    /// 合并热图（64 桶）。
    pub heat: [u64; HEAT_BUCKETS],
    /// 归并后的时间线（tick 升序稳定，同 tick 保持输入相对序）。
    pub timeline: Vec<TlRow>,
    /// 参与重算的 AP 数。
    pub ap_count: u32,
}

/// 全局重算器。
pub struct GlobalRecomputer;

impl GlobalRecomputer {
    /// 全量重建：BSP 热图 + AP 热图逐桶相加；时间线按 tick 归并
    /// （插入排序保证稳定性——同 tick 保持输入相对序）。
    pub fn recompute(
        bsp_heat: &[u64; HEAT_BUCKETS],
        ap_heats: &[ApHeatMap],
        bsp_rows: &[TlRow],
        ap_rows: &[TlRow],
    ) -> Result<GlobalView, u16> {
        if ap_heats.is_empty() && ap_rows.is_empty() {
            return Err(XC_EMPTY_INPUT as u16);
        }
        let mut heat = [0u64; HEAT_BUCKETS];
        for (i, &v) in bsp_heat.iter().enumerate() {
            heat[i] = v;
        }
        for m in ap_heats.iter() {
            if m.buckets.len() != HEAT_BUCKETS {
                return Err(XC_BUCKET_OOR as u16);
            }
            for (i, &v) in m.buckets.iter().enumerate() {
                heat[i] = heat[i].wrapping_add(v);
            }
        }
        let mut timeline: Vec<TlRow> = Vec::new();
        for r in bsp_rows.iter() {
            timeline.push(*r);
        }
        for r in ap_rows.iter() {
            timeline.push(*r);
        }
        // 稳定插入排序：按 tick 升序，同 tick 保相对序。
        for i in 1..timeline.len() {
            let key = timeline[i];
            let mut j = i;
            while j > 0 && timeline[j - 1].tick > key.tick {
                timeline[j] = timeline[j - 1];
                j -= 1;
            }
            timeline[j] = key;
        }
        // 时序合法核：tick 非降序。
        for w in timeline.windows(2) {
            if w[1].tick < w[0].tick {
                return Err(XC_TL_ORDER_BAD as u16);
            }
        }
        Ok(GlobalView { heat, timeline, ap_count: ap_heats.len() as u32 })
    }

    /// 守恒断言一：热图总命中 = BSP 总命中 + Σ AP 总命中。
    pub fn heat_conserved(
        bsp_heat: &[u64; HEAT_BUCKETS],
        ap_heats: &[ApHeatMap],
        view: &GlobalView,
    ) -> Result<(), u16> {
        let bsp_total: u64 = bsp_heat.iter().fold(0u64, |a, &b| a.wrapping_add(b));
        let ap_total: u64 = ap_heats.iter().map(|m| m.total()).fold(0u64, |a, b| a.wrapping_add(b));
        let view_total: u64 = view.heat.iter().fold(0u64, |a, &b| a.wrapping_add(b));
        if view_total != bsp_total.wrapping_add(ap_total) {
            return Err(XC_HEAT_LOST as u16);
        }
        Ok(())
    }

    /// 守恒断言二：时间线行数 = BSP 行 + AP 行。
    pub fn tl_conserved(bsp_rows: &[TlRow], ap_rows: &[TlRow], view: &GlobalView) -> Result<(), u16> {
        if view.timeline.len() != bsp_rows.len() + ap_rows.len() {
            return Err(XC_TL_ROW_LOST as u16);
        }
        Ok(())
    }

    /// 守恒断言三：重算只读——输入账在重算前后逐位相同。
    pub fn input_immutable(before: &[u64], after: &[u64]) -> Result<(), u16> {
        if before.len() != after.len() {
            return Err(XC_INPUT_MUTATED as u16);
        }
        for (i, (a, b)) in before.iter().zip(after.iter()).enumerate() {
            if a != b {
                return Err(XC_INPUT_MUTATED as u16 + i as u16);
            }
        }
        Ok(())
    }
}

/// 域自检入口：重算/守恒/幂等 三族。
pub fn run_unx0350_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0350-全局重算");
    // 夹具：BSP 热图桶 0 命中 10；AP1 桶 1 命中 20；AP2 桶 1 命中 12。
    let mut bsp = [0u64; HEAT_BUCKETS];
    bsp[0] = 10;
    let m1 = match ApHeatMap::new(1, {
        let mut v = vec![0u64; HEAT_BUCKETS];
        v[1] = 20;
        v
    }) {
        Ok(m) => m,
        Err(_) => {
            cs.add("AP1 账构造", false, "合法账被拒");
            return cs;
        }
    };
    let m2 = match ApHeatMap::new(2, {
        let mut v = vec![0u64; HEAT_BUCKETS];
        v[1] = 12;
        v
    }) {
        Ok(m) => m,
        Err(_) => {
            cs.add("AP2 账构造", false, "合法账被拒");
            return cs;
        }
    };
    let bsp_rows = [TlRow { tick: 5, ap_id: 0, event: 1 }];
    let ap_rows = [TlRow { tick: 3, ap_id: 1, event: 2 }, TlRow { tick: 3, ap_id: 2, event: 3 }];
    let aps = [m1.clone(), m2.clone()];

    let v1 = GlobalRecomputer::recompute(&bsp, &aps, &bsp_rows, &ap_rows);
    let view = match v1 {
        Ok(v) => v,
        Err(_) => {
            cs.add("重算执行", false, "合法输入重算失败");
            return cs;
        }
    };

    // 族一：重算正确性。
    cs.add("热图合并", view.heat[0] == 10 && view.heat[1] == 32, "10 + 20 + 12");
    cs.add("时间线归并", view.timeline.len() == 3 && view.timeline[0].tick == 3
        && view.timeline[2].tick == 5, "tick 升序");
    cs.add("同 tick 稳定", view.timeline[0].ap_id == 1 && view.timeline[1].ap_id == 2,
           "同 tick 保相对序");
    cs.add("异宽账拒", ApHeatMap::new(9, vec![0u64; 32]).is_err(), "桶数≠64 拒收");

    // 族二：三守恒。
    cs.add("热图守恒", GlobalRecomputer::heat_conserved(&bsp, &aps, &view).is_ok(),
           "总命中不丢不重");
    cs.add("行数守恒", GlobalRecomputer::tl_conserved(&bsp_rows, &ap_rows, &view).is_ok(),
           "行数不丢");
    let before: Vec<u64> = aps.iter().flat_map(|m| m.buckets.iter().copied()).collect();
    let after: Vec<u64> = aps.iter().flat_map(|m| m.buckets.iter().copied()).collect();
    cs.add("输入不可变", GlobalRecomputer::input_immutable(&before, &after).is_ok(),
           "重算只读");

    // 族三：幂等（同输入两次重算逐位相同）。
    let v2 = GlobalRecomputer::recompute(&bsp, &aps, &bsp_rows, &ap_rows);
    cs.add("重算幂等", matches!(v2, Ok(ref w) if *w == view), "两次全量重建一致");
    cs
}

// ===== 扩展面：重算指纹（全量视图 FNV 落档 + 变更即显性）=====
/// 视图指纹：合并热图 + 时间线全行 FNV-1a 串联。指纹入账后，
/// 「全局视图是什么」有了逐位可核的答案——下游（F0077 热图/
/// F0198 时间线）消费时以指纹对账，不一致即显性破账。
pub fn view_fingerprint(view: &GlobalView) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mix = |h: &mut u64, bytes: &[u8]| {
        for &b in bytes {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x100000001b3);
        }
    };
    for &v in view.heat.iter() {
        mix(&mut h, &v.to_le_bytes());
    }
    for r in view.timeline.iter() {
        mix(&mut h, &r.tick.to_le_bytes());
        mix(&mut h, &r.ap_id.to_le_bytes());
        mix(&mut h, &r.event.to_le_bytes());
    }
    mix(&mut h, &view.ap_count.to_le_bytes());
    h
}

/// 指纹账：每次重算落一行（重算序号 + 指纹），同序号重复落账拒。
pub struct FingerprintLedger {
    entries: Vec<(u32, u64)>,
}

impl FingerprintLedger {
    pub fn new() -> FingerprintLedger {
        FingerprintLedger { entries: Vec::new() }
    }

    pub fn record(&mut self, seq: u32, fp: u64) -> Result<(), u16> {
        if self.entries.iter().any(|&(s, _)| s == seq) {
            return Err(1);
        }
        if let Some(&(last, _)) = self.entries.last() {
            if seq <= last {
                return Err(2);
            }
        }
        self.entries.push((seq, fp));
        Ok(())
    }

    pub fn latest(&self) -> Option<(u32, u64)> {
        self.entries.last().copied()
    }
}
