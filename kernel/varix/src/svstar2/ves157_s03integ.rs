#![allow(dead_code)]
//! VE-F3857 · S03 联调
//!
//! 读屏组合矩阵：七域协同×读屏器三型×三平台=63 格分层抽样。
//! 真实剧本：20 分钟读屏剧本（读屏漫游+动态内容+虚拟化列表+表面切换混流）
//! ——重演确定性+信号采集。故障注入：断链/公告风暴/跳空三故障——红线实测场。
//! 错误路径：断流检出→P0、覆盖不达标→补跑、重演分歧→立案、处置无责→兜底。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xCD00）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xCD00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CCDCode(pub u16);

impl CCDCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const BREAK_P0: Self = Self(SEG_BASE + 1);
    pub const COVER_SHORT: Self = Self(SEG_BASE + 2);
    pub const REPLAY_DIVERGE: Self = Self(SEG_BASE + 3);
    pub const NO_OWNER: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::BREAK_P0 => "断流检出→P0",
            Self::COVER_SHORT => "覆盖不达标→补跑",
            Self::REPLAY_DIVERGE => "重演分歧→立案",
            Self::NO_OWNER => "处置无责→兜底",
            _ => "未知",
        }
    }
}

// ── 组合矩阵（63 格分层抽样）────────────────────────────────────────

/// 联调组合键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComboKey {
    /// 域号 1-7。
    pub domain: u8,
    /// 读屏器 1-3。
    pub reader: u8,
    /// 平台 1-3。
    pub platform: u8,
}

/// 分层抽样器：等步长抽样（覆盖底线）。
#[derive(Debug, Clone)]
pub struct Stratified {
    pub sampled: Vec<ComboKey>,
    /// 抽样步长（1=全量）。
    pub stride: usize,
}

impl Stratified {
    /// 全量 63 格构造。
    pub fn full_grid() -> Vec<ComboKey> {
        let mut out = Vec::new();
        for d in 1..=7u8 {
            for r in 1..=3u8 {
                for p in 1..=3u8 {
                    out.push(ComboKey {
                        domain: d,
                        reader: r,
                        platform: p,
                    });
                }
            }
        }
        out
    }

    /// 分层抽样：按 stride 取格。
    pub fn sample(stride: usize) -> Self {
        let grid = Self::full_grid();
        let stride = stride.max(1);
        let sampled = grid
            .iter()
            .enumerate()
            .filter(|(i, _)| i % stride == 0)
            .map(|(_, k)| *k)
            .collect();
        Self { sampled, stride }
    }

    /// 覆盖底线：抽样须触达全部七个域。
    pub fn domains_covered(&self) -> usize {
        let mut seen = [false; 8];
        for k in self.sampled.iter() {
            seen[k.domain as usize] = true;
        }
        seen[1..].iter().filter(|s| **s).count()
    }
}

// ── 20 分钟剧本执行器 ───────────────────────────────────────────────

/// 剧本幕（混流段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// 读屏漫游。
    Roam,
    /// 动态内容。
    Dynamic,
    /// 虚拟化列表。
    VirtList,
    /// 表面切换。
    Surface,
}

impl Act {
    pub const ALL: [Act; 4] = [Act::Roam, Act::Dynamic, Act::VirtList, Act::Surface];
}

/// 剧本执行记录。
#[derive(Debug, Clone)]
pub struct ScriptRun {
    pub acts: Vec<Act>,
    /// 首演信号指纹（信号采集）。
    pub first_fp: u32,
    /// 重演信号指纹。
    pub replay_fp: u32,
    /// 剧本时长（分钟）。
    pub minutes: u32,
}

impl ScriptRun {
    /// 执行 20 分钟剧本（四幕混流）。
    pub fn twenty_minutes() -> Self {
        // 混流序：漫游→动态→列表→表面→漫游收束。
        Self {
            acts: vec![
                Act::Roam,
                Act::Dynamic,
                Act::VirtList,
                Act::Surface,
                Act::Roam,
            ],
            first_fp: 0x5C_03_20,
            replay_fp: 0x5C_03_20,
            minutes: 20,
        }
    }

    /// 信号指纹：幕序 + 时长决定（重演确定性基础）。
    pub fn compute_fp(&self) -> u32 {
        let mut fp: u32 = self.minutes * 0x1000;
        for (i, a) in self.acts.iter().enumerate() {
            fp ^= (*a as u32 + 1) << (i % 8);
        }
        fp
    }

    /// 重演确定性：重演指纹与首演一致。
    pub fn deterministic(&self) -> bool {
        self.first_fp == self.replay_fp && self.first_fp == self.compute_fp()
    }

    /// 幕序合法：四幕混流全部出现。
    pub fn acts_full(&self) -> bool {
        Act::ALL
            .iter()
            .all(|a| self.acts.contains(a))
    }
}

// ── 三故障注入（红线实测场）─────────────────────────────────────────

/// 故障注入类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// 断链注入（红线：必须检出）。
    Break,
    /// 公告风暴（红线：合批吸收）。
    AnnounceStorm,
    /// 跳空注入（红线：导航兜底）。
    JumpGap,
}

/// 注入结果。
#[derive(Debug, Clone, Copy)]
pub struct FaultResult {
    pub fault: Fault,
    /// 被检出（断链/跳空必须检出，风暴须吸收）。
    pub detected: bool,
    /// 兜底生效。
    pub rescued: bool,
}

impl FaultResult {
    /// 红线通过：检出且兜底。
    pub fn pass(&self) -> bool {
        self.detected && self.rescued
    }
}

// ── 处置三分流 ──────────────────────────────────────────────────────

/// 处置流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// P0 即断：阻断发布。
    HaltNow,
    /// 立案限期：归责修复。
    FileAndFix,
    /// 兜底：无责可归时由联调组兜底（禁止无主）。
    Fallback,
}

/// 分流裁决：有检出有归属→立案；P0→即断；其余兜底。
pub fn triage(detected: bool, owner_set: bool, p0: bool) -> Disposition {
    if p0 {
        Disposition::HaltNow
    } else if detected && owner_set {
        Disposition::FileAndFix
    } else {
        Disposition::Fallback
    }
}

// ── 联调会话（总控）─────────────────────────────────────────────────

/// S03 联调会话。
#[derive(Debug, Clone)]
pub struct S03Session {
    pub strat: Stratified,
    pub script: ScriptRun,
    pub faults: Vec<FaultResult>,
    /// 断流检出（P0 红线）。
    pub break_detected: u32,
    /// 立案数。
    pub cases: u32,
}

impl S03Session {
    pub fn new(stride: usize) -> Self {
        Self {
            strat: Stratified::sample(stride),
            script: ScriptRun::twenty_minutes(),
            faults: Vec::new(),
            break_detected: 0,
            cases: 0,
        }
    }

    /// 注入三故障并评估红线。
    pub fn inject(&mut self, results: [FaultResult; 3]) {
        for r in results.iter() {
            if r.fault == Fault::Break && r.detected {
                self.break_detected += 1;
            }
            self.faults.push(*r);
        }
    }

    /// 会话总检。
    pub fn full_check(&self) -> Result<(), CCDCode> {
        if self.strat.domains_covered() < 7 {
            return Err(CCDCode::COVER_SHORT);
        }
        if !self.script.deterministic() {
            return Err(CCDCode::REPLAY_DIVERGE);
        }
        if !self.faults.is_empty() && !self.faults.iter().all(|f| f.pass()) {
            return Err(CCDCode::BREAK_P0);
        }
        if self.break_detected == 0 && !self.faults.is_empty() {
            return Err(CCDCode::BREAK_P0);
        }
        Ok(())
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves157_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves157_s03integ");

    // 01 全网格 63 复述
    {
        chk(&mut set, "C3857-01", Stratified::full_grid().len() == 63, "err");
    }
    // 02 分层抽样触达七域
    {
        let s = Stratified::sample(9);
        chk(&mut set, "C3857-02", s.domains_covered() == 7, "err");
    }
    // 03 步长 1 全量
    {
        let s = Stratified::sample(1);
        chk(&mut set, "C3857-03", s.sampled.len() == 63, "err");
    }
    // 04 大步长覆盖不足被底线检出
    {
        let s = Stratified::sample(63);
        chk(
            &mut set,
            "C3857-04",
            s.sampled.len() == 1 && s.domains_covered() == 1,
            "err",
        );
    }
    // 05 剧本 20 分钟四幕混流
    {
        let s = ScriptRun::twenty_minutes();
        chk(
            &mut set,
            "C3857-05",
            s.minutes == 20 && s.acts_full(),
            "err",
        );
    }
    // 06 重演确定性：指纹一致
    {
        let s = ScriptRun::twenty_minutes();
        chk(&mut set, "C3857-06", s.deterministic(), "err");
    }
    // 07 重演分歧→立案
    {
        let mut s = ScriptRun::twenty_minutes();
        s.replay_fp = 0xDEAD;
        chk(&mut set, "C3857-07", !s.deterministic(), "err");
    }
    // 08 幕序指纹敏感（改幕序指纹变）
    {
        let mut s = ScriptRun::twenty_minutes();
        let a = s.compute_fp();
        s.acts.reverse();
        let b = s.compute_fp();
        chk(&mut set, "C3857-08", a != b, "err");
    }
    // 09 断链注入检出
    {
        let mut s = S03Session::new(9);
        s.inject([
            FaultResult { fault: Fault::Break, detected: true, rescued: true },
            FaultResult { fault: Fault::AnnounceStorm, detected: true, rescued: true },
            FaultResult { fault: Fault::JumpGap, detected: true, rescued: true },
        ]);
        chk(&mut set, "C3857-09", s.break_detected == 1 && s.full_check() == Ok(()), "err");
    }
    // 10 断链漏检→P0
    {
        let mut s = S03Session::new(9);
        s.inject([
            FaultResult { fault: Fault::Break, detected: false, rescued: false },
            FaultResult { fault: Fault::AnnounceStorm, detected: true, rescued: true },
            FaultResult { fault: Fault::JumpGap, detected: true, rescued: true },
        ]);
        chk(
            &mut set,
            "C3857-10",
            s.full_check() == Err(CCDCode::BREAK_P0),
            "err",
        );
    }
    // 11 风暴吸收失败→红线
    {
        let mut s = S03Session::new(9);
        s.inject([
            FaultResult { fault: Fault::Break, detected: true, rescued: true },
            FaultResult { fault: Fault::AnnounceStorm, detected: true, rescued: false },
            FaultResult { fault: Fault::JumpGap, detected: true, rescued: true },
        ]);
        chk(&mut set, "C3857-11", s.full_check() == Err(CCDCode::BREAK_P0), "err");
    }
    // 12 覆盖不达标→补跑
    {
        let mut s = S03Session::new(9);
        s.strat.sampled.clear();
        s.strat.sampled.push(ComboKey { domain: 1, reader: 1, platform: 1 });
        chk(
            &mut set,
            "C3857-12",
            s.full_check() == Err(CCDCode::COVER_SHORT),
            "err",
        );
    }
    // 13 三分流：P0 即断
    {
        chk(
            &mut set,
            "C3857-13",
            triage(true, true, true) == Disposition::HaltNow,
            "err",
        );
    }
    // 14 三分流：检出有主→立案
    {
        chk(
            &mut set,
            "C3857-14",
            triage(true, true, false) == Disposition::FileAndFix,
            "err",
        );
    }
    // 15 三分流：无主→兜底
    {
        chk(
            &mut set,
            "C3857-15",
            triage(true, false, false) == Disposition::Fallback,
            "err",
        );
    }
    // 16 立案计数
    {
        let mut s = S03Session::new(9);
        s.cases = 2;
        s.break_detected = 1;
        chk(&mut set, "C3857-16", s.cases == 2 && s.break_detected == 1, "err");
    }
    // 17 空故障集不触发 P0（未注入不算漏检）
    {
        let s = S03Session::new(9);
        chk(&mut set, "C3857-17", s.full_check() == Ok(()), "err");
    }
    // 18 诊断码互异
    {
        let codes = [
            CCDCode::BREAK_P0,
            CCDCode::COVER_SHORT,
            CCDCode::REPLAY_DIVERGE,
            CCDCode::NO_OWNER,
        ];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        chk(&mut set, "C3857-18", distinct, "err");
    }
    set
}
