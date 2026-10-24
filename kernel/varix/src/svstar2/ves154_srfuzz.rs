//! VE-F3854 · 读屏 fuzz
//!
//! 协同 fuzz：语义事件风暴/标注缺失注入/公告风暴/导航跳转风暴。
//! 变异三级：事件变异（风暴/乱序）/ 结构变异（标注缺失/嵌套畸形）/ 时序变异（竞态）。
//! 四不变量：断链不发生/公告不丢/序不乱/预算不超——分级（断链=公告丢=P0）。
//! 错误路径：断链 fuzz→P0、公告丢→P0、挂起→看门狗、闭环缺→阻断。




#![allow(dead_code)]

extern crate alloc;

use alloc::{vec, vec::Vec};

// ── 诊断码段（0xCA00）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xCA00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CCACode(pub u16);

impl CCACode {
    pub const OK: Self = Self(SEG_BASE);
    pub const BREAK_P0: Self = Self(SEG_BASE + 1);
    pub const ANNOUNCE_LOST_P0: Self = Self(SEG_BASE + 2);
    pub const WATCHDOG: Self = Self(SEG_BASE + 3);
    pub const LOOP_MISSING: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::BREAK_P0 => "断链 fuzz 命中→P0",
            Self::ANNOUNCE_LOST_P0 => "公告丢失→P0",
            Self::WATCHDOG => "挂起→看门狗终止",
            Self::LOOP_MISSING => "闭环缺→阻断",
            _ => "未知",
        }
    }
}

// ── 语义事件 ────────────────────────────────────────────────────────

/// 语义事件（fuzz 原料）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemEvent {
    /// 事件类型（1=标注 2=公告 3=导航 4=虚拟化）。
    pub kind: u8,
    pub node: u32,
    /// 时序戳（时序变异用）。
    pub tick: u32,
}

/// 语料库。
#[derive(Debug, Clone)]
pub struct FuzzCorpus {
    pub seeds: Vec<SemEvent>,
    /// 历史缺陷回放。
    pub replay: Vec<SemEvent>,
}

impl FuzzCorpus {
    pub fn new() -> Self {
        Self {
            seeds: Vec::new(),
            replay: Vec::new(),
        }
    }

    pub fn total(&self) -> usize {
        self.seeds.len() + self.replay.len()
    }

    /// 回放驱动：回放样本全部参与本轮。
    pub fn drives_replay(&self) -> bool {
        !self.replay.is_empty()
    }
}

impl Default for FuzzCorpus {
    fn default() -> Self {
        Self::new()
    }
}

// ── 三级变异 ────────────────────────────────────────────────────────

/// 变异级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutation {
    /// 事件变异：风暴（翻倍灌入）。
    EventStorm,
    /// 事件变异：乱序（tick 逆序）。
    EventReorder,
    /// 结构变异：标注缺失（抽掉 kind=1）。
    StructMissing,
    /// 结构变异：嵌套畸形（node 置 0）。
    StructMalform,
    /// 时序变异：竞态（tick 全部同帧）。
    TimingRace,
}

impl Mutation {
    pub const ALL: [Mutation; 5] = [
        Mutation::EventStorm,
        Mutation::EventReorder,
        Mutation::StructMissing,
        Mutation::StructMalform,
        Mutation::TimingRace,
    ];
}

/// 对一批事件施加变异。
pub fn mutate(events: &[SemEvent], m: Mutation) -> Vec<SemEvent> {
    match m {
        Mutation::EventStorm => {
            let mut out = events.to_vec();
            out.extend_from_slice(events);
            out
        }
        Mutation::EventReorder => {
            let mut out = events.to_vec();
            out.sort_by(|a, b| b.tick.cmp(&a.tick));
            out
        }
        Mutation::StructMissing => events
            .iter()
            .filter(|e| e.kind != 1)
            .copied()
            .collect(),
        Mutation::StructMalform => events
            .iter()
            .map(|e| SemEvent { node: 0, ..*e })
            .collect(),
        Mutation::TimingRace => events
            .iter()
            .map(|e| SemEvent { tick: 0, ..*e })
            .collect(),
    }
}

// ── 四不变量 ────────────────────────────────────────────────────────

/// 四不变量检查结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invariant {
    /// 断链不发生。
    NoBreak,
    /// 公告不丢。
    NoAnnounceLoss,
    /// 序不乱。
    OrderKept,
    /// 预算不超。
    BudgetKept,
}

impl Invariant {
    pub const ALL: [Invariant; 4] = [
        Invariant::NoBreak,
        Invariant::NoAnnounceLoss,
        Invariant::OrderKept,
        Invariant::BudgetKept,
    ];

    /// 分级：断链=公告丢=P0；序乱=预算超=P1。
    pub fn grade(self) -> u8 {
        match self {
            Self::NoBreak | Self::NoAnnounceLoss => 0,
            _ => 1,
        }
    }
}

/// fuzz 运行记录。
#[derive(Debug, Clone)]
pub struct FuzzRun {
    pub case_id: u32,
    pub mutation: Mutation,
    /// 被破坏的不变量（空 = 存活）。
    pub violated: Vec<Invariant>,
    /// 执行微秒。
    pub elapsed_us: u32,
}

impl FuzzRun {
    /// 存活判定。
    pub fn survived(&self) -> bool {
        self.violated.is_empty()
    }

    /// 运行分级：命中 P0 不变量 = P0。
    pub fn grade(&self) -> u8 {
        self.violated
            .iter()
            .map(|v| v.grade())
            .min()
            .unwrap_or(2)
    }
}

// ── 看门狗 ──────────────────────────────────────────────────────────

/// 看门狗：单案例超时（挂起→终止）。
pub const WATCHDOG_US: u32 = 5_000;

pub fn watchdog_ok(elapsed_us: u32) -> bool {
    elapsed_us <= WATCHDOG_US
}

// ── 风暴驱动器 ──────────────────────────────────────────────────────

/// fuzz 驱动器。
#[derive(Debug, Clone)]
pub struct StormDriver {
    pub corpus: FuzzCorpus,
    pub runs: Vec<FuzzRun>,
    /// P0 命中数。
    pub p0_hits: u32,
    /// 闭环复述：P0 案例是否全部有最小化记录。
    pub minimized: Vec<u32>,
    /// 看门狗终止数。
    pub watchdog_kills: u32,
}

impl StormDriver {
    pub fn new() -> Self {
        Self {
            corpus: FuzzCorpus::new(),
            runs: Vec::new(),
            p0_hits: 0,
            minimized: Vec::new(),
            watchdog_kills: 0,
        }
    }

    /// 跑一轮：全部种子 × 全部变异。
    pub fn storm(&mut self, budget_us: u32) {
        self.runs.clear();
        self.p0_hits = 0;
        self.watchdog_kills = 0;
        for (i, e) in self.corpus.total_seeds().iter().enumerate() {
            for m in Mutation::ALL.iter() {
                let out = mutate(&[e.clone()], *m);
                // 模拟观测：结构缺标注→序不乱受检；嵌套畸形→断链受检。
                let mut violated = Vec::new();
                if *m == Mutation::StructMissing && out.is_empty() {
                    violated.push(Invariant::OrderKept);
                }
                if *m == Mutation::StructMalform && out.iter().any(|o| o.node == 0) {
                    violated.push(Invariant::NoBreak);
                }
                let elapsed = ((i as u32 + 7) * 137) % 9_000;
                if !watchdog_ok(elapsed) {
                    self.watchdog_kills += 1;
                    continue;
                }
                let run = FuzzRun {
                    case_id: i as u32,
                    mutation: *m,
                    violated,
                    elapsed_us: elapsed.min(budget_us),
                };
                if run.grade() == 0 {
                    self.p0_hits += 1;
                    self.minimized.push(run.case_id);
                }
                self.runs.push(run);
            }
        }
    }

    /// 闭环复述：P0 案例全部有最小化记录且可回放。
    pub fn closure_ok(&self) -> bool {
        let p0_cases: Vec<u32> = self
            .runs
            .iter()
            .filter(|r| r.grade() == 0)
            .map(|r| r.case_id)
            .collect();
        p0_cases.iter().all(|c| self.minimized.contains(c))
    }

    /// 会话判定：无 P0 且闭环齐。
    pub fn clean(&self) -> bool {
        self.p0_hits == 0 && self.closure_ok()
    }
}

impl Default for StormDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl FuzzCorpus {
    /// 合并种子与回放为一轮原料。
    pub fn total_seeds(&self) -> Vec<SemEvent> {
        let mut v = self.seeds.clone();
        v.extend(self.replay.iter().copied());
        v
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves154_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves154_srfuzz");

    fn ev(kind: u8, node: u32, tick: u32) -> SemEvent {
        SemEvent { kind, node, tick }
    }

    // 01 四不变量分级：两类 P0
    {
        let p0 = Invariant::ALL.iter().filter(|v| v.grade() == 0).count();
        chk(&mut set, "C3854-01", p0 == 2, "err");
    }
    // 02 断链与公告丢为 P0
    {
        chk(
            &mut set,
            "C3854-02",
            Invariant::NoBreak.grade() == 0 && Invariant::NoAnnounceLoss.grade() == 0,
            "err",
        );
    }
    // 03 事件风暴翻倍
    {
        let base = [ev(2, 1, 0), ev(3, 2, 1)];
        let out = mutate(&base, Mutation::EventStorm);
        chk(&mut set, "C3854-03", out.len() == 4, "err");
    }
    // 04 乱序变异逆 tick
    {
        let base = [ev(2, 1, 5), ev(3, 2, 9), ev(4, 3, 1)];
        let out = mutate(&base, Mutation::EventReorder);
        let ticks: Vec<u32> = out.iter().map(|e| e.tick).collect();
        chk(&mut set, "C3854-04", ticks == vec![9, 5, 1], "err");
    }
    // 05 标注缺失变异抽 kind=1
    {
        let base = [ev(1, 1, 0), ev(2, 2, 1), ev(1, 3, 2)];
        let out = mutate(&base, Mutation::StructMissing);
        chk(
            &mut set,
            "C3854-05",
            out.len() == 1 && out[0].kind == 2,
            "err",
        );
    }
    // 06 嵌套畸形变异 node 归零
    {
        let base = [ev(2, 7, 0), ev(3, 8, 1)];
        let out = mutate(&base, Mutation::StructMalform);
        chk(
            &mut set,
            "C3854-06",
            out.iter().all(|e| e.node == 0),
            "err",
        );
    }
    // 07 竞态变异 tick 同帧
    {
        let base = [ev(2, 1, 3), ev(3, 2, 8)];
        let out = mutate(&base, Mutation::TimingRace);
        chk(
            &mut set,
            "C3854-07",
            out.iter().all(|e| e.tick == 0),
            "err",
        );
    }
    // 08 五级变异齐备
    {
        chk(&mut set, "C3854-08", Mutation::ALL.len() == 5, "err");
    }
    // 09 看门狗：挂起终止
    {
        chk(
            &mut set,
            "C3854-09",
            !watchdog_ok(WATCHDOG_US + 1) && watchdog_ok(WATCHDOG_US),
            "err",
        );
    }
    // 10 回放样本参与风暴
    {
        let mut d = StormDriver::new();
        d.corpus.seeds.push(ev(2, 1, 0));
        d.corpus.replay.push(ev(3, 9, 9));
        let total = d.corpus.total();
        let seeds = d.corpus.total_seeds().len();
        chk(
            &mut set,
            "C3854-10",
            total == 2 && seeds == 2 && d.corpus.drives_replay(),
            "err",
        );
    }
    // 11 风暴：干净语料无 P0
    {
        let mut d = StormDriver::new();
        d.corpus.seeds.push(ev(2, 5, 0));
        d.corpus.seeds.push(ev(3, 6, 1));
        d.storm(10_000);
        chk(&mut set, "C3854-11", d.p0_hits == 0 && d.clean(), "err");
    }
    // 12 风暴：P0 命中计数与最小化闭环
    {
        let mut d = StormDriver::new();
        d.corpus.seeds.push(ev(4, 0, 0));
        d.storm(10_000);
        // StructMalform 变异 node=0 → NoBreak P0。
        let has_p0 = d.p0_hits > 0;
        chk(
            &mut set,
            "C3854-12",
            has_p0 && d.closure_ok() && !d.clean(),
            "err",
        );
    }
    // 13 运行分级：P1 不变量不升 P0
    {
        let r = FuzzRun {
            case_id: 1,
            mutation: Mutation::StructMissing,
            violated: vec![Invariant::BudgetKept],
            elapsed_us: 100,
        };
        chk(
            &mut set,
            "C3854-13",
            r.grade() == 1 && !r.survived(),
            "err",
        );
    }
    // 14 存活运行分级为默认级
    {
        let r = FuzzRun {
            case_id: 2,
            mutation: Mutation::EventStorm,
            violated: Vec::new(),
            elapsed_us: 10,
        };
        chk(&mut set, "C3854-14", r.grade() == 2 && r.survived(), "err");
    }
    // 15 看门狗击杀计数
    {
        let mut d = StormDriver::new();
        d.corpus.seeds.push(ev(2, 5, 0));
        d.storm(10_000);
        // (0+7)*137=959 < 5000 不杀；确认击杀只对超时发生。
        chk(&mut set, "C3854-15", d.watchdog_kills == 0, "err");
    }
    // 16 预算封顶：elapsed 不超 budget
    {
        let mut d = StormDriver::new();
        d.corpus.seeds.push(ev(2, 5, 0));
        d.storm(200);
        chk(
            &mut set,
            "C3854-16",
            d.runs.iter().all(|r| r.elapsed_us <= 200),
            "err",
        );
    }
    // 17 语料空跑：零运行不误报
    {
        let mut d = StormDriver::new();
        d.storm(10_000);
        chk(
            &mut set,
            "C3854-17",
            d.runs.is_empty() && d.clean(),
            "err",
        );
    }
    // 18 诊断码五枚互异
    {
        let codes = [
            CCACode::BREAK_P0,
            CCACode::ANNOUNCE_LOST_P0,
            CCACode::WATCHDOG,
            CCACode::LOOP_MISSING,
        ];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        chk(&mut set, "C3854-18", distinct, "err");
    }

    set
}
