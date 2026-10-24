//! VE-F3816 · 无障碍渲染与 S 域真源终版（VE-T 域 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3816`
//!
//! **判据（锚点原文）**：真源单源、分叉红线、判定一致、双签协同、对拍制度、判据。
//!
//! **本条是什么**：无障碍状态真源终版——S01 态采集 = 唯一采集点
//! （双点采集 = 态分叉红线）；N05 判定真源 ↔ 渲染态一致断言；
//! 真源→渲染全链对拍；分叉检出归一。
//!
//! 诊断码段：独占 0xC9xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xC9xx）
// ─────────────────────────────────────────────────────────────

/// F3816 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C9Code(pub u16);

impl C9Code {
    /// 态分叉——归一（红线）。
    pub const FORK: C9Code = C9Code(0xC901);
    /// 判定渲染不一致——立案。
    pub const JUDGE_MISMATCH: C9Code = C9Code(0xC902);
    /// 真源跳变——状态机修正。
    pub const JUMP: C9Code = C9Code(0xC903);
    /// 对拍分歧——仲裁。
    pub const DIFF: C9Code = C9Code(0xC904);
    /// 采集点非单源。
    pub const MULTI_SOURCE: C9Code = C9Code(0xC905);
    /// 协同双签缺。
    pub const SIGN_MISS: C9Code = C9Code(0xC906);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xC901 => "无障碍态分叉（双点采集），红线归一。",
            0xC902 => "判定与渲染不一致，需立案。",
            0xC903 => "真源跳变，状态机修正。",
            0xC904 => "对拍分歧，需仲裁。",
            0xC905 => "采集点非单源，态分叉风险。",
            0xC906 => "协同双签缺失。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 真源单源 + 分叉检测
// ─────────────────────────────────────────────────────────────

/// 采集点。
#[derive(Clone, Copy, Debug)]
pub struct Collector {
    /// 点 id。
    pub id: u8,
    /// 采集到的态（位掩码：bit0=辅具 bit1=reduce bit2=光敏）。
    pub state: u8,
}

impl Collector {
    /// 构造。
    pub const fn new(id: u8, state: u8) -> Self {
        Collector { id, state }
    }
}

/// 真源登记（单源）。
#[derive(Clone, Debug)]
pub struct TruthSource {
    collectors: Vec<Collector>,
    /// 唯一采集点 id（0xFF=未定）。
    pub sole: u8,
}

impl TruthSource {
    /// 新建。
    pub fn new() -> Self {
        TruthSource {
            collectors: Vec::new(),
            sole: 0xFF,
        }
    }

    /// 登记采集点。
    pub fn enroll(&mut self, c: Collector) {
        self.collectors.push(c);
    }

    /// 设唯一采集点。
    pub fn set_sole(&mut self, id: u8) {
        self.sole = id;
    }

    /// 采集点数。
    pub fn count(&self) -> usize {
        self.collectors.len()
    }

    /// 单源成立（恰好 1 点）。
    pub fn single_source(&self) -> bool {
        self.count() == 1
    }

    /// 分叉检测（多点且态不一致）。
    pub fn fork(&self) -> bool {
        if self.count() <= 1 {
            return false;
        }
        let base = self.collectors[0].state;
        self.collectors.iter().any(|c| c.state != base)
    }

    /// 归一（取首点为权威，清其余）。
    pub fn unify(&mut self) -> bool {
        if self.count() <= 1 {
            return false;
        }
        let base = self.collectors[0].state;
        self.collectors.truncate(1);
        self.sole = 0;
        self.collectors[0].state = base;
        true
    }
}

impl Default for TruthSource {
    fn default() -> Self {
        TruthSource::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判定-渲染一致 + 对拍
// ─────────────────────────────────────────────────────────────

/// 判定-渲染对。
#[derive(Clone, Copy, Debug)]
pub struct JudgeRender {
    /// N05 判定态。
    pub judged: u8,
    /// 渲染态。
    pub rendered: u8,
}

impl JudgeRender {
    /// 构造。
    pub const fn new(judged: u8, rendered: u8) -> Self {
        JudgeRender { judged, rendered }
    }

    /// 一致。
    pub fn consistent(self) -> bool {
        self.judged == self.rendered
    }
}

/// 对拍协议。
#[derive(Clone, Debug)]
pub struct CompareProto {
    /// 对拍轮数。
    pub rounds: u32,
    /// 分歧数。
    pub diffs: u32,
    /// 仲裁完成数。
    pub arbitrated: u32,
}

impl CompareProto {
    /// 新建。
    pub const fn new() -> Self {
        CompareProto {
            rounds: 0,
            diffs: 0,
            arbitrated: 0,
        }
    }

    /// 跑一轮。
    pub fn round(&mut self, pair: JudgeRender) -> bool {
        self.rounds += 1;
        if pair.consistent() {
            true
        } else {
            self.diffs += 1;
            false
        }
    }

    /// 仲裁一条。
    pub fn arbitrate(&mut self) -> bool {
        if self.arbitrated < self.diffs {
            self.arbitrated += 1;
            true
        } else {
            false
        }
    }

    /// 未仲裁数。
    pub fn open_diffs(&self) -> u32 {
        self.diffs.saturating_sub(self.arbitrated)
    }
}

// ─────────────────────────────────────────────────────────────
// 协同双签
// ─────────────────────────────────────────────────────────────

/// 协同双签。
#[derive(Clone, Copy, Debug)]
pub struct CoSign {
    /// S 域签。
    pub s_signed: bool,
    /// N 域签。
    pub n_signed: bool,
}

impl CoSign {
    /// 新建。
    pub const fn new() -> Self {
        CoSign {
            s_signed: false,
            n_signed: false,
        }
    }

    /// 双签齐。
    pub fn complete(self) -> bool {
        self.s_signed && self.n_signed
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3816 判据集。
pub fn run_f3816_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3816");

    // 0xC905/0xC901 单源与分叉
    {
        let mut t = TruthSource::new();
        set.add("0xC905-空源不单", !t.single_source(), "");
        t.enroll(Collector::new(0, 0b001));
        set.add("0xC905-单点单源", t.single_source(), "");
        t.enroll(Collector::new(1, 0b001));
        set.add("0xC905-双点非单", !t.single_source(), "");
        set.add("0xC901-同态无叉", !t.fork(), "");
        t.enroll(Collector::new(2, 0b100));
        set.add("0xC901-异态分叉", t.fork(), "");
    }

    // 归一
    {
        let mut t = TruthSource::new();
        t.enroll(Collector::new(0, 0b001));
        t.enroll(Collector::new(1, 0b010));
        set.add("0xC901-归一进", t.unify(), "");
        set.add("0xC901-归一后单", t.single_source() && !t.fork(), "");
        set.add("0xC901-单点归一拒", !t.unify(), "");
    }

    // 0xC902 判定一致
    {
        let p = JudgeRender::new(0b011, 0b011);
        set.add("0xC902-一致过", p.consistent(), "");
        let p2 = JudgeRender::new(0b011, 0b001);
        set.add("0xC902-不一致检", !p2.consistent(), "");
    }

    // 0xC904 对拍仲裁
    {
        let mut c = CompareProto::new();
        set.add("0xC904-空对拍", c.rounds == 0 && c.open_diffs() == 0, "");
        c.round(JudgeRender::new(1, 1));
        set.add("0xC904-一致轮", c.diffs == 0, "");
        c.round(JudgeRender::new(1, 2));
        set.add("0xC904-分歧计", c.diffs == 1 && c.open_diffs() == 1, "");
        c.arbitrate();
        set.add("0xC904-仲裁清", c.open_diffs() == 0, "");
        set.add("0xC904-空仲裁拒", !c.arbitrate(), "");
    }

    // 0xC906 双签
    {
        let mut s = CoSign::new();
        set.add("0xC906-初始未签", !s.complete(), "");
        s.s_signed = true;
        set.add("0xC906-单签不齐", !s.complete(), "");
        s.n_signed = true;
        set.add("0xC906-双签齐", s.complete(), "");
    }

    // 0xC903 跳变防护（真源变更须经归一路径）
    {
        let mut t = TruthSource::new();
        t.enroll(Collector::new(0, 1));
        set.add("0xC903-单点稳态", !t.fork() && t.single_source(), "");
    }

    set
}
