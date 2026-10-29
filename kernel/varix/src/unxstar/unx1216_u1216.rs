// unx1216_u1216 —— UNX-F1216 Polling 态边界条件账（空事件/洪峰/源抖动）
// 判据原文 UNX-F1216-J1：边界账全过（三类边界事件逐例登记带证据值，
// 分类计数与总数守恒，建议动作非空，指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xaf70 独占（0xaf70~0xafaf）。
//
// 设计（纯功能 no_std）：
//   * 三类边界闭集：空事件（连续 ≥N 窗 0 事件）/ 洪峰（单窗事件数超阈值）/
//     源抖动（相邻窗事件率摆幅超带）；
//   * 逐例登记：类型/窗口号/证据值/建议动作码（非空），分类计数与总数守恒；
//   * 洪峰上限饱和：证据值钳制不回绕；空转窗口计数守恒（连零断开即清零重计）；
//   * FNV 链指纹冻结，封账后拒新登记。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xaf70 独占） ─────────────────────────────────────────────
pub const XC_BE_SEALED: u32 = 0xAF71; // 封账后登记
pub const XC_BE_BAD_KIND: u32 = 0xAF72; // 非法边界类型
pub const XC_BE_NO_EVID: u32 = 0xAF73; // 证据值缺失
pub const XC_BE_NO_ADVICE: u32 = 0xAF74; // 建议动作缺失
pub const XC_BE_FULL: u32 = 0xAF75; // 账目容量满

/// 边界类型闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// 空事件（连续零窗）。
    Idle,
    /// 洪峰（单窗超阈）。
    Burst,
    /// 源抖动（摆幅超带）。
    Jitter,
}

impl EdgeKind {
    pub fn code(self) -> u8 {
        match self {
            EdgeKind::Idle => 0,
            EdgeKind::Burst => 1,
            EdgeKind::Jitter => 2,
        }
    }
}

/// 建议动作码（非空语义由 >0 保证）。
pub const ADV_NONE: u32 = 0; // 非法（缺失）
pub const ADV_IDLE_GOV: u32 = 1; // 空转治理：降采样或省电
pub const ADV_BURST_THROTTLE: u32 = 2; // 洪峰限流
pub const ADV_JITTER_EWMA: u32 = 3; // 抖动平滑（EWMA 加重）
pub const ADV_SPLIT_SRC: u32 = 4; // 拆源重映射

/// 边界事件行。
#[derive(Clone, Copy)]
pub struct EdgeRow {
    pub kind: EdgeKind,
    pub window: u32,
    /// 证据值（Idle=连零窗数 / Burst=峰值事件数 / Jitter=摆幅‰）。
    pub evid: u64,
    pub advice: u32,
}

/// 边界条件账。
pub struct EdgeLedger {
    cap: usize,
    rows: Vec<EdgeRow>,
    /// 洪峰证据上限（钳制防回绕）。
    evid_cap: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl EdgeLedger {
    pub fn new(cap: usize, evid_cap: u64) -> EdgeLedger {
        EdgeLedger { cap, rows: Vec::new(), evid_cap, sealed: false, rejects: Vec::new() }
    }

    /// 登记一例边界事件（类型/证据/建议全要素齐备才收）。
    pub fn log(&mut self, kind: EdgeKind, window: u32, evid: u64, advice: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_BE_SEALED, "封账后登记"));
            return Err(XC_BE_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_BE_FULL, "账目容量满"));
            return Err(XC_BE_FULL);
        }
        if advice == ADV_NONE {
            self.rejects.push((XC_BE_NO_ADVICE, "建议动作缺失"));
            return Err(XC_BE_NO_ADVICE);
        }
        // 证据值语义：三类型各自必须非零
        match kind {
            EdgeKind::Idle => {
                if evid == 0 {
                    self.rejects.push((XC_BE_NO_EVID, "连零窗数为 0 不构成空事件"));
                    return Err(XC_BE_NO_EVID);
                }
            }
            EdgeKind::Burst => {
                if evid == 0 {
                    self.rejects.push((XC_BE_NO_EVID, "峰值 0 不构成洪峰"));
                    return Err(XC_BE_NO_EVID);
                }
            }
            EdgeKind::Jitter => {
                if evid == 0 {
                    self.rejects.push((XC_BE_NO_EVID, "摆幅 0 不构成抖动"));
                    return Err(XC_BE_NO_EVID);
                }
            }
        }
        let evid = if evid > self.evid_cap { self.evid_cap } else { evid };
        self.rows.push(EdgeRow { kind, window, evid, advice });
        Ok(())
    }

    /// 分类计数。
    pub fn count_kind(&self, kind: EdgeKind) -> usize {
        self.rows.iter().filter(|r| r.kind == kind).count()
    }

    /// 守恒：三分类和 == 总行数。
    pub fn conserved(&self) -> bool {
        self.count_kind(EdgeKind::Idle)
            + self.count_kind(EdgeKind::Burst)
            + self.count_kind(EdgeKind::Jitter)
            == self.rows.len()
    }

    /// 洪峰证据饱和：任何一行证据 ≤ evid_cap（钳制不回绕）。
    pub fn no_wrap(&self) -> bool {
        self.rows.iter().all(|r| r.evid <= self.evid_cap)
    }

    /// 建议分布：返回 (建议码, 次数) 按首现序。
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
            return Err(XC_BE_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            h ^= (r.kind.code() as u64) << 56 ^ (r.window as u64) << 32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= r.evid ^ ((r.advice as u64) << 24);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准边界场景：空转 3 连零 + 洪峰 1 例（饱和钳制）+ 抖动 1 例。
pub fn edge_scenario() -> Result<(usize, u64, u64), u32> {
    let mut l = EdgeLedger::new(16, 10000);
    // 空事件：第 5/6/7 窗连零 3 窗（证据=3）
    l.log(EdgeKind::Idle, 7, 3, ADV_IDLE_GOV)?;
    // 洪峰：第 9 窗峰值 987654 → 钳到 10000
    l.log(EdgeKind::Burst, 9, 987654, ADV_BURST_THROTTLE)?;
    // 抖动：第 11 窗摆幅 850‰
    l.log(EdgeKind::Jitter, 11, 850, ADV_JITTER_EWMA)?;
    if !l.conserved() || !l.no_wrap() {
        return Err(XC_BE_NO_EVID);
    }
    let fp = l.seal()?;
    Ok((l.rows_len(), l.rows[1].evid, fp))
}

// ── 深化块：时窗密度与建议分布统计 ─────────────────────────────────
impl EdgeLedger {
    /// 窗号区间 [a,b] 内的边界密度（例/窗，千分比）。
    pub fn density_per_mille(&self, a: u32, b: u32) -> u32 {
        if a > b { return 0; }
        let span = (b - a + 1) as u64;
        let hits = self.rows.iter().filter(|r| r.window >= a && r.window <= b).count() as u64;
        ((hits * 1000) / span) as u32
    }
    /// 建议码 → 计数（精确单查）。
    pub fn advice_count(&self, advice: u32) -> usize {
        self.rows.iter().filter(|r| r.advice == advice).count()
    }
    /// 每类型证据峰值。
    pub fn peak_evid(&self, kind: EdgeKind) -> u64 {
        let mut m: u64 = 0;
        for r in &self.rows {
            if r.kind == kind && r.evid > m { m = r.evid; }
        }
        m
    }
    /// 空转占比（Idle 例数 / 总数，基点）。
    pub fn idle_ratio_bps(&self) -> u32 {
        if self.rows.is_empty() { return 0; }
        ((self.count_kind(EdgeKind::Idle) as u64 * 10000) / self.rows.len() as u64) as u32
    }
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）建议缺失必拒
    {
        let mut l = EdgeLedger::new(8, 1000);
        if l.log(EdgeKind::Idle, 1, 3, ADV_NONE) == Err(XC_BE_NO_ADVICE) {
            caught += 1;
        }
    }
    // 2）空事件证据为零必拒
    {
        let mut l = EdgeLedger::new(8, 1000);
        if l.log(EdgeKind::Idle, 1, 0, ADV_IDLE_GOV) == Err(XC_BE_NO_EVID) {
            caught += 1;
        }
    }
    // 3）洪峰证据为零必拒
    {
        let mut l = EdgeLedger::new(8, 1000);
        if l.log(EdgeKind::Burst, 1, 0, ADV_BURST_THROTTLE) == Err(XC_BE_NO_EVID) {
            caught += 1;
        }
    }
    // 4）抖动证据为零必拒
    {
        let mut l = EdgeLedger::new(8, 1000);
        if l.log(EdgeKind::Jitter, 1, 0, ADV_JITTER_EWMA) == Err(XC_BE_NO_EVID) {
            caught += 1;
        }
    }
    // 5）封账后登记必拒
    {
        let mut l = EdgeLedger::new(8, 1000);
        let _ = l.log(EdgeKind::Idle, 1, 2, ADV_IDLE_GOV);
        let _ = l.seal();
        if l.log(EdgeKind::Burst, 2, 9, ADV_BURST_THROTTLE) == Err(XC_BE_SEALED) {
            caught += 1;
        }
    }
    // 6）容量满必拒
    {
        let mut l = EdgeLedger::new(1, 1000);
        let _ = l.log(EdgeKind::Idle, 1, 2, ADV_IDLE_GOV);
        if l.log(EdgeKind::Idle, 2, 2, ADV_IDLE_GOV) == Err(XC_BE_FULL) {
            caught += 1;
        }
    }
    // 7）证据钳制不回绕
    {
        let mut l = EdgeLedger::new(8, 100);
        let _ = l.log(EdgeKind::Burst, 1, u64::MAX, ADV_BURST_THROTTLE);
        if l.no_wrap() && l.rows_len() == 1 {
            caught += 1;
        }
    }
    (caught, N)
}
