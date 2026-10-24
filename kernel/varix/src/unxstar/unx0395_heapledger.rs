//! unx0395_heapledger —— UNX-F0395 堆账对冲总账（峰值/存量）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0395 · 堆账对冲总账 · 目标 220 行
//! 判据原文：UNX-F0395-J1 堆全生命周期对冲总账：每笔分配记
//!           「+量@步」，每笔归还记「-量@步」；任意前缀和
//!           （存量）不得为负、不得超过堆容量（双闸）；峰值
//!           = 前缀和最大值，收官存量 = 峰值 - 累计归还增量；
//!           账平核：收官存量 = Σ分配 - Σ归还（恒等式）；峰值
//!           可重放（同流水同峰值）；流水可序列化（步号 + 增量
//!           词位流 + FNV）。
//! 组件：流水行（步号 + 有符号增量）+ 总账（前缀扫描 + 双闸）+
//!       峰值账（最大值跟踪）+ 对冲核（收官恒等式）+ 序列化。
//! 职责定位：对冲总账是「堆的 ECG 心电图」——每笔分配/归还
//!   都是心跳，前缀和就是心率；前缀和破下限说明归还多于分配
//!   （有人白拿内存），破上限说明分配超容（堆溢出）；峰值是
//!   容量规划的依据：堆尺寸按峰值配，配小了 OOM 配大了浪费。
//! 性能逐项分解：记流水 O(1)；扫描 O(流长)；峰值 O(1)（增量
//!   跟踪）；序列化 O(流长)。
//! 诊断码段：0x20D80（0x20D81~0x20D89 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HT_VERSION: &str = "unx0395_heapledger-v1";

/// 诊断段基址（0x20D80 段 · 本模块独占）。
pub const HT_SEG_BASE: u32 = 0x20D80;
pub const XC_NEG_LIVE: u32 = 0x20D81; // 存量为负（超还）
pub const XC_OVER_CAP: u32 = 0x20D82; // 存量超容
pub const XC_STEP_DUP: u32 = 0x20D83; // 步号重复
pub const XC_STEP_REV: u32 = 0x20D84; // 步号回退
pub const XC_ZERO_DELTA: u32 = 0x20D85; // 零增量流水拒
pub const XC_UNBAL: u32 = 0x20D86; // 收官恒等破
pub const XC_REPLAY: u32 = 0x20D87; // 峰值重放不一致
pub const XC_EMPTY: u32 = 0x20D88; // 空流水扫描拒
pub const XC_MAGIC: u32 = 0x20D89; // 序列化魔数破

/// 有符号增量（i64 承载，正 = 分配，负 = 归还）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowRow {
    /// 步号（严格递增）。
    pub step: u64,
    /// 增量（非零）。
    pub delta: i64,
}

/// 对冲总账。
pub struct HeapLedger {
    cap: u64,
    rows: Vec<FlowRow>,
    /// 当前存量。
    live: i64,
    /// 峰值。
    peak: u64,
    /// 累计分配。
    total_alloc: u64,
    /// 累计归还。
    total_free: u64,
}

impl HeapLedger {
    /// 建账：堆容量。
    pub fn new(cap: u64) -> Result<HeapLedger, u32> {
        if cap == 0 {
            return Err(XC_OVER_CAP);
        }
        Ok(HeapLedger {
            cap,
            rows: Vec::new(),
            live: 0,
            peak: 0,
            total_alloc: 0,
            total_free: 0,
        })
    }

    /// 记流水：步号必须严格递增、增量非零；双闸即时核。
    pub fn record(&mut self, step: u64, delta: i64) -> Result<u64, u32> {
        if delta == 0 {
            return Err(XC_ZERO_DELTA);
        }
        if let Some(last) = self.rows.last() {
            if step == last.step {
                return Err(XC_STEP_DUP);
            }
            if step < last.step {
                return Err(XC_STEP_REV);
            }
        }
        self.live += delta;
        if self.live < 0 {
            self.live -= delta; // 回滚
            return Err(XC_NEG_LIVE);
        }
        if self.live as u64 > self.cap {
            self.live -= delta; // 回滚
            return Err(XC_OVER_CAP);
        }
        if delta > 0 {
            self.total_alloc += delta as u64;
        } else {
            self.total_free += (-delta) as u64;
        }
        let lv = self.live as u64;
        if lv > self.peak {
            self.peak = lv;
        }
        self.rows.push(FlowRow { step, delta });
        Ok(lv)
    }

    /// 收官恒等：存量 = Σ分配 - Σ归还。
    pub fn balance(&self) -> Result<(), u32> {
        if (self.total_alloc - self.total_free) as i64 != self.live {
            return Err(XC_UNBAL);
        }
        Ok(())
    }

    /// 前缀双闸重扫（独立复核路径）：任意前缀和 ∈ [0, cap]。
    pub fn rescan(&self) -> Result<(), u32> {
        if self.rows.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut acc: i64 = 0;
        for r in self.rows.iter() {
            acc += r.delta;
            if acc < 0 {
                return Err(XC_NEG_LIVE);
            }
            if acc as u64 > self.cap {
                return Err(XC_OVER_CAP);
            }
        }
        Ok(())
    }

    /// 峰值。
    pub fn peak(&self) -> u64 {
        self.peak
    }

    /// 收官存量。
    pub fn live(&self) -> i64 {
        self.live
    }

    /// 流水长度。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 序列化：魔数 + 容量 + 流长 + 各行词位 + FNV32。
    pub fn digest(&self) -> (Vec<u32>, u32) {
        let mut words: Vec<u32> = Vec::new();
        words.push(0x1ED5_0001u32); // 魔数：堆总账 v1
        words.push(self.cap as u32);
        words.push((self.cap >> 32) as u32);
        words.push(self.rows.len() as u32);
        for r in self.rows.iter() {
            words.push(r.step as u32);
            words.push((r.step >> 32) as u32);
            words.push(r.delta as u32);
            words.push((r.delta >> 32) as u32);
        }
        let mut h: u32 = 0x811C_9DC5;
        for &w in words.iter() {
            h ^= w;
            h = h.wrapping_mul(0x0100_0193);
        }
        words.push(h);
        (words, h)
    }

    /// 峰值重放：从流水独立重算峰值。
    pub fn replay_peak(&self) -> Result<u64, u32> {
        if self.rows.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut acc: i64 = 0;
        let mut pk: u64 = 0;
        for r in self.rows.iter() {
            acc += r.delta;
            let a = if acc < 0 { 0u64 } else { acc as u64 };
            if a > pk {
                pk = a;
            }
        }
        if pk != self.peak {
            return Err(XC_REPLAY);
        }
        Ok(pk)
    }
}

pub fn run_unx0395_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0395_heapledger");
    // 族一：标准分配-归还流水。
    let mut l = match HeapLedger::new(1000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账", false, "拒");
            return cs;
        }
    };
    cs.add("步 1 +400", l.record(1, 400) == Ok(400), "存量 400");
    cs.add("步 2 +300", l.record(2, 300) == Ok(700), "存量 700");
    cs.add("步 3 -200", l.record(3, -200) == Ok(500), "存量 500");
    cs.add("步 4 +100", l.record(4, 100) == Ok(600), "存量 600");
    cs.add("步 5 -600", l.record(5, -600) == Ok(0), "清零");
    cs.add("峰值 700", l.peak() == 700, "前缀最大");
    cs.add("收官存量 0", l.live() == 0, "全对冲");
    cs.add("恒等式平", l.balance().is_ok(), "1000-1000=0");
    cs.add("重扫双闸绿", l.rescan().is_ok(), "全前缀合法");
    cs.add("峰值重放合", l.replay_peak() == Ok(700), "独立重算");
    // 族二：超容闸。
    let mut o = match HeapLedger::new(100) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账2", false, "拒");
            return cs;
        }
    };
    let _ = o.record(1, 80);
    cs.add("超容拒", o.record(2, 30) == Err(XC_OVER_CAP), "110 > 100");
    cs.add("拒绝后存量 80", o.live() == 80, "回滚生效");
    cs.add("回滚后可继续", o.record(2, 20) == Ok(100), "贴容成功");
    cs.add("贴容峰值 100", o.peak() == 100, "顶格");
    // 族三：超还闸。
    let mut n = match HeapLedger::new(100) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账3", false, "拒");
            return cs;
        }
    };
    let _ = n.record(1, 50);
    cs.add("超还拒", n.record(2, -60) == Err(XC_NEG_LIVE), "-10 < 0");
    cs.add("超还后存量 50", n.live() == 50, "回滚生效");
    // 族四：步号纪律。
    cs.add("零增量拒", n.record(3, 0) == Err(XC_ZERO_DELTA), "delta=0");
    cs.add("步号重复拒", n.record(2, 10) == Err(XC_STEP_DUP), "步 2 已在");
    cs.add("步号回退拒", n.record(1, 10) == Err(XC_STEP_REV), "步 1 < 2");
    // 族五：容量零拒。
    cs.add("零容量拒", matches!(HeapLedger::new(0), Err(XC_OVER_CAP)), "cap=0");
    // 族六：空流水重放拒。
    let e = match HeapLedger::new(64) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账4", false, "拒");
            return cs;
        }
    };
    cs.add("空扫拒", e.rescan() == Err(XC_EMPTY), "无流水");
    cs.add("空重放拒", matches!(e.replay_peak(), Err(XC_EMPTY)), "无流水");
    // 族七：峰值重放不一致检出。
    let mut m = match HeapLedger::new(500) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账5", false, "拒");
            return cs;
        }
    };
    let _ = m.record(1, 300);
    let _ = m.record(2, 100);
    cs.add("峰值 400", m.peak() == 400, "300 → 400");
    cs.add("重放 400", m.replay_peak() == Ok(400), "一致");
    cs.add("流水长 2", m.len() == 2, "两行");
    cs
}
