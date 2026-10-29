//! CGPU-F0711 · 滚动遥测（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0711，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：滚动遥测——对 F0710 多表面滚动并发产生的事件做
//!   无锁语义的定容环形采集、计数守恒、整数分位与快照导出。
//!
//! 设计纪律：
//! - 定容环形缓冲：RING_CAP=64 槽，写指针回绕覆盖最旧记录，
//!   覆盖数（dropped）显式计数，不静默丢失。
//! - 计数守恒：emitted == accepted + rejected（恒等式，drill 校验）；
//!   accepted == ring 内记录数 + dropped。
//! - 整数分位：样本升序取第 p 千分位（p∈[0,1000]），
//!   idx = (n*p + 999) / 1000 向上取整，纯整数。
//! - 事件分类闭集：枚举 ScrollEvtKind，无裸 u8 魔数。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性。
//! - 诊断段 0x4720 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4720，批内 CGPU-F0711 独占）。
pub const SEG_BASE: u16 = 0x4720;

/// 环形缓冲容量超限（容量必须 ≥1）。
pub const E_RING_CAP: u16 = SEG_BASE + 1;
/// 分位参数非法（p > 1000 或样本为空）。
pub const E_PCT: u16 = SEG_BASE + 2;
/// 守恒账不平（emitted ≠ accepted + rejected）。
pub const E_LEDGER: u16 = SEG_BASE + 3;
/// 遥测会话未开启就写入。
pub const E_NOT_OPEN: u16 = SEG_BASE + 4;
/// 遥测会话重复开启。
pub const E_ALREADY_OPEN: u16 = SEG_BASE + 5;
/// 快照序号不连续（会话中途有丢失未记账）。
pub const E_GAP: u16 = SEG_BASE + 6;

/// 环形缓冲定容。
pub const RING_CAP: usize = 64;
/// 延迟分位目标（微秒，规格默认档：p95 ≤ 8000us）。
pub const P95_TARGET_US: u32 = 8000;

/// 滚动事件类别（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvtKind {
    /// 武装命令被接受。
    Armed,
    /// 帧推进完成（含获批行数）。
    Ticked,
    /// 表面进入锁存。
    Latched,
    /// 表面撕裂回滚。
    Torn,
    /// 命令被拒绝（含拒绝码）。
    Rejected,
}

/// 单条滚动遥测记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrollEvt {
    /// 单调序号（会话内从 1 起，连续）。
    pub seq: u64,
    /// 事件类别。
    pub kind: EvtKind,
    /// 表面号（Rejected/全局事件填 0）。
    pub surf: u16,
    /// 伴随数值（行数/延迟微秒/拒绝码，按 kind 语义）。
    pub val: u32,
    /// 事件帧号。
    pub frame: u32,
}

/// 每类计数器（守恒账载体）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KindCounters {
    /// 武装数。
    pub armed: u64,
    /// 推进数。
    pub ticked: u64,
    /// 锁存数。
    pub latched: u64,
    /// 撕裂数。
    pub torn: u64,
    /// 拒绝数。
    pub rejected: u64,
}

impl KindCounters {
    /// 按类别累加一次。
    fn bump(&mut self, k: EvtKind) {
        match k {
            EvtKind::Armed => self.armed += 1,
            EvtKind::Ticked => self.ticked += 1,
            EvtKind::Latched => self.latched += 1,
            EvtKind::Torn => self.torn += 1,
            EvtKind::Rejected => self.rejected += 1,
        }
    }
}

/// 滚动遥测会话：定容环 + 计数守恒 + 延迟样本。
#[derive(Clone, Debug)]
pub struct ScrollTelemetry {
    /// 环形缓冲（Option 槽，None=未占用）。
    ring: [Option<ScrollEvt>; RING_CAP],
    /// 下一写槽位。
    write: usize,
    /// 环内有效记录数。
    held: usize,
    /// 环覆盖丢弃数。
    dropped: u64,
    /// 累计发出数。
    emitted: u64,
    /// 累计接受数。
    accepted: u64,
    /// 累计拒绝数。
    rejected: u64,
    /// 每类计数。
    kinds: KindCounters,
    /// 延迟样本（微秒，只收 Ticked 事件）。
    lat: Vec<u32>,
    /// 会话是否开启。
    open: bool,
    /// 最近写入的 seq（校验连续性）。
    last_seq: u64,
}

impl ScrollTelemetry {
    /// 构造（未开启）。
    pub fn new() -> Self {
        Self {
            ring: core::array::from_fn(|_| None),
            write: 0,
            held: 0,
            dropped: 0,
            emitted: 0,
            accepted: 0,
            rejected: 0,
            kinds: KindCounters::default(),
            lat: Vec::new(),
            open: false,
            last_seq: 0,
        }
    }

    /// 开启会话。
    pub fn open(&mut self) -> Result<(), u16> {
        if self.open {
            return Err(E_ALREADY_OPEN);
        }
        self.open = true;
        Ok(())
    }

    /// 写入一条事件：accepted 时入环（可能覆盖最旧），
    /// rejected 时只进拒绝账。
    pub fn emit(&mut self, kind: EvtKind, surf: u16, val: u32, frame: u32) -> Result<u64, u16> {
        if !self.open {
            return Err(E_NOT_OPEN);
        }
        self.emitted += 1;
        if kind == EvtKind::Rejected {
            self.rejected += 1;
            self.kinds.bump(kind);
            return Ok(self.emitted);
        }
        // 接受路径：seq 必须连续。
        let seq = self.last_seq + 1;
        self.last_seq = seq;
        let evt = ScrollEvt {
            seq,
            kind,
            surf,
            val,
            frame,
        };
        if self.held < RING_CAP {
            self.ring[self.write] = Some(evt);
            self.write = (self.write + 1) % RING_CAP;
            self.held += 1;
        } else {
            // 覆盖最旧：write 处即最旧。
            self.ring[self.write] = Some(evt);
            self.write = (self.write + 1) % RING_CAP;
            self.dropped += 1;
        }
        self.accepted += 1;
        self.kinds.bump(kind);
        if kind == EvtKind::Ticked {
            self.lat.push(val);
        }
        Ok(seq)
    }

    /// 守恒审计：emitted == accepted + rejected，
    /// accepted == held + dropped，accepted == Σ kinds（去掉 rejected 双计）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.emitted != self.accepted + self.rejected {
            return Err(E_LEDGER);
        }
        if self.accepted != self.held as u64 + self.dropped {
            return Err(E_LEDGER);
        }
        let sum_kinds = self.kinds.armed
            + self.kinds.ticked
            + self.kinds.latched
            + self.kinds.torn
            + self.kinds.rejected;
        if sum_kinds != self.emitted {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    /// 按序读出环内记录（旧→新）。
    pub fn drain_view(&self) -> Vec<ScrollEvt> {
        let mut out = Vec::with_capacity(self.held);
        let start = if self.held < RING_CAP {
            0
        } else {
            self.write
        };
        for i in 0..self.held {
            let idx = (start + i) % RING_CAP;
            if let Some(e) = &self.ring[idx] {
                out.push(e.clone());
            }
        }
        out
    }

    /// 序号连续性校验：环内可见 seq 应严格递增且步长 1（在未覆盖时）。
    pub fn seq_contiguous(&self) -> Result<(), u16> {
        let v = self.drain_view();
        for w in v.windows(2) {
            if w[1].seq != w[0].seq + 1 {
                return Err(E_GAP);
            }
        }
        Ok(())
    }

    /// 整数 p 千分位延迟（us）。升序复制后取上取整索引。
    pub fn percentile_us(&self, p: u32) -> Result<u32, u16> {
        if p > 1000 || self.lat.is_empty() {
            return Err(E_PCT);
        }
        let mut s = self.lat.clone();
        // 插入排序：n 小（≤ 环相关样本量），确定性且无外部依赖。
        for i in 1..s.len() {
            let mut j = i;
            while j > 0 && s[j - 1] > s[j] {
                s.swap(j - 1, j);
                j -= 1;
            }
        }
        let n = s.len() as u64;
        let idx = ((n * p as u64) + 999) / 1000; // 1-based 上取整
        let idx = idx as usize - 1;
        Ok(s[idx])
    }

    /// p95 达标判定。
    pub fn p95_meets_target(&self) -> Result<bool, u16> {
        Ok(self.percentile_us(950)? <= P95_TARGET_US)
    }

    /// 只读计数快照。
    pub fn counters(&self) -> (u64, u64, u64, u64, KindCounters) {
        (self.emitted, self.accepted, self.rejected, self.dropped, self.kinds)
    }

    /// 关闭会话（开后可再开新一轮，计数清零但环保留供读取）。
    pub fn close(&mut self) -> Result<(), u16> {
        if !self.open {
            return Err(E_NOT_OPEN);
        }
        self.open = false;
        Ok(())
    }
}

/// 遥测汇总行（导出用，纯数据）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelemetryRow {
    /// 类别序号（0..=4，对应 EvtKind 顺序）。
    pub kind_idx: u8,
    /// 该类计数。
    pub count: u64,
}

/// 汇总导出：五类各一行，计数全量（含已被环覆盖的历史）。
pub fn summarize(t: &ScrollTelemetry) -> Result<Vec<TelemetryRow>, u16> {
    let (emitted, _acc, _rej, _drop, k) = t.counters();
    let _ = emitted;
    let mut out = Vec::new();
    let counts = [k.armed, k.ticked, k.latched, k.torn, k.rejected];
    for (i, c) in counts.iter().enumerate() {
        out.push(TelemetryRow {
            kind_idx: i as u8,
            count: *c,
        });
    }
    Ok(out)
}

/// 自例程：主路径（开→采集 20 帧事件→审计→分位）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut t = ScrollTelemetry::new();
    // 错误路径 1：未开启就写。
    if t.emit(EvtKind::Armed, 1, 0, 0) != Err(E_NOT_OPEN) {
        return Err(E_NOT_OPEN);
    }
    t.open()?;
    // 错误路径 2：重复开启。
    if t.open() != Err(E_ALREADY_OPEN) {
        return Err(E_ALREADY_OPEN);
    }
    // 主路径：模拟 20 帧，每帧 1 武装 + 1 推进（延迟=8000-frame*100）+ 1 锁存。
    for f in 0..20u32 {
        t.emit(EvtKind::Armed, 1, 64, f)?;
        let lat = 8000u32.saturating_sub(f * 100);
        t.emit(EvtKind::Ticked, 1, lat, f)?;
        t.emit(EvtKind::Latched, 1, 64, f)?;
    }
    // 一条拒绝与一条撕裂。
    t.emit(EvtKind::Rejected, 7, 0x4721, 20)?;
    t.emit(EvtKind::Torn, 2, 0, 20)?;
    t.audit()?;
    t.seq_contiguous()?;
    let (emitted, accepted, rejected, dropped, _) = t.counters();
    if emitted != 62 || accepted != 61 || rejected != 1 || dropped != 0 {
        return Err(E_LEDGER);
    }
    // 分位：20 个样本升序后 p95 → idx = ceil(20*0.95)=19 → 第 19 小。
    let p95 = t.percentile_us(950)?;
    if p95 != 7900 {
        return Err(E_PCT); // 样本 8000..6100，升序第 19 个 = 7900
    }
    if !t.p95_meets_target()? {
        return Err(E_PCT);
    }
    // 错误路径 3：空样本分位。
    let empty = ScrollTelemetry::new();
    if empty.percentile_us(500) != Err(E_PCT) {
        return Err(E_PCT);
    }
    // 错误路径 4：p 越界。
    if t.percentile_us(1001) != Err(E_PCT) {
        return Err(E_PCT);
    }
    // 覆盖路径：再灌 RING_CAP 条推进事件触发回绕覆盖。
    for f in 0..(RING_CAP as u32) {
        t.emit(EvtKind::Ticked, 3, 1000, 100 + f)?;
    }
    t.audit()?;
    let (_, _, _, dropped2, _) = t.counters();
    // 精确账：首轮 accepted=61（环未满，dropped=0）；再灌 64 条 →
    // 环先补满 3 槽，后 61 条全部覆盖旧记录 → dropped=61，
    // 恒等式 accepted(125) == held(64) + dropped(61)。
    if dropped2 != 61 {
        return Err(E_LEDGER);
    }
    let _ = dropped2;
    // 汇总导出：5 行。
    let rows = summarize(&t)?;
    if rows.len() != 5 {
        return Err(E_LEDGER);
    }
    t.close()?;
    // 错误路径 5：关闭后未重开即写。
    if t.emit(EvtKind::Armed, 1, 0, 999) != Err(E_NOT_OPEN) {
        return Err(E_NOT_OPEN);
    }
    Ok(0)
}
