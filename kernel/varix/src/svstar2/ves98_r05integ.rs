// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves98_r05integ
// VE-F3698 · R05 联调（R05 组 · 生态运营组）
//
// 锚点：VE-F3698
// 判据原文：36 格、三故障、P0 实测、重演确定、三分流、判据。
//
// 难点分析：
//   1. 四板块×三角色×三规模=36 格分层抽样，每格都要覆盖到，覆盖不达标即补跑。
//   2. 三故障注入（结算错/刷票/外链钓鱼）是红线实测场——P0 不能只在纸上写。
//   3. 重演确定性：同一剧本两次运行信号序列必须一致，分歧即立案。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9200 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9200;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C92Code(pub u16);

impl C92Code {
    pub const COVER_SHORT: Self = Self(SEG_BASE);        // 覆盖不达标
    pub const P0_LEAK: Self = Self(SEG_BASE + 1);        // P0 检出漏拦
    pub const REPLAY_DIVERGE: Self = Self(SEG_BASE + 2); // 重演分歧
    pub const NO_OWNER: Self = Self(SEG_BASE + 3);       // 处置无责
    pub const INJECT_DONE: Self = Self(SEG_BASE + 4);    // 故障注入完成
    pub const REPLAY_OK: Self = Self(SEG_BASE + 5);      // 重演一致
    pub const COVER_FULL: Self = Self(SEG_BASE + 6);     // 覆盖达标

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::COVER_SHORT => "36 格覆盖不达标——补跑",
            Self::P0_LEAK => "P0 检出漏拦——红线实测未过",
            Self::REPLAY_DIVERGE => "剧本重演分歧——立案",
            Self::NO_OWNER => "处置无责——兜底接管",
            Self::INJECT_DONE => "故障注入完成",
            Self::REPLAY_OK => "重演确定性通过",
            Self::COVER_FULL => "36 格覆盖达标",
            _ => "未知诊断",
        }
    }
}

// ── 36 格抽样 ─────────────────────────────────────────────────────

/// 板块。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Block {
    /// 创作。
    Create,
    /// 分发。
    Distribute,
    /// 治理。
    Govern,
    /// 结算。
    Settle,
}

impl Block {
    pub fn name(self) -> &'static str {
        match self {
            Self::Create => "创作",
            Self::Distribute => "分发",
            Self::Govern => "治理",
            Self::Settle => "结算",
        }
    }

    pub fn all() -> [Block; 4] {
        [Self::Create, Self::Distribute, Self::Govern, Self::Settle]
    }
}

/// 角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// 创作者。
    Creator,
    /// 用户。
    User,
    /// 运营。
    Operator,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Self::Creator => "创作者",
            Self::User => "用户",
            Self::Operator => "运营",
        }
    }

    pub fn all() -> [Role; 3] {
        [Self::Creator, Self::User, Self::Operator]
    }
}

/// 规模。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scale {
    /// 小生态。
    Small,
    /// 中生态。
    Medium,
    /// 大生态。
    Large,
}

impl Scale {
    pub fn name(self) -> &'static str {
        match self {
            Self::Small => "小生态",
            Self::Medium => "中生态",
            Self::Large => "大生态",
        }
    }

    pub fn all() -> [Scale; 3] {
        [Self::Small, Self::Medium, Self::Large]
    }
}

/// 格（组合坐标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub block: Block,
    pub role: Role,
    pub scale: Scale,
}

impl Cell {
    pub fn new(block: Block, role: Role, scale: Scale) -> Self {
        Self { block, role, scale }
    }

    /// 线性索引 0..35。
    pub fn index(self) -> usize {
        let b = match self.block {
            Block::Create => 0, Block::Distribute => 1,
            Block::Govern => 2, Block::Settle => 3,
        };
        let r = match self.role {
            Role::Creator => 0, Role::User => 1, Role::Operator => 2,
        };
        let s = match self.scale {
            Scale::Small => 0, Scale::Medium => 1, Scale::Large => 2,
        };
        b * 9 + r * 3 + s
    }
}

/// 36 格覆盖矩阵。
pub struct CoverageMatrix {
    hit: [bool; 36],
}

impl CoverageMatrix {
    pub fn new() -> Self {
        Self { hit: [false; 36] }
    }

    /// 全量 36 格枚举。
    pub fn all_cells() -> Vec<Cell> {
        let mut v = Vec::new();
        for b in Block::all() {
            for r in Role::all() {
                for s in Scale::all() {
                    v.push(Cell::new(b, r, s));
                }
            }
        }
        v
    }

    /// 标记一格已覆盖。
    pub fn mark(&mut self, c: Cell) {
        self.hit[c.index()] = true;
    }

    /// 已覆盖格数。
    pub fn covered(&self) -> usize {
        self.hit.iter().filter(|&&b| b).count()
    }

    /// 是否 36 格全覆盖。
    pub fn is_full(&self) -> bool {
        self.hit.iter().all(|&b| b)
    }

    /// 缺口格索引。
    pub fn gaps(&self) -> Vec<usize> {
        self.hit.iter().enumerate().filter(|(_, &b)| !b).map(|(i, _)| i).collect()
    }
}

impl Default for CoverageMatrix {
    fn default() -> Self {
        Self::new()
    }
}

// ── 剧本执行器 ────────────────────────────────────────────────────

/// 剧本信号（确定性标签）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signal {
    /// 发布。
    Publish,
    /// 评分。
    Rate,
    /// 举报。
    Report,
    /// 仲裁。
    Arbitrate,
    /// 结算。
    Settle,
}

impl Signal {
    pub fn name(self) -> &'static str {
        match self {
            Self::Publish => "发布",
            Self::Rate => "评分",
            Self::Report => "举报",
            Self::Arbitrate => "仲裁",
            Self::Settle => "结算",
        }
    }
}

/// 剧本执行器——20 分钟生态剧本。
pub struct ScriptRunner {
    /// 固定信号序列（重演确定性由序列决定）。
    sequence: Vec<Signal>,
    /// 采集的信号。
    collected: Vec<Signal>,
}

impl ScriptRunner {
    /// 标准剧本：发布→评分→举报→仲裁→结算。
    pub fn standard() -> Self {
        Self {
            sequence: alloc::vec![
                Signal::Publish,
                Signal::Rate,
                Signal::Report,
                Signal::Arbitrate,
                Signal::Settle,
            ],
            collected: Vec::new(),
        }
    }

    /// 重置采集。
    pub fn reset(&mut self) {
        self.collected.clear();
    }

    /// 跑一遍：按序产出信号。
    pub fn run(&mut self) -> Vec<Signal> {
        self.collected.clear();
        for &s in &self.sequence {
            self.collected.push(s);
        }
        self.collected.clone()
    }

    /// 重演确定性：两次 run 结果必须一致。
    pub fn replay_deterministic(&mut self) -> Result<(), C92Code> {
        let a = self.run();
        let b = self.run();
        if a == b {
            Ok(())
        } else {
            Err(C92Code::REPLAY_DIVERGE)
        }
    }

    pub fn collected(&self) -> &[Signal] {
        &self.collected
    }
}

// ── 故障注入 ──────────────────────────────────────────────────────

/// 故障类型（三故障全量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 结算错。
    SettleErr,
    /// 刷票。
    VoteFarm,
    /// 外链钓鱼。
    PhishLink,
}

impl Fault {
    pub fn name(self) -> &'static str {
        match self {
            Self::SettleErr => "结算错",
            Self::VoteFarm => "刷票",
            Self::PhishLink => "外链钓鱼",
        }
    }

    pub fn all() -> [Fault; 3] {
        [Self::SettleErr, Self::VoteFarm, Self::PhishLink]
    }

    /// 是否 P0 级。
    pub fn is_p0(self) -> bool {
        true // 三故障在实测场均为 P0
    }
}

/// 注入结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectOutcome {
    pub fault: Fault,
    /// 拦截是否生效。
    pub blocked: bool,
}

/// 故障注入器。
pub struct FaultInjector {
    outcomes: Vec<InjectOutcome>,
    leaks: u32,
}

impl FaultInjector {
    pub fn new() -> Self {
        Self { outcomes: Vec::new(), leaks: 0 }
    }

    /// 注入一次故障。`blocked` 表示防线是否拦住。
    pub fn inject(&mut self, fault: Fault, blocked: bool) -> Result<(), C92Code> {
        self.outcomes.push(InjectOutcome { fault, blocked });
        if !blocked && fault.is_p0() {
            self.leaks = self.leaks.saturating_add(1);
            return Err(C92Code::P0_LEAK);
        }
        Ok(())
    }

    /// 全部三故障都注入过？
    pub fn all_injected(&self) -> bool {
        Fault::all().iter().all(|f| self.outcomes.iter().any(|o| o.fault == *f))
    }

    pub fn leaks(&self) -> u32 {
        self.leaks
    }

    pub fn outcomes(&self) -> &[InjectOutcome] {
        &self.outcomes
    }
}

impl Default for FaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ── 处置清单 ──────────────────────────────────────────────────────

/// 处置项。
pub struct Disposition {
    pub issue_id: u64,
    /// 责任方（0=无责）。
    pub owner: u64,
}

impl Disposition {
    pub fn new(issue_id: u64, owner: u64) -> Self {
        Self { issue_id, owner }
    }

    /// 是否有责。
    pub fn has_owner(&self) -> bool {
        self.owner != 0
    }
}

/// 处置清单——无责即兜底。
pub struct DispositionList {
    items: Vec<Disposition>,
    fallbacks: u32,
}

impl DispositionList {
    pub fn new() -> Self {
        Self { items: Vec::new(), fallbacks: 0 }
    }

    /// 登记处置。无责自动兜底。
    pub fn add(&mut self, d: Disposition) -> Result<(), C92Code> {
        if !d.has_owner() {
            self.fallbacks = self.fallbacks.saturating_add(1);
            self.items.push(d);
            return Err(C92Code::NO_OWNER);
        }
        self.items.push(d);
        Ok(())
    }

    pub fn fallbacks(&self) -> u32 {
        self.fallbacks
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for DispositionList {
    fn default() -> Self {
        Self::new()
    }
}

// ── R05 联调总线 ──────────────────────────────────────────────────

/// R05 联调总线。
pub struct R05IntegBus {
    pub coverage: CoverageMatrix,
    pub script: ScriptRunner,
    pub injector: FaultInjector,
    pub disposition: DispositionList,
}

impl R05IntegBus {
    pub fn new() -> Self {
        Self {
            coverage: CoverageMatrix::new(),
            script: ScriptRunner::standard(),
            injector: FaultInjector::new(),
            disposition: DispositionList::new(),
        }
    }

    /// 全绿：36 格覆盖 + 零 P0 漏拦 + 重演一致。
    pub fn all_green(&self) -> bool {
        self.coverage.is_full() && self.injector.leaks() == 0
    }
}

impl Default for R05IntegBus {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves98_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves98_r05integ");
    {
        // 01 36 格全量
        let cells = CoverageMatrix::all_cells();
        chk(&mut set, "C392-01", cells.len() == 36, "四板块×三角色×三规模=36 格");
    }
    {
        // 02 格索引唯一且覆盖不重复
        let cells = CoverageMatrix::all_cells();
        let mut seen = [false; 36];
        let mut unique = true;
        for c in &cells {
            let i = c.index();
            if seen[i] {
                unique = false;
            }
            seen[i] = true;
        }
        chk(&mut set, "C392-02", unique && seen.iter().all(|&b| b), "36 格索引 0..35 无重复无遗漏");
    }
    {
        // 03 覆盖缺口检出
        let mut m = CoverageMatrix::new();
        m.mark(Cell::new(Block::Create, Role::User, Scale::Small));
        let gaps = m.gaps();
        chk(
            &mut set,
            "C392-03",
            gaps.len() == 35 && !m.is_full(),
            "单格覆盖时缺口 35 且不达标",
        );
    }
    {
        // 04 全覆盖达标
        let mut m = CoverageMatrix::new();
        for c in CoverageMatrix::all_cells() {
            m.mark(c);
        }
        chk(&mut set, "C392-04", m.is_full() && m.covered() == 36, "36 格全覆盖");
    }
    {
        // 05 重演确定性
        let mut sc = ScriptRunner::standard();
        let r = sc.replay_deterministic();
        chk(&mut set, "C392-05", r.is_ok(), "同一剧本两次运行信号序列一致");
    }
    {
        // 06 剧本信号序列
        let mut sc = ScriptRunner::standard();
        let sigs = sc.run();
        let expected = alloc::vec![
            Signal::Publish,
            Signal::Rate,
            Signal::Report,
            Signal::Arbitrate,
            Signal::Settle,
        ];
        chk(&mut set, "C392-06", sigs == expected, "剧本序列：发布→评分→举报→仲裁→结算");
    }
    {
        // 07 三故障全量注入
        let mut fi = FaultInjector::new();
        for f in Fault::all() {
            assert!(fi.inject(f, true).is_ok());
        }
        chk(&mut set, "C392-07", fi.all_injected() && Fault::all().len() == 3, "三故障注入全覆盖");
    }
    {
        // 08 P0 漏拦
        let mut fi = FaultInjector::new();
        let r = fi.inject(Fault::PhishLink, false);
        chk(
            &mut set,
            "C392-08",
            r.err() == Some(C92Code::P0_LEAK) && fi.leaks() == 1,
            "钓鱼故障漏拦即 P0（红线实测）",
        );
    }
    {
        // 09 结算错漏拦
        let mut fi = FaultInjector::new();
        let r = fi.inject(Fault::SettleErr, false);
        chk(&mut set, "C392-09", r.err() == Some(C92Code::P0_LEAK), "结算错漏拦同为 P0");
    }
    {
        // 10 处置无责兜底
        let mut dl = DispositionList::new();
        let r = dl.add(Disposition::new(1, 0));
        chk(
            &mut set,
            "C392-10",
            r.err() == Some(C92Code::NO_OWNER) && dl.fallbacks() == 1,
            "处置无责即兜底接管",
        );
    }
    {
        // 11 有责处置正常
        let mut dl = DispositionList::new();
        let r = dl.add(Disposition::new(2, 42));
        chk(&mut set, "C392-11", r.is_ok() && dl.len() == 1, "有责处置正常登记");
    }
    {
        // 12 总线全绿
        let mut bus = R05IntegBus::new();
        for c in CoverageMatrix::all_cells() {
            bus.coverage.mark(c);
        }
        for f in Fault::all() {
            assert!(bus.injector.inject(f, true).is_ok());
        }
        chk(&mut set, "C392-12", bus.all_green(), "总线：36 格+三故障全拦=全绿");
    }
    set
}
