//! cgb850_amdcaps —— CGPU-F0850 AMD 能力探测专项。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0850 AMD 能力探测专项  <SEG 0x74C0>
//! 目标行数：≥340（判据：纯功能代码 ≥340 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥340 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0850；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 能力探测专项，整数位面）：
//!   1. 能力位闭集 16 位（写死语义）：位 0 波形光栅 / 位 1 无限
//!      缓存 / 位 2 网格着色 / 位 3 光追 / 位 4 VCN 编解码 / 位 5
//!      双媒体引擎 / 位 6 智能访问内存 / 位 7 细分着色 / 位 8 异步
//!      计算 / 位 9 半速率像素 / 位 10 DCC 压缩 / 位 11 影子采
//!      样 / 位 12 时钟门控 / 位 13 电压调节 / 位 14 多 GPU / 位 15
//!      调试探针；
//!   2. 探测状态机：Probe → Confirm → Lock；Lock 后能力位图只读
//!      （XC_CAPS_LOCKED）；
//!   3. 探测账（定容 32 槽探测事件环）守恒：入账 = 在环 + 逐出；
//!   4. 位图自洽判据（写死规则）：位 5 双媒体引擎必含位 4；位 3
//!      光追必含位 2 网格着色；位 6 必含位 10 DCC；违规红码
//!      （XC_CAPS_INCONSIST）；
//!   5. 代际约束：GCN4 不支持位 2/3/6；RDNA3 全支持；写死违规
//!      表，越代纪置位红码（XC_CAPS_GEN_VIOLATION）；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x74C0 段为本模块独占（SEG_BASE=0x74C0），判别式
//!   SEG_BASE+1..+N；与 0x74A0/0x74E0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const AC_VERSION: &str = "cgb850_amdcaps-v1";

/// 诊断段基址（0x74C0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x74C0;

// ── XcCode 显别式（SEG_BASE+1..+N）────────────────────────────────────
/// 0x74C1 能力位越界（0..=15 之外）。
pub const XC_BAD_BIT: u16 = SEG_BASE + 1;
/// 0x74C2 探测账漂移。
pub const XC_PROBE_DRIFT: u16 = SEG_BASE + 2;
/// 0x74C3 能力位图已锁定仍写入。
pub const XC_CAPS_LOCKED: u16 = SEG_BASE + 3;
/// 0x74C4 位图自洽违规（含位规则破坏）。
pub const XC_CAPS_INCONSIST: u16 = SEG_BASE + 4;
/// 0x74C5 代纪违规（代际不支持却置位）。
pub const XC_CAPS_GEN_VIOLATION: u16 = SEG_BASE + 5;
/// 0x74C6 探测事件重复（同位同轮）。
pub const XC_PROBE_DUP: u16 = SEG_BASE + 6;
/// 0x74C7 未锁定即出结论。
pub const XC_NOT_LOCKED: u16 = SEG_BASE + 7;
/// 0x74C8 代际越界。
pub const XC_BAD_GEN: u16 = SEG_BASE + 8;
/// 0x74C9 位图全空（一个能力都没有不可锁）。
pub const XC_CAPS_EMPTY: u16 = SEG_BASE + 9;

/// 探测账容量（写死：32）。
pub const PROBE_CAP: usize = 32;
/// 能力位总数（写死：16）。
pub const BIT_CAP: u32 = 16;

/// 能力位号闭集。
pub const B_WAVE_RASTER: u32 = 0;
pub const B_INFCACHE: u32 = 1;
pub const B_MESH: u32 = 2;
pub const B_RAYTRACE: u32 = 3;
pub const B_VCN: u32 = 4;
pub const B_DUAL_MEDIA: u32 = 5;
pub const B_SAM: u32 = 6;
pub const B_TESS: u32 = 7;
pub const B_ASYNC: u32 = 8;
pub const B_HALFRATE: u32 = 9;
pub const B_DCC: u32 = 10;
pub const B_SHADOWSAMPLE: u32 = 11;
pub const B_CLKGATE: u32 = 12;
pub const B_VOLREG: u32 = 13;
pub const B_MULTIGPU: u32 = 14;
pub const B_DBGPROBE: u32 = 15;

/// 代际闭集：0=GCN4 1=GCN5 2=RDNA1 3=RDNA2 4=RDNA3。
pub const G_GCN4: usize = 0;
pub const G_GCN5: usize = 1;
pub const G_RDNA1: usize = 2;
pub const G_RDNA2: usize = 3;
pub const G_RDNA3: usize = 4;
/// 代际上限。
pub const GEN_CAP: usize = 5;

/// 每代际禁止位掩码（1 = 该代际禁止置位；写死违规表）。
pub const GEN_FORBID: [u32; GEN_CAP] = [
    // GCN4：禁 网格着色(2)/光追(3)/SAM(6)/双媒体(5)。
    (1 << B_MESH) | (1 << B_RAYTRACE) | (1 << B_SAM) | (1 << B_DUAL_MEDIA),
    // GCN5：禁 网格着色(2)/光追(3)/SAM(6)。
    (1 << B_MESH) | (1 << B_RAYTRACE) | (1 << B_SAM),
    // RDNA1：禁 光追(3)/SAM(6)。
    (1 << B_RAYTRACE) | (1 << B_SAM),
    // RDNA2：无禁止。
    0,
    // RDNA3：无禁止。
    0,
];

/// 探测状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PPhase {
    /// 探测中。
    Probe,
    /// 已确认（复核中）。
    Confirm,
    /// 已锁定（只读）。
    Lock,
}

/// 单条探测事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeEvent {
    /// 能力位（0..=15）。
    pub bit: u32,
    /// 探测轮次（单调）。
    pub round: u32,
    /// 置位/清位。
    pub set: bool,
}

/// 能力探测册。
pub struct CapsProbe {
    /// 代际号。
    pub gen: usize,
    /// 能力位图。
    pub bits: u32,
    /// 探测状态。
    pub phase: PPhase,
    /// 探测事件环（定容 32）。
    pub ring: Vec<ProbeEvent>,
    /// 环头（满后覆盖位）。
    pub head: usize,
    /// 累计入账。
    pub pushed: u64,
    /// 累计逐出。
    pub evicted: u64,
    /// 最后探测轮次。
    pub last_round: u32,
    /// 累计红码。
    pub red_count: u64,
}

impl CapsProbe {
    /// 开探测册。
    pub fn new(gen: usize) -> Result<Self, u16> {
        if gen >= GEN_CAP {
            return Err(XC_BAD_GEN);
        }
        Ok(CapsProbe {
            gen,
            bits: 0,
            phase: PPhase::Probe,
            ring: Vec::new(),
            head: 0,
            pushed: 0,
            evicted: 0,
            last_round: 0,
            red_count: 0,
        })
    }

    /// 探测账守恒。
    pub fn balanced(&self) -> bool {
        self.pushed == self.ring.len() as u64 + self.evicted
    }

    /// 置位/清位能力（Lock 拒写；代纪禁位红；轮次单调）。
    pub fn probe(&mut self, bit: u32, round: u32, set: bool) -> Result<(), u16> {
        if self.phase == PPhase::Lock {
            self.red_count += 1;
            return Err(XC_CAPS_LOCKED);
        }
        if bit >= BIT_CAP {
            self.red_count += 1;
            return Err(XC_BAD_BIT);
        }
        if round <= self.last_round {
            self.red_count += 1;
            return Err(XC_PROBE_DUP);
        }
        let forbidden = GEN_FORBID[self.gen] & (1u32 << bit);
        if set && forbidden != 0 {
            self.red_count += 1;
            return Err(XC_CAPS_GEN_VIOLATION);
        }
        // 重复事件判定：同位同轮。
        let mut i = 0;
        while i < self.ring.len() {
            if self.ring[i].bit == bit && self.ring[i].round == round {
                self.red_count += 1;
                return Err(XC_PROBE_DUP);
            }
            i += 1;
        }
        let ev = ProbeEvent { bit, round, set };
        if self.ring.len() == PROBE_CAP {
            self.ring[self.head] = ev;
            self.head = (self.head + 1) % PROBE_CAP;
            self.evicted += 1;
            self.pushed += 1;
        } else {
            self.ring.push(ev);
            self.pushed += 1;
        }
        if set {
            self.bits |= 1u32 << bit;
        } else {
            self.bits &= !(1u32 << bit);
        }
        self.last_round = round;
        Ok(())
    }

    /// 位图自洽判据：双媒体⇒VCN、光追⇒网格、SAM⇒DCC。
    pub fn consistent(&self) -> Result<(), u16> {
        let has = |b: u32| self.bits & (1u32 << b) != 0;
        if has(B_DUAL_MEDIA) && !has(B_VCN) {
            return Err(XC_CAPS_INCONSIST);
        }
        if has(B_RAYTRACE) && !has(B_MESH) {
            return Err(XC_CAPS_INCONSIST);
        }
        if has(B_SAM) && !has(B_DCC) {
            return Err(XC_CAPS_INCONSIST);
        }
        Ok(())
    }

    /// 进入 Confirm：跑自洽 + 守恒。
    pub fn confirm(&mut self) -> Result<(), u16> {
        if self.phase != PPhase::Probe {
            self.red_count += 1;
            return Err(XC_CAPS_LOCKED);
        }
        if !self.balanced() {
            self.red_count += 1;
            return Err(XC_PROBE_DRIFT);
        }
        self.consistent()?;
        if self.bits == 0 {
            self.red_count += 1;
            return Err(XC_CAPS_EMPTY);
        }
        self.phase = PPhase::Confirm;
        Ok(())
    }

    /// 锁定：Confirm 期复核通过才可 Lock。
    pub fn lock(&mut self) -> Result<u32, u16> {
        if self.phase != PPhase::Confirm {
            self.red_count += 1;
            return Err(XC_NOT_LOCKED);
        }
        self.consistent()?;
        if !self.balanced() {
            self.red_count += 1;
            return Err(XC_PROBE_DRIFT);
        }
        self.phase = PPhase::Lock;
        Ok(self.bits)
    }

    /// 支持判定。
    pub fn has(&mut self, bit: u32) -> Result<bool, u16> {
        if bit >= BIT_CAP {
            self.red_count += 1;
            return Err(XC_BAD_BIT);
        }
        Ok(self.bits & (1u32 << bit) != 0)
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amdcaps gen=");
        s += &self.gen.to_string();
        s += " bits=0x";
        let mut nib = 8;
        while nib > 0 {
            nib -= 1;
            let d = (self.bits >> (nib * 4)) & 0xF;
            let ch = if d < 10 {
                (b'0' + d as u8) as char
            } else {
                (b'A' + (d - 10) as u8) as char
            };
            s.push(ch);
        }
        s += " probes=";
        s += &self.pushed.to_string();
        s
    }

    /// 全账自检。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_PROBE_DRIFT);
        }
        self.consistent()?;
        Ok(())
    }
}

/// 代际支持位数速查（探测结论的独立复核面）。
pub fn gen_support_count(gen: usize) -> Result<u32, u16> {
    if gen >= GEN_CAP {
        return Err(XC_BAD_GEN);
    }
    let forbid = GEN_FORBID[gen];
    let mut n = 0;
    let mut b = 0;
    while b < BIT_CAP {
        if forbid & (1u32 << b) == 0 {
            n += 1;
        }
        b += 1;
    }
    Ok(n)
}

/// 自例程：RDNA2 全探测 → 自洽修复 → 锁定 → 复核 → 负例五连。
pub fn drill() -> Result<u32, u16> {
    let mut p = CapsProbe::new(G_RDNA2)?;
    let mut round: u32 = 0;
    // 基础位集：VCN、DCC、异步、门控、调节、网格。
    let base: [u32; 6] = [B_VCN, B_DCC, B_ASYNC, B_CLKGATE, B_VOLREG, B_MESH];
    let mut i = 0;
    while i < base.len() {
        round += 1;
        p.probe(base[i], round, true)?;
        i += 1;
    }
    // 自洽修复：补双媒体与 SAM 的前置已满足。
    round += 1;
    p.probe(B_DUAL_MEDIA, round, true)?;
    round += 1;
    p.probe(B_SAM, round, true)?;
    // 负例：GCN4 置 SAM（代纪禁位）。
    let mut g4 = CapsProbe::new(G_GCN4)?;
    if g4.probe(B_SAM, 1, true) != Err(XC_CAPS_GEN_VIOLATION) {
        return Err(XC_CAPS_GEN_VIOLATION);
    }
    // 负例：同轮重复。
    let r = p.last_round;
    if p.probe(B_TESS, r, true) != Err(XC_PROBE_DUP) {
        return Err(XC_PROBE_DUP);
    }
    // 负例：位越界。
    if p.probe(BIT_CAP, 1, true) != Err(XC_BAD_BIT) {
        return Err(XC_BAD_BIT);
    }
    // 负例：自洽违规（光追缺网格——先清网格再置光追）。
    let mut q = CapsProbe::new(G_RDNA3)?;
    q.probe(B_DCC, 1, true)?;
    q.probe(B_RAYTRACE, 2, true)?;
    if q.consistent() != Err(XC_CAPS_INCONSIST) {
        return Err(XC_CAPS_INCONSIST);
    }
    q.probe(B_MESH, 3, true)?;
    q.audit()?;
    // 正链：确认 → 锁定。
    p.confirm()?;
    // 负例：Probe 期锁定应红（阶段不符按 NOT_LOCKED）。
    let mut late = CapsProbe::new(G_RDNA1)?;
    late.probe(B_VCN, 1, true)?;
    if late.lock() != Err(XC_NOT_LOCKED) {
        return Err(XC_NOT_LOCKED);
    }
    let bits = p.lock()?;
    // 负例：Lock 后写入。
    if p.probe(B_TESS, 99, true) != Err(XC_CAPS_LOCKED) {
        return Err(XC_CAPS_LOCKED);
    }
    p.audit()?;
    // 复核：位计数与速查一致。
    let mut cnt = 0;
    let mut b = 0;
    while b < BIT_CAP {
        if p.has(b)? {
            cnt += 1;
        }
        b += 1;
    }
    let support = gen_support_count(G_RDNA2)?;
    if cnt > support {
        return Err(XC_CAPS_GEN_VIOLATION);
    }
    if p.summary().len() == 0 || bits == 0 {
        return Err(XC_CAPS_EMPTY);
    }
    Ok(0)
}
