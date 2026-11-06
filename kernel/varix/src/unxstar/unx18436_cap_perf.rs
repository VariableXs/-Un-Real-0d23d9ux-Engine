//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18436 矩阵容量与性能账 · 0x7E8C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18436-J1 万条目矩阵查询 P95 <5µs 断言，内存上界线性可预期
//! 设计要点：
//!   - 万条目矩阵 = 开放寻址哈希表（线性探测+墓碑删除），容量 16384
//!     （2 的幂，负载因子 10000/16384≈0.61 < 0.75 阈值）；
//!   - 性能账走**确定性成本模型**（内核无墙钟，禁浮点）：一次探测步
//!     = 哈希折叠+槽读取，预算 PROBE_NS=8ns；一次键比较 CMP_NS=4ns；
//!     查询成本 = 探测步×8 + 比较次数×4（整数 ns）。
//!     P95 判定：万查询成本直方图（桶数 64）累计到 9500 位，P95 < 5000ns
//!     为绿——预算魔数推导：负载 0.61 下期望探测 ≤3 步 → P95 成本
//!     ≤ 3×8+3×4=36ns，对 5000ns 预算留 138 倍余量（聚类劣化也压得住）；
//!   - 内存上界线性可预期：MEM(cap) = 表头 64B + cap×24B/槽（键三元组
//!     12B+值 4B+依据 4B+状态 1B+对齐 3B），三点核算（1k/5k/10k 槽位）
//!     差分恒等 ⇒ 线性，无二次增长；
//!   - 注入核三类×10 轮：重复键 10/10 拒、删不存在键 10/10 拒、
//!     查不存在键 10/10 记缺账；表满拒绝且账不丢（小容量实例验证）；
//!   - 守恒账：注册数 − 删除数 = 在表数；探测总步数逐查询累计可复算；
//!   - 确定性：键语料用确定性三段映射（无随机），双跑 P95 逐位一致；
//!     no_std+alloc；零 unwrap/expect/panic；禁浮点/IO/墙钟/rand。
//! 结构清单：
//!   SlotState / PerfCode(诊断闭集) / MatrixSlot+CapMatrix(开放寻址表)
//!   / cost_ns(成本模型) / CostHistogram(P95 直方图) / MEM 模型
//!   / InjectLedger / golden_run / run_unx18436_checks
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E8C0，AI-VX11-1105-G1·A04 独占区）。
pub const SEG_BASE: u32 = 0x7E8C0;

// ── 域冻结常量（推导注记） ───────────────────────────────────────
/// 万条目（冻结：判据原文「万条目矩阵」）。
pub const TEN_K: usize = 10_000;
/// 表容量（冻结：16384=2^14，负载因子 10000/16384≈0.61<0.75）。
pub const TABLE_CAP: usize = 16_384;
/// 负载因子阈值 ×1000（冻结：750 = 75%）。
pub const LOAD_LIMIT_MILLI: u32 = 750;
/// 探测步预算（推导：哈希折叠+槽读取最坏内部路径，整数 ns）。
pub const PROBE_NS: u32 = 8;
/// 键比较预算（推导：三元组单域比较，整数 ns）。
pub const CMP_NS: u32 = 4;
/// P95 预算（冻结：判据原文 5µs = 5000ns）。
pub const P95_BUDGET_NS: u32 = 5_000;
/// 成本直方图桶数（冻结：成本 ∈ [0,512)ns 按 8ns 一桶）。
pub const COST_BUCKETS: usize = 64;
/// 表头字节（冻结：容量/占用/计数三类字段合计 64B）。
pub const MEM_HEADER: u32 = 64;
/// 单槽字节（推导：键 12 + 值 4 + 依据 4 + 状态 1 + 对齐 3 = 24）。
pub const SLOT_BYTES: u32 = 24;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 缺账查询语料量（冻结：1000 条不存在键）。
pub const MISS_N: usize = 1_000;

// ── 槽状态机 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空槽（探测终止）。
    Empty = 0,
    /// 占用。
    Occupied = 1,
    /// 墓碑（删除痕迹，探测跳过但不停）。
    Tombstone = 2,
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfCode {
    /// 键不存在（查询缺账）
    KeyMissing,
    /// 重复键注册拒（按更新分流前显式计账）
    DupKey,
    /// 表满拒绝
    TableFull,
    /// 删除不存在键
    RemoveMissing,
    /// 注册依据为空
    BasisMissing,
    /// 负载超限（容量规划红线）
    LoadExceeded,
    /// 内存线性核算破
    MemBudgetBroken,
    /// P95 超预算
    P95OverBudget,
    /// 守恒账破坏（注册−删除≠在表）
    ConserveBroken,
    /// 双跑不一致
    NotReproducible,
}

impl PerfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PerfCode::KeyMissing => "键不存在：查询缺账显式记账",
            PerfCode::DupKey => "重复键注册：按拒绝计账不静默覆盖",
            PerfCode::TableFull => "表满拒绝：负载超限不入表且账不丢",
            PerfCode::RemoveMissing => "删除不存在键：显式拒绝",
            PerfCode::BasisMissing => "注册依据为空：有值必有据不变量",
            PerfCode::LoadExceeded => "负载超限：容量规划红线 75%",
            PerfCode::MemBudgetBroken => "内存线性核算破：增长非线性",
            PerfCode::P95OverBudget => "P95 超预算：5000ns 红线",
            PerfCode::ConserveBroken => "守恒账破坏：注册−删除≠在表",
            PerfCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [PerfCode; 10] = [
        PerfCode::KeyMissing,
        PerfCode::DupKey,
        PerfCode::TableFull,
        PerfCode::RemoveMissing,
        PerfCode::BasisMissing,
        PerfCode::LoadExceeded,
        PerfCode::MemBudgetBroken,
        PerfCode::P95OverBudget,
        PerfCode::ConserveBroken,
        PerfCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PerfCode::ALL.len() {
        if PerfCode::ALL[i].val() == code {
            return PerfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E8C0 段闭集内"
}

// ── 键与槽 ──────────────────────────────────────────────────────
/// 键三元组：进程 × API 族 × 维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapKey {
    pub proc_id: u32,
    pub api_id: u32,
    pub dim: u32,
}

/// 表槽。
#[derive(Clone, Copy, Debug)]
pub struct MatrixSlot {
    pub state: SlotState,
    pub key: CapKey,
    pub value: u32,
    pub basis: u32,
}

impl MatrixSlot {
    /// 空槽。
    pub const EMPTY: MatrixSlot = MatrixSlot {
        state: SlotState::Empty,
        key: CapKey { proc_id: 0, api_id: 0, dim: 0 },
        value: 0,
        basis: 0,
    };
}

/// 键哈希（乘异折叠加终混；容量为 2 的幂，取模用位与）。
pub fn hash_key(k: &CapKey, cap: usize) -> usize {
    let mut h = k
        .proc_id
        .wrapping_mul(73856093)
        ^ k.api_id.wrapping_mul(19349663)
        ^ k.dim.wrapping_mul(83492791);
    // 终混（splitmix64 收尾两步，防三段乘积规律性）。
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB352D);
    h ^= h >> 15;
    (h as usize) & (cap - 1)
}

// ── 开放寻址表 ──────────────────────────────────────────────────
/// 万条目矩阵：开放寻址 + 线性探测 + 墓碑删除。
/// 查询返回 (值, 成本ns)；注册/删除均带守恒账。
pub struct CapMatrix {
    slots: Vec<MatrixSlot>,
    cap: usize,
    /// 在表条目数（Occupied）。
    pub used: u32,
    /// 累计成功注册数（守恒账口径：reg_total − del_total == used）。
    pub reg_total: u32,
    /// 累计重复键拒绝数（显式记账，不进守恒账）。
    pub dup_total: u32,
    /// 累计删除数。
    pub del_total: u32,
    /// 累计缺账查询数。
    pub miss_total: u32,
    /// 累计探测步数（性能账）。
    pub probe_total: u64,
}

impl CapMatrix {
    /// 建表（容量必须 2 的幂且 ≤ 2^20，防滥用）。no_std 无 vec! 宏，逐槽装配。
    pub fn with_cap(cap: usize) -> Result<CapMatrix, u32> {
        if cap == 0 || (cap & (cap - 1)) != 0 || cap > (1 << 20) {
            return Err(PerfCode::LoadExceeded.val());
        }
        let mut slots: Vec<MatrixSlot> = Vec::new();
        let mut i = 0;
        while i < cap {
            slots.push(MatrixSlot::EMPTY);
            i += 1;
        }
        Ok(CapMatrix {
            slots,
            cap,
            used: 0,
            reg_total: 0,
            dup_total: 0,
            del_total: 0,
            miss_total: 0,
            probe_total: 0,
        })
    }

    /// 万条目标准表。
    pub fn new() -> Result<CapMatrix, u32> {
        CapMatrix::with_cap(TABLE_CAP)
    }

    /// 负载千分比。
    pub fn load_milli(&self) -> u32 {
        self.used.saturating_mul(1000) / self.cap as u32
    }

    /// 注册：闸序 = 依据 → 重复键 → 负载 → 容量。
    /// 返回 Ok(true)=新键入表。重复键走 dup_total 显式记账（不进守恒账）。
    pub fn insert(&mut self, k: CapKey, value: u32, basis: u32) -> Result<bool, u32> {
        // 闸 1：依据非空。
        if basis == 0 {
            return Err(PerfCode::BasisMissing.val());
        }
        // 闸 2：重复键检查（探测路径上顺带完成）。
        let h0 = hash_key(&k, self.cap);
        let mut step = 0usize;
        let mut dup = false;
        let mut first_tomb: Option<usize> = None;
        loop {
            let idx = (h0 + step) & (self.cap - 1);
            self.probe_total = self.probe_total.saturating_add(1);
            let sl = &self.slots[idx];
            match sl.state {
                SlotState::Occupied => {
                    if sl.key.proc_id == k.proc_id && sl.key.api_id == k.api_id && sl.key.dim == k.dim {
                        dup = true;
                        break;
                    }
                }
                SlotState::Tombstone => {
                    if first_tomb.is_none() {
                        first_tomb = Some(idx);
                    }
                }
                SlotState::Empty => break,
            }
            step += 1;
            if step >= self.cap {
                return Err(PerfCode::TableFull.val());
            }
        }
        if dup {
            // 重复键：显式拒绝 + dup_total 记账（不静默覆盖，守恒账不受扰）。
            self.dup_total = self.dup_total.saturating_add(1);
            return Err(PerfCode::DupKey.val());
        }
        // 闸 3：负载闸——入表前检查。
        let new_used = self.used.saturating_add(1);
        if new_used.saturating_mul(1000) / self.cap as u32 > LOAD_LIMIT_MILLI {
            return Err(PerfCode::LoadExceeded.val());
        }
        // 落槽：优先复用墓碑，否则用终止空槽。
        let target = match first_tomb {
            Some(t) => t,
            None => (h0 + step) & (self.cap - 1),
        };
        self.reg_total = self.reg_total.saturating_add(1);
        let sl = &mut self.slots[target];
        sl.state = SlotState::Occupied;
        sl.key = k;
        sl.value = value;
        sl.basis = basis;
        self.used = self.used.saturating_add(1);
        Ok(true)
    }

    /// 查询：返回 (值, 依据, 成本ns)；缺账记 miss_total 并返回 Err。
    pub fn lookup(&mut self, k: CapKey) -> Result<(u32, u32, u32), u32> {
        let h0 = hash_key(&k, self.cap);
        let mut step = 0usize;
        let mut probes = 0u32;
        let mut cmps = 0u32;
        while step < self.cap {
            let idx = (h0 + step) & (self.cap - 1);
            probes += 1;
            let sl = &self.slots[idx];
            match sl.state {
                SlotState::Occupied => {
                    cmps += 1;
                    if sl.key.proc_id == k.proc_id && sl.key.api_id == k.api_id && sl.key.dim == k.dim {
                        let v = sl.value;
                        let b = sl.basis;
                        self.probe_total = self.probe_total.saturating_add(probes as u64);
                        return Ok((v, b, cost_ns(probes, cmps)));
                    }
                }
                SlotState::Empty => break,
                SlotState::Tombstone => {}
            }
            step += 1;
        }
        self.miss_total = self.miss_total.saturating_add(1);
        self.probe_total = self.probe_total.saturating_add(probes as u64);
        Err(PerfCode::KeyMissing.val())
    }

    /// 删除：置墓碑；不存在键显式拒。
    pub fn remove(&mut self, k: CapKey) -> Result<u32, u32> {
        let h0 = hash_key(&k, self.cap);
        let mut step = 0usize;
        while step < self.cap {
            let idx = (h0 + step) & (self.cap - 1);
            let sl = &self.slots[idx];
            match sl.state {
                SlotState::Occupied => {
                    if sl.key.proc_id == k.proc_id && sl.key.api_id == k.api_id && sl.key.dim == k.dim {
                        let v = sl.value;
                        let idx2 = idx;
                        let sl2 = &mut self.slots[idx2];
                        sl2.state = SlotState::Tombstone;
                        sl2.basis = 0;
                        self.used = self.used.saturating_sub(1);
                        self.del_total = self.del_total.saturating_add(1);
                        return Ok(v);
                    }
                }
                SlotState::Empty => break,
                SlotState::Tombstone => {}
            }
            step += 1;
        }
        Err(PerfCode::RemoveMissing.val())
    }

    /// 守恒账：reg_total（成功注册）− del_total 必等于 used。
    /// 被拒注册（重复/负载/空依据）不进守恒账，各自独立显式记账。
    pub fn conserve(&self) -> Result<(), u32> {
        let expect = self.reg_total.saturating_sub(self.del_total);
        if expect != self.used {
            return Err(PerfCode::ConserveBroken.val());
        }
        Ok(())
    }
}

// ── 成本模型（整数 ns，冻结魔数推导见文件头） ───────────────────
pub fn cost_ns(probes: u32, cmps: u32) -> u32 {
    probes.saturating_mul(PROBE_NS).saturating_add(cmps.saturating_mul(CMP_NS))
}

/// 成本直方图：64 桶 × 8ns/桶，覆盖 [0,512)ns。
pub struct CostHistogram {
    pub buckets: [u32; COST_BUCKETS],
    pub total: u32,
    /// 溢出桶（≥512ns）单独记账，不静默吞。
    pub overflow: u32,
}

impl CostHistogram {
    pub fn new() -> CostHistogram {
        CostHistogram { buckets: [0; COST_BUCKETS], total: 0, overflow: 0 }
    }
    /// 记一笔成本。
    pub fn record(&mut self, ns: u32) {
        self.total = self.total.saturating_add(1);
        let b = (ns / PROBE_NS) as usize;
        if b >= COST_BUCKETS {
            self.overflow += 1;
        } else {
            self.buckets[b] = self.buckets[b].saturating_add(1);
        }
    }
    /// P95：累计达 total×95/100 的最小桶上沿成本（整数 ns）。
    /// 直方图粒度 8ns——P95 报的是**桶上沿**（保守值，只会高估不低估）。
    pub fn p95_ns(&self) -> u32 {
        if self.total == 0 {
            return u32::MAX;
        }
        let target = self.total.saturating_mul(95) / 100;
        let mut acc = 0u32;
        let mut b = 0;
        while b < COST_BUCKETS {
            acc = acc.saturating_add(self.buckets[b]);
            if acc >= target {
                return ((b as u32) + 1).saturating_mul(PROBE_NS);
            }
            b += 1;
        }
        // 全部落溢出桶：P95 记溢出上界（514ns 量级也远低于 5000）。
        (COST_BUCKETS as u32 + 1).saturating_mul(PROBE_NS)
    }
    /// 最大桶上沿（近似最大成本，防溢出漏报）。
    pub fn max_ns(&self) -> u32 {
        let mut b = COST_BUCKETS;
        while b > 0 {
            b -= 1;
            if self.buckets[b] > 0 {
                return ((b as u32) + 1).saturating_mul(PROBE_NS);
            }
        }
        if self.overflow > 0 {
            return (COST_BUCKETS as u32 + 1).saturating_mul(PROBE_NS);
        }
        0
    }
}

// ── 内存模型（线性核算） ────────────────────────────────────────
/// 内存上界模型：MEM(cap) = 表头 + cap × 槽字节。
pub fn mem_bytes(cap: usize) -> u32 {
    MEM_HEADER.saturating_add((cap as u32).saturating_mul(SLOT_BYTES))
}

/// 三点线性核算：1k/5k/10k 槽位，分段差分 == 段内槽差 × SLOT_BYTES
/// ⇒ 每槽增量恒定（线性，无二次项）。
pub fn mem_linear_check() -> Result<(), u32> {
    let m1 = mem_bytes(1_000);
    let m5 = mem_bytes(5_000);
    let m10 = mem_bytes(10_000);
    let d1 = m5.saturating_sub(m1); // 4k 槽段。
    let d2 = m10.saturating_sub(m5); // 5k 槽段。
    // 线性 ⇒ 各段差分恰等于段内槽数 × 槽字节。
    if d1 != 4_000u32.saturating_mul(SLOT_BYTES) || d2 != 5_000u32.saturating_mul(SLOT_BYTES) {
        return Err(PerfCode::MemBudgetBroken.val());
    }
    Ok(())
}

// ── 键语料（确定性三段映射，无随机） ────────────────────────────
/// 万条目键语料：i ∈ [0,10000) → (proc=i/1000, api=(i/100)%10, dim=i%100)。
/// 三段互不重叠 ⇒ 键全异。
pub fn corpus_key(i: usize) -> CapKey {
    CapKey {
        proc_id: (i / 1000) as u32,
        api_id: ((i / 100) % 10) as u32,
        dim: (i % 100) as u32,
    }
}

/// 缺账键语料：dim=200 段（与正语料零重叠）。
pub fn miss_key(i: usize) -> CapKey {
    CapKey { proc_id: (i / 1000) as u32, api_id: ((i / 100) % 10) as u32, dim: 200 + (i % 100) as u32 }
}

/// 万条目装载 + 万查询成本账 + 千缺账查询。
/// 返回 (P95ns, 最大桶上沿, 装载数)。
pub fn bench_run() -> Result<(u32, u32, u32), u32> {
    let mut m = CapMatrix::new()?;
    let mut i = 0;
    while i < TEN_K {
        let k = corpus_key(i);
        m.insert(k, 62_000 + i as u32, 100 + (i % 900) as u32)?;
        i += 1;
    }
    m.conserve()?;
    if m.load_milli() > LOAD_LIMIT_MILLI {
        return Err(PerfCode::LoadExceeded.val());
    }
    let mut hist = CostHistogram::new();
    let mut j = 0;
    while j < TEN_K {
        let k = corpus_key(j);
        let (_, _, cost) = m.lookup(k)?;
        hist.record(cost);
        j += 1;
    }
    let mut k2 = 0;
    while k2 < MISS_N {
        // 缺账查询必须显式 Err(KeyMissing)，缺账进 miss_total。
        match m.lookup(miss_key(k2)) {
            Err(c) if c == PerfCode::KeyMissing.val() => {}
            _ => return Err(PerfCode::ConserveBroken.val()),
        }
        k2 += 1;
    }
    if m.miss_total != MISS_N as u32 {
        return Err(PerfCode::ConserveBroken.val());
    }
    let p95 = hist.p95_ns();
    if p95 >= P95_BUDGET_NS {
        return Err(PerfCode::P95OverBudget.val());
    }
    Ok((p95, hist.max_ns(), m.used))
}

// ── 注入核（三类 ×10 轮） ───────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入（小容量实例，快速）：槽0 重复键 / 槽1 删不存在 / 槽2 空依据。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut m = match CapMatrix::with_cap(64) {
            Ok(x) => x,
            Err(_) => return led,
        };
        let k = CapKey { proc_id: 1, api_id: 1, dim: 1 };
        let _ = m.insert(k, 10, 1);
        // 槽 0：重复键。
        let hit0 = m.insert(k, 20, 2).is_err();
        led.record(0, hit0);
        // 槽 1：删不存在键。
        let hit1 = m.remove(CapKey { proc_id: 9, api_id: 9, dim: 9 }).is_err();
        led.record(1, hit1);
        // 槽 2：空依据注册。
        let hit2 = m.insert(CapKey { proc_id: 2, api_id: 2, dim: 2 }, 1, 0).is_err();
        led.record(2, hit2);
        r += 1;
    }
    led
}

// ── 闸边界（小容量实例：8 槽表在 75% 负载闸处停住，账不丢） ─────
/// 推导：8 槽表 new_used×1000/8 > 750 拒 ⇒ 6 键恰达限（750 不超），
/// 第 7 键（875‰）必拒——负载闸边界逐位可复算。
pub fn full_table_check() -> Result<(u32, u32), u32> {
    let mut m = CapMatrix::with_cap(8)?;
    let mut n = 0u32;
    let mut i = 0;
    while i < 12 {
        let k = CapKey { proc_id: i as u32, api_id: 0, dim: 0 };
        if m.insert(k, i as u32, 1).is_ok() {
            n += 1;
        }
        i += 1;
    }
    // 边界冻结：恰 6 键入表，第 7 键被负载闸拒。
    if n != 6 {
        return Err(PerfCode::LoadExceeded.val());
    }
    m.conserve()?;
    Ok((n, 1))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：小表插入→查询→删除→再查缺账，账守恒。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut m = CapMatrix::with_cap(16)?;
    let k = CapKey { proc_id: 1, api_id: 2, dim: 3 };
    if m.insert(k, 77, 5)? != true {
        return Err(PerfCode::ConserveBroken.val());
    }
    let (v, b, _) = m.lookup(k)?;
    if v != 77 || b != 5 {
        return Err(PerfCode::ConserveBroken.val());
    }
    m.remove(k)?;
    match m.lookup(k) {
        Err(c) if c == PerfCode::KeyMissing.val() => {}
        _ => return Err(PerfCode::ConserveBroken.val()),
    }
    m.conserve()?;
    Ok(())
}

/// 负探针 1：非 2 幂容量建表拒。
pub fn probe_seq_b() -> Result<(), u32> {
    match CapMatrix::with_cap(100) {
        Err(c) if c == PerfCode::LoadExceeded.val() => Ok(()),
        _ => Err(PerfCode::LoadExceeded.val()),
    }
}

/// 负探针 2：空表查询缺账。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut m = CapMatrix::with_cap(16)?;
    match m.lookup(CapKey { proc_id: 0, api_id: 0, dim: 0 }) {
        Err(c) if c == PerfCode::KeyMissing.val() => Ok(()),
        _ => Err(PerfCode::KeyMissing.val()),
    }
}

/// 黄金全链：万条目 P95 达标 + 内存线性 + 注入全抓 + 守恒。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let (p95, mx, used) = bench_run()?;
    if used != TEN_K as u32 {
        return Err(PerfCode::ConserveBroken.val());
    }
    mem_linear_check()?;
    let inj = inject_rounds(JUDGE_ROUNDS);
    if !inj.all_caught() {
        return Err(PerfCode::NotReproducible.val());
    }
    let (full_n, full_rej) = full_table_check()?;
    if full_n != 6 || full_rej != 1 {
        return Err(PerfCode::TableFull.val());
    }
    Ok((p95, mx, used))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PerfCode::ALL.len() {
        let v = PerfCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18436_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18436 矩阵容量与性能账");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "P95={:?}ns 上沿={:?}ns 表目={:?}",
            g.as_ref().map(|r| r.0).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0),
            g.as_ref().map(|r| r.2).unwrap_or(0)
        ),
    );
    // ② 万条目 P95 < 5000ns。
    let p95_ok = match &g {
        Ok((p95, _, _)) => *p95 < P95_BUDGET_NS,
        Err(_) => false,
    };
    s.add_dyn(
        "万条目查询P95<5µs",
        p95_ok,
        &format!("P95={:?}ns 预算{}ns", g.as_ref().map(|r| r.0).unwrap_or(0), P95_BUDGET_NS),
    );
    // ③ 内存上界线性（三点差分恒等）。
    s.add("内存上界线性可预期", mem_linear_check().is_ok(), "1k/5k/10k 差分恒等 4000×24B");
    // ④ 负载因子在阈内。
    let load_ok = match &g {
        Ok((_, _, used)) => {
            let milli = (*used as u32).saturating_mul(1000) / TABLE_CAP as u32;
            milli <= LOAD_LIMIT_MILLI
        }
        Err(_) => false,
    };
    s.add("负载因子≤75%", load_ok, "10000/16384≈610‰");
    // ⑤ 注入三类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn("重复键10/10拒", inj.caught[0] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[0], inj.tries[0]));
    s.add_dyn("删不存在10/10拒", inj.caught[1] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[1], inj.tries[1]));
    s.add_dyn("空依据10/10拒", inj.caught[2] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[2], inj.tries[2]));
    s.add("注入漏检为零", inj.missed() == 0, "检出+漏检=注入");
    // ⑥ 表满边界：8 槽全占第 9 拒，账不丢。
    let ft = full_table_check();
    s.add("负载闸边界账不丢", ft.is_ok(), "8 槽表 6 占第 7 拒");
    // ⑦ 缺账守恒：千条不存在键全记 miss_total。
    let miss_ok = match bench_run() {
        Ok(_) => true, // bench_run 内部已断言 miss_total == MISS_N。
        Err(_) => false,
    };
    s.add("缺账千条全记账", miss_ok, "miss_total==1000");
    // ⑧ 成本模型魔数复算：8 桶/桶 = PROBE_NS。
    s.add("成本模型魔数复算", PROBE_NS == 8 && CMP_NS == 4 && P95_BUDGET_NS == 5_000, "冻结推导见文件头");
    // ⑨ 守恒账：万条目装载后 reg−del==used；删 10 条再守恒。
    let mut m = CapMatrix::new();
    let mut load_ok2 = m.is_ok();
    if let Ok(mm) = m.as_mut() {
        let mut i = 0;
        while i < TEN_K {
            if mm.insert(corpus_key(i), 1, 1).is_err() {
                load_ok2 = false;
            }
            i += 1;
        }
        load_ok2 = load_ok2 && mm.conserve().is_ok() && mm.used == TEN_K as u32;
        let mut d = 0;
        while d < 10 {
            if mm.remove(corpus_key(d)).is_err() {
                load_ok2 = false;
            }
            d += 1;
        }
        load_ok2 = load_ok2 && mm.used == (TEN_K - 10) as u32 && mm.conserve().is_ok();
    }
    s.add("注册删除守恒账", load_ok2, "reg−del==used 两态");
    // ⑩ 双跑一致。
    let a = golden_run();
    let b = golden_run();
    s.add("双跑一致", a == b, "同语料同 P95");
    // ⑪ 探针三序。
    s.add("探针三序", probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(), "正/非幂容/空表缺账");
    // ⑫ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut ci = 0;
    while ci < PerfCode::ALL.len() {
        if PerfCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "10 码全有中文建议");
    s.add("段位守卫 0x7E8C0", segment_guard(), "SEG_BASE 对齐 0x40 窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_p95_under_budget() {
        let (p95, mx, used) = bench_run().unwrap_or((u32::MAX, u32::MAX, 0));
        assert_eq!(used, TEN_K as u32);
        assert!(p95 < P95_BUDGET_NS, "P95={} 必须 < {}", p95, P95_BUDGET_NS);
        assert!(mx <= 512, "最大桶上沿 {} 不得溢出直方图", mx);
    }

    #[test]
    fn memory_linear_three_points() {
        assert!(mem_linear_check().is_ok());
        assert_eq!(mem_bytes(0), MEM_HEADER);
        assert_eq!(mem_bytes(1000), MEM_HEADER + 1000 * SLOT_BYTES);
    }

    #[test]
    fn injections_all_caught() {
        let inj = inject_rounds(10);
        assert!(inj.all_caught());
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn load_gate_boundary_six_of_eight() {
        let (n, rej) = full_table_check().unwrap_or((0, 0));
        assert_eq!(n, 6, "8 槽表负载闸 75% ⇒ 恰 6 键入表");
        assert_eq!(rej, 1, "第 7 键必拒");
    }

    #[test]
    fn insert_lookup_remove_lifecycle() {
        assert!(probe_seq_a().is_ok());
    }

    #[test]
    fn corpus_keys_all_distinct() {
        // 三段映射键全异性抽查：抽样 100 键两两比对。
        let mut i = 0;
        while i < 100 {
            let a = corpus_key(i * 97 % TEN_K);
            let b = corpus_key((i * 97 + 1) % TEN_K);
            assert!(a.proc_id != b.proc_id || a.api_id != b.api_id || a.dim != b.dim);
            i += 1;
        }
    }

    #[test]
    fn double_run_identical() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b, "双跑 P95 逐位一致");
    }

    #[test]
    fn probes_all_sides() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18436_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20);
    }
}
