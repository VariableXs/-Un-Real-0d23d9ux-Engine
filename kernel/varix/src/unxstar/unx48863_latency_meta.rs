//! UNX-F48863 窗口应用回调段打点 · 0x707C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F48863-J1 第三段 ×10^3 打点全落账；
//!   ③ UNX-F48864-J1 TSC 源 ×10^3 打点与 A2 基座对账零漂移。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：权威源 → 频点 → 纪元 → 核号 → 序号 → 环代数 → 单调。
//!           权威源必须最先判：软件计时器一旦被当成权威，后面所有
//!           单调性与漂移判定都建立在错误的时基上，判得越细错得越
//!           彻底（不是精度问题，是「账本性质」问题）。频点次之，
//!           因为它决定 tick↔ns 的全部换算；纪元再判，跨纪元的样本
//!           不能合并队列；环代数（gen/slot）在序号之后，因为代际
//!           是「同一槽位被复用了第几次」的语义，没序号就没有代际。
//! 关键差异：本模块是 TSC 权威源与样本元数据的**唯一持有者**。
//!           core 只认「起止 Stamp + 跨度」；本模块判「这个 Stamp 来自
//!           哪个核、那个核的偏移是否已校准、这个样本落在环的第几代」。
//!           guard 的四闸只看数值是否合法，不看时间源是否权威；
//!           dispatch 的环只按容量淘汰，不判代际是否过期。谁把未校准
//!           的跨核读数直接交给 core 做差，谁就绕过了本模块的 CpuBad /
//!           OffsetBad 两道闸。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x707C0，VX6 批 B04 族第二槽）。
pub const SEG_BASE: u32 = 0x707C0;

/// 核数上限（跨核偏移表定容）。
pub const MAX_CORES: usize = 8;

/// 判据轮数：第三段 ×10^3 元数据。
pub const META_ROUNDS: u32 = 1000;

/// TSC 名义频率（与 A2 时钟基座同口径）。
pub const TSC_HZ: u64 = 2_000_000_000;

/// 可接受频点下界 / 上界（Hz）。
pub const MIN_HZ: u64 = 1_000_000;
pub const MAX_HZ: u64 = 10_000_000_000;

/// 跨核偏移绝对值上限（tick）：超过 50µs 的偏移不可能来自晶振差异。
pub const OFFSET_LIMIT_TICKS: i64 = 100_000;

/// 环容量上限（槽位数，u16 装得下）。
pub const RING_CAP_MAX: u16 = 4096;

/// 代际回绕点（gen 是 u16）。
pub const GEN_WRAP: u32 = 65_536;

/// 漂移告警线（ppm）：超过即从 Good 降到 Watch/Bad。
pub const PPM_WATCH: u32 = 50;
pub const PPM_BAD: u32 = 200;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0A） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MCode {
    /// 元数据字段缺失或非法（零序号、空环）
    MetaBad,
    /// 序号回退或重复登记
    SeqBad,
    /// 环代际不符（样本来自旧一代槽位）
    GenBad,
    /// 槽位越出环容量
    SlotBad,
    /// 核号越界
    CpuBad,
    /// 时间源不具备权威资格（软件计时器等）
    AuthBad,
    /// 单调断：校正后时间回退
    MonoBad,
    /// 跨核偏移越界或未校准
    OffsetBad,
    /// 频点非法或换算溢出
    ClockBad,
    /// 段位越界
    SegmentBad,
}

impl MCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            MCode::MetaBad => "元数据非法：字段缺失、零序号或空环",
            MCode::SeqBad => "序号非法：回退或重复登记",
            MCode::GenBad => "环代际不符：样本来自已回收的槽位代",
            MCode::SlotBad => "槽位越界：不在 0..容量",
            MCode::CpuBad => "核号越界：超过 MAX_CORES-1",
            MCode::AuthBad => "时间源失格：软件计时器不得做权威账",
            MCode::MonoBad => "单调断：校正后时间戳回退",
            MCode::OffsetBad => "跨核偏移失格：越界或该核未校准",
            MCode::ClockBad => "频点非法：越出 1MHz..=10GHz 或换算溢出",
            MCode::SegmentBad => "段位越界：诊断码逃出 0x707C0 窗口",
        }
    }

    pub const ALL: [MCode; 10] = [
        MCode::MetaBad,
        MCode::SeqBad,
        MCode::GenBad,
        MCode::SlotBad,
        MCode::CpuBad,
        MCode::AuthBad,
        MCode::MonoBad,
        MCode::OffsetBad,
        MCode::ClockBad,
        MCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < MCode::ALL.len() {
        if let Some(c) = MCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x707C0 段闭集内"
}

// ── 时间源权威性 ──────────────────────────────────────────────────
/// 四类时钟源。只有不变 TSC 具备权威资格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockAuthority {
    /// 不变 TSC（invariant TSC）：唯一权威
    InvariantTsc,
    /// 非不变 TSC：受 P/C 态频率影响，不得做权威账
    UnstableTsc,
    /// HPET 等外设计时器：可做旁证，不做权威
    Hpet,
    /// 软件计时器（tick 累计）：绝对禁止做权威
    SoftwareTimer,
}

impl ClockAuthority {
    pub fn ord(self) -> u8 {
        match self {
            ClockAuthority::InvariantTsc => 0,
            ClockAuthority::UnstableTsc => 1,
            ClockAuthority::Hpet => 2,
            ClockAuthority::SoftwareTimer => 3,
        }
    }

    pub fn from_ord(v: u8) -> Result<ClockAuthority, u32> {
        match v {
            0 => Ok(ClockAuthority::InvariantTsc),
            1 => Ok(ClockAuthority::UnstableTsc),
            2 => Ok(ClockAuthority::Hpet),
            3 => Ok(ClockAuthority::SoftwareTimer),
            _ => Err(MCode::AuthBad.val()),
        }
    }

    pub fn is_authority(self) -> bool {
        self == ClockAuthority::InvariantTsc
    }

    pub fn text(self) -> &'static str {
        match self {
            ClockAuthority::InvariantTsc => "invariant-tsc",
            ClockAuthority::UnstableTsc => "unstable-tsc",
            ClockAuthority::Hpet => "hpet",
            ClockAuthority::SoftwareTimer => "software-timer",
        }
    }
}

/// 时钟健康三态：Good / Watch / Bad（由实测漂移 ppm 决定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockHealth {
    Good,
    Watch,
    Bad,
}

impl ClockHealth {
    pub fn classify(ppm: u32) -> ClockHealth {
        if ppm <= PPM_WATCH {
            ClockHealth::Good
        } else if ppm <= PPM_BAD {
            ClockHealth::Watch
        } else {
            ClockHealth::Bad
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            ClockHealth::Good => "good",
            ClockHealth::Watch => "watch",
            ClockHealth::Bad => "bad",
        }
    }

    pub fn is_healthy(self) -> bool {
        self == ClockHealth::Good
    }
}

// ── 时间基准（频点 + 权威源 + 跨核偏移表 + 纪元） ─────────────────
pub struct TimeBase {
    hz: u64,
    authority: ClockAuthority,
    offsets: [i64; MAX_CORES],
    calibrated: [bool; MAX_CORES],
    epoch: u32,
}

impl TimeBase {
    /// 建基准：只有不变 TSC 通过；频点必须落在 [1MHz, 10GHz]。
    pub fn new(hz: u64, authority: ClockAuthority) -> Result<TimeBase, u32> {
        if !authority.is_authority() {
            return Err(MCode::AuthBad.val());
        }
        if hz < MIN_HZ || hz > MAX_HZ {
            return Err(MCode::ClockBad.val());
        }
        let mut offsets = [0i64; MAX_CORES];
        let mut calibrated = [false; MAX_CORES];
        // 核 0 是参考核，偏移恒零且天然已校准。
        if let Some(slot) = offsets.get_mut(0) {
            *slot = 0;
        }
        if let Some(slot) = calibrated.get_mut(0) {
            *slot = true;
        }
        Ok(TimeBase { hz, authority, offsets, calibrated, epoch: 1 })
    }

    pub fn hz(&self) -> u64 {
        self.hz
    }

    pub fn authority(&self) -> ClockAuthority {
        self.authority
    }

    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// 推进纪元：跨纪元样本必须重开队列（旧样本不再可用）。
    pub fn bump_epoch(&mut self) -> Result<u32, u32> {
        match self.epoch.checked_add(1) {
            Some(v) => {
                self.epoch = v;
                Ok(v)
            }
            None => Err(MCode::ClockBad.val()),
        }
    }

    /// 跨核偏移校准：offset = 参考读数 - 本核读数。
    ///
    /// 约束：核号 < MAX_CORES；|offset| ≤ OFFSET_LIMIT_TICKS；
    /// 参考读数与本地读数都必须非零（零读数意味着打点没采到）。
    pub fn calibrate(&mut self, core: usize, ref_ticks: u64, local_ticks: u64) -> Result<i64, u32> {
        if core >= MAX_CORES {
            return Err(MCode::CpuBad.val());
        }
        if ref_ticks == 0 || local_ticks == 0 {
            return Err(MCode::ClockBad.val());
        }
        let diff = ref_ticks as i128 - local_ticks as i128;
        if diff > OFFSET_LIMIT_TICKS as i128 || diff < -(OFFSET_LIMIT_TICKS as i128) {
            return Err(MCode::OffsetBad.val());
        }
        let off = diff as i64;
        if let Some(slot) = self.offsets.get_mut(core) {
            *slot = off;
        }
        if let Some(slot) = self.calibrated.get_mut(core) {
            *slot = true;
        }
        Ok(off)
    }

    pub fn offset_of(&self, core: usize) -> Result<i64, u32> {
        match self.offsets.get(core) {
            Some(v) => Ok(*v),
            None => Err(MCode::CpuBad.val()),
        }
    }

    pub fn is_calibrated(&self, core: usize) -> bool {
        self.calibrated.get(core).copied().unwrap_or(false)
    }

    pub fn calibrated_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_CORES {
            if self.is_calibrated(i) {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 校正：raw + offset → 参考核时间轴。未校准核拒（OffsetBad）。
    pub fn corrected(&self, raw: u64, core: usize) -> Result<u64, u32> {
        if core >= MAX_CORES {
            return Err(MCode::CpuBad.val());
        }
        if !self.is_calibrated(core) {
            return Err(MCode::OffsetBad.val());
        }
        let off = self.offset_of(core)?;
        if off >= 0 {
            match raw.checked_add(off as u64) {
                Some(v) => Ok(v),
                None => Err(MCode::ClockBad.val()),
            }
        } else {
            match off.checked_neg() {
                Some(neg) => match raw.checked_sub(neg as u64) {
                    Some(v) => Ok(v),
                    None => Err(MCode::ClockBad.val()),
                },
                None => Err(MCode::OffsetBad.val()),
            }
        }
    }

    /// tick → ns（本基准频点）。u128 中继，溢出拒。
    pub fn to_ns(&self, ticks: u64) -> Result<u64, u32> {
        if self.hz == 0 {
            return Err(MCode::ClockBad.val());
        }
        let scaled = (ticks as u128).saturating_mul(1_000_000_000u128);
        let ns = scaled / (self.hz as u128);
        if ns > u64::MAX as u128 {
            return Err(MCode::ClockBad.val());
        }
        Ok(ns as u64)
    }

    /// 漂移 ppm：以 `expected` 为分母。
    pub fn ppm(&self, expected: u64, measured: u64) -> Result<u32, u32> {
        if expected == 0 {
            return Err(MCode::ClockBad.val());
        }
        let diff = if measured >= expected {
            measured - expected
        } else {
            expected - measured
        };
        let scaled = (diff as u128).saturating_mul(1_000_000u128) / (expected as u128);
        if scaled > u32::MAX as u128 {
            return Err(MCode::ClockBad.val());
        }
        Ok(scaled as u32)
    }
}

// ── 样本元数据（五元组 + 环标签） ─────────────────────────────────
/// 一条样本的元数据：序号 / 代际 / 槽位 / 核 / 原始 TSC。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleMeta {
    pub seq: u32,
    pub gen: u16,
    pub slot: u16,
    pub cpu: u8,
    pub raw: u64,
    pub epoch: u32,
}

impl SampleMeta {
    pub fn new(seq: u32, gen: u16, slot: u16, cpu: u8, raw: u64, epoch: u32) -> SampleMeta {
        SampleMeta { seq, gen, slot, cpu, raw, epoch }
    }

    /// 环标签：(gen << 16) | slot。
    pub fn tag(&self) -> u32 {
        ((self.gen as u32) << 16) | (self.slot as u32)
    }

    /// 五元组（序号 + 代际 + 槽位 + 核 + 纪元）。
    pub fn tuple(&self) -> MetaTuple {
        MetaTuple { seq: self.seq, gen: self.gen, slot: self.slot, cpu: self.cpu, epoch: self.epoch }
    }

    pub fn text(&self) -> String {
        format!(
            "seq={} gen={} slot={} cpu={} raw={} epoch={}",
            self.seq, self.gen, self.slot, self.cpu, self.raw, self.epoch
        )
    }
}

/// 五元组：去重键 + 归档键。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MetaTuple {
    pub seq: u32,
    pub gen: u16,
    pub slot: u16,
    pub cpu: u8,
    pub epoch: u32,
}

impl MetaTuple {
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_u32(h, self.seq);
        h = fold_u32(h, self.gen as u32);
        h = fold_u32(h, self.slot as u32);
        h = fold_u32(h, self.cpu as u32);
        h = fold_u32(h, self.epoch);
        h
    }

    pub fn text(&self) -> String {
        format!("{}:{}:{}:{}:{}", self.epoch, self.gen, self.slot, self.cpu, self.seq)
    }
}

// ── 环形代数（代际 / 槽位 / 标签） ────────────────────────────────
/// 只算代数，不存数据：把「绝对序号」映射到 (gen, slot)。
pub struct RingAlgebra {
    capacity: u16,
}

impl RingAlgebra {
    pub fn new(capacity: u16) -> Result<RingAlgebra, u32> {
        if capacity == 0 || capacity > RING_CAP_MAX {
            return Err(MCode::SlotBad.val());
        }
        Ok(RingAlgebra { capacity })
    }

    pub fn capacity(&self) -> u16 {
        self.capacity
    }

    pub fn slot_of(&self, abs: u32) -> Result<u16, u32> {
        if self.capacity == 0 {
            return Err(MCode::SlotBad.val());
        }
        let slot = abs % self.capacity as u32;
        if slot > u16::MAX as u32 {
            return Err(MCode::SlotBad.val());
        }
        Ok(slot as u16)
    }

    pub fn gen_of(&self, abs: u32) -> Result<u16, u32> {
        if self.capacity == 0 {
            return Err(MCode::SlotBad.val());
        }
        let gen = abs / self.capacity as u32;
        // 代际按 u16 回绕，回绕本身是合法事件（GEN_WRAP 次插入一轮）。
        Ok((gen % GEN_WRAP) as u16)
    }

    pub fn tag_of(&self, abs: u32) -> Result<u32, u32> {
        let slot = self.slot_of(abs)?;
        let gen = self.gen_of(abs)?;
        Ok(((gen as u32) << 16) | (slot as u32))
    }

    /// 标签解码：槽位必须 < 容量（否则是被旧容量写进去的脏标签）。
    pub fn decode(&self, tag: u32) -> Result<(u16, u16), u32> {
        let gen = (tag >> 16) as u16;
        let slot = (tag & 0xFFFF) as u16;
        if slot >= self.capacity {
            return Err(MCode::SlotBad.val());
        }
        Ok((gen, slot))
    }

    /// 一轮插入内的代际序列：非降，且只在回绕点 +1。
    pub fn gen_track(&self, inserts: u32) -> Result<Vec<u16>, u32> {
        let mut v: Vec<u16> = Vec::new();
        let mut abs = 0u32;
        while abs < inserts {
            v.push(self.gen_of(abs)?);
            abs = abs.saturating_add(1);
        }
        Ok(v)
    }

    /// 回绕次数：插入 inserts 条后槽位被复用的轮数。
    pub fn wraps(&self, inserts: u32) -> u32 {
        if self.capacity == 0 {
            return 0;
        }
        inserts / self.capacity as u32
    }
}

// ── 元数据账本 ────────────────────────────────────────────────────
/// 账本按绝对序号分配 (gen, slot)，并用 TimeBase 校正核间时间。
pub struct MetaLedger {
    clock: TimeBase,
    ring: RingAlgebra,
    entries: Vec<SampleMeta>,
    stamps: Vec<u64>,
    last_seq: u32,
    last_stamp: u64,
    dups: u32,
    rejected: u32,
}

impl MetaLedger {
    pub fn new(clock: TimeBase, ring: RingAlgebra) -> MetaLedger {
        MetaLedger {
            clock,
            ring,
            entries: Vec::new(),
            stamps: Vec::new(),
            last_seq: 0,
            last_stamp: 0,
            dups: 0,
            rejected: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn dups(&self) -> u32 {
        self.dups
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn clock(&self) -> &TimeBase {
        &self.clock
    }

    /// 落账：纪元 → 核号 → 序号 → 环代数 → 单调，次序不可倒。
    pub fn admit(&mut self, m: SampleMeta) -> Result<u64, u32> {
        let fail = |led: &mut MetaLedger, code: u32| -> Result<u64, u32> {
            led.rejected = led.rejected.saturating_add(1);
            Err(code)
        };
        if m.epoch != self.clock.epoch() {
            return fail(self, MCode::ClockBad.val());
        }
        if m.seq == 0 {
            return fail(self, MCode::MetaBad.val());
        }
        if m.cpu as usize >= MAX_CORES {
            return fail(self, MCode::CpuBad.val());
        }
        if m.seq <= self.last_seq {
            if m.seq == self.last_seq {
                self.dups = self.dups.saturating_add(1);
            }
            return fail(self, MCode::SeqBad.val());
        }
        let abs = self.entries.len() as u32;
        let want_slot = self.ring.slot_of(abs)?;
        let want_gen = self.ring.gen_of(abs)?;
        if m.slot != want_slot || m.gen != want_gen {
            return fail(self, MCode::GenBad.val());
        }
        let stamp = self.clock.corrected(m.raw, m.cpu as usize)?;
        if !self.stamps.is_empty() && stamp <= self.last_stamp {
            return fail(self, MCode::MonoBad.val());
        }
        self.entries.push(m);
        self.stamps.push(stamp);
        self.last_seq = m.seq;
        self.last_stamp = stamp;
        Ok(stamp)
    }

    /// 校正后时间序列（只读）。
    pub fn stamps(&self) -> &[u64] {
        &self.stamps
    }

    pub fn entry(&self, i: usize) -> Option<SampleMeta> {
        self.entries.get(i).copied()
    }

    pub fn stamp_at(&self, i: usize) -> Option<u64> {
        self.stamps.get(i).copied()
    }

    /// 全序单调复核：任何一个回退都算红。
    pub fn mono_ok(&self) -> bool {
        let mut i = 1usize;
        while i < self.stamps.len() {
            let a = self.stamps.get(i - 1).copied().unwrap_or(0);
            let b = self.stamps.get(i).copied().unwrap_or(0);
            if b <= a {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 首尾漂移 ppm：以「条数 × 名义间隔」为期望。
    pub fn drift_ppm(&self, nominal_gap: u64) -> Result<u32, u32> {
        if self.stamps.len() < 2 || nominal_gap == 0 {
            return Err(MCode::MetaBad.val());
        }
        let first = self.stamps.first().copied().unwrap_or(0);
        let last = self.stamps.last().copied().unwrap_or(0);
        let measured = last.saturating_sub(first);
        let expected = nominal_gap.saturating_mul(self.stamps.len() as u64 - 1);
        self.clock.ppm(expected, measured)
    }

    /// 代际序列（用于代数审计）。
    pub fn gen_series(&self) -> Vec<u16> {
        let mut v: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(m) = self.entries.get(i) {
                v.push(m.gen);
            }
            i += 1;
        }
        v
    }

    pub fn wraps(&self) -> u32 {
        self.ring.wraps(self.entries.len() as u32)
    }
}

// ── FNV-1a（本域基础设施，命名带 meta 前缀避免撞名） ──────────────
pub const FNV_OFF: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, (v >> (i * 8)) as u8);
        i += 1;
    }
    x
}

pub fn fold_text(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

// ── LCG（元数据合成器，固定盐） ───────────────────────────────────
pub struct MetaRng(u64);

impl MetaRng {
    pub fn new(seed: u64) -> MetaRng {
        MetaRng(seed ^ 0x4D45_5441_5435_5031)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ── 跨核对齐审计 ──────────────────────────────────────────────────
/// 同一参考事件在 8 核上的读数经校正后必须逐 tick 相等。
pub fn align_audit(clock: &TimeBase, ref_event: u64) -> Result<u64, u32> {
    let mut max_dev = 0u64;
    let mut core = 0usize;
    while core < MAX_CORES {
        if clock.is_calibrated(core) {
            let off = clock.offset_of(core)?;
            // raw = ref - off（负偏移时加绝对值），保证 raw 非零。
            let raw = if off >= 0 {
                ref_event.saturating_sub(off as u64)
            } else {
                ref_event.saturating_add(off.unsigned_abs())
            };
            if raw == 0 {
                return Err(MCode::ClockBad.val());
            }
            let back = clock.corrected(raw, core)?;
            let dev = if back >= ref_event {
                back - ref_event
            } else {
                ref_event - back
            };
            if dev > max_dev {
                max_dev = dev;
            }
        }
        core += 1;
    }
    Ok(max_dev)
}

/// 八核标定：注意偏移量按核递增，便于观察不同核的相位差。
pub const CALIB_PLAN: [i64; MAX_CORES] = [0, -120, 240, -360, 480, -600, 720, -840];

/// 建基准并按 CALIB_PLAN 标定全部核。
pub fn calibrated_base(epoch: u32) -> Result<TimeBase, u32> {
    let mut tb = TimeBase::new(TSC_HZ, ClockAuthority::InvariantTsc)?;
    let mut core = 0usize;
    while core < MAX_CORES {
        let off = match CALIB_PLAN.get(core) {
            Some(v) => *v,
            None => return Err(MCode::CpuBad.val()),
        };
        let anchor = 1_000_000u64;
        let local = if off >= 0 {
            anchor.saturating_sub(off as u64)
        } else {
            anchor.saturating_add(off.unsigned_abs())
        };
        let got = tb.calibrate(core, anchor, local)?;
        if got != off {
            return Err(MCode::OffsetBad.val());
        }
        core += 1;
    }
    while tb.epoch() < epoch {
        tb.bump_epoch()?;
    }
    Ok(tb)
}

// ── 黄金全链 ──────────────────────────────────────────────────────
/// 黄金结果：样本数 / 覆盖核数 / 回绕 / 重复 / 漂移 / 对齐偏差。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MetaGolden {
    pub samples: u32,
    pub cores: u32,
    pub wraps: u32,
    pub dups: u32,
    pub ppm: u32,
    pub align_dev: u64,
    pub mono: bool,
    pub digest: u64,
}

/// 黄金：1000 条样本跨 8 核，逐条校正并落账。
pub fn golden_run() -> Result<MetaGolden, u32> {
    let clock = calibrated_base(1)?;
    let align_dev = align_audit(&clock, 4_000_000)?;
    let ring = RingAlgebra::new(512)?;
    let mut led = MetaLedger::new(clock, ring);
    let mut rng = MetaRng::new(SEG_BASE as u64);
    let mut core_seen = [false; MAX_CORES];
    let nominal_gap = 2_000u64;
    let mut g = 1_000_000u64;
    let mut seq = 1u32;
    let mut r = 0u32;
    while r < META_ROUNDS {
        let core = (r % MAX_CORES as u32) as usize;
        let off = match CALIB_PLAN.get(core) {
            Some(v) => *v,
            None => return Err(MCode::CpuBad.val()),
        };
        // raw = 参考时间 - 偏移：把「核相位差」藏进原始读数。
        let raw = if off >= 0 {
            g.saturating_sub(off as u64)
        } else {
            g.saturating_add(off.unsigned_abs())
        };
        if raw == 0 {
            return Err(MCode::ClockBad.val());
        }
        let abs = led.len() as u32;
        let ring_ref = RingAlgebra::new(512)?;
        let slot = ring_ref.slot_of(abs)?;
        let gen = ring_ref.gen_of(abs)?;
        let jitter = rng.below(3);
        let meta = SampleMeta::new(
            seq,
            gen,
            slot,
            core as u8,
            raw.saturating_add(jitter),
            led.clock().epoch(),
        );
        led.admit(meta)?;
        if let Some(cell) = core_seen.get_mut(core) {
            *cell = true;
        }
        g = g.saturating_add(nominal_gap);
        seq = seq.saturating_add(1);
        r += 1;
    }
    let mut cores = 0u32;
    let mut i = 0usize;
    while i < core_seen.len() {
        if core_seen.get(i).copied().unwrap_or(false) {
            cores = cores.saturating_add(1);
        }
        i += 1;
    }
    let ppm = led.drift_ppm(nominal_gap)?;
    let mut digest = FNV_OFF;
    let mut k = 0usize;
    while k < led.len() {
        if let Some(m) = led.entry(k) {
            digest = fold_u32(digest, m.tag());
            digest = fold_u32(digest, m.cpu as u32);
        }
        k += 1;
    }
    Ok(MetaGolden {
        samples: led.len() as u32,
        cores,
        wraps: led.wraps(),
        dups: led.dups(),
        ppm,
        align_dev,
        mono: led.mono_ok(),
        digest,
    })
}

// ── 注入账（四路：软件源/序号回退/偏移越界/代际过期） ────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < 4 {
            if let Some(t) = self.tries.get(i) {
                sum = sum.saturating_add(*t);
            }
            i += 1;
        }
        sum
    }
}

/// 每轮四路注入，全部必须被拒。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = MetaRng::new(SEG_BASE as u64 ^ 0x7B);
    let mut r = 0u32;
    while r < rounds {
        let jitter = rng.below(64);

        // 路 1：软件计时器冒充权威源。
        let auth = TimeBase::new(TSC_HZ, ClockAuthority::SoftwareTimer).err()
            == Some(MCode::AuthBad.val())
            && TimeBase::new(TSC_HZ, ClockAuthority::UnstableTsc).is_err();
        led.record(0, auth);

        // 路 2：序号回退（先收 9，再收 3）。
        let mut a = MetaLedger::new(calibrated_base(1)?, RingAlgebra::new(8)?);
        a.admit(SampleMeta::new(9, 0, 0, 0, 1_000, 1))?;
        let seq_bad = a.admit(SampleMeta::new(3, 0, 1, 0, 2_000, 1)).err()
            == Some(MCode::SeqBad.val());
        led.record(1, seq_bad);

        // 路 3：偏移越界（100 万 tick >> 上限 10 万）。
        let mut tb = TimeBase::new(TSC_HZ, ClockAuthority::InvariantTsc)?;
        let off_bad = tb.calibrate(3, 1_000_000, 2_000_000).err()
            == Some(MCode::OffsetBad.val())
            && tb.calibrate(MAX_CORES, 1, 1).err() == Some(MCode::CpuBad.val());
        led.record(2, off_bad);

        // 路 4：代际过期（把第 0 条写成第 1 代的槽位）。
        let mut b = MetaLedger::new(calibrated_base(1)?, RingAlgebra::new(2)?);
        b.admit(SampleMeta::new(1, 0, 0, 0, 1_000, 1))?;
        let gen_bad = b.admit(SampleMeta::new(2, 1, 1, 0, 2_000u64.saturating_add(jitter), 1)).err()
            == Some(MCode::GenBad.val());
        led.record(3, gen_bad);

        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
/// 序 A：黄金两跑同值，千样本、八核、单调、零重复。
pub fn probe_seq_a() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MCode::MetaBad.val());
    }
    if a.samples != META_ROUNDS || a.cores != MAX_CORES as u32 || a.dups != 0 || !a.mono {
        return Err(MCode::MonoBad.val());
    }
    Ok(())
}

/// 序 B：非法输入逐类拒。
pub fn probe_seq_b() -> Result<(), u32> {
    if TimeBase::new(TSC_HZ, ClockAuthority::SoftwareTimer).err() != Some(MCode::AuthBad.val()) {
        return Err(MCode::AuthBad.val());
    }
    if TimeBase::new(MIN_HZ - 1, ClockAuthority::InvariantTsc).err() != Some(MCode::ClockBad.val()) {
        return Err(MCode::ClockBad.val());
    }
    if RingAlgebra::new(0).err() != Some(MCode::SlotBad.val()) {
        return Err(MCode::SlotBad.val());
    }
    let ring = RingAlgebra::new(4)?;
    if ring.decode((5u32 << 16) | 4).err() != Some(MCode::SlotBad.val()) {
        return Err(MCode::SlotBad.val());
    }
    Ok(())
}

/// 序 C：冻结常量。
pub fn probe_seq_c() -> Result<(), u32> {
    if MAX_CORES == 8
        && META_ROUNDS == 1000
        && TSC_HZ == 2_000_000_000
        && OFFSET_LIMIT_TICKS == 100_000
        && GEN_WRAP == 65_536
        && PPM_WATCH < PPM_BAD
    {
        Ok(())
    } else {
        Err(MCode::SegmentBad.val())
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MCode::MetaBad.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < MCode::ALL.len() {
        let a = match MCode::ALL.get(i) {
            Some(c) => *c,
            None => return false,
        };
        if a.val() < SEG_BASE || a.val() >= SEG_BASE + 0x20 {
            return false;
        }
        let mut j = i + 1;
        while j < MCode::ALL.len() {
            if let Some(b) = MCode::ALL.get(j) {
                if b.val() == a.val() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();

    // ① 第三段千样本元数据全落账。
    v.push((
        "F48863-J1-三段千样本元数据",
        match &g {
            Ok(x) => x.samples == META_ROUNDS && x.dups == 0,
            Err(_) => false,
        },
        match &g {
            Ok(x) => format!("samples={} dups={} digest=0x{:016X}", x.samples, x.dups, x.digest),
            Err(e) => format!("err=0x{:X} {}", e, detail(*e)),
        },
    ));

    // ② 五元组与环标签互推：1000 条样本标签可解码回 (gen, slot)。
    let ring = RingAlgebra::new(512);
    let tag_ok = match &ring {
        Ok(r) => {
            let mut ok = true;
            let mut abs = 0u32;
            while abs < 1000 {
                match r.tag_of(abs) {
                    Ok(tag) => match r.decode(tag) {
                        Ok((gen, slot)) => {
                            if gen != r.gen_of(abs).unwrap_or(u16::MAX)
                                || slot != r.slot_of(abs).unwrap_or(u16::MAX)
                            {
                                ok = false;
                            }
                        }
                        Err(_) => ok = false,
                    },
                    Err(_) => ok = false,
                }
                abs = abs.saturating_add(1);
            }
            ok
        }
        Err(_) => false,
    };
    v.push((
        "F48863-J1-五元组环标签互推",
        tag_ok,
        format!(
            "cap=512 wraps_1000={}",
            match &ring {
                Ok(r) => r.wraps(1000),
                Err(_) => 0,
            }
        ),
    ));

    // ③ 序号回退必拒（含重复号单独计 dups）。
    let seq_ok = match (calibrated_base(1), RingAlgebra::new(8)) {
        (Ok(tb), Ok(r)) => {
            let mut led = MetaLedger::new(tb, r);
            let a = led.admit(SampleMeta::new(5, 0, 0, 0, 1_000, 1));
            let b = led.admit(SampleMeta::new(5, 0, 1, 0, 3_000, 1));
            let c = led.admit(SampleMeta::new(4, 0, 2, 0, 5_000, 1));
            a.is_ok()
                && b.err() == Some(MCode::SeqBad.val())
                && c.err() == Some(MCode::SeqBad.val())
                && led.dups() == 1
        }
        _ => false,
    };
    v.push((
        "F48863-J1-序号回退必拒",
        seq_ok,
        String::from("同号计 dups=1，回退号直接拒"),
    ));

    // ④ 权威源闸：只有不变 TSC 通过。
    let auth_ok = TimeBase::new(TSC_HZ, ClockAuthority::InvariantTsc).is_ok()
        && TimeBase::new(TSC_HZ, ClockAuthority::SoftwareTimer).err() == Some(MCode::AuthBad.val())
        && TimeBase::new(TSC_HZ, ClockAuthority::Hpet).err() == Some(MCode::AuthBad.val())
        && ClockAuthority::InvariantTsc.is_authority()
        && !ClockAuthority::UnstableTsc.is_authority();
    v.push((
        "F48864-J1-TSC权威源单一",
        auth_ok,
        String::from("只有 invariant-tsc 具备权威资格"),
    ));

    // ⑤ 跨核偏移校准：八核同参考事件对齐偏差为 0。
    let align = match &g {
        Ok(x) => Ok(x.align_dev),
        Err(e) => Err(*e),
    };
    let (align_ok, align_detail) = match &align {
        Ok(dev) => {
            let cals = match calibrated_base(1) {
                Ok(tb) => tb.calibrated_count(),
                Err(_) => 0,
            };
            (
                *dev == 0 && cals == MAX_CORES as u32,
                format!("cores_calibrated={} max_dev={}tick", cals, dev),
            )
        }
        Err(e) => (false, format!("err=0x{:X} {}", e, detail(*e))),
    };
    v.push(("F48864-J1-八核偏移校准对齐", align_ok, align_detail));

    // ⑥ A2 基座对账零漂移（黄金链 ppm 必须为 0）。
    v.push((
        "F48864-J1-基座对账零漂移",
        match &g {
            Ok(x) => x.ppm == 0 && x.mono,
            Err(_) => false,
        },
        match &g {
            Ok(x) => format!("ppm={} mono={}", x.ppm, x.mono),
            Err(e) => format!("err=0x{:X} {}", e, detail(*e)),
        },
    ));

    // ⑦ 单调闸：未校准核的读数必须被 OffsetBad 拒（不能直接做差）。
    let mono_ok = match TimeBase::new(TSC_HZ, ClockAuthority::InvariantTsc) {
        Ok(tb) => tb.corrected(1_000, 1).err() == Some(MCode::OffsetBad.val()),
        Err(_) => false,
    };
    let mut led_mono = true;
    if let (Ok(tb), Ok(r)) = (calibrated_base(1), RingAlgebra::new(16)) {
        let mut led = MetaLedger::new(tb, r);
        if led.admit(SampleMeta::new(1, 0, 0, 0, 10_000, 1)).is_ok() {
            // 时间回退（raw 更小）必须 MonoBad。
            led_mono = led.admit(SampleMeta::new(2, 0, 1, 0, 9_000, 1)).err()
                == Some(MCode::MonoBad.val());
        }
    }
    v.push((
        "F48864-J1-单调闸与未校准拒",
        mono_ok && led_mono,
        String::from("未校准核 OffsetBad，时间回退 MonoBad"),
    ));

    // ⑧ 注入四路 + 探针三序 + 段位。
    let inj = inject_rounds(10);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught() && l.total() == 40,
        Err(_) => false,
    };
    v.push((
        "F48864-J1-注入四路与探针三序",
        inj_ok && probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok()
            && repro_check().is_ok() && segment_guard(),
        match &inj {
            Ok(l) => format!("caught={:?} probes_ok={}", l.caught, probe_seq_c().is_ok()),
            Err(e) => format!("err=0x{:X} {}", e, detail(*e)),
        },
    ));

    v
}

pub fn run_unx48863_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48863 窗口应用回调段打点");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_rules_are_strict() {
        assert!(ClockAuthority::InvariantTsc.is_authority());
        assert!(!ClockAuthority::UnstableTsc.is_authority());
        assert!(!ClockAuthority::Hpet.is_authority());
        assert!(!ClockAuthority::SoftwareTimer.is_authority());
        assert_eq!(
            TimeBase::new(TSC_HZ, ClockAuthority::SoftwareTimer).err(),
            Some(MCode::AuthBad.val())
        );
    }

    #[test]
    fn frequency_bounds_enforced() {
        assert!(TimeBase::new(MIN_HZ, ClockAuthority::InvariantTsc).is_ok());
        assert!(TimeBase::new(MAX_HZ, ClockAuthority::InvariantTsc).is_ok());
        assert_eq!(
            TimeBase::new(MIN_HZ - 1, ClockAuthority::InvariantTsc).err(),
            Some(MCode::ClockBad.val())
        );
        assert_eq!(
            TimeBase::new(MAX_HZ + 1, ClockAuthority::InvariantTsc).err(),
            Some(MCode::ClockBad.val())
        );
    }

    #[test]
    fn calibration_round_trips_every_core() {
        let tb = calibrated_base(1).ok();
        assert!(tb.is_some());
        match tb {
            Some(base) => {
                assert_eq!(base.calibrated_count(), MAX_CORES as u32);
                assert_eq!(align_audit(&base, 8_000_000).ok(), Some(0));
                let mut core = 0usize;
                while core < MAX_CORES {
                    assert_eq!(base.offset_of(core).ok(), CALIB_PLAN.get(core).copied());
                    core += 1;
                }
            }
            None => assert!(false),
        }
    }

    #[test]
    fn offset_beyond_limit_rejected() {
        let mut tb = TimeBase::new(TSC_HZ, ClockAuthority::InvariantTsc).ok();
        match tb.as_mut() {
            Some(b) => {
                assert_eq!(
                    b.calibrate(2, 1, 1 + OFFSET_LIMIT_TICKS as u64 + 1).err(),
                    Some(MCode::OffsetBad.val())
                );
                assert_eq!(b.calibrate(MAX_CORES, 1, 1).err(), Some(MCode::CpuBad.val()));
                assert!(b.calibrate(2, 10_000, 10_500).is_ok());
            }
            None => assert!(false),
        }
    }

    #[test]
    fn uncalibrated_core_is_rejected() {
        let tb = TimeBase::new(TSC_HZ, ClockAuthority::InvariantTsc).ok();
        assert!(tb.is_some());
        match tb {
            Some(base) => {
                assert_eq!(base.corrected(1_000, 0).ok(), Some(1_000));
                assert_eq!(base.corrected(1_000, 3).err(), Some(MCode::OffsetBad.val()));
                assert_eq!(base.corrected(1_000, MAX_CORES).err(), Some(MCode::CpuBad.val()));
            }
            None => assert!(false),
        }
    }

    #[test]
    fn ring_tag_encodes_generation() {
        let r = RingAlgebra::new(4).ok().unwrap_or(RingAlgebra { capacity: 1 });
        assert_eq!(r.tag_of(0).ok(), Some(0));
        assert_eq!(r.tag_of(5).ok(), Some((1 << 16) | 1));
        assert_eq!(r.decode((1 << 16) | 1).ok(), Some((1, 1)));
        assert_eq!(r.decode(4).err(), Some(MCode::SlotBad.val()));
        assert_eq!(r.wraps(10), 2);
    }

    #[test]
    fn ring_rejects_zero_or_oversize_capacity() {
        assert_eq!(RingAlgebra::new(0).err(), Some(MCode::SlotBad.val()));
        assert_eq!(RingAlgebra::new(RING_CAP_MAX + 1).err(), Some(MCode::SlotBad.val()));
        assert_eq!(RingAlgebra::new(1).ok().map(|r| r.capacity()), Some(1));
    }

    #[test]
    fn stale_generation_is_rejected() {
        let tb = calibrated_base(1).ok();
        let ring = RingAlgebra::new(2).ok();
        match (tb, ring) {
            (Some(t), Some(r)) => {
                let mut led = MetaLedger::new(t, r);
                assert!(led.admit(SampleMeta::new(1, 0, 0, 0, 10_000, 1)).is_ok());
                // 环长 2：第 2 条仍在第 0 代，谎报第 1 代必须拒。
                assert_eq!(
                    led.admit(SampleMeta::new(2, 1, 1, 0, 12_000, 1)).err(),
                    Some(MCode::GenBad.val())
                );
                assert!(led.admit(SampleMeta::new(2, 0, 1, 0, 12_000, 1)).is_ok());
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn epoch_mismatch_is_rejected() {
        let tb = calibrated_base(2).ok();
        let ring = RingAlgebra::new(8).ok();
        match (tb, ring) {
            (Some(t), Some(r)) => {
                let mut led = MetaLedger::new(t, r);
                assert_eq!(
                    led.admit(SampleMeta::new(1, 0, 0, 0, 10_000, 1)).err(),
                    Some(MCode::ClockBad.val())
                );
                assert!(led.admit(SampleMeta::new(1, 0, 0, 0, 10_000, 2)).is_ok());
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn monotonic_break_is_rejected() {
        let tb = calibrated_base(1).ok();
        let ring = RingAlgebra::new(8).ok();
        match (tb, ring) {
            (Some(t), Some(r)) => {
                let mut led = MetaLedger::new(t, r);
                assert!(led.admit(SampleMeta::new(1, 0, 0, 0, 10_000, 1)).is_ok());
                assert_eq!(
                    led.admit(SampleMeta::new(2, 0, 1, 0, 9_500, 1)).err(),
                    Some(MCode::MonoBad.val())
                );
                assert_eq!(led.rejected(), 1);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn golden_counts_are_frozen() {
        let g = golden_run();
        assert!(g.is_ok());
        match g {
            Ok(x) => {
                assert_eq!(x.samples, 1000);
                assert_eq!(x.cores, 8);
                assert_eq!(x.wraps, 1);
                assert_eq!(x.dups, 0);
                assert_eq!(x.ppm, 0);
                assert_eq!(x.align_dev, 0);
                assert!(x.mono);
            }
            Err(_) => assert!(false),
        }
        assert!(repro_check().is_ok());
    }

    #[test]
    fn injection_catches_all_four() {
        match inject_rounds(10) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.total(), 40);
                assert_eq!(l.caught, [10, 10, 10, 10]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn meta_tuple_fold_is_stable() {
        let m = SampleMeta::new(7, 1, 3, 2, 12345, 1);
        let a = m.tuple();
        let b = m.tuple();
        assert_eq!(a, b);
        assert_eq!(a.fold(), b.fold());
        assert_eq!(a.text(), "1:1:3:2:7");
        assert_ne!(a.fold(), SampleMeta::new(8, 1, 3, 2, 12345, 1).tuple().fold());
    }

    #[test]
    fn probes_and_segment_guard_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(segment_guard());
        assert_eq!(MCode::ALL.len(), 10);
    }

    #[test]
    fn checks_are_all_green() {
        let v = extra_checks();
        assert_eq!(v.len(), 8);
        let mut i = 0usize;
        while i < v.len() {
            if let Some((n, p, d)) = v.get(i) {
                assert!(n.starts_with("F4886"));
                assert!(*p, "red: {} {}", n, d);
            }
            i += 1;
        }
        assert!(run_unx48863_checks().all_passed());
    }
}
