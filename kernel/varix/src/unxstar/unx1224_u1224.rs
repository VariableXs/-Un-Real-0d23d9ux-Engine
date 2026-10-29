// unx1224_u1224 —— UNX-F1224 Draining 态切换窗口账（F0039 drain 窗口语义执行细化）
// 判据原文 UNX-F1224-J1：窗口账全过（开窗/关窗配对、预算内关窗率达标、
// 超预算立案、窗口守恒（开=关+在开）、漂移账逐窗量化、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb630 独占（0xb630~0xb66f）。
//
// 设计（纯功能 no_std）：
//   * drain 窗：每窗（窗号、开窗 tick、预算时长）登记，关闭时带排空事件数
//     与实际关窗 tick；
//   * 窗口状态机：Open → Closed（一次关窗）；重复关窗拒、未开先关拒；
//   * 漂移账：漂移 = 实际关窗 - 预算关窗（正=超时），逐窗量化记账；
//   * 超预算窗口立案（预算告警账），预算内关窗率 = 预算内数 / 关窗数；
//   * 守恒：开窗数 = 关窗数 + 在开数；FNV 链指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb630 独占） ─────────────────────────────────────────────
pub const XC_WN_SEALED: u32 = 0xB631; // 封账后开/关窗
pub const XC_WN_DUP_OPEN: u32 = 0xB632; // 已在开窗再开窗
pub const XC_WN_NOT_OPEN: u32 = 0xB633; // 无在开窗却关窗
pub const XC_WN_DUP_CLOSE: u32 = 0xB634; // 窗重复关闭
pub const XC_WN_BACKTICK: u32 = 0xB635; // 关窗 tick 早于开窗
pub const XC_WN_FULL: u32 = 0xB636; // 窗账容量满
pub const XC_WN_ZERO_BUDGET: u32 = 0xB637; // 零预算开窗

/// 窗口记录。
#[derive(Clone, Copy)]
pub struct WinRow {
    pub wno: u32,
    pub open_tick: u64,
    /// 预算时长（tick）。
    pub budget: u64,
    pub close_tick: Option<u64>,
    /// 本窗排空事件数。
    pub drained: u32,
    /// 漂移（关窗 - 预算关窗；未关窗为 0）。
    pub drift: i64,
}

/// 窗口账。
pub struct DrainWindowLedger {
    cap: usize,
    rows: Vec<WinRow>,
    cur: Option<usize>,
    over_budget: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl DrainWindowLedger {
    pub fn new(cap: usize) -> DrainWindowLedger {
        DrainWindowLedger {
            cap,
            rows: Vec::new(),
            cur: None,
            over_budget: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 开窗（在开窗存在即拒）。
    pub fn open(&mut self, wno: u32, tick: u64, budget: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_WN_SEALED, "封账后开窗"));
            return Err(XC_WN_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_WN_FULL, "窗账容量满"));
            return Err(XC_WN_FULL);
        }
        if budget == 0 {
            self.rejects.push((XC_WN_ZERO_BUDGET, "零预算开窗"));
            return Err(XC_WN_ZERO_BUDGET);
        }
        if self.cur.is_some() {
            self.rejects.push((XC_WN_DUP_OPEN, "已在开窗再开窗"));
            return Err(XC_WN_DUP_OPEN);
        }
        self.rows.push(WinRow {
            wno,
            open_tick: tick,
            budget,
            close_tick: None,
            drained: 0,
            drift: 0,
        });
        self.cur = Some(self.rows.len() - 1);
        Ok(())
    }

    /// 关窗（排空事件数随记）。
    pub fn close(&mut self, tick: u64, drained: u32) -> Result<i64, u32> {
        if self.sealed {
            self.rejects.push((XC_WN_SEALED, "封账后关窗"));
            return Err(XC_WN_SEALED);
        }
        let idx = match self.cur {
            Some(i) => i,
            None => {
                self.rejects.push((XC_WN_NOT_OPEN, "无在开窗却关窗"));
                return Err(XC_WN_NOT_OPEN);
            }
        };
        let open_tick = self.rows[idx].open_tick;
        if tick < open_tick {
            self.rejects.push((XC_WN_BACKTICK, "关窗早于开窗"));
            return Err(XC_WN_BACKTICK);
        }
        let budget = self.rows[idx].budget;
        let due = open_tick.saturating_add(budget);
        let drift = tick as i64 - due as i64;
        if drift > 0 {
            self.over_budget = self.over_budget.saturating_add(1);
        }
        self.rows[idx].close_tick = Some(tick);
        self.rows[idx].drained = drained;
        self.rows[idx].drift = drift;
        self.cur = None;
        Ok(drift)
    }

    /// 关窗数。
    pub fn closed(&self) -> usize {
        self.rows.iter().filter(|r| r.close_tick.is_some()).count()
    }
    /// 开窗数。
    pub fn opened(&self) -> usize {
        self.rows.len()
    }

    /// 守恒：开 = 关 + 在开。
    pub fn conserved(&self) -> bool {
        self.opened() == self.closed() + if self.cur.is_some() { 1 } else { 0 }
    }

    /// 预算内关窗率（基点 bps）：(关窗数-超预算)/关窗数 * 10000。
    pub fn in_budget_rate_bps(&self) -> Result<u32, u32> {
        let c = self.closed();
        if c == 0 {
            return Err(XC_WN_NOT_OPEN);
        }
        Ok(((c as u64 - self.over_budget as u64) * 10000 / c as u64) as u32)
    }

    /// 漂移总和（有符号饱和）。
    pub fn drift_total(&self) -> i64 {
        let mut s: i64 = 0;
        for r in &self.rows {
            s = s.saturating_add(r.drift);
        }
        s
    }

    /// 排空总数。
    pub fn drained_total(&self) -> u64 {
        let mut s: u64 = 0;
        for r in &self.rows {
            s = s.saturating_add(r.drained as u64);
        }
        s
    }

    pub fn over_budget_count(&self) -> u32 {
        self.over_budget
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_WN_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            h ^= r.wno as u64 ^ r.open_tick.rotate_left(7);
            h = h.wrapping_mul(0x0100_0193);
            h ^= r.budget.rotate_left(3) ^ (r.drained as u64).rotate_left(29);
            h = h.wrapping_mul(0x0100_0193);
            let d = r.drift as u64;
            h ^= d.rotate_left(11);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准场景：4 窗（3 窗预算内 1 窗超预算）+ 守恒 + 速率账。
pub fn window_scenario() -> Result<(usize, u32, u32, u64), u32> {
    let mut l = DrainWindowLedger::new(8);
    l.open(1, 100, 50)?;
    l.close(140, 30)?; // 漂移 -10（提前）
    l.open(2, 200, 50)?;
    l.close(250, 40)?; // 漂移 0
    l.open(3, 300, 50)?;
    l.close(370, 25)?; // 漂移 +20（超预算立案）
    l.open(4, 400, 50)?;
    l.close(440, 60)?; // 漂移 -10
    if !l.conserved() {
        return Err(XC_WN_DUP_OPEN);
    }
    let rate = l.in_budget_rate_bps()?;
    if rate != 7500 {
        return Err(XC_WN_ZERO_BUDGET);
    }
    if l.over_budget_count() != 1 {
        return Err(XC_WN_BACKTICK);
    }
    let fp = l.seal()?;
    Ok((l.closed(), l.over_budget_count(), rate, fp))
}

// ── 深化块：漂移分桶与压缩视图 ─────────────────────────────────────
impl DrainWindowLedger {
    /// 漂移分桶（5 桶：≤-50 / -50~0 / 0 / 0~50 / >50）。
    pub fn drift_buckets(&self) -> [u32; 5] {
        let mut b = [0u32; 5];
        for r in &self.rows {
            if r.close_tick.is_none() { continue; }
            if r.drift <= -50 { b[0] += 1; }
            else if r.drift < 0 { b[1] += 1; }
            else if r.drift == 0 { b[2] += 1; }
            else if r.drift <= 50 { b[3] += 1; }
            else { b[4] += 1; }
        }
        b
    }
    /// 压缩视图：(窗号, 漂移) 按漂移升序稳定排。
    pub fn compact_view(&self) -> Vec<(u32, i64)> {
        let mut v: Vec<(u32, i64)> = vec![];
        for r in &self.rows {
            if r.close_tick.is_some() { v.push((r.wno, r.drift)); }
        }
        for i in 0..v.len() {
            let mut min = i;
            for j in i + 1..v.len() {
                if v[j].1 < v[min].1 { min = j; }
            }
            let t = v[i]; v[i] = v[min]; v[min] = t;
        }
        v
    }
    /// 平均窗耗时（tick）。
    pub fn avg_span(&self) -> u64 {
        let mut s: u64 = 0;
        let mut n: u64 = 0;
        for r in &self.rows {
            if let Some(c) = r.close_tick {
                s = s.saturating_add(c.saturating_sub(r.open_tick));
                n += 1;
            }
        }
        if n == 0 { 0 } else { s / n }
    }
    /// 单窗查询。
    pub fn row_of(&self, wno: u32) -> Option<WinRow> {
        self.rows.iter().find(|r| r.wno == wno).copied()
    }
}

/// 注入核：八类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 8;

    // 1）在开窗再开窗必拒
    {
        let mut l = DrainWindowLedger::new(4);
        let _ = l.open(1, 0, 10);
        if l.open(2, 5, 10) == Err(XC_WN_DUP_OPEN) {
            caught += 1;
        }
    }
    // 2）无在开窗关窗必拒
    {
        let mut l = DrainWindowLedger::new(4);
        if l.close(5, 1) == Err(XC_WN_NOT_OPEN) {
            caught += 1;
        }
    }
    // 3）关窗早于开窗必拒
    {
        let mut l = DrainWindowLedger::new(4);
        let _ = l.open(1, 100, 10);
        if l.close(90, 1) == Err(XC_WN_BACKTICK) {
            caught += 1;
        }
    }
    // 4）零预算开窗必拒
    {
        let mut l = DrainWindowLedger::new(4);
        if l.open(1, 0, 0) == Err(XC_WN_ZERO_BUDGET) {
            caught += 1;
        }
    }
    // 5）封账后开窗必拒
    {
        let mut l = DrainWindowLedger::new(4);
        let _ = l.open(1, 0, 10);
        let _ = l.close(5, 1);
        let _ = l.seal();
        if l.open(2, 10, 10) == Err(XC_WN_SEALED) {
            caught += 1;
        }
    }
    // 6）容量满必拒
    {
        let mut l = DrainWindowLedger::new(1);
        let _ = l.open(1, 0, 10);
        let _ = l.close(5, 1);
        if l.open(2, 10, 10) == Err(XC_WN_FULL) {
            caught += 1;
        }
    }
    // 7）超预算立案计数
    {
        let mut l = DrainWindowLedger::new(4);
        let _ = l.open(1, 0, 10);
        let _ = l.close(25, 5); // 漂移 +15
        if l.over_budget_count() == 1 && l.drift_total() == 15 {
            caught += 1;
        }
    }
    // 8）排空总数与守恒
    {
        let mut l = DrainWindowLedger::new(4);
        let _ = l.open(1, 0, 10);
        let _ = l.close(5, 7);
        let _ = l.open(2, 10, 10);
        let _ = l.close(15, 3);
        if l.drained_total() == 10 && l.conserved() {
            caught += 1;
        }
    }
    (caught, N)
}
