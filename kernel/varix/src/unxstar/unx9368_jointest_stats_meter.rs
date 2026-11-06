//! UNX-F9368 meter 计量联测（SyscallMeter × C3 统计账） · 0x7C980段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9368 逐字）：
//!   UNX-F9368-J1 计量联测判据过，SyscallMeter 统计与 C3 统计账对接可观测，
//!   计数逐号一致格过。
//!
//! 设计要点：
//!   ① 两条独立账路，结构刻意不同：仪表用**稠密数组**（16 槽，槽号 = nr -
//!      0x4000，O(1) 定位），C3 统计账用**有序稀疏行**（Vec<行>，按 nr 升
//!      序插入，任意号都能落账，含窗口外）。两条路只有「事件流」是共享的，
//!      其余代码零复用——逐号相等才是真对账，而不是同一份代码自己比自己。
//!   ② 窗口契约：仪表只认 0x4000..=0x400F 十六个号，窗口外一律折进
//!      out_of_band（显性记账，不静默丢）。C3 账相反：任意号都建行，窗口外
//!      行由 `foreign_count()` 汇总。两边对「外带」的口径必须收敛到同一个数。
//!   ③ 观测面与账面的分工：计数/累计时长与事件顺序无关（交换律），环形
//!      样本窗与顺序强相关。判据必须同时钉住「顺序无关」与「顺序敏感」，
//!      否则「把环也当账」或「把账当环」的错误都测不出来。
//!   ④ 环形窗 16 槽，满后覆盖最旧；head 按 16 回绕（无取模，显式折返）。
//!      逐槽可复算：ring_at(k) 必须等于脚本流倒数第 16-k 个样本。
//!   ⑤ 百分位整数化：min / p50 / p99 / max / mean 全部整数运算。n ≤ 16 时
//!      p99 下标恒为 n-1（99% 分位在这么小的窗口里必落在末元素），这条是
//!      定死的语义而不是近似。
//!   ⑥ 注入 10 轮，每轮用不同的前缀长度（32..320 个事件）复算对账，账本
//!      满足 injected == detected + escaped 且 escaped == 0。
//!   ⑦ 指纹 FNV-1a 32 位：逐号计数 + 累计时长 + 环摘要，双跑相等。
//! ─────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9368 独占 0x7C980..=0x7C9BF）。
pub const SEG_BASE: u32 = 0x7C980;

/// 计量窗口下界（native band 起点）。
pub const TRACK_BASE: u64 = 0x4000;
/// 仪表逐号槽数（0x4000..=0x400F）。
pub const TRACKED: usize = 16;
/// 环形样本窗容量。
pub const RING: usize = 16;
/// C3 统计账行容量（16 个窗口内号 + 8 个窗口外号）。
pub const LEDGER_ROWS: usize = 24;
/// 单轮事件数。
pub const EVENTS_PER_ROUND: usize = 32;
/// 注入轮数。
pub const ROUNDS: u32 = 10;
/// compat band 起点（窗口外号的取样来源）。
pub const COMPAT_BASE: u64 = 0x4100;

/// Linux errno（正数口径）。
pub const ENOMEM: i32 = 12;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9368 段内诊断码（12 个）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MeterCode {
    /// 同一号两账计数不等
    CountDiverge,
    /// 同一号两账累计时长不等
    NsDiverge,
    /// 观测总数不等
    ObservedDiverge,
    /// 拒绝数不等
    DeniedDiverge,
    /// 外带口径不等
    ForeignDiverge,
    /// 窗口边界算错（多算/少算一号）
    WindowEdge,
    /// 环未按容量覆盖最旧
    RingNotWrapped,
    /// 环游标错位
    RingCursor,
    /// 账面被顺序污染（应顺序无关）
    OrderLeak,
    /// 观测面丢序（应顺序相关）
    OrderBlind,
    /// 账行容量溢出
    LedgerFull,
    /// 双跑不一致
    NotReproducible,
}

impl MeterCode {
    /// 段内码值（0x7C980 全宽收不进 u16，取段基址低 16 位 + 序号）。
    pub fn code(self) -> u16 {
        (SEG_BASE as u16).wrapping_add(1 + self as u16)
    }

    /// 绝对码值（段基址 + 序号，两两唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与下一步动作。
    pub fn detail(self) -> &'static str {
        match self {
            MeterCode::CountDiverge => "逐号计数发散：仪表与 C3 账对同一号计数不等；先查该号是否落在窗口边缘",
            MeterCode::NsDiverge => "逐号时长发散：同一号累计 ns 不等；查是否有一侧把被拒调用也计入时长",
            MeterCode::ObservedDiverge => "观测总数不等：两侧对「一次调用」的计数口径不同（含外带）",
            MeterCode::DeniedDiverge => "拒绝数不等：被拒调用必须两侧同增，缺一侧即口径漂移",
            MeterCode::ForeignDiverge => "外带口径不等：窗口外调用仪表折 out_of_band、账本建窗前行，两数必须相等",
            MeterCode::WindowEdge => "窗口边界错：0x4000..=0x400F 恰 16 个号，0x4010 起必须外带",
            MeterCode::RingNotWrapped => "环未覆盖：容量 16 满载后新样本必须覆盖最旧，ring_len 恒 16",
            MeterCode::RingCursor => "环游标错位：head 应按 16 显式折返，head==0 表示正好写满整圈",
            MeterCode::OrderLeak => "账面被顺序污染：计数/时长必须与事件顺序无关（交换律）",
            MeterCode::OrderBlind => "观测面丢序：环外带样本必须保序，否则时序诊断无从谈起",
            MeterCode::LedgerFull => "C3 账行满：24 行封顶，新号必须显性回 ENOMEM 并计 dropped",
            MeterCode::NotReproducible => "双跑不一致：同脚本流两次结果不同，存在隐藏状态",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [MeterCode] = &[
        MeterCode::CountDiverge,
        MeterCode::NsDiverge,
        MeterCode::ObservedDiverge,
        MeterCode::DeniedDiverge,
        MeterCode::ForeignDiverge,
        MeterCode::WindowEdge,
        MeterCode::RingNotWrapped,
        MeterCode::RingCursor,
        MeterCode::OrderLeak,
        MeterCode::OrderBlind,
        MeterCode::LedgerFull,
        MeterCode::NotReproducible,
    ];
}

// ── 仪表面（稠密数组） ────────────────────────────────────────────────
/// 环内一个样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    pub nr: u32,
    pub ns: u32,
}

/// 空样本（数组重复初始化用）。
pub const SAMPLE_INIT: Sample = Sample { nr: 0, ns: 0 };

/// 时延统计（全整数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatStats {
    pub count: u32,
    pub min: u64,
    pub p50: u64,
    pub p99: u64,
    pub max: u64,
    pub mean: u64,
}

impl LatStats {
    pub const fn zero() -> LatStats {
        LatStats { count: 0, min: 0, p50: 0, p99: 0, max: 0, mean: 0 }
    }

    /// 五元组自洽：min ≤ p50 ≤ p99 ≤ max 且 count 非零。
    pub fn ordered(&self) -> bool {
        self.count > 0 && self.min <= self.p50 && self.p50 <= self.p99 && self.p99 <= self.max
    }
}

/// SyscallMeter：16 槽稠密账 + 16 槽环。
#[derive(Clone, Copy, Debug)]
pub struct SyscallMeter {
    counts: [u64; TRACKED],
    ns: [u64; TRACKED],
    ring: [Sample; RING],
    head: usize,
    ring_len: usize,
    pub observed: u64,
    pub denied: u64,
    pub out_of_band: u64,
}

impl SyscallMeter {
    pub const fn new() -> SyscallMeter {
        SyscallMeter {
            counts: [0u64; TRACKED],
            ns: [0u64; TRACKED],
            ring: [SAMPLE_INIT; RING],
            head: 0,
            ring_len: 0,
            observed: 0,
            denied: 0,
            out_of_band: 0,
        }
    }

    /// 号 → 槽。窗口外返回 None（折外带，不丢）。
    pub fn slot(nr: u64) -> Option<usize> {
        if nr >= TRACK_BASE && nr < TRACK_BASE + TRACKED as u64 {
            Some((nr - TRACK_BASE) as usize)
        } else {
            None
        }
    }

    pub fn count_at(&self, i: usize) -> u64 {
        self.counts.get(i).copied().unwrap_or(0)
    }

    pub fn ns_at(&self, i: usize) -> u64 {
        self.ns.get(i).copied().unwrap_or(0)
    }

    pub fn count_of(&self, nr: u64) -> u64 {
        match Self::slot(nr) {
            Some(i) => self.count_at(i),
            None => 0,
        }
    }

    /// 观测一次调用。顺序：记总数 → 记拒绝 → 落槽/外带 → 压环。
    pub fn observe(&mut self, nr: u64, ns: u64, denied: bool) {
        self.observed = self.observed.saturating_add(1);
        if denied {
            self.denied = self.denied.saturating_add(1);
        }
        match Self::slot(nr) {
            Some(i) => {
                let c = self.count_at(i).saturating_add(1);
                let t = self.ns_at(i).saturating_add(ns);
                if let Some(slot) = self.counts.get_mut(i) {
                    *slot = c;
                }
                if let Some(slot) = self.ns.get_mut(i) {
                    *slot = t;
                }
            }
            None => self.out_of_band = self.out_of_band.saturating_add(1),
        }
        let h = self.head;
        if let Some(slot) = self.ring.get_mut(h) {
            *slot = Sample { nr: nr as u32, ns: ns.min(u32::MAX as u64) as u32 };
        }
        // 显式折返，不用取模。
        self.head = if h + 1 >= RING { 0 } else { h + 1 };
        if self.ring_len < RING {
            self.ring_len = self.ring_len + 1;
        }
    }

    /// 窗口内计数和。
    pub fn in_band_count(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < TRACKED {
            sum = sum.saturating_add(self.count_at(i));
            i += 1;
        }
        sum
    }

    /// 窗口内累计时长和。
    pub fn in_band_ns(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < TRACKED {
            sum = sum.saturating_add(self.ns_at(i));
            i += 1;
        }
        sum
    }

    /// 守恒：窗口内计数 + 外带 == 观测总数。
    pub fn conserved(&self) -> bool {
        self.in_band_count().saturating_add(self.out_of_band) == self.observed
    }

    pub fn ring_len(&self) -> usize {
        self.ring_len
    }

    pub fn head(&self) -> usize {
        self.head
    }

    /// 逻辑序第 k 个样本（0 = 最旧）。越界返回 None。
    pub fn ring_at(&self, k: usize) -> Option<Sample> {
        if k >= self.ring_len {
            return None;
        }
        let mut idx = self.head + RING - self.ring_len;
        if idx >= RING {
            idx -= RING;
        }
        let mut j = 0usize;
        while j < k {
            idx = if idx + 1 >= RING { 0 } else { idx + 1 };
            j += 1;
        }
        self.ring.get(idx).copied()
    }

    /// 环的逻辑序 ns 序列（百分位输入）。
    pub fn ring_ns(&self) -> [u32; RING] {
        let mut out = [0u32; RING];
        let mut k = 0usize;
        while k < self.ring_len {
            if let (Some(s), Some(slot)) = (self.ring_at(k), out.get_mut(k)) {
                *slot = s.ns;
            }
            k += 1;
        }
        out
    }

    /// 环摘要（顺序敏感）：逐样本按小端字节喂入。
    pub fn ring_digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut k = 0usize;
        while k < self.ring_len {
            if let Some(s) = self.ring_at(k) {
                let nrb = s.nr.to_le_bytes();
                let nsb = s.ns.to_le_bytes();
                let mut i = 0usize;
                while i < nrb.len() {
                    if let Some(x) = nrb.get(i) {
                        h ^= *x as u32;
                        h = h.wrapping_mul(0x0100_0193);
                    }
                    if let Some(x) = nsb.get(i) {
                        h ^= *x as u32;
                        h = h.wrapping_mul(0x0100_0193);
                    }
                    i += 1;
                }
            }
            k += 1;
        }
        h
    }

    /// 清零（重跑用）。
    pub fn reset(&mut self) {
        self.counts = [0u64; TRACKED];
        self.ns = [0u64; TRACKED];
        self.ring = [SAMPLE_INIT; RING];
        self.head = 0;
        self.ring_len = 0;
        self.observed = 0;
        self.denied = 0;
        self.out_of_band = 0;
    }
}

/// 定容数组取值的无下标读取。
fn at_u32(a: &[u32; RING], i: usize) -> u32 {
    a.get(i).copied().unwrap_or(0)
}

/// 整数百分位：n ≤ 16 时 p99 下标恒为 n-1（窗口太小，99% 分位必落末元素）。
pub fn percentiles(samples: &[u32]) -> LatStats {
    let n = if samples.len() < RING { samples.len() } else { RING };
    if n == 0 {
        return LatStats::zero();
    }
    let mut buf = [0u32; RING];
    let mut i = 0usize;
    while i < n {
        if let (Some(v), Some(slot)) = (samples.get(i), buf.get_mut(i)) {
            *slot = *v;
        }
        i += 1;
    }
    // 插入排序（稳定、无堆、n ≤ 16）：用 swap 避免同时双借用。
    let mut k = 1usize;
    while k < n {
        let v = at_u32(&buf, k);
        let mut j = k;
        while j > 0 {
            let prev = at_u32(&buf, j - 1);
            if prev <= v {
                break;
            }
            buf.swap(j - 1, j);
            j -= 1;
        }
        k += 1;
    }
    let mut sum = 0u64;
    let mut t = 0usize;
    while t < n {
        sum = sum.saturating_add(at_u32(&buf, t) as u64);
        t += 1;
    }
    let p50_idx = n >> 1;
    LatStats {
        count: n as u32,
        min: at_u32(&buf, 0) as u64,
        p50: at_u32(&buf, p50_idx) as u64,
        p99: at_u32(&buf, n - 1) as u64,
        max: at_u32(&buf, n - 1) as u64,
        mean: sum / n as u64,
    }
}

// ── C3 统计账（有序稀疏行） ───────────────────────────────────────────
/// 账面一行：号 + 计数 + 累计时长。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerRow {
    pub nr: u64,
    pub count: u64,
    pub ns: u64,
}

/// 有序稀疏账本：任意号都能落账，行数 24 封顶。
#[derive(Clone, Debug)]
pub struct StatsLedger {
    rows: Vec<LedgerRow>,
    observed: u64,
    denied: u64,
    dropped: u32,
}

impl StatsLedger {
    pub fn new() -> StatsLedger {
        StatsLedger { rows: Vec::new(), observed: 0, denied: 0, dropped: 0 }
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub fn observed(&self) -> u64 {
        self.observed
    }

    pub fn denied(&self) -> u64 {
        self.denied
    }

    /// 行视图（对账与判据用，不出下标）。
    pub fn row(&self, i: usize) -> Option<LedgerRow> {
        self.rows.get(i).copied()
    }

    /// 落账：按 nr 升序插入；同号并行累加；行满回 ENOMEM（总数与拒绝数已计，
    /// 这是显性部分成功，不是静默丢）。
    pub fn record(&mut self, nr: u64, ns: u64, denied: bool) -> Result<(), i32> {
        self.observed = self.observed.saturating_add(1);
        if denied {
            self.denied = self.denied.saturating_add(1);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let hit = match self.rows.get(i) {
                Some(r) => r.nr == nr,
                None => false,
            };
            if hit {
                let c = self.count_of(nr).saturating_add(1);
                let t = self.ns_of(nr).saturating_add(ns);
                if let Some(r) = self.rows.get_mut(i) {
                    r.count = c;
                    r.ns = t;
                }
                return Ok(());
            }
            let past = match self.rows.get(i) {
                Some(r) => r.nr > nr,
                None => false,
            };
            if past {
                break;
            }
            i += 1;
        }
        if self.rows.len() >= LEDGER_ROWS {
            self.dropped = self.dropped.saturating_add(1);
            return Err(ENOMEM);
        }
        self.rows.insert(i, LedgerRow { nr, count: 1, ns });
        Ok(())
    }

    pub fn count_of(&self, nr: u64) -> u64 {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.nr == nr {
                    return r.count;
                }
                if r.nr > nr {
                    return 0;
                }
            }
            i += 1;
        }
        0
    }

    pub fn ns_of(&self, nr: u64) -> u64 {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.nr == nr {
                    return r.ns;
                }
                if r.nr > nr {
                    return 0;
                }
            }
            i += 1;
        }
        0
    }

    /// 窗口外行的计数和（与仪表 out_of_band 对账）。
    pub fn foreign_count(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if SyscallMeter::slot(r.nr).is_none() {
                    sum = sum.saturating_add(r.count);
                }
            }
            i += 1;
        }
        sum
    }

    /// 行计数总和（守恒：Σ行计数 == observed）。
    pub fn total_count(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                sum = sum.saturating_add(r.count);
            }
            i += 1;
        }
        sum
    }

    /// 升序不变量（插入序必须维持）。
    pub fn sorted(&self) -> bool {
        let mut i = 1usize;
        while i < self.rows.len() {
            if let (Some(a), Some(b)) = (self.rows.get(i - 1), self.rows.get(i)) {
                if a.nr >= b.nr {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

// ── 逐号对账 ──────────────────────────────────────────────────────────
/// 对账结果：16 号逐格 + 三个总数口径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Recon {
    pub numbers: u32,
    pub count_equal: u32,
    pub ns_equal: u32,
    pub observed_equal: bool,
    pub denied_equal: bool,
    pub foreign_equal: bool,
}

impl Recon {
    pub const fn zero() -> Recon {
        Recon {
            numbers: 0,
            count_equal: 0,
            ns_equal: 0,
            observed_equal: false,
            denied_equal: false,
            foreign_equal: false,
        }
    }

    pub fn all_equal(&self) -> bool {
        self.numbers == TRACKED as u32
            && self.count_equal == self.numbers
            && self.ns_equal == self.numbers
            && self.observed_equal
            && self.denied_equal
            && self.foreign_equal
    }
}

/// 逐号对账：16 个窗口内号逐一比计数与累计时长，再比三个总数口径。
pub fn reconcile(meter: &SyscallMeter, ledger: &StatsLedger) -> Recon {
    let mut r = Recon::zero();
    let mut i = 0usize;
    while i < TRACKED {
        let nr = TRACK_BASE + i as u64;
        r.numbers = r.numbers.saturating_add(1);
        if meter.count_at(i) == ledger.count_of(nr) {
            r.count_equal = r.count_equal.saturating_add(1);
        }
        if meter.ns_at(i) == ledger.ns_of(nr) {
            r.ns_equal = r.ns_equal.saturating_add(1);
        }
        i += 1;
    }
    r.observed_equal = meter.observed == ledger.observed();
    r.denied_equal = meter.denied == ledger.denied();
    r.foreign_equal = meter.out_of_band == ledger.foreign_count();
    r
}

// ── 确定性事件流 ──────────────────────────────────────────────────────
/// 一次调用的脚本事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub nr: u64,
    pub ns: u64,
    pub denied: bool,
}

/// 自写整数同余发生器（无系统随机、无墙钟）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 一步同余：state = state * a + c（wrapping，语义明确）。
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }
}

/// 生成 320 个脚本事件：十六号轮转 + 每 32 个事件夹一个 compat 号。
pub fn script_events() -> Vec<Event> {
    let mut out: Vec<Event> = Vec::new();
    let mut lcg = Lcg::new(0x9368_0001);
    let total = EVENTS_PER_ROUND * ROUNDS as usize;
    let mut i = 0usize;
    while i < total {
        let v = lcg.next();
        // 每 32 个事件有一个落在 compat band（窗口外）。
        let nr = if i & 31 == 0 {
            COMPAT_BASE + (v & 0x7)
        } else {
            TRACK_BASE + (v & 0xF)
        };
        let ns = 1 + ((v >> 8) & 0xFF);
        let denied = (v & 0x3) == 0;
        out.push(Event { nr, ns, denied });
        i += 1;
    }
    out
}

/// 把一段事件同时喂给两条账路。
pub fn run_events(events: &[Event]) -> (SyscallMeter, StatsLedger) {
    let mut meter = SyscallMeter::new();
    let mut ledger = StatsLedger::new();
    let mut i = 0usize;
    while i < events.len() {
        if let Some(e) = events.get(i) {
            meter.observe(e.nr, e.ns, e.denied);
            // 账面容量 24 行，脚本流最多 23 个不同号，正常不会满；
            // 真满了也是显性 Err，不影响计数对账（两侧都各记一次观测）。
            let rec = ledger.record(e.nr, e.ns, e.denied);
            if rec.is_err() {
                // 行满：继续喂仪表，账面已计 observed/denied 并记 dropped。
            }
        }
        i += 1;
    }
    (meter, ledger)
}

/// 注入账：轮数与检出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    pub fn record(&mut self, ok: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if ok {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EINVAL);
        }
        Ok(())
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 十轮注入：第 r 轮只喂前 (r+1)×32 个事件，对账必须逐轮全等。
pub fn run_injection() -> (InjectLedger, Recon) {
    let events = script_events();
    let mut ledger = InjectLedger::zero();
    let mut last = Recon::zero();
    let mut round = 0u32;
    while round < ROUNDS {
        let n = EVENTS_PER_ROUND * (round as usize + 1);
        let (meter, book) = run_events(&events[..n.min(events.len())]);
        let r = reconcile(&meter, &book);
        let ok = r.all_equal() && meter.conserved();
        if ledger.record(ok).is_err() {
            return (ledger, r);
        }
        last = r;
        round = round.saturating_add(1);
    }
    (ledger, last)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 把 u64 拆四字节喂入。
fn feed_u64(out: &mut Vec<u8>, v: u64) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}

/// 全量账指纹：逐号计数 + 逐号时长 + 环摘要 + 三个总数。
pub fn accounting_fingerprint() -> u32 {
    let events = script_events();
    let (meter, book) = run_events(&events);
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < TRACKED {
        feed_u64(&mut bytes, meter.count_at(i));
        feed_u64(&mut bytes, meter.ns_at(i));
        feed_u64(&mut bytes, book.count_of(TRACK_BASE + i as u64));
        i += 1;
    }
    feed_u64(&mut bytes, meter.observed);
    feed_u64(&mut bytes, meter.denied);
    feed_u64(&mut bytes, meter.out_of_band);
    feed_u64(&mut bytes, book.foreign_count() as u64);
    feed_u64(&mut bytes, meter.ring_digest() as u64);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9368 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9368_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9368");

    s.add("段位守卫：0x7C980 落本批窗口且 0x40 对齐",
          SEG_BASE == 0x7C980 && SEG_BASE % 0x40 == 0 && (0x7C000..=0x7D1C0).contains(&SEG_BASE),
          "段基址唯一，段间不得互相覆盖");

    let mut m = SyscallMeter::new();
    m.observe(TRACK_BASE, 100, false);
    s.add("常态单事件：一号一次 → 计数 1、观测 1、环 1",
          m.count_of(TRACK_BASE) == 1 && m.observed == 1 && m.ring_len() == 1
              && m.denied == 0 && m.out_of_band == 0 && m.conserved(),
          "最小可观测单元：一次调用必须且只能落一格");

    let events = script_events();
    let (meter, book) = run_events(&events);
    let recon = reconcile(&meter, &book);
    s.add("逐号一致：16/16 号计数两账相等",
          recon.count_equal == TRACKED as u32 && recon.numbers == TRACKED as u32,
          "稠密槽与稀疏行是两条独立实现，逐号相等才算对账");

    s.add("逐号一致：16/16 号累计时长两账相等",
          recon.ns_equal == TRACKED as u32,
          "时长口径易漂：被拒调用是否计时必须两侧一致");

    s.add("总数口径：观测/拒绝/外带三项两账相等",
          recon.observed_equal && recon.denied_equal && recon.foreign_equal
              && meter.observed == EVENTS_PER_ROUND as u64 * ROUNDS as u64,
          "320 个事件必须两侧都记满 320 次观测");

    s.add("守恒：窗口内计数 + 外带 == 观测总数",
          meter.conserved() && book.total_count() == book.observed(),
          "计数守恒是账本的第一不变量：丢一次调用就是一次漏账");

    s.add("外带口径：compat 号落 out_of_band 且不占 16 槽",
          meter.out_of_band > 0
              && meter.out_of_band == book.foreign_count()
              && meter.count_of(COMPAT_BASE) == 0
              && meter.in_band_count() + meter.out_of_band == meter.observed,
          "窗口外号折外带而非丢弃：丢弃会让 observed 与逐号账对不上");

    s.add("窗口边界：0x400F 是最后一槽，0x4010 起全外带",
          SyscallMeter::slot(TRACK_BASE) == Some(0)
              && SyscallMeter::slot(TRACK_BASE + 15) == Some(15)
              && SyscallMeter::slot(TRACK_BASE + 16).is_none()
              && SyscallMeter::slot(TRACK_BASE - 1).is_none(),
          "边界差一格的错误在这里显形");

    s.add("环容量：满载 16 槽，游标回绕到 0",
          meter.ring_len() == RING && meter.head() == 0
              && EVENTS_PER_ROUND * ROUNDS as usize == 320,
          "320 恰是 16 的整圈：head 回到 0 才能证明折返逻辑没有偏移");

    let mut ring_ok = true;
    let mut k = 0usize;
    while k < RING {
        let want = events.len().saturating_sub(RING).saturating_add(k);
        let got = meter.ring_at(k);
        let exp = events.get(want).copied();
        match (got, exp) {
            (Some(a), Some(b)) => {
                if a.nr as u64 != b.nr || a.ns as u64 != b.ns {
                    ring_ok = false;
                }
            }
            _ => ring_ok = false,
        }
        k += 1;
    }
    s.add("环序：逻辑序第 k 个样本 == 脚本流倒数第 16-k 个",
          ring_ok && meter.ring_at(RING).is_none(),
          "环是观测面：样本必须保序，否则时序归因无从谈起");

    let small = [7u32, 3, 9, 1, 5, 2, 8, 4, 6, 10];
    let stats = percentiles(&small);
    s.add("百分位：1..=10 十样本的 min/p50/p99/max/mean 精确",
          stats.min == 1 && stats.p50 == 6 && stats.p99 == 10 && stats.max == 10
              && stats.mean == 5 && stats.count == 10 && stats.ordered(),
          "整数百分位：n≤16 时 p99 下标恒为 n-1，这是语义不是近似");

    let empty = percentiles(&[]);
    s.add("零样本：统计全零且不做除法",
          empty == LatStats::zero() && empty.count == 0 && !empty.ordered(),
          "空窗口必须短路：除以 0 在 no_std 里是直接 panic");

    let mut ord1 = SyscallMeter::new();
    let mut ord2 = SyscallMeter::new();
    let seq = [TRACK_BASE, TRACK_BASE + 1, TRACK_BASE + 2, TRACK_BASE, TRACK_BASE + 1];
    let mut q = 0usize;
    while q < seq.len() {
        if let Some(nr) = seq.get(q) {
            ord1.observe(*nr, 10 + q as u64, false);
        }
        if let Some(nr) = seq.get(seq.len() - 1 - q) {
            ord2.observe(*nr, 10 + (seq.len() - 1 - q) as u64, false);
        }
        q += 1;
    }
    s.add("顺序无关：同一多重集正序/逆序 → 计数与时长全等",
          ord1.count_of(TRACK_BASE) == ord2.count_of(TRACK_BASE)
              && ord1.count_of(TRACK_BASE + 1) == ord2.count_of(TRACK_BASE + 1)
              && ord1.count_of(TRACK_BASE + 2) == ord2.count_of(TRACK_BASE + 2)
              && ord1.in_band_ns() == ord2.in_band_ns(),
          "账面必须是交换半群：顺序污染账面即对账失效");

    s.add("顺序敏感：同一多重集正序/逆序 → 环摘要不同",
          ord1.ring_digest() != ord2.ring_digest() && ord1.ring_len() == ord2.ring_len(),
          "观测面必须保序：两条账对顺序的态度相反，这才是分工的实证");

    let (inj, last) = run_injection();
    s.add("注入 10 轮：逐轮对账全等且账本守恒",
          inj.injected == ROUNDS && inj.detected == ROUNDS && inj.escaped == 0
              && inj.conserved() && last.all_equal(),
          "十个不同前缀长度都要过：只测最长前缀会漏掉早期对账缺陷");

    let mut cap = StatsLedger::new();
    let mut filled = 0u64;
    while filled < LEDGER_ROWS as u64 {
        let nr = TRACK_BASE + filled;
        if cap.record(nr, 1, false).is_ok() {
            filled += 1;
        } else {
            break;
        }
    }
    let over = cap.record(TRACK_BASE + 100, 1, false);
    s.add("账行容量：24 行填满后新号回 ENOMEM 且 dropped 记 1",
          filled == LEDGER_ROWS as u64
              && over == Err(ENOMEM)
              && cap.rows_len() == LEDGER_ROWS
              && cap.dropped() == 1
              && cap.observed() == LEDGER_ROWS as u64 + 1
              && cap.sorted(),
          "行满是显性部分成功：总数已计、行未建，dropped 必须留痕");

    let mut mono = meter;
    let before = mono.count_of(TRACK_BASE);
    mono.observe(TRACK_BASE, 1, false);
    mono.observe(TRACK_BASE, 1, false);
    let mut rst = mono;
    rst.reset();
    s.add("单调与重置：再观测只增、reset 归零",
          mono.count_of(TRACK_BASE) == before + 2 && mono.conserved()
              && rst.observed == 0 && rst.in_band_count() == 0 && rst.ring_len() == 0,
          "累计计数禁止回退；重置必须清干净，半清状态会让下一轮对账假绿");

    let fp_a = accounting_fingerprint();
    let fp_b = accounting_fingerprint();
    s.add("双跑一致：全量账指纹逐字节相等",
          fp_a == fp_b,
          "同脚本流两次必须相等，隐藏状态会立刻暴露");

    let mut uniq = true;
    let mut i = 0usize;
    while i < MeterCode::ALL.len() {
        let mut j = i + 1;
        while j < MeterCode::ALL.len() {
            if let (Some(a), Some(b)) = (MeterCode::ALL.get(i), MeterCode::ALL.get(j)) {
                if a.val() == b.val() || a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内序号与绝对码值都必须唯一，否则红项无法定位");

    s.add("文案闭集：12 个诊断码均有中文建议",
          MeterCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码都要给得出下一步动作");

    let recon_text = format!(
        "号 {} / 计数等 {} / 时长等 {} / 观测 {} / 拒绝 {} / 外带 {}",
        recon.numbers, recon.count_equal, recon.ns_equal,
        meter.observed, meter.denied, meter.out_of_band
    );
    s.add_dyn("对账快照", recon.all_equal(), &recon_text);

    let ring_text = format!(
        "环 {} 槽 head {} 摘要 0x{:08X} 首样本 ns {}",
        meter.ring_len(), meter.head(), meter.ring_digest(),
        match meter.ring_at(0) { Some(x) => x.ns, None => 0 }
    );
    s.add_dyn("环快照", meter.ring_len() == RING && meter.head() == 0, &ring_text);

    let fp_text = format!("账指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_counts_single_number() {
        let mut m = SyscallMeter::new();
        m.observe(TRACK_BASE + 3, 50, false);
        m.observe(TRACK_BASE + 3, 60, true);
        assert_eq!(m.count_of(TRACK_BASE + 3), 2);
        assert_eq!(m.ns_at(3), 110);
        assert_eq!(m.denied, 1);
        assert_eq!(m.observed, 2);
        assert_eq!(m.conserved(), true);
    }

    #[test]
    fn ledger_keeps_rows_sorted() {
        let mut b = StatsLedger::new();
        assert_eq!(b.record(TRACK_BASE + 5, 1, false).is_ok(), true);
        assert_eq!(b.record(TRACK_BASE + 1, 2, false).is_ok(), true);
        assert_eq!(b.record(TRACK_BASE + 3, 3, false).is_ok(), true);
        assert_eq!(b.rows_len(), 3);
        assert_eq!(b.sorted(), true);
        assert_eq!(b.count_of(TRACK_BASE + 1), 1);
        assert_eq!(b.ns_of(TRACK_BASE + 3), 3);
        assert_eq!(b.count_of(TRACK_BASE + 9), 0);
    }

    #[test]
    fn reconcile_matches_dense_and_sparse() {
        let events = script_events();
        let (m, b) = run_events(&events);
        let r = reconcile(&m, &b);
        assert_eq!(r.count_equal, 16);
        assert_eq!(r.ns_equal, 16);
        assert_eq!(r.observed_equal, true);
        assert_eq!(r.foreign_equal, true);
        assert_eq!(r.all_equal(), true);
    }

    #[test]
    fn ring_wraps_at_sixteen() {
        let events = script_events();
        let (m, _) = run_events(&events);
        assert_eq!(m.ring_len(), RING);
        assert_eq!(m.head(), 0);
        let first = m.ring_at(0);
        let last = m.ring_at(RING - 1);
        assert_eq!(first.is_some(), true);
        assert_eq!(last.is_some(), true);
        assert_eq!(m.ring_at(RING).is_none(), true);
    }

    #[test]
    fn order_matters_only_for_ring() {
        let mut a = SyscallMeter::new();
        let mut b = SyscallMeter::new();
        a.observe(1, 10, false);
        a.observe(2, 20, false);
        b.observe(2, 20, false);
        b.observe(1, 10, false);
        assert_eq!(a.in_band_count(), b.in_band_count());
        assert_eq!(a.out_of_band, 2);
        assert_eq!(b.out_of_band, 2);
        assert_ne!(a.ring_digest(), b.ring_digest());
    }

    #[test]
    fn percentiles_are_exact_on_ten_samples() {
        let s = percentiles(&[7, 3, 9, 1, 5, 2, 8, 4, 6, 10]);
        assert_eq!(s.count, 10);
        assert_eq!(s.min, 1);
        assert_eq!(s.p50, 6);
        assert_eq!(s.p99, 10);
        assert_eq!(s.max, 10);
        assert_eq!(s.mean, 5);
        assert_eq!(s.ordered(), true);
    }

    #[test]
    fn empty_window_is_all_zero() {
        let s = percentiles(&[]);
        assert_eq!(s.count, 0);
        assert_eq!(s.mean, 0);
        assert_eq!(s.ordered(), false);
    }

    #[test]
    fn ledger_capacity_is_explicit() {
        let mut b = StatsLedger::new();
        let mut i = 0u64;
        while i < LEDGER_ROWS as u64 {
            let r = b.record(TRACK_BASE + i, 1, false);
            assert_eq!(r.is_ok(), true);
            i += 1;
        }
        assert_eq!(b.rows_len(), LEDGER_ROWS);
        assert_eq!(b.record(TRACK_BASE + 100, 1, false), Err(ENOMEM));
        assert_eq!(b.dropped(), 1);
        assert_eq!(b.observed(), LEDGER_ROWS as u64 + 1);
    }

    #[test]
    fn injection_is_conserved() {
        let (inj, last) = run_injection();
        assert_eq!(inj.injected, ROUNDS);
        assert_eq!(inj.detected, ROUNDS);
        assert_eq!(inj.escaped, 0);
        assert_eq!(inj.conserved(), true);
        assert_eq!(last.all_equal(), true);
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(accounting_fingerprint(), accounting_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9368_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
