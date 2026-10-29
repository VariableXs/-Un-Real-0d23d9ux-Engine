// unx1236_u1236 —— UNX-F1236 Draining 态边界条件账（空排空/风暴中切换）
// 判据原文 UNX-F1236-J1：边界账全过（空排空/风暴中切换两类逐例登记带证据
// 与建议、分类计数与总数守恒、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb930 独占（0xb930~0xb96f）。
//
// 设计（纯功能 no_std）：
//   * 空排空：开窗零事件（证据=窗号与预算），建议=收窗防浪费；
//   * 风暴中切换：排空中事件率突增越阈（证据=突增率），建议=展期或降级；
//   * 分类闭集两类、逐例（类型/窗号/证据/建议）登记；
//   * 守恒：分类和 == 总数；FNV 链指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb930 独占） ─────────────────────────────────────────────
pub const XC_DB_SEALED: u32 = 0xB931; // 封账后登记
pub const XC_DB_NO_EVID: u32 = 0xB932; // 证据缺失
pub const XC_DB_NO_ADVICE: u32 = 0xB933; // 建议缺失
pub const XC_DB_FULL: u32 = 0xB934; // 账容量满
pub const XC_DB_ZERO_BUDGET: u32 = 0xB935; // 空排空证据预算为 0（非法）

/// 边界类型。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DrainEdge {
    /// 空排空（开窗零事件）。
    EmptyDrain,
    /// 风暴中切换（排空遇风暴）。
    StormSwitch,
}

/// 建议动作。
pub const ADV_NONE: u32 = 0;
pub const ADV_CLOSE_WIN: u32 = 1;  // 收窗防浪费
pub const ADV_EXTEND: u32 = 2;     // 展期排空
pub const ADV_DEGRADE: u32 = 3;    // 降级切换

/// 边界行。
#[derive(Clone, Copy)]
pub struct EdgeRow {
    pub kind: DrainEdge,
    pub wno: u32,
    /// 证据（空排空=预算 tick；风暴=突增率‰）。
    pub evid: u64,
    pub advice: u32,
}

/// 边界账。
pub struct DrainEdgeLedger {
    cap: usize,
    rows: Vec<EdgeRow>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl DrainEdgeLedger {
    pub fn new(cap: usize) -> DrainEdgeLedger {
        DrainEdgeLedger { cap, rows: vec![], sealed: false, rejects: Vec::new() }
    }

    /// 登记一例。
    pub fn log(&mut self, kind: DrainEdge, wno: u32, evid: u64, advice: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_DB_SEALED, "封账后登记"));
            return Err(XC_DB_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_DB_FULL, "账容量满"));
            return Err(XC_DB_FULL);
        }
        if advice == ADV_NONE {
            self.rejects.push((XC_DB_NO_ADVICE, "建议缺失"));
            return Err(XC_DB_NO_ADVICE);
        }
        match kind {
            DrainEdge::EmptyDrain => {
                if evid == 0 {
                    self.rejects.push((XC_DB_ZERO_BUDGET, "空排空预算为 0"));
                    return Err(XC_DB_ZERO_BUDGET);
                }
            }
            DrainEdge::StormSwitch => {
                if evid == 0 {
                    self.rejects.push((XC_DB_NO_EVID, "突增率 0 不构成风暴"));
                    return Err(XC_DB_NO_EVID);
                }
            }
        }
        self.rows.push(EdgeRow { kind, wno, evid, advice });
        Ok(())
    }

    pub fn count_kind(&self, k: DrainEdge) -> usize {
        self.rows.iter().filter(|r| r.kind == k).count()
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.count_kind(DrainEdge::EmptyDrain) + self.count_kind(DrainEdge::StormSwitch)
            == self.rows.len()
    }

    /// 证据总和。
    pub fn evid_total(&self) -> u64 {
        let mut s: u64 = 0;
        for r in &self.rows {
            s = s.saturating_add(r.evid);
        }
        s
    }

    /// 建议分布（首现序）。
    pub fn advice_dist(&self) -> Vec<(u32, usize)> {
        let mut out: Vec<(u32, usize)> = vec![];
        for r in &self.rows {
            let mut hit = false;
            for x in out.iter_mut() {
                if x.0 == r.advice {
                    x.1 += 1;
                    hit = true;
                    break;
                }
            }
            if !hit {
                out.push((r.advice, 1));
            }
        }
        out
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_DB_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            let k: u64 = match r.kind {
                DrainEdge::EmptyDrain => 0,
                DrainEdge::StormSwitch => 1,
            };
            h ^= k.rotate_left(3) ^ (r.wno as u64).rotate_left(17) ^ r.evid.rotate_left(29);
            h = h.wrapping_mul(0x0100_0193);
            h ^= (r.advice as u64).rotate_left(41);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准场景：空排空 2 例 + 风暴切换 2 例，守恒。
pub fn edge_scenario() -> Result<(usize, u64, u64), u32> {
    let mut l = DrainEdgeLedger::new(8);
    l.log(DrainEdge::EmptyDrain, 3, 50, ADV_CLOSE_WIN)?;
    l.log(DrainEdge::EmptyDrain, 7, 50, ADV_CLOSE_WIN)?;
    l.log(DrainEdge::StormSwitch, 11, 400, ADV_EXTEND)?;
    l.log(DrainEdge::StormSwitch, 15, 900, ADV_DEGRADE)?;
    if !l.conserved() {
        return Err(XC_DB_NO_EVID);
    }
    let fp = l.seal()?;
    Ok((l.rows_len(), l.evid_total(), fp))
}

// ── 深化块：边界密度与风暴聚类 ─────────────────────────────────────
impl DrainEdgeLedger {
    /// 边界密度（例/窗号跨度，千分比）。
    pub fn density_per_mille(&self) -> u32 {
        if self.rows.is_empty() { return 0; }
        let mut lo = u32::MAX;
        let mut hi = 0u32;
        for r in &self.rows {
            if r.wno < lo { lo = r.wno; }
            if r.wno > hi { hi = r.wno; }
        }
        let span = (hi - lo + 1) as u64;
        ((self.rows.len() as u64 * 1000) / span) as u32
    }
    /// 风暴窗聚类：风暴例按证据降序稳定排（Top n）。
    pub fn storm_cluster(&self, top: usize) -> Vec<(u32, u64)> {
        let mut v: Vec<(u32, u64)> = vec![];
        for r in &self.rows {
            if r.kind == DrainEdge::StormSwitch { v.push((r.wno, r.evid)); }
        }
        for i in 0..v.len() {
            let mut max = i;
            for j in i + 1..v.len() {
                if v[j].1 > v[max].1 { max = j; }
            }
            let t = v[i]; v[i] = v[max]; v[max] = t;
        }
        if top >= v.len() { v } else { v[..top].to_vec() }
    }
    /// 空排空清单（登记序窗号）。
    pub fn empty_list(&self) -> Vec<u32> {
        let mut v: Vec<u32> = vec![];
        for r in &self.rows {
            if r.kind == DrainEdge::EmptyDrain { v.push(r.wno); }
        }
        v
    }
    /// 单例证据查询。
    pub fn evid_of(&self, wno: u32) -> Option<u64> {
        self.rows.iter().find(|r| r.wno == wno).map(|r| r.evid)
    }
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）建议缺失必拒
    {
        let mut l = DrainEdgeLedger::new(4);
        if l.log(DrainEdge::EmptyDrain, 1, 10, ADV_NONE) == Err(XC_DB_NO_ADVICE) {
            caught += 1;
        }
    }
    // 2）空排空预算 0 必拒
    {
        let mut l = DrainEdgeLedger::new(4);
        if l.log(DrainEdge::EmptyDrain, 1, 0, ADV_CLOSE_WIN) == Err(XC_DB_ZERO_BUDGET) {
            caught += 1;
        }
    }
    // 3）风暴证据 0 必拒
    {
        let mut l = DrainEdgeLedger::new(4);
        if l.log(DrainEdge::StormSwitch, 1, 0, ADV_EXTEND) == Err(XC_DB_NO_EVID) {
            caught += 1;
        }
    }
    // 4）封账后登记必拒
    {
        let mut l = DrainEdgeLedger::new(4);
        let _ = l.log(DrainEdge::EmptyDrain, 1, 10, ADV_CLOSE_WIN);
        let _ = l.seal();
        if l.log(DrainEdge::EmptyDrain, 2, 10, ADV_CLOSE_WIN) == Err(XC_DB_SEALED) {
            caught += 1;
        }
    }
    // 5）容量满必拒
    {
        let mut l = DrainEdgeLedger::new(1);
        let _ = l.log(DrainEdge::EmptyDrain, 1, 10, ADV_CLOSE_WIN);
        if l.log(DrainEdge::EmptyDrain, 2, 10, ADV_CLOSE_WIN) == Err(XC_DB_FULL) {
            caught += 1;
        }
    }
    // 6）分类守恒与建议分布
    {
        let mut l = DrainEdgeLedger::new(8);
        let _ = l.log(DrainEdge::EmptyDrain, 1, 10, ADV_CLOSE_WIN);
        let _ = l.log(DrainEdge::StormSwitch, 2, 20, ADV_DEGRADE);
        let d = l.advice_dist();
        if l.conserved() && d.len() == 2 {
            caught += 1;
        }
    }
    (caught, N)
}
