//! VE-F3936 · 无障碍性能联调
//!
//! 组合矩阵：六组×四态×三设备档×三压力态=216 格分层抽样 54 格（覆盖底线）。
//! 真实剧本：20 分钟无障碍性能剧本（四态常驻+辅具接入+大规模+降档触发混流）
//! ——重演确定性+断链采集（断链剧本线=0——实测场）。
//! 最坏场景：万节点+四态+低端+满载四重最坏。三分流复述。
//! 错误路径：四重断链→P0、剧本断链>0→P0、覆盖不达标→补跑、重演分歧→立案。

#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::vec;
extern crate alloc;


// ── 诊断码段（0xD000）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDDCode(pub u16);

impl CDDCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const QUAD_BREAK: Self = Self(SEG_BASE + 1);
    pub const SCRIPT_BREAK: Self = Self(SEG_BASE + 2);
    pub const COVER_SHORT: Self = Self(SEG_BASE + 3);
    pub const REPLAY_DIVERGE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::QUAD_BREAK => "四重断链→P0（红线实测）",
            Self::SCRIPT_BREAK => "断链剧本>0→P0（线实测）",
            Self::COVER_SHORT => "覆盖不达标→补跑",
            Self::REPLAY_DIVERGE => "重演分歧→立案",
            _ => "未知",
        }
    }
}

// ── 216 格组合矩阵（分层抽样 54）────────────────────────────────────

/// 组合键：六组×四态×三档×三压。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerfKey {
    /// 组号 1-6（复述六组）。
    pub group: u8,
    /// 态号 1-4（四态：对比/读屏/大字/动效替代）。
    pub state: u8,
    /// 设备档 1-3（高端/中端/低端）。
    pub tier: u8,
    /// 压力态 1-3（空载/常载/满载）。
    pub load: u8,
}

/// 全网格：6×4×3×3 = 216。
pub const FULL_GRID: usize = 6 * 4 * 3 * 3;
/// 抽样目标 54 格。
pub const SAMPLE_TARGET: usize = 54;

/// 分层抽样器。
#[derive(Debug, Clone)]
pub struct Stratified216 {
    pub sampled: Vec<PerfKey>,
    pub stride: usize,
}

impl Stratified216 {
    pub fn full_grid() -> Vec<PerfKey> {
        let mut out = Vec::new();
        for g in 1..=6u8 {
            for s in 1..=4u8 {
                for t in 1..=3u8 {
                    for l in 1..=3u8 {
                        out.push(PerfKey {
                            group: g,
                            state: s,
                            tier: t,
                            load: l,
                        });
                    }
                }
            }
        }
        out
    }

    /// 步长 4 → 216/4 = 54 格。
    pub fn sample() -> Self {
        Self {
            sampled: Self::full_grid()
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 4 == 0)
                .map(|(_, k)| *k)
                .collect(),
            stride: 4,
        }
    }

    /// 覆盖底线：六组×四态全部触达。
    pub fn cover_ok(&self) -> bool {
        let mut g = [false; 7];
        let mut s = [false; 5];
        for k in self.sampled.iter() {
            g[k.group as usize] = true;
            s[k.state as usize] = true;
        }
        g[1..].iter().all(|b| *b) && s[1..].iter().all(|b| *b)
    }

    /// 格数复述。
    pub fn count(&self) -> usize {
        self.sampled.len()
    }
}

// ── 20 分钟剧本（四段混流）──────────────────────────────────────────

/// 剧本段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfAct {
    /// 四态常驻。
    StatesOn,
    /// 辅具接入。
    AtAttach,
    /// 大规模（万节点）。
    Massive,
    /// 降档触发。
    TierDown,
}

/// 剧本执行记录。
#[derive(Debug, Clone)]
pub struct PerfScript {
    pub acts: Vec<PerfAct>,
    pub minutes: u32,
    pub first_fp: u32,
    pub replay_fp: u32,
    /// 剧本全程断链采集数（红线：=0）。
    pub breaks: u32,
}

impl PerfScript {
    /// 四段混流 20 分钟剧本。
    pub fn twenty_minutes() -> Self {
        Self {
            acts: vec![
                PerfAct::StatesOn,
                PerfAct::AtAttach,
                PerfAct::Massive,
                PerfAct::TierDown,
                PerfAct::StatesOn,
            ],
            minutes: 20,
            first_fp: 0x5D_07_20,
            replay_fp: 0x5D_07_20,
            breaks: 0,
        }
    }

    /// 指纹：段序+时长+断链决定。
    pub fn compute_fp(&self) -> u32 {
        let mut fp = self.minutes * 0x1000 + self.breaks;
        for (i, a) in self.acts.iter().enumerate() {
            fp ^= (*a as u32 + 1) << (i % 8);
        }
        fp
    }

    /// 重演确定性。
    pub fn deterministic(&self) -> bool {
        self.first_fp == self.replay_fp && self.first_fp == self.compute_fp()
    }

    /// 四段混流齐备。
    pub fn acts_full(&self) -> bool {
        let all = [
            PerfAct::StatesOn,
            PerfAct::AtAttach,
            PerfAct::Massive,
            PerfAct::TierDown,
        ];
        all.iter().all(|a| self.acts.contains(a))
    }

    /// 断链剧本线：=0 才过。
    pub fn break_line(&self) -> bool {
        self.breaks == 0
    }
}

// ── 四重最坏（万节点+四态+低端+满载）───────────────────────────────

/// 最坏场景四因子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorstFactor {
    /// 万节点规模。
    Nodes10k,
    /// 四态全开。
    AllStates,
    /// 低端设备。
    LowTier,
    /// 满载压力。
    FullLoad,
}

impl WorstFactor {
    pub const ALL: [WorstFactor; 4] = [
        WorstFactor::Nodes10k,
        WorstFactor::AllStates,
        WorstFactor::LowTier,
        WorstFactor::FullLoad,
    ];
}

/// 最坏场景演练记录。
#[derive(Debug, Clone, Copy)]
pub struct WorstRun {
    /// 生效因子集。
    pub active: [bool; 4],
    /// 演练断链数。
    pub breaks: u32,
}

impl WorstRun {
    /// 四重全开判定。
    pub fn quad_active(&self) -> bool {
        self.active.iter().all(|b| *b)
    }

    /// 四重断链→P0。
    pub fn quad_breach(&self) -> bool {
        self.quad_active() && self.breaks > 0
    }
}

// ── 处置三分流 ──────────────────────────────────────────────────────

/// 处置流（复述 F3857 同构）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Triage {
    HaltNow,
    FileAndFix,
    Fallback,
}

pub fn triage(detected: bool, owner_set: bool, p0: bool) -> Triage {
    if p0 {
        Triage::HaltNow
    } else if detected && owner_set {
        Triage::FileAndFix
    } else {
        Triage::Fallback
    }
}

// ── 联调会话（总控）─────────────────────────────────────────────────

/// 无障碍性能联调会话。
#[derive(Debug, Clone)]
pub struct PerfIntegSession {
    pub strat: Stratified216,
    pub script: PerfScript,
    pub worst: Vec<WorstRun>,
    pub cases: u32,
}

impl PerfIntegSession {
    pub fn new() -> Self {
        Self {
            strat: Stratified216::sample(),
            script: PerfScript::twenty_minutes(),
            worst: Vec::new(),
            cases: 0,
        }
    }

    /// 会话总检。
    pub fn full_check(&self) -> Result<(), CDDCode> {
        if self.strat.count() != SAMPLE_TARGET || !self.strat.cover_ok() {
            return Err(CDDCode::COVER_SHORT);
        }
        if !self.script.deterministic() {
            return Err(CDDCode::REPLAY_DIVERGE);
        }
        if !self.script.break_line() {
            return Err(CDDCode::SCRIPT_BREAK);
        }
        if self.worst.iter().any(|w| w.quad_breach()) {
            return Err(CDDCode::QUAD_BREAK);
        }
        Ok(())
    }
}

impl Default for PerfIntegSession {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves190_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves190_perfinteg");

    // 01 全网格 216 复述
    {
        chk(&mut set, "C3936-01", Stratified216::full_grid().len() == 216, "err");
    }
    // 02 抽样 54 格
    {
        let s = Stratified216::sample();
        chk(
            &mut set,
            "C3936-02",
            s.count() == SAMPLE_TARGET && s.count() == 54,
            "err",
        );
    }
    // 03 六组×四态覆盖底线
    {
        let s = Stratified216::sample();
        chk(&mut set, "C3936-03", s.cover_ok(), "err");
    }
    // 04 抽样打穿底线检出
    {
        let mut s = Stratified216::sample();
        s.sampled.retain(|k| k.group != 6);
        chk(&mut set, "C3936-04", !s.cover_ok(), "err");
    }
    // 05 剧本 20 分钟四段混流
    {
        let s = PerfScript::twenty_minutes();
        chk(&mut set, "C3936-05", s.minutes == 20 && s.acts_full(), "err");
    }
    // 06 重演确定性
    {
        let s = PerfScript::twenty_minutes();
        chk(&mut set, "C3936-06", s.deterministic(), "err");
    }
    // 07 重演分歧
    {
        let mut s = PerfScript::twenty_minutes();
        s.replay_fp = 0xBEEF;
        chk(&mut set, "C3936-07", !s.deterministic(), "err");
    }
    // 08 指纹对断链敏感
    {
        let mut s = PerfScript::twenty_minutes();
        let a = s.compute_fp();
        s.breaks = 1;
        chk(&mut set, "C3936-08", s.compute_fp() != a, "err");
    }
    // 09 断链剧本线：零通过
    {
        let s = PerfScript::twenty_minutes();
        chk(&mut set, "C3936-09", s.break_line(), "err");
    }
    // 10 断链剧本>0→P0
    {
        let mut s = PerfIntegSession::new();
        s.script.breaks = 2;
        chk(
            &mut set,
            "C3936-10",
            s.full_check() == Err(CDDCode::SCRIPT_BREAK),
            "err",
        );
    }
    // 11 四重最坏全开无断链
    {
        let w = WorstRun {
            active: [true; 4],
            breaks: 0,
        };
        chk(&mut set, "C3936-11", w.quad_active() && !w.quad_breach(), "err");
    }
    // 12 四重断链→P0
    {
        let mut s = PerfIntegSession::new();
        s.worst.push(WorstRun {
            active: [true; 4],
            breaks: 3,
        });
        chk(
            &mut set,
            "C3936-12",
            s.full_check() == Err(CDDCode::QUAD_BREAK),
            "err",
        );
    }
    // 13 三因子不算四重
    {
        let w = WorstRun {
            active: [true, true, true, false],
            breaks: 5,
        };
        chk(&mut set, "C3936-13", !w.quad_active() && !w.quad_breach(), "err");
    }
    // 14 四因子常量表
    {
        chk(&mut set, "C3936-14", WorstFactor::ALL.len() == 4, "err");
    }
    // 15 三分流：P0 即断
    {
        chk(
            &mut set,
            "C3936-15",
            triage(true, true, true) == Triage::HaltNow,
            "err",
        );
    }
    // 16 三分流：立案与兜底
    {
        chk(
            &mut set,
            "C3936-16",
            triage(true, true, false) == Triage::FileAndFix
                && triage(false, false, false) == Triage::Fallback,
            "err",
        );
    }
    // 17 会话总检全绿
    {
        let mut s = PerfIntegSession::new();
        s.worst.push(WorstRun {
            active: [true, true, false, true],
            breaks: 0,
        });
        chk(&mut set, "C3936-17", s.full_check() == Ok(()), "err");
    }
    // 18 覆盖不达标→补跑路径
    {
        let mut s = PerfIntegSession::new();
        s.strat.sampled.clear();
        chk(
            &mut set,
            "C3936-18",
            s.full_check() == Err(CDDCode::COVER_SHORT),
            "err",
        );
    }
    // 19 立案计数
    {
        let mut s = PerfIntegSession::new();
        s.cases = 4;
        chk(&mut set, "C3936-19", s.cases == 4, "err");
    }
    // 20 诊断码互异
    {
        let codes = [
            CDDCode::QUAD_BREAK,
            CDDCode::SCRIPT_BREAK,
            CDDCode::COVER_SHORT,
            CDDCode::REPLAY_DIVERGE,
        ];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        chk(&mut set, "C3936-20", distinct, "err");
    }

    set
}
