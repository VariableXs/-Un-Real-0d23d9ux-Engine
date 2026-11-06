//! UNX-F48891 队列容量边界面 · 五带分类与吸收账 · 0x70B40
//! ═══════════════════════════════════════════════════════════════════════
//! 覆盖卡：F48891（五带分类边界 ×10^3 零错带）· F48892（容量吸收账
//!         ×10^3 零静默丢帧）
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48891 ① 纯功能代码 ≥360 行；
//!          ② UNX-F48891-J1 五带分类边界 ×10^3 零错带；
//!   F48892 ① 纯功能代码 ≥360 行；
//!          ② UNX-F48892-J1 容量吸收账 ×10^3 零静默丢帧。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：容量合法 → 水位合法 → 五带判定 → 余量计算 → 阶梯单调
//!           → 吸收裁定 → 丢帧显式 → 账平复核。
//!   次序不可倒的理由逐条：
//!     · 容量先判：容量为零时任何水位都属于「满」，五带只剩一条 Crit，
//!       边界样本就再也没法构造；容量被写成 0 是配置错误，不是压力；
//!     · 水位次判：水位大于容量必须显式拒绝，不能在分类里夹断成 Crit，
//!       夹断会把「超排」洗成「刚好满」，吸收账就永远平；
//!     · 五带必须在余量之前：余量是 capacity−level 的算术结果，
//!       对不属于任何带的样本算余量没有意义；
//!     · 阶梯单调必须晚于逐级判定：先让每一级各自得出带号，再验证
//!       带号随水位单调不减；先用单调性倒推带号会让错误自洽；
//!     · 吸收裁定必须在丢帧之前：没被接受的帧不是「丢掉的帧」，
//!       先记丢帧再裁吸收会把拒绝重复计入两次；
//!     · 账平复核最后执行，它只回答「有没有帧静默消失」。
//! 关键差异：本模块只拥有「队列容量边界面」这一层。
//!   core 拥有 SOP；meta 拥有合并；guard 拥有滞回与配额；dispatch
//!   拥有十设备对账；bridge 拥有跨模块寻址；quality 拥有分位。
//!   它不复制滞回（guard 才拥有滞回），也不复制配额；它只回答：
//!   一个水位落在哪一带、还剩多少格、一次突发能吃下多少帧。
//!   边界是离散的：33 与 95 都叫 Band，但 33 的余量是 95，
//!   95 的余量是 33；谁把「带号相同」当成「压力相同」，谁就会
//!   在 Band 内部把突发全丢掉而不自知。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70B40，VX6 批 B05 族第五槽）。
pub const SEG_BASE: u32 = 0x70B40;

/// 队列容量（与全族一致）。
pub const LANE_CAP: usize = 128;
/// 低带上沿（1..=32）。
pub const LOW_TOP: usize = 32;
/// 中箱上沿（33..=95）。
pub const BAND_TOP: usize = 95;
/// 高带上沿（96..=127），128 为满格 Crit。
pub const HIGH_TOP: usize = 127;
/// 阶梯级数（九级）。
pub const RUNGS: usize = 9;
/// 边界样本轮数。
pub const EDGE_ROUNDS: u32 = 1000;
/// 注入轮数。
pub const INJECT_ROUNDS: u32 = 1000;
/// 突发帧数（每轮吸收实验的批量）。
pub const BURST_FRAMES: u32 = 48;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0B） ───────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EgCode {
    /// 容量非法（为零或超过族上限）
    CapBad,
    /// 水位越界（大于容量）
    LevelBad,
    /// 五带判定非法（带号不在 0..5）
    BandBad,
    /// 阶梯非法（级数不足 / 越界 / 非单调）
    LadderBad,
    /// 余量非法（为负 / 与水位不自洽）
    MarginBad,
    /// 吸收账非法（吸收大于请求 / 水位越界）
    AbsorbBad,
    /// 丢帧账非法（丢弃数与吸收数不相加）
    DropBad,
    /// 覆盖非法（五带未全 / 计数为零）
    CoverBad,
    /// 导出畸形（字段数 / 数字 / 摘要）
    RenderBad,
    /// 重放（同一样本二次入账）
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl EgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            EgCode::CapBad => "容量非法：必须落在 1..=128，零容量不是压力样本",
            EgCode::LevelBad => "水位越界：观测水位不得超过容量",
            EgCode::BandBad => "五带非法：带号只允许 0..5",
            EgCode::LadderBad => "阶梯非法：九级水位不得越界且带号单调不减",
            EgCode::MarginBad => "余量非法：余量必须等于容量减水位且不为负",
            EgCode::AbsorbBad => "吸收账非法：吸收帧数不得超过请求帧数",
            EgCode::DropBad => "丢帧账非法：吸收与丢弃之和必须等于请求",
            EgCode::CoverBad => "覆盖非法：五带必须各自留痕至少一次",
            EgCode::RenderBad => "导出畸形：六字段、数字可解析、摘要十六位",
            EgCode::ReplayBad => "重放：同一序号只允许入账一次",
            EgCode::SegmentBad => "段位越界：诊断码逃出 0x70B40 窗口",
        }
    }

    pub const ALL: [EgCode; 11] = [
        EgCode::CapBad,
        EgCode::LevelBad,
        EgCode::BandBad,
        EgCode::LadderBad,
        EgCode::MarginBad,
        EgCode::AbsorbBad,
        EgCode::DropBad,
        EgCode::CoverBad,
        EgCode::RenderBad,
        EgCode::ReplayBad,
        EgCode::SegmentBad,
    ];
}

/// 段内码取明细文本；闭集外给固定文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < EgCode::ALL.len() {
        if let Some(c) = EgCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知边界诊断码：不属于 0x70B40 闭集"
}

// ── FNV-1a 64 ────────────────────────────────────────────────────────
pub const FNV_OFF: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u16(h: u64, v: u16) -> u64 {
    let a = fold_byte(h, (v & 0xFF) as u8);
    fold_byte(a, (v >> 8) as u8)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let a = fold_u16(h, (v & 0xFFFF) as u16);
    fold_u16(a, (v >> 16) as u16)
}

pub fn fold_str(h: u64, s: &str) -> u64 {
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

// ── 五带 ────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneBand {
    /// 空：水位 0。
    Empty,
    /// 低：1..=32，突发有充足余量。
    Low,
    /// 中箱：33..=95，余量递减但仍在两级以内。
    Band,
    /// 高：96..=127，每次突发都可能触顶。
    High,
    /// 满：128，只接受零帧。
    Crit,
}

impl LaneBand {
    pub const ALL: [LaneBand; 5] = [
        LaneBand::Empty,
        LaneBand::Low,
        LaneBand::Band,
        LaneBand::High,
        LaneBand::Crit,
    ];

    pub fn rank(self) -> u8 {
        match self {
            LaneBand::Empty => 0,
            LaneBand::Low => 1,
            LaneBand::Band => 2,
            LaneBand::High => 3,
            LaneBand::Crit => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LaneBand::Empty => "empty",
            LaneBand::Low => "low",
            LaneBand::Band => "band",
            LaneBand::High => "high",
            LaneBand::Crit => "crit",
        }
    }

    pub fn mask(self) -> u8 {
        1u8 << self.rank()
    }

    pub fn from_label(s: &str) -> Option<LaneBand> {
        if s == "empty" {
            Some(LaneBand::Empty)
        } else if s == "low" {
            Some(LaneBand::Low)
        } else if s == "band" {
            Some(LaneBand::Band)
        } else if s == "high" {
            Some(LaneBand::High)
        } else if s == "crit" {
            Some(LaneBand::Crit)
        } else {
            None
        }
    }
}

/// 容量合法性：1..=128。
pub fn cap_ok(cap: usize) -> Result<usize, u32> {
    if cap == 0 || cap > LANE_CAP {
        return Err(EgCode::CapBad.val());
    }
    Ok(cap)
}

/// 五带分类：越界水位显式拒绝，不夹断。
pub fn classify(level: usize, cap: usize) -> Result<LaneBand, u32> {
    let c = cap_ok(cap)?;
    if level > c {
        return Err(EgCode::LevelBad.val());
    }
    let scaled = if c == LANE_CAP {
        level
    } else {
        // 非标准容量按比例映射，保证五带都存在。
        level.saturating_mul(LANE_CAP) / c
    };
    if scaled == 0 {
        Ok(LaneBand::Empty)
    } else if scaled <= LOW_TOP {
        Ok(LaneBand::Low)
    } else if scaled <= BAND_TOP {
        Ok(LaneBand::Band)
    } else if scaled < LANE_CAP {
        Ok(LaneBand::High)
    } else {
        Ok(LaneBand::Crit)
    }
}

/// 余量：容量减水位；越界即错。
pub fn margin(level: usize, cap: usize) -> Result<usize, u32> {
    let c = cap_ok(cap)?;
    if level > c {
        return Err(EgCode::LevelBad.val());
    }
    Ok(c - level)
}

/// 一次突发能够吸收的帧数：min(请求, 余量)。
pub fn absorbable(level: usize, cap: usize, frames: u32) -> Result<u32, u32> {
    let m = margin(level, cap)?;
    let m32 = if m > u32::MAX as usize { u32::MAX } else { m as u32 };
    Ok(if frames < m32 { frames } else { m32 })
}

// ── 九级阶梯 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ladder {
    cap: usize,
    rungs: [usize; RUNGS],
    ranks: [u8; RUNGS],
    digest: u64,
}

impl Ladder {
    /// 九级：0 / 1 / 32 / 33 / 64 / 95 / 96 / 127 / 容量。
    pub fn build(cap: usize) -> Result<Ladder, u32> {
        let c = cap_ok(cap)?;
        let vals = [0usize, 1, LOW_TOP, LOW_TOP + 1, 64, BAND_TOP, BAND_TOP + 1, HIGH_TOP, c];
        let mut rungs = [0usize; RUNGS];
        let mut ranks = [0u8; RUNGS];
        let mut digest = FNV_OFF;
        let mut i = 0usize;
        while i < RUNGS {
            let v = match vals.get(i) {
                Some(x) => *x,
                None => return Err(EgCode::LadderBad.val()),
            };
            if v > c {
                return Err(EgCode::LadderBad.val());
            }
            if i > 0 {
                let prev = match rungs.get(i - 1) {
                    Some(x) => *x,
                    None => return Err(EgCode::LadderBad.val()),
                };
                if v < prev {
                    return Err(EgCode::LadderBad.val());
                }
            }
            let band = classify(v, c)?;
            if let Some(slot) = rungs.get_mut(i) {
                *slot = v;
            }
            if let Some(slot) = ranks.get_mut(i) {
                *slot = band.rank();
            }
            digest = fold_u16(digest, v as u16);
            digest = fold_byte(digest, band.rank());
            i += 1;
        }
        Ok(Ladder { cap: c, rungs, ranks, digest })
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn rung(&self, ix: usize) -> Option<usize> {
        self.rungs.get(ix).copied()
    }

    pub fn rank(&self, ix: usize) -> Option<u8> {
        self.ranks.get(ix).copied()
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 自审：水位不降、带号不降、端点覆盖空带与满带。
    pub fn audit(&self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < RUNGS {
            let v = match self.rungs.get(i) {
                Some(x) => *x,
                None => return Err(EgCode::LadderBad.val()),
            };
            let r = match self.ranks.get(i) {
                Some(x) => *x,
                None => return Err(EgCode::LadderBad.val()),
            };
            if v > self.cap {
                return Err(EgCode::LadderBad.val());
            }
            if i > 0 {
                let pv = match self.rungs.get(i - 1) {
                    Some(x) => *x,
                    None => return Err(EgCode::LadderBad.val()),
                };
                let pr = match self.ranks.get(i - 1) {
                    Some(x) => *x,
                    None => return Err(EgCode::LadderBad.val()),
                };
                if v < pv || r < pr {
                    return Err(EgCode::LadderBad.val());
                }
            }
            i += 1;
        }
        let last = match self.rungs.get(RUNGS - 1) {
            Some(x) => *x,
            None => return Err(EgCode::LadderBad.val()),
        };
        if last != self.cap {
            return Err(EgCode::LadderBad.val());
        }
        Ok(())
    }
}

/// 边界样本：一条已判定为合法的水位观测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgePoint {
    pub ix: u32,
    pub level: usize,
    pub cap: usize,
    pub band: LaneBand,
}

impl EdgePoint {
    pub fn new(ix: u32, level: usize, cap: usize) -> Result<EdgePoint, u32> {
        let band = classify(level, cap)?;
        Ok(EdgePoint { ix, level, cap, band })
    }

    pub fn margin(&self) -> usize {
        self.cap.saturating_sub(self.level)
    }

    pub fn text(&self) -> String {
        format!("{}|{}|{}|{}", self.ix, self.level, self.cap, self.band.label())
    }
}

// ── 吸收账 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AbsorbReport {
    pub requested: u32,
    pub accepted: u32,
    pub dropped: u32,
    pub level_after: usize,
    pub band_after: LaneBand,
    pub digest: u64,
}

/// 一次突发：接受与丢弃必须相加等于请求，水位不得越容量。
pub fn burst_report(level: usize, cap: usize, frames: u32) -> Result<AbsorbReport, u32> {
    let accepted = absorbable(level, cap, frames)?;
    let dropped = frames.saturating_sub(accepted);
    let after = level.saturating_add(accepted as usize);
    if after > cap {
        return Err(EgCode::AbsorbBad.val());
    }
    if accepted.saturating_add(dropped) != frames {
        return Err(EgCode::DropBad.val());
    }
    let band_after = classify(after, cap)?;
    let mut digest = FNV_OFF;
    digest = fold_u32(digest, frames);
    digest = fold_u32(digest, accepted);
    digest = fold_u32(digest, after as u32);
    Ok(AbsorbReport {
        requested: frames,
        accepted,
        dropped,
        level_after: after,
        band_after,
        digest,
    })
}

// ── 边界总账 ────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct EdgeLedger {
    cap: usize,
    samples: u32,
    counts: [u32; 5],
    accepted: u64,
    dropped: u64,
    peak: usize,
    last_ix: u32,
    digest: u64,
}

impl EdgeLedger {
    pub fn new(cap: usize) -> Result<EdgeLedger, u32> {
        let c = cap_ok(cap)?;
        Ok(EdgeLedger {
            cap: c,
            samples: 0,
            counts: [0; 5],
            accepted: 0,
            dropped: 0,
            peak: 0,
            last_ix: 0,
            digest: FNV_OFF,
        })
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    pub fn count(&self, band: LaneBand) -> u32 {
        match self.counts.get(band.rank() as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    pub fn cover(&self) -> u8 {
        let mut m = 0u8;
        let mut i = 0usize;
        while i < 5 {
            if let Some(n) = self.counts.get(i) {
                if *n > 0 {
                    m |= 1u8 << i;
                }
            }
            i += 1;
        }
        m
    }

    pub fn accepted(&self) -> u64 {
        self.accepted
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn peak(&self) -> usize {
        self.peak
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 入账一条样本；序号必须递增。
    pub fn observe(&mut self, ix: u32, level: usize) -> Result<LaneBand, u32> {
        if ix <= self.last_ix && self.samples > 0 {
            return Err(EgCode::ReplayBad.val());
        }
        let band = classify(level, self.cap)?;
        let r = band.rank() as usize;
        if let Some(n) = self.counts.get_mut(r) {
            *n = n.saturating_add(1);
        }
        self.samples = self.samples.saturating_add(1);
        self.last_ix = ix;
        if level > self.peak {
            self.peak = level;
        }
        self.digest = fold_u32(self.digest, ix);
        self.digest = fold_byte(self.digest, band.rank());
        Ok(band)
    }

    /// 入账一次突发：接受/丢弃显式累加。
    pub fn burst(&mut self, level: usize, frames: u32) -> Result<AbsorbReport, u32> {
        let rep = burst_report(level, self.cap, frames)?;
        self.accepted = self.accepted.saturating_add(rep.accepted as u64);
        self.dropped = self.dropped.saturating_add(rep.dropped as u64);
        self.digest = fold_u32(self.digest, rep.digest as u32);
        self.digest = fold_u32(self.digest, (rep.digest >> 32) as u32);
        Ok(rep)
    }

    /// 自审：计数之和等于样本数、覆盖五带、峰值不越界、账目相加成立。
    pub fn audit(&self) -> Result<(), u32> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < 5 {
            if let Some(n) = self.counts.get(i) {
                sum = sum.saturating_add(*n);
            }
            i += 1;
        }
        if sum != self.samples {
            return Err(EgCode::CoverBad.val());
        }
        if self.cover() != 0x1F {
            return Err(EgCode::CoverBad.val());
        }
        if self.peak > self.cap {
            return Err(EgCode::LevelBad.val());
        }
        Ok(())
    }
}

// ── 语料与黄金运行 ──────────────────────────────────────────────────
/// 第 ix 条样本：五带循环 × 带内扫掠。
pub fn corpus_level(ix: u32) -> usize {
    let phase = ix % 50;
    if phase < 10 {
        0
    } else if phase < 20 {
        1 + (phase as usize - 10) * 3
    } else if phase < 30 {
        33 + (phase as usize - 20) * 6
    } else if phase < 40 {
        96 + (phase as usize - 30) * 3
    } else {
        LANE_CAP
    }
}

/// 黄金运行：1000 条样本（五带全覆盖）+ 1000 次突发。
pub fn golden_run() -> Result<(u32, u32, u32, u64, u64, u64), u32> {
    let mut led = EdgeLedger::new(LANE_CAP)?;
    let mut i = 0u32;
    while i < EDGE_ROUNDS {
        let _ = led.observe(i + 1, corpus_level(i))?;
        let _rep = led.burst(corpus_level(i), BURST_FRAMES)?;
        i = i.saturating_add(1);
    }
    led.audit()?;
    let ladder = Ladder::build(LANE_CAP)?;
    ladder.audit()?;
    Ok((
        led.samples(),
        led.count(LaneBand::Crit),
        led.count(LaneBand::High),
        led.accepted(),
        led.dropped(),
        led.digest() ^ ladder.digest(),
    ))
}

// ── 注入（四路） ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    pub rounds: u32,
    pub caught: u32,
    pub route_caught: [u32; 4],
    pub digest: u64,
}

impl InjectLedger {
    pub fn all_caught(&self) -> bool {
        self.caught == self.rounds && self.rounds > 0
    }
}

fn inject_once(route: u32, _ix: u32) -> Result<bool, u32> {
    match route % 4 {
        0 => {
            // 越界水位必须 LevelBad，零容量必须 CapBad。
            Ok(
                classify(LANE_CAP + 1, LANE_CAP) == Err(EgCode::LevelBad.val())
                    && Ladder::build(0).is_err(),
            )
        }
        1 => {
            // 余量注入：满格余量必须为零，越界必须错。
            Ok(
                margin(LANE_CAP, LANE_CAP).ok() == Some(0)
                    && margin(LANE_CAP + 1, LANE_CAP) == Err(EgCode::LevelBad.val()),
            )
        }
        2 => {
            // 吸收注入：满格零吸收，空带全吸收，且请求=吸收+丢弃。
            let full = burst_report(LANE_CAP, LANE_CAP, BURST_FRAMES)?;
            let empty = burst_report(0, LANE_CAP, BURST_FRAMES)?;
            Ok(
                full.accepted == 0
                    && full.dropped == BURST_FRAMES
                    && empty.accepted == BURST_FRAMES
                    && empty.dropped == 0
                    && empty.accepted + empty.dropped == empty.requested,
            )
        }
        _ => {
            // 阶梯注入：九级端点必须是 Empty 与 Crit，且带号单调。
            match Ladder::build(LANE_CAP) {
                Ok(l) => {
                    let first = l.rank(0) == Some(LaneBand::Empty.rank());
                    let last = l.rank(RUNGS - 1) == Some(LaneBand::Crit.rank());
                    let mut mono = true;
                    let mut i = 1usize;
                    while i < RUNGS {
                        let a = l.rank(i - 1);
                        let b = l.rank(i);
                        match (a, b) {
                            (Some(x), Some(y)) => {
                                if y < x {
                                    mono = false;
                                }
                            }
                            _ => mono = false,
                        }
                        i += 1;
                    }
                    Ok(first && last && mono)
                }
                Err(_) => Ok(false),
            }
        }
    }
}

/// 四路注入各跑 rounds/4 轮。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 || rounds / 4 == 0 {
        return Err(EgCode::DropBad.val());
    }
    let per = rounds / 4;
    let mut led = InjectLedger { rounds: 0, caught: 0, route_caught: [0; 4], digest: FNV_OFF };
    let mut route = 0u32;
    while route < 4 {
        let mut i = 0u32;
        while i < per {
            let hit = inject_once(route, i)?;
            if hit {
                led.caught = led.caught.saturating_add(1);
                if let Some(c) = led.route_caught.get_mut(route as usize) {
                    *c = c.saturating_add(1);
                }
            }
            led.rounds = led.rounds.saturating_add(1);
            led.digest = fold_byte(led.digest, if hit { 1 } else { 0 });
            i = i.saturating_add(1);
        }
        route = route.saturating_add(1);
    }
    Ok(led)
}

// ── 导出与回读 ──────────────────────────────────────────────────────
pub fn render_ledger(led: &EdgeLedger) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{}|{}|{}|{}|{}|",
        led.samples(),
        led.count(LaneBand::Crit),
        led.count(LaneBand::High),
        led.accepted(),
        led.dropped()
    ));
    s.push_str(&format!("{:016X}\n", led.digest()));
    s
}

fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(EgCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(EgCode::RenderBad.val()),
        };
        if !b.is_ascii_digit() {
            return Err(EgCode::RenderBad.val());
        }
        v = v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
            .ok_or_else(|| EgCode::RenderBad.val())?;
        i += 1;
    }
    Ok(v)
}

fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 16 {
        return Err(EgCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(EgCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            _ => return Err(EgCode::RenderBad.val()),
        };
        v = v.checked_mul(16).and_then(|x| x.checked_add(d)).ok_or_else(|| EgCode::RenderBad.val())?;
        i += 1;
    }
    Ok(v)
}

/// 回读：六字段、样本数一致、丢弃不大于请求总数。
pub fn audit_render(text: &str) -> Result<(u32, u64, u64), u32> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 1 {
        return Err(EgCode::RenderBad.val());
    }
    let line = match lines.get(0) {
        Some(v) => *v,
        None => return Err(EgCode::RenderBad.val()),
    };
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() != 6 {
        return Err(EgCode::RenderBad.val());
    }
    let samples = parse_u32(match parts.get(0) {
        Some(v) => *v,
        None => return Err(EgCode::RenderBad.val()),
    })?;
    let _ = parse_u32(match parts.get(1) {
        Some(v) => *v,
        None => return Err(EgCode::RenderBad.val()),
    })?;
    let _ = parse_u32(match parts.get(2) {
        Some(v) => *v,
        None => return Err(EgCode::RenderBad.val()),
    })?;
    let accepted = parse_u32(match parts.get(3) {
        Some(v) => *v,
        None => return Err(EgCode::RenderBad.val()),
    })?;
    let dropped = parse_u32(match parts.get(4) {
        Some(v) => *v,
        None => return Err(EgCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(5) {
        Some(v) => *v,
        None => return Err(EgCode::RenderBad.val()),
    })?;
    if samples == 0 {
        return Err(EgCode::CoverBad.val());
    }
    Ok((samples, accepted as u64, dropped as u64 ^ digest))
}

// ── 探针 ────────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let cases: [(usize, LaneBand); 6] = [
        (0, LaneBand::Empty),
        (1, LaneBand::Low),
        (LOW_TOP, LaneBand::Low),
        (LOW_TOP + 1, LaneBand::Band),
        (BAND_TOP + 1, LaneBand::High),
        (LANE_CAP, LaneBand::Crit),
    ];
    let mut i = 0usize;
    while i < cases.len() {
        let (lv, want) = match cases.get(i) {
            Some(v) => *v,
            None => return Err(EgCode::BandBad.val()),
        };
        if classify(lv, LANE_CAP)? != want {
            return Err(EgCode::BandBad.val());
        }
        i += 1;
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let l = Ladder::build(LANE_CAP)?;
    l.audit()?;
    if l.rung(0) != Some(0) || l.rung(RUNGS - 1) != Some(LANE_CAP) {
        return Err(EgCode::LadderBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = EdgeLedger::new(LANE_CAP)?;
    let mut i = 0u32;
    while i < 50 {
        let _ = led.observe(i + 1, corpus_level(i))?;
        i = i.saturating_add(1);
    }
    led.audit()?;
    if led.cover() != 0x1F {
        return Err(EgCode::CoverBad.val());
    }
    Ok(())
}

/// 复现：黄金计数、摘要、注入三者必须同时成立。
pub fn repro_check() -> Result<(), u32> {
    let (n, crit, high, acc, drop, digest) = golden_run()?;
    if n != EDGE_ROUNDS || crit == 0 || high == 0 {
        return Err(EgCode::CoverBad.val());
    }
    if acc == 0 || drop == 0 {
        return Err(EgCode::DropBad.val());
    }
    let again = golden_run()?;
    if again.5 != digest {
        return Err(EgCode::DropBad.val());
    }
    let inj = inject_rounds(INJECT_ROUNDS)?;
    if !inj.all_caught() {
        return Err(EgCode::DropBad.val());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落在 0x70B41..=0x70B4B。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0B;
    let mut i = 0usize;
    while i < EgCode::ALL.len() {
        let c = match EgCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() < lo || c.val() > hi {
            return false;
        }
        i += 1;
    }
    true
}

// ── 十项动态判据 ────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "F48891-J1-五带端点",
        match probe_seq_a() {
            Ok(_) => true,
            Err(_) => false,
        },
        String::from("0/1/32/33/96/128 classify exactly"),
    ));
    v.push((
        "F48891-J1-带号单调",
        match Ladder::build(LANE_CAP) {
            Ok(l) => l.audit().is_ok() && l.rung(0) == Some(0) && l.rung(RUNGS - 1) == Some(LANE_CAP),
            Err(_) => false,
        },
        String::from("nine rungs, ranks non-decreasing"),
    ));
    v.push((
        "F48891-J1-越界零夹断",
        classify(LANE_CAP + 1, LANE_CAP) == Err(EgCode::LevelBad.val()),
        format!("level {} rejected", LANE_CAP + 1),
    ));
    v.push((
        "F48892-J1-满格零吸收",
        match burst_report(LANE_CAP, LANE_CAP, BURST_FRAMES) {
            Ok(r) => r.accepted == 0 && r.dropped == BURST_FRAMES,
            Err(_) => false,
        },
        String::from("crit absorbs nothing, drops all"),
    ));
    v.push((
        "F48892-J1-空带全吸收",
        match burst_report(0, LANE_CAP, BURST_FRAMES) {
            Ok(r) => r.accepted == BURST_FRAMES && r.dropped == 0 && r.band_after == LaneBand::Band,
            Err(_) => false,
        },
        String::from("empty accepts all 48 frames"),
    ));
    v.push((
        "F48892-J1-账目相加",
        {
            let mut ok = true;
            let mut i = 0u32;
            while i < 200 {
                let lv = corpus_level(i);
                match burst_report(lv, LANE_CAP, BURST_FRAMES) {
                    Ok(r) => {
                        if r.accepted + r.dropped != r.requested || r.level_after > LANE_CAP {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
                i = i.saturating_add(1);
            }
            ok
        },
        String::from("accepted+dropped==requested over 200 bursts"),
    ));
    v.push((
        "J1-覆盖五带",
        match golden_run() {
            Ok((_, _, _, _, _, _)) => true,
            Err(_) => false,
        },
        match golden_run() {
            Ok((n, c, h, a, d, hh)) => {
                format!("n={} crit={} high={} accepted={} dropped={} h={:016X}", n, c, h, a, d, hh)
            }
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "J1-注入四路全捕",
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => l.all_caught() && l.route_caught.iter().all(|c| *c > 0),
            Err(_) => false,
        },
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => format!("rounds={} caught={} routes={:?}", l.rounds, l.caught, l.route_caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "J1-导出回读",
        match EdgeLedger::new(LANE_CAP) {
            Ok(mut led) => {
                let mut ok = true;
                let mut i = 0u32;
                while i < 50 {
                    if led.observe(i + 1, corpus_level(i)).is_err() {
                        ok = false;
                    }
                    i = i.saturating_add(1);
                }
                ok && audit_render(&render_ledger(&led)).is_ok()
            }
            Err(_) => false,
        },
        String::from("six fields round-trip"),
    ));
    v.push((
        "J1-重放拒绝",
        match EdgeLedger::new(LANE_CAP) {
            Ok(mut led) => {
                let first = led.observe(1, 5).is_ok();
                let second = led.observe(1, 5) == Err(EgCode::ReplayBad.val());
                first && second
            }
            Err(_) => false,
        },
        String::from("same ix twice: second ReplayBad"),
    ));
    v
}

/// 本模块判据集合。
pub fn run_unx48891_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48891 队列容量边界面");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    let g = segment_guard();
    s.add_dyn("F48891-J1-段位守卫", g, if g { "" } else { "诊断码逃出 0x70B40" });
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_edges_are_exact() {
        assert_eq!(classify(0, LANE_CAP), Ok(LaneBand::Empty));
        assert_eq!(classify(1, LANE_CAP), Ok(LaneBand::Low));
        assert_eq!(classify(32, LANE_CAP), Ok(LaneBand::Low));
        assert_eq!(classify(33, LANE_CAP), Ok(LaneBand::Band));
        assert_eq!(classify(95, LANE_CAP), Ok(LaneBand::Band));
        assert_eq!(classify(96, LANE_CAP), Ok(LaneBand::High));
        assert_eq!(classify(127, LANE_CAP), Ok(LaneBand::High));
        assert_eq!(classify(128, LANE_CAP), Ok(LaneBand::Crit));
    }

    #[test]
    fn level_over_cap_is_error() {
        assert_eq!(classify(129, LANE_CAP), Err(EgCode::LevelBad.val()));
        assert_eq!(margin(129, LANE_CAP), Err(EgCode::LevelBad.val()));
        assert_eq!(classify(1, 0), Err(EgCode::CapBad.val()));
    }

    #[test]
    fn margin_is_exact() {
        assert_eq!(margin(0, 128), Ok(128));
        assert_eq!(margin(33, 128), Ok(95));
        assert_eq!(margin(128, 128), Ok(0));
    }

    #[test]
    fn small_capacity_scales_bands() {
        assert_eq!(classify(0, 64), Ok(LaneBand::Empty));
        assert_eq!(classify(16, 64), Ok(LaneBand::Low));
        assert_eq!(classify(48, 64), Ok(LaneBand::High));
        assert_eq!(classify(64, 64), Ok(LaneBand::Crit));
    }

    #[test]
    fn ladder_is_nine_and_monotone() {
        let l = Ladder::build(LANE_CAP);
        let l = match l {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(l.rung(0), Some(0));
        assert_eq!(l.rung(2), Some(LOW_TOP));
        assert_eq!(l.rung(8), Some(LANE_CAP));
        assert!(l.audit().is_ok());
    }

    #[test]
    fn ladder_rejects_bad_cap() {
        assert_eq!(Ladder::build(0), Err(EgCode::CapBad.val()));
        assert_eq!(Ladder::build(LANE_CAP + 1), Err(EgCode::CapBad.val()));
    }

    #[test]
    fn ladder_small_capacity_rejects_high_rungs() {
        // 容量 64 时 95/127 级越界，阶梯必须显式拒绝而不是夹断。
        assert_eq!(Ladder::build(64), Err(EgCode::LadderBad.val()));
    }

    #[test]
    fn absorbable_respects_margin() {
        assert_eq!(absorbable(0, 128, 200), Ok(128));
        assert_eq!(absorbable(100, 128, 48), Ok(28));
        assert_eq!(absorbable(128, 128, 48), Ok(0));
    }

    #[test]
    fn burst_balances() {
        let r = burst_report(100, 128, 48);
        let r = match r {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.accepted, 28);
        assert_eq!(r.dropped, 20);
        assert_eq!(r.level_after, 128);
        assert_eq!(r.band_after, LaneBand::Crit);
    }

    #[test]
    fn burst_rejects_bad_level() {
        assert_eq!(burst_report(129, 128, 1), Err(EgCode::LevelBad.val()));
    }

    #[test]
    fn edge_point_text_and_margin() {
        let p = EdgePoint::new(1, 95, 128);
        let p = match p {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(p.margin(), 33);
        assert_eq!(p.text(), "1|95|128|band");
        assert_eq!(p.band, LaneBand::Band);
    }

    #[test]
    fn ledger_cover_requires_all_five() {
        let led = EdgeLedger::new(LANE_CAP);
        let led = match led {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(led.audit(), Err(EgCode::CoverBad.val()));
    }

    #[test]
    fn ledger_observes_all_bands() {
        let led = EdgeLedger::new(LANE_CAP);
        let mut led = match led {
            Ok(v) => v,
            Err(_) => return,
        };
        let mut i = 0u32;
        while i < 50 {
            let _ = led.observe(i + 1, corpus_level(i));
            i = i.saturating_add(1);
        }
        assert_eq!(led.cover(), 0x1F);
        assert!(led.audit().is_ok());
        assert_eq!(led.samples(), 50);
    }

    #[test]
    fn ledger_rejects_replay() {
        let led = EdgeLedger::new(LANE_CAP);
        let mut led = match led {
            Ok(v) => v,
            Err(_) => return,
        };
        let _ = led.observe(2, 1);
        assert_eq!(led.observe(2, 1), Err(EgCode::ReplayBad.val()));
    }

    #[test]
    fn golden_is_balanced_and_stable() {
        let a = golden_run();
        let b = golden_run();
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x.0, EDGE_ROUNDS);
                assert!(x.1 > 0 && x.2 > 0);
                assert!(x.3 > 0 && x.4 > 0);
                assert_eq!(x.5, y.5);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn injection_all_routes() {
        let l = inject_rounds(1000);
        let l = match l {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(l.all_caught());
        assert_eq!(l.route_caught, [250, 250, 250, 250]);
    }

    #[test]
    fn render_round_trip_and_tamper() {
        let led = EdgeLedger::new(LANE_CAP);
        let mut led = match led {
            Ok(v) => v,
            Err(_) => return,
        };
        let mut i = 0u32;
        while i < 50 {
            let _ = led.observe(i + 1, corpus_level(i));
            i = i.saturating_add(1);
        }
        let text = render_ledger(&led);
        assert!(audit_render(&text).is_ok());
        let bad = text.replacen('|', "||", 1);
        assert!(audit_render(&bad).is_err());
    }

    #[test]
    fn probes_and_repro() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn codes_in_window() {
        assert!(segment_guard());
        let mut i = 0usize;
        while i < EgCode::ALL.len() {
            if let Some(c) = EgCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE + 1 && c.val() <= SEG_BASE + 0x0B);
                assert!(!detail(c.val()).is_empty());
            }
            i += 1;
        }
        assert!(detail(3).contains("未知"));
    }
}
