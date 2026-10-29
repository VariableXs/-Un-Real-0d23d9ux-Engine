//! UNX-F0697 · 真机登记随闸门补测账（双轨登记）· 诊断段 0x57a0（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0697] 真机登记随闸门补测账（双轨登记） <SEG 0x57a0>
//!   UNX-F0697-J1 双轨登记账齐（QEMU 演练档结论列 + 真机补测登记列两列
//!   分离）20/20 条目登记且零混列。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 双轨闭集：Track::QemuRehearsal（演练档）/ Track::RealMachine
//!      （真机补测），轨外无登记面；条目 20 条定容（ENTRIES=20）。
//!   2. 两列分离：每条目恰两列槽位（qemu 列 + real 列），结论载荷带轨标，
//!      轨标与存放列不符即混列（E_COL_MIXED）——零混列判据的检出通道。
//!   3. 随闸门联动：真机补测登记必须挂在闸门事件（gate_id 闭集）上，
//!      无闸挂载的登记拒收。
//!   4. 20/20 守恒：双列登记数之和 == 条目数 × 2；缺列/重列显性检出。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x57a0~0x57bF 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x57a0 独占 0x57a0~0x57bF） ────────────────────────────
pub const SEG_BASE: u16 = 0x57a0;

/// 0x57a1 账已封板仍尝试登记
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x57a2 条目号越界（合法 1..=20）
const E_ENTRY_RANGE: u16 = SEG_BASE + 0x2;
/// 0x57a3 轨号非法（双轨闭集外）
const E_TRACK_BAD: u16 = SEG_BASE + 0x3;
/// 0x57a4 混列检出：载荷轨标与存放列不符
const E_COL_MIXED: u16 = SEG_BASE + 0x4;
/// 0x57a5 同列重登：同条目同列二写
const E_COL_DUP: u16 = SEG_BASE + 0x5;
/// 0x57a6 缺列：20/20 未齐即封板
const E_COL_MISSING: u16 = SEG_BASE + 0x6;
/// 0x57a7 闸门号越界（合法 0..GATE_IDS）
const E_GATE_RANGE: u16 = SEG_BASE + 0x7;
/// 0x57a8 无闸挂载：真机补测未挂闸门事件
const E_NO_GATE: u16 = SEG_BASE + 0x8;
/// 0x57a9 结论码越闭集（合法 0..=3）
const E_VERDICT_RANGE: u16 = SEG_BASE + 0x9;
/// 0x57aa 双列守恒破：登记数之和 ≠ 条目数×2
const E_CONSERVATION: u16 = SEG_BASE + 0xA;
/// 0x57ab 演练列挂真机闸：闸门轨与登记轨错位
const E_GATE_TRACK: u16 = SEG_BASE + 0xB;
/// 0x57ac 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x57ad 结论时戳回退：同列时戳必须单调不减
const E_STAMP_BACK: u16 = SEG_BASE + 0xD;
/// 0x57ae 轨账指针悬空：解引未登记列
const E_REF_DANGLE: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x57a0~0x57bF 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "账已封板：双轨账封板后只读，补登请开新账",
        E_ENTRY_RANGE => "条目号越界：合法 1..=20",
        E_TRACK_BAD => "轨号非法：双轨闭集仅 QEMU 演练/真机补测",
        E_COL_MIXED => "混列检出：载荷轨标与存放列不符，请改走对应列登记",
        E_COL_DUP => "同列重登：该条目该列已登记，拒收二写",
        E_COL_MISSING => "缺列：20/20 双列未齐禁止封板",
        E_GATE_RANGE => "闸门号越界：合法 0..GATE_IDS",
        E_NO_GATE => "无闸挂载：真机补测必须挂在闸门事件上",
        E_VERDICT_RANGE => "结论码越闭集：合法 0..=3",
        E_CONSERVATION => "守恒破：双列登记数之和 ≠ 条目数×2",
        E_GATE_TRACK => "闸门轨错位：演练列只挂演练闸，真机列只挂补测闸",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_STAMP_BACK => "时戳回退：同列时戳必须单调不减",
        E_REF_DANGLE => "轨账指针悬空：解引未登记列",
        _ => "未知诊断码（0x57a0 段外或保留位）",
    }
}

// ── 常量（来源：判据「20/20 条目」与闸门闭集） ───────────────────────
/// 条目数（判据原文：20/20 条目登记）。
pub const ENTRIES: usize = 20;
/// 闸门事件数闭集（演练闸 0、补测闸 1、收官闸 2）。
pub const GATE_IDS: usize = 3;
/// 结论码闭集容量（0=过/1=败/2=跳过/3=不可判）。
pub const VERDICT_CODES: u8 = 4;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(ENTRIES == 20);
const _: () = assert!(GATE_IDS == 3);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

// ── 双轨闭集 ─────────────────────────────────────────────────────────
/// 登记双轨闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    /// 轨 0：QEMU 演练档（闸门前置演练结论列）。
    QemuRehearsal,
    /// 轨 1：真机补测（闸门放行后真机补测登记列）。
    RealMachine,
}

impl Track {
    /// 轨号。
    pub const fn id(self) -> u8 {
        match self {
            Track::QemuRehearsal => 0,
            Track::RealMachine => 1,
        }
    }

    /// 从轨号重建。
    pub fn from_id(v: u8) -> Result<Track, u16> {
        match v {
            0 => Ok(Track::QemuRehearsal),
            1 => Ok(Track::RealMachine),
            _ => Err(E_TRACK_BAD),
        }
    }

    /// 本轨合法闸门号：演练轨→演练闸(0)；真机轨→补测闸(1)或收官闸(2)。
    pub const fn gate_ok(self, gate: usize) -> bool {
        match self {
            Track::QemuRehearsal => gate == 0,
            Track::RealMachine => gate == 1 || gate == 2,
        }
    }
}

// ── 结论闭集 ─────────────────────────────────────────────────────────
/// 结论码闭集 0..=3。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryVerdict {
    /// 0：过。
    Pass,
    /// 1：败。
    Fail,
    /// 2：跳过（演练档适用）。
    Skipped,
    /// 3：不可判。
    Inconclusive,
}

impl EntryVerdict {
    /// 结论码。
    pub const fn code(self) -> u8 {
        match self {
            EntryVerdict::Pass => 0,
            EntryVerdict::Fail => 1,
            EntryVerdict::Skipped => 2,
            EntryVerdict::Inconclusive => 3,
        }
    }

    /// 从码重建。
    pub fn from_code(c: u8) -> Result<EntryVerdict, u16> {
        match c {
            0 => Ok(EntryVerdict::Pass),
            1 => Ok(EntryVerdict::Fail),
            2 => Ok(EntryVerdict::Skipped),
            3 => Ok(EntryVerdict::Inconclusive),
            _ => Err(E_VERDICT_RANGE),
        }
    }
}

// ── 列载荷 ───────────────────────────────────────────────────────────
/// 一列载荷：轨标 + 结论码 + 闸门号 + 确定性时戳。
/// 轨标是零混列判据的锚：载荷写入哪一列，轨标必须等于该列轨号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnPayload {
    /// 轨标（与存放列一致才零混列）。
    pub track: u8,
    /// 结论码（闭集 0..=3）。
    pub verdict: u8,
    /// 闸门号（随闸门联动锚）。
    pub gate: u8,
    /// 确定性时戳（单调不减）。
    pub stamp: u64,
}

// ── 条目 ─────────────────────────────────────────────────────────────
/// 双轨条目：恰两列槽位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualEntry {
    /// 条目号 1..=20。
    pub no: u16,
    /// 列 0：QEMU 演练档结论列（None=未登记）。
    pub qemu_col: Option<ColumnPayload>,
    /// 列 1：真机补测登记列（None=未登记）。
    pub real_col: Option<ColumnPayload>,
}

impl DualEntry {
    /// 双列齐判定。
    pub const fn both_columns(&self) -> bool {
        self.qemu_col.is_some() && self.real_col.is_some()
    }
}

// ── 双轨登记账 ───────────────────────────────────────────────────────
/// 真机登记随闸门补测账：20 条定容双列账 + 零混列审计 + 守恒核对。
pub struct DualTrackLedger {
    /// 条目向量（20 条定容）。
    entries: Vec<DualEntry>,
    /// 确定性时钟（每登记 +1）。
    clock: u64,
    /// 封板位。
    sealed: bool,
    /// 混列检出计数。
    mix_hits: u32,
}

impl DualTrackLedger {
    /// 新账：20 条空条目。
    pub fn new() -> Self {
        let mut entries = Vec::new();
        let mut i = 1usize;
        while i <= ENTRIES {
            entries.push(DualEntry {
                no: i as u16,
                qemu_col: None,
                real_col: None,
            });
            i += 1;
        }
        DualTrackLedger {
            entries,
            clock: 0,
            sealed: false,
            mix_hits: 0,
        }
    }

    /// 混列检出计数。
    pub const fn mix_hits(&self) -> u32 {
        self.mix_hits
    }

    /// 双列登记计数（Σ列）。
    pub fn column_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].qemu_col.is_some() {
                n += 1;
            }
            if self.entries[i].real_col.is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 逐条目只读。
    pub fn entry(&self, no: usize) -> Result<&DualEntry, u16> {
        if no < 1 || no > ENTRIES {
            return Err(E_ENTRY_RANGE);
        }
        Ok(&self.entries[no - 1])
    }

    /// 登记（唯一写通道）：轨 + 条目号 + 结论 + 闸门号。
    /// 零混列保证：载荷轨标由此通道写死为登记轨，轨-列错位样本
    /// 只能经混列注入通道构造，注入必被 audit_mix 检出。
    pub fn register(
        &mut self,
        track: Track,
        no: usize,
        verdict: EntryVerdict,
        gate: usize,
    ) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if no < 1 || no > ENTRIES {
            return Err(E_ENTRY_RANGE);
        }
        if gate >= GATE_IDS {
            return Err(E_GATE_RANGE);
        }
        // 随闸门联动：轨-闸错位拒收。
        if !track.gate_ok(gate) {
            return Err(E_GATE_TRACK);
        }
        self.clock = self.clock.wrapping_add(1);
        let payload = ColumnPayload {
            track: track.id(),
            verdict: verdict.code(),
            gate: gate as u8,
            stamp: self.clock,
        };
        let slot = &mut self.entries[no - 1];
        let col = match track {
            Track::QemuRehearsal => &mut slot.qemu_col,
            Track::RealMachine => &mut slot.real_col,
        };
        if col.is_some() {
            return Err(E_COL_DUP);
        }
        *col = Some(payload);
        Ok(())
    }

    /// 混列注入通道（验收专用）：向指定列塞入异轨标载荷。
    /// 注入必被 audit_mix 检出（混列检出判据载体）。
    pub fn inject_mix(&mut self, col_slot: Track, no: usize) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if no < 1 || no > ENTRIES {
            return Err(E_ENTRY_RANGE);
        }
        // 异轨标：演练列塞真机标 / 真机列塞演练标。
        let wrong_track = match col_slot {
            Track::QemuRehearsal => Track::RealMachine.id(),
            Track::RealMachine => Track::QemuRehearsal.id(),
        };
        self.clock = self.clock.wrapping_add(1);
        let payload = ColumnPayload {
            track: wrong_track,
            verdict: EntryVerdict::Inconclusive.code(),
            gate: 0,
            stamp: self.clock,
        };
        let slot = &mut self.entries[no - 1];
        let col = match col_slot {
            Track::QemuRehearsal => &mut slot.qemu_col,
            Track::RealMachine => &mut slot.real_col,
        };
        if col.is_some() {
            return Err(E_COL_DUP);
        }
        *col = Some(payload);
        self.mix_hits = self.mix_hits.wrapping_add(1);
        Ok(())
    }

    /// 零混列审计：逐列核对轨标与列位（检出 100%）。
    pub fn audit_mix(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(p) = self.entries[i].qemu_col {
                if p.track != Track::QemuRehearsal.id() {
                    return Err(E_COL_MIXED);
                }
                if p.gate as usize >= GATE_IDS {
                    return Err(E_GATE_RANGE);
                }
            }
            if let Some(p) = self.entries[i].real_col {
                if p.track != Track::RealMachine.id() {
                    return Err(E_COL_MIXED);
                }
                if p.gate as usize >= GATE_IDS {
                    return Err(E_GATE_RANGE);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 时戳单调审计：逐列按登记序（时戳）核对全账单调不减。
    pub fn audit_stamps(&self) -> Result<(), u16> {
        let mut prev: u64 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(p) = self.entries[i].qemu_col {
                if p.stamp < prev {
                    return Err(E_STAMP_BACK);
                }
                prev = p.stamp;
            }
            if let Some(p) = self.entries[i].real_col {
                if p.stamp < prev {
                    return Err(E_STAMP_BACK);
                }
                prev = p.stamp;
            }
            i += 1;
        }
        Ok(())
    }

    /// 20/20 齐备审计：每条目双列齐（判据「20/20 条目登记且零混列」）。
    pub fn audit_full(&self) -> Result<(), u16> {
        self.audit_mix()?;
        self.audit_stamps()?;
        let mut i = 0usize;
        while i < self.entries.len() {
            if !self.entries[i].both_columns() {
                return Err(E_COL_MISSING);
            }
            i += 1;
        }
        // 守恒：双列登记数 == 条目数 × 2。
        if self.column_count() != ENTRIES * 2 {
            return Err(E_CONSERVATION);
        }
        Ok(())
    }

    /// 封板：齐备审计过后落锁。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.audit_full()?;
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// 全账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.entries.len() {
            h = fnv_mix(h, (self.entries[i].no & 0xFF) as u8);
            if let Some(p) = self.entries[i].qemu_col {
                h = fnv_mix(h, p.track | 0x10);
                h = fnv_mix(h, p.verdict | 0x20);
                h = fnv_mix(h, p.gate | 0x30);
                h = fnv_mix(h, (p.stamp & 0xFF) as u8);
            }
            if let Some(p) = self.entries[i].real_col {
                h = fnv_mix(h, p.track | 0x40);
                h = fnv_mix(h, p.verdict | 0x50);
                h = fnv_mix(h, p.gate | 0x60);
                h = fnv_mix(h, (p.stamp & 0xFF) as u8);
            }
            i += 1;
        }
        h
    }

    /// 轨账解引：条目号 + 轨 → 载荷（未登记列悬空）。
    pub fn deref(&self, no: usize, track: Track) -> Result<ColumnPayload, u16> {
        let e = self.entry(no)?;
        let col = match track {
            Track::QemuRehearsal => e.qemu_col,
            Track::RealMachine => e.real_col,
        };
        match col {
            Some(p) => Ok(p),
            None => Err(E_REF_DANGLE),
        }
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.entries.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("双轨登记摘要: 条目=");
        out.push_str(&itoa(self.entries.len() as u64));
        out.push_str(" 列登记=");
        out.push_str(&itoa(self.column_count() as u64));
        out.push_str(" 混列检出=");
        out.push_str(&itoa(self.mix_hits as u64));
        out.push_str(" 指纹=");
        out.push_str(&hex16(self.fingerprint()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：20/20 双列登记（演练列挂演练闸，真机列挂补测闸） ——
    let mut led = DualTrackLedger::new();
    let mut i = 1usize;
    while i <= ENTRIES {
        // 演练列：结论按条目号程序化（偶过奇跳，闭集覆盖）。
        let qv = if i % 2 == 0 {
            EntryVerdict::Pass
        } else {
            EntryVerdict::Skipped
        };
        led.register(Track::QemuRehearsal, i, qv, 0)?;
        // 真机列：补测闸挂载，结论全过。
        led.register(Track::RealMachine, i, EntryVerdict::Pass, 1)?;
        i += 1;
    }
    // 收官闸真机补登一条收尾（闸 2 合法）。
    // （20 条已满双列，补登必撞重列——先验证后再封板。）
    if led.register(Track::RealMachine, 1, EntryVerdict::Pass, 2) != Err(E_COL_DUP) {
        return Err(E_COL_DUP);
    }
    led.audit_mix()?;
    led.audit_stamps()?;
    led.audit_full()?;
    let fp = led.seal()?;
    if fp == 0 {
        return Err(E_CONSERVATION);
    }
    // 封板后登记必拦。
    if led.register(Track::QemuRehearsal, 2, EntryVerdict::Fail, 0) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    // 双列解引抽查：1 号条目两列均可解引。
    let p0 = led.deref(1, Track::QemuRehearsal)?;
    let p1 = led.deref(1, Track::RealMachine)?;
    if p0.track != 0 || p1.track != 1 {
        return Err(E_COL_MIXED);
    }
    // 摘要非空。
    let s = led.summary()?;
    if s.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 混列注入检出：注入必中 ——
    let mut led2 = DualTrackLedger::new();
    led2.register(Track::QemuRehearsal, 1, EntryVerdict::Pass, 0)?;
    led2.inject_mix(Track::RealMachine, 1)?;
    if led2.audit_mix() != Err(E_COL_MIXED) {
        return Err(E_COL_MIXED);
    }
    if led2.mix_hits() != 1 {
        return Err(E_COL_MIXED);
    }

    // —— 缺列拦截：只登演练列即封板必拦 ——
    let mut led3 = DualTrackLedger::new();
    let mut i3 = 1usize;
    while i3 <= ENTRIES {
        led3.register(Track::QemuRehearsal, i3, EntryVerdict::Pass, 0)?;
        i3 += 1;
    }
    if led3.seal() != Err(E_COL_MISSING) {
        return Err(E_COL_MISSING);
    }

    // —— 错误路径样本 ——
    if Track::from_id(2) != Err(E_TRACK_BAD) {
        return Err(E_TRACK_BAD);
    }
    if EntryVerdict::from_code(4) != Err(E_VERDICT_RANGE) {
        return Err(E_VERDICT_RANGE);
    }
    let mut led4 = DualTrackLedger::new();
    if led4.register(Track::QemuRehearsal, 0, EntryVerdict::Pass, 0) != Err(E_ENTRY_RANGE) {
        return Err(E_ENTRY_RANGE);
    }
    if led4.register(Track::QemuRehearsal, 21, EntryVerdict::Pass, 0) != Err(E_ENTRY_RANGE) {
        return Err(E_ENTRY_RANGE);
    }
    if led4.register(Track::QemuRehearsal, 1, EntryVerdict::Pass, 3) != Err(E_GATE_RANGE) {
        return Err(E_GATE_RANGE);
    }
    // 闸门轨错位：演练列挂补测闸必拦。
    if led4.register(Track::QemuRehearsal, 1, EntryVerdict::Pass, 1) != Err(E_GATE_TRACK) {
        return Err(E_GATE_TRACK);
    }
    // 真机列挂演练闸必拦。
    if led4.register(Track::RealMachine, 1, EntryVerdict::Pass, 0) != Err(E_GATE_TRACK) {
        return Err(E_GATE_TRACK);
    }
    // 悬空解引。
    if led4.deref(1, Track::QemuRehearsal) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }
    // 条目解引越界。
    if led4.entry(0) != Err(E_ENTRY_RANGE) {
        return Err(E_ENTRY_RANGE);
    }
    if led4.entry(ENTRIES + 1) != Err(E_ENTRY_RANGE) {
        return Err(E_ENTRY_RANGE);
    }

    // 诊断表抽查。
    if diag_detail(E_COL_MIXED) == "未知诊断码（0x57a0 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x57a0 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
