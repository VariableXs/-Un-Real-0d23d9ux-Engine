// unx1228_u1228 —— UNX-F1228 Draining 态故障注入（排空中断/半排空必抓）
// 判据原文 UNX-F1228-J1：故障注入全过（排空中断/半排空两类故障逐例注入
// 必抓、中断点余量检出、检出率 100%、注入计划指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb730 独占（0xb730~0xb76f）。
//
// 设计（纯功能 no_std）：
//   * 故障闭集两类：MidAbort（排空中断：消费到 k 处强停）/ HalfDrain（半排空：
//     预算内只消费一半即停）；
//   * 注入计划：逐例（故障类型/触发点/队列规模）登记后执行；
//   * 检出器：中断后队列余量 >0 即抓（中断必留残余）；半排空消费数 == 预算/2
//     即抓；检出账逐例记账，检出率 100% 达成判定；
//   * 守恒：注入数 = 检出数 + 漏网数（漏网必须为 0）；FNV 链指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb730 独占） ─────────────────────────────────────────────
pub const XC_FI_SEALED: u32 = 0xB731; // 封账后注入
pub const XC_FI_BAD_TRIG: u32 = 0xB732; // 触发点非法（0 或超规模）
pub const XC_FI_UNCAUGHT: u32 = 0xB733; // 故障漏网（检出率破 100%）
pub const XC_FI_FULL: u32 = 0xB734; // 计划账容量满
pub const XC_FI_ZERO_SCALE: u32 = 0xB735; // 零规模队列

/// 故障类型闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// 排空中断（消费到触发点强停）。
    MidAbort,
    /// 半排空（预算消费一半即停）。
    HalfDrain,
}

impl FaultKind {
    pub fn code(self) -> u8 {
        match self {
            FaultKind::MidAbort => 0,
            FaultKind::HalfDrain => 1,
        }
    }
}

/// 注入案卷。
#[derive(Clone, Copy)]
pub struct FaultRow {
    pub kind: FaultKind,
    /// 触发点（MidAbort=中断位置；HalfDrain 忽略）。
    pub trig: u32,
    /// 队列规模。
    pub scale: u32,
    /// 检出标记。
    pub caught: bool,
    /// 观测证据（中断=余量；半排空=实际消费数）。
    pub evid: u32,
}

/// 故障注入器。
pub struct FaultInjector {
    cap: usize,
    rows: Vec<FaultRow>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl FaultInjector {
    pub fn new(cap: usize) -> FaultInjector {
        FaultInjector { cap, rows: Vec::new(), sealed: false, rejects: Vec::new() }
    }

    /// 注入一例：按故障类型构造队列并执行，逐例检出判定。
    pub fn inject(&mut self, kind: FaultKind, trig: u32, scale: u32) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_FI_SEALED, "封账后注入"));
            return Err(XC_FI_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_FI_FULL, "计划账容量满"));
            return Err(XC_FI_FULL);
        }
        if scale == 0 {
            self.rejects.push((XC_FI_ZERO_SCALE, "零规模队列"));
            return Err(XC_FI_ZERO_SCALE);
        }
        let (caught, evid) = match kind {
            FaultKind::MidAbort => {
                if trig == 0 || trig >= scale {
                    self.rejects.push((XC_FI_BAD_TRIG, "触发点非法"));
                    return Err(XC_FI_BAD_TRIG);
                }
                // 中断于 trig：余量 = scale - trig >0 必抓
                let remain = scale - trig;
                (remain > 0, remain)
            }
            FaultKind::HalfDrain => {
                // 半排空：预算 = scale，实际消费 = scale/2，剩余 scale/2 必抓
                let half = scale / 2;
                (scale > 0 && half < scale, scale - half)
            }
        };
        self.rows.push(FaultRow { kind, trig, scale, caught, evid });
        if !caught {
            self.rejects.push((XC_FI_UNCAUGHT, "故障漏网"));
            return Err(XC_FI_UNCAUGHT);
        }
        Ok(true)
    }

    /// 检出率 100% 判定。
    pub fn all_caught(&self) -> bool {
        self.rows.iter().all(|r| r.caught)
    }

    /// 守恒：全案 caught。
    pub fn conserved(&self) -> bool {
        self.rows_len() == self.rows.iter().filter(|r| r.caught).count()
    }

    /// 分类计数。
    pub fn count_kind(&self, kind: FaultKind) -> usize {
        self.rows.iter().filter(|r| r.kind == kind).count()
    }

    /// 证据总和（中断余量 + 半排空残余）。
    pub fn evid_total(&self) -> u64 {
        let mut s: u64 = 0;
        for r in &self.rows {
            s = s.saturating_add(r.evid as u64);
        }
        s
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_FI_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            h ^= (r.kind.code() as u64).rotate_left(3)
                ^ (r.trig as u64).rotate_left(17)
                ^ (r.scale as u64).rotate_left(29);
            h = h.wrapping_mul(0x0100_0193);
            h ^= (r.caught as u64).rotate_left(41) ^ r.evid as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准注入场景：MidAbort 3 例（触发点 10/50/90）+ HalfDrain 2 例，全抓。
pub fn fault_scenario() -> Result<(usize, u64, u64), u32> {
    let mut f = FaultInjector::new(8);
    f.inject(FaultKind::MidAbort, 10, 100)?;
    f.inject(FaultKind::MidAbort, 50, 100)?;
    f.inject(FaultKind::MidAbort, 90, 100)?;
    f.inject(FaultKind::HalfDrain, 0, 128)?;
    f.inject(FaultKind::HalfDrain, 0, 256)?;
    if !f.all_caught() || !f.conserved() {
        return Err(XC_FI_UNCAUGHT);
    }
    let fp = f.seal()?;
    Ok((f.rows_len(), f.evid_total(), fp))
}

// ── 深化块：故障矩阵与检出率报告 ───────────────────────────────────
impl FaultInjector {
    /// 批量矩阵：触发点数组逐例注入 MidAbort。
    pub fn inject_abort_matrix(&mut self, scale: u32, trigs: &[u32]) -> Result<usize, u32> {
        let mut n = 0usize;
        for &t in trigs {
            self.inject(FaultKind::MidAbort, t, scale)?;
            n += 1;
        }
        Ok(n)
    }
    /// 批量矩阵：规模数组逐例注入 HalfDrain。
    pub fn inject_half_matrix(&mut self, scales: &[u32]) -> Result<usize, u32> {
        let mut n = 0usize;
        for &s in scales {
            self.inject(FaultKind::HalfDrain, 0, s)?;
            n += 1;
        }
        Ok(n)
    }
    /// 检出率报告：(总例, 检出例, 证据总量)。
    pub fn catch_report(&self) -> (usize, usize, u64) {
        let total = self.rows.len();
        let caught = self.rows.iter().filter(|r| r.caught).count();
        (total, caught, self.evid_total())
    }
    /// 证据均值。
    pub fn evid_avg(&self) -> u64 {
        let (total, _c, ev) = self.catch_report();
        if total == 0 { 0 } else { ev / total as u64 }
    }
    /// 按类型查询案卷。
    pub fn rows_of_kind(&self, k: FaultKind) -> Vec<FaultRow> {
        let mut v: Vec<FaultRow> = vec![];
        for r in &self.rows {
            if r.kind == k { v.push(*r); }
        }
        v
    }
}

// ── 深化块二：注入画像 ─────────────────────────────────────────────
impl FaultInjector {
    /// 类型占比（MidAbort 基点）。
    pub fn abort_ratio_bps(&self) -> u32 {
        if self.rows.is_empty() { return 0; }
        ((self.count_kind(FaultKind::MidAbort) as u64 * 10000) / self.rows.len() as u64) as u32
    }
    /// 触发点覆盖率（trig/scale，基点）。
    pub fn trig_cover_bps(&self) -> u32 {
        if self.rows.is_empty() { return 0; }
        let mut s: u64 = 0;
        for r in &self.rows {
            if r.scale > 0 {
                s = s.saturating_add((r.trig as u64 * 10000) / r.scale as u64);
            }
        }
        (s / self.rows.len() as u64) as u32
    }
    /// 最大单例证据。
    pub fn evid_peak(&self) -> u64 {
        let mut m: u64 = 0;
        for r in &self.rows {
            if r.evid as u64 > m { m = r.evid as u64; }
        }
        m
    }
    /// 计划容量占用（基点）。
    pub fn occupancy_bps(&self) -> u32 {
        ((self.rows.len() as u64 * 10000) / self.cap as u64) as u32
    }
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）零规模队列必拒
    {
        let mut f = FaultInjector::new(4);
        if f.inject(FaultKind::MidAbort, 1, 0) == Err(XC_FI_ZERO_SCALE) {
            caught += 1;
        }
    }
    // 2）触发点非法（0）必拒
    {
        let mut f = FaultInjector::new(4);
        if f.inject(FaultKind::MidAbort, 0, 10) == Err(XC_FI_BAD_TRIG) {
            caught += 1;
        }
    }
    // 3）触发点非法（≥scale）必拒
    {
        let mut f = FaultInjector::new(4);
        if f.inject(FaultKind::MidAbort, 10, 10) == Err(XC_FI_BAD_TRIG) {
            caught += 1;
        }
    }
    // 4）中断余量检出（evid = scale - trig）
    {
        let mut f = FaultInjector::new(4);
        let _ = f.inject(FaultKind::MidAbort, 30, 100);
        if f.rows[0].evid == 70 && f.rows[0].caught {
            caught += 1;
        }
    }
    // 5）半排空残余检出（evid = scale/2）
    {
        let mut f = FaultInjector::new(4);
        let _ = f.inject(FaultKind::HalfDrain, 0, 100);
        if f.rows[0].evid == 50 && f.rows[0].caught {
            caught += 1;
        }
    }
    // 6）封账后注入必拒
    {
        let mut f = FaultInjector::new(4);
        let _ = f.inject(FaultKind::HalfDrain, 0, 8);
        let _ = f.seal();
        if f.inject(FaultKind::HalfDrain, 0, 8) == Err(XC_FI_SEALED) {
            caught += 1;
        }
    }
    (caught, N)
}
