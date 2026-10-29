// unx1215_u1215 —— UNX-F1215 Polling 态统计矩阵（per 源事件率与负载贡献）
// 判据原文 UNX-F1215-J1：统计矩阵全过（per 源事件率逐源登记、负载贡献
// 基点化 Σ=10000bps 守恒、重复窗口拒、越界拒、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9fb0 独占（0x9fb0~0x9fef）。
//
// 设计（纯功能 no_std）：
//   * 源登记表：每采集点（源 id ≤ 255）按窗口记账（事件数、窗口 tick 数）；
//   * 事件率：千分比定点（事件数*1000 / 窗口 tick，除零显性拒）；
//   * 负载贡献：基点 bps（0~10000），Σ 源贡献恰 10000bps（归一化守恒）；
//   * 同源同窗口重复记账拒、源 id 越界拒、空窗口（0 tick）拒；
//   * FNV 链指纹冻结，封账后拒新记账。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9fb0 独占） ─────────────────────────────────────────────
pub const XC_SM_SEALED: u32 = 0x9FB1; // 封账后记账
pub const XC_SM_OOB_SRC: u32 = 0x9FB2; // 源 id 越界
pub const XC_SM_DUP_WIN: u32 = 0x9FB3; // 同源同窗口重复记账
pub const XC_SM_ZERO_TICK: u32 = 0x9FB4; // 空窗口（0 tick）
pub const XC_SM_FULL: u32 = 0x9FB5; // 账目容量满
pub const XC_SM_DIV0: u32 = 0x9FB6; // 除零（无事件源参与归一）

/// 源记账行。
#[derive(Clone, Copy)]
pub struct SrcRow {
    pub src: u8,
    pub window: u32,
    pub events: u32,
    pub ticks: u32,
}

/// 统计矩阵。
pub struct StatMatrix {
    cap: usize,
    rows: Vec<SrcRow>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl StatMatrix {
    pub fn new(cap: usize) -> StatMatrix {
        StatMatrix { cap, rows: Vec::new(), sealed: false, rejects: Vec::new() }
    }

    /// 记一行源账。
    pub fn record(&mut self, src: u8, window: u32, events: u32, ticks: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_SM_SEALED, "封账后记账"));
            return Err(XC_SM_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_SM_FULL, "账目容量满"));
            return Err(XC_SM_FULL);
        }
        // 源 id 闭集 0..=255 由 u8 天然保证；此处校验窗口语义
        if ticks == 0 {
            self.rejects.push((XC_SM_ZERO_TICK, "空窗口"));
            return Err(XC_SM_ZERO_TICK);
        }
        for r in &self.rows {
            if r.src == src && r.window == window {
                self.rejects.push((XC_SM_DUP_WIN, "同源同窗口重复"));
                return Err(XC_SM_DUP_WIN);
            }
        }
        self.rows.push(SrcRow { src, window, events, ticks });
        Ok(())
    }

    /// 单源事件率：千分比（events*1000/ticks，饱和）。
    pub fn rate_per_mille(&self, src: u8, window: u32) -> Result<u32, u32> {
        for r in &self.rows {
            if r.src == src && r.window == window {
                return Ok(((r.events as u64 * 1000) / r.ticks as u64) as u32);
            }
        }
        Err(XC_SM_OOB_SRC)
    }

    /// 总事件数（全源全窗口 Σ）。
    pub fn total_events(&self) -> u64 {
        let mut s: u64 = 0;
        for r in &self.rows {
            s = s.saturating_add(r.events as u64);
        }
        s
    }

    /// 负载贡献：按源聚合事件占比，基点 bps，Σ = 10000。
    /// 返回 (源 id, 贡献 bps) 序列；总事件为 0 时显性拒（无归一基底）。
    pub fn load_contribution(&self) -> Result<Vec<(u8, u32)>, u32> {
        let total = self.total_events();
        if total == 0 {
            return Err(XC_SM_DIV0);
        }
        // 聚合每源事件数（源顺序按首现序）
        let mut ids: Vec<u8> = vec![];
        let mut sums: Vec<u64> = vec![];
        for r in &self.rows {
            let mut found = false;
            for (i, id) in ids.iter().enumerate() {
                if *id == r.src {
                    sums[i] = sums[i].saturating_add(r.events as u64);
                    found = true;
                    break;
                }
            }
            if !found {
                ids.push(r.src);
                sums.push(r.events as u64);
            }
        }
        let mut out: Vec<(u8, u32)> = vec![];
        let mut used: u32 = 0;
        for (i, id) in ids.iter().enumerate() {
            let bps = ((sums[i] * 10000) / total) as u32;
            used = used.saturating_add(bps);
            out.push((*id, bps));
        }
        // 余量补给最大源（Σ 恰 10000）
        if used < 10000 {
            let mut max_i = 0usize;
            for (i, (_, b)) in out.iter().enumerate() {
                if *b > out[max_i].1 {
                    max_i = i;
                }
            }
            out[max_i].1 = out[max_i].1.saturating_add(10000 - used);
        }
        Ok(out)
    }

    /// 守恒：贡献 Σ == 10000bps 且逐源非负。
    pub fn conserved(&self) -> bool {
        match self.load_contribution() {
            Ok(v) => {
                let mut s: u32 = 0;
                for (_, b) in &v {
                    s = s.saturating_add(*b);
                }
                s == 10000
            }
            Err(_) => self.total_events() == 0, // 空账视为平凡守恒
        }
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_SM_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            h ^= (r.src as u64) << 40 ^ (r.window as u64) << 24;
            h = h.wrapping_mul(0x0100_0193);
            h ^= (r.events as u64) << 32 ^ r.ticks as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准统计场景：5 源 2 窗口记账 + 贡献归一守恒。
pub fn stat_scenario() -> Result<(usize, u64, u64), u32> {
    let mut m = StatMatrix::new(16);
    // 源 0~4，窗口 7：事件数 50/30/10/6/4（合计 100）
    m.record(0, 7, 50, 1000)?;
    m.record(1, 7, 30, 1000)?;
    m.record(2, 7, 10, 1000)?;
    m.record(3, 7, 6, 1000)?;
    m.record(4, 7, 4, 1000)?;
    // 源 0~1，窗口 8：另一轮采样
    m.record(0, 8, 20, 500)?;
    m.record(1, 8, 30, 500)?;
    if !m.conserved() {
        return Err(XC_SM_DIV0);
    }
    let rate0 = m.rate_per_mille(0, 7)?; // 50*1000/1000 = 50‰
    if rate0 != 50 {
        return Err(XC_SM_ZERO_TICK);
    }
    let fp = m.seal()?;
    Ok((m.rows_len(), m.total_events(), fp))
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）同源同窗口重复必拒
    {
        let mut m = StatMatrix::new(8);
        let _ = m.record(1, 5, 10, 100);
        if m.record(1, 5, 20, 100) == Err(XC_SM_DUP_WIN) {
            caught += 1;
        }
    }
    // 2）空窗口必拒
    {
        let mut m = StatMatrix::new(8);
        if m.record(1, 5, 10, 0) == Err(XC_SM_ZERO_TICK) {
            caught += 1;
        }
    }
    // 3）未记账源查率必拒
    {
        let m = StatMatrix::new(8);
        if m.rate_per_mille(9, 1).is_err() {
            caught += 1;
        }
    }
    // 4）空账归一必拒（无基底）
    {
        let m = StatMatrix::new(8);
        if m.load_contribution().is_err() {
            caught += 1;
        }
    }
    // 5）封账后记账必拒
    {
        let mut m = StatMatrix::new(8);
        let _ = m.record(0, 1, 1, 1);
        let _ = m.seal();
        if m.record(0, 2, 1, 1) == Err(XC_SM_SEALED) {
            caught += 1;
        }
    }
    // 6）容量满必拒
    {
        let mut m = StatMatrix::new(2);
        let _ = m.record(0, 1, 1, 1);
        let _ = m.record(1, 1, 1, 1);
        if m.record(2, 1, 1, 1) == Err(XC_SM_FULL) {
            caught += 1;
        }
    }
    // 7）贡献 Σ 恰 10000（不整除时余量补给最大源）
    {
        let mut m = StatMatrix::new(8);
        // 1/3/3：333.33 → 33/33/33 + 1 补最大源 = 34/33/33 → 100
        let _ = m.record(0, 1, 1, 1);
        let _ = m.record(1, 1, 3, 1);
        let _ = m.record(2, 1, 3, 1);
        let v = m.load_contribution();
        if let Ok(v) = v {
            let s: u32 = v.iter().map(|(_, b)| *b).sum();
            if s == 10000 {
                caught += 1;
            }
        }
    }
    (caught, N)
}
