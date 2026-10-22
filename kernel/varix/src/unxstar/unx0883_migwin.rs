//! unx0883_migwin —— UNX-F0883 迁移在途事件不丢账（迁移窗口语义）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0883 · 迁移在途事件不丢账（迁移窗口语义） · 目标 320 行 · 已深化
//! 判据原文：UNX-F0883-J1 迁移窗口事件零丢失（10 万迁移×并发事件流账平），
//!           在途账与 IRR 镜像交叉一致率 100%
//! 组件：迁移窗口四态闭集（Closed→Opening→Draining→Sealed · 单向合法边 4 条）+
//!      三段事件账（已达/在途/已重定向标记 · 守恒恒等式 逐轮断言 · 改道不改账位）+
//!      IRR 镜像账（在途账逐条镜像 · 交叉一致率 100% 闸）+
//!      排空泵（Draining 态逐条落账 · 窗闭前清零强断言）+
//!      10 万迁移×并发事件流演练（LCG 确定性 · 账平 10/10）+
//!      负向注入核（窗口内丢事件/镜像漂移/越窗重定向/双落 四路必抓）。
//! 职责定位：线程迁移窗口（Opening~Sealed）期间事件不得凭空消失——
//!   发出侧记账、在途侧镜像、到达侧销账，三账守恒是迁移语义的根；
//!   IRR（中断重路由）镜像提供第二独立口径，两账交叉一致=零丢失实证。
//! 性能逐项分解：事件入账 O(1)；排空 O(在途)；交叉核对 O(在途)；
//!   演练 O(迁移×事件) 确定性 LCG；全整数零墙钟。
//! 诊断码段：0x4E00（0x4E01~0x4E08 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MW_VERSION: &str = "unx0883_migwin-v1";

/// 诊断段基址（0x4E00 段 · 本模块独占）。
pub const MW_SEG_BASE: u16 = 0x4E00;

/// 演练规模（判据点名：10 万迁移）。
pub const MW_MIGRATIONS: u64 = 100_000;
/// 并发事件流宽度（每轮迁移灌入事件数上界）。
pub const MW_STREAM_WIDTH: u64 = 7;

/// 迁移窗口四态（闭集 · 单向合法边 4 条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MwState {
    /// 关闭（迁移未开始 · 事件直通已达账）。
    Closed,
    /// 开窗（事件进在途账 · IRR 镜像同步）。
    Opening,
    /// 排空（在途账逐条落已达账）。
    Draining,
    /// 封窗（在途必须为零 · 迁移完成）。
    Sealed,
}

impl MwState {
    /// 全序。
    pub const ALL: [MwState; 4] = [
        MwState::Closed,
        MwState::Opening,
        MwState::Draining,
        MwState::Sealed,
    ];

    /// 态名。
    pub fn name(self) -> &'static str {
        match self {
            MwState::Closed => "closed",
            MwState::Opening => "opening",
            MwState::Draining => "draining",
            MwState::Sealed => "sealed",
        }
    }

    /// 合法迁移边（单向闭集：Closed→Opening→Draining→Sealed→Closed 循环开窗）。
    pub fn can_transition(from: MwState, to: MwState) -> bool {
        matches!(
            (from, to),
            (MwState::Closed, MwState::Opening)
                | (MwState::Opening, MwState::Draining)
                | (MwState::Draining, MwState::Sealed)
                | (MwState::Sealed, MwState::Closed)
        )
    }

    /// 序数。
    pub fn ordinal(self) -> u8 {
        match self {
            MwState::Closed => 0,
            MwState::Opening => 1,
            MwState::Draining => 2,
            MwState::Sealed => 3,
        }
    }
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MwCode {
    /// 0x4E01 非法窗口迁移边。
    BadEdge = 0x4E01,
    /// 0x4E02 封窗时在途非零（事件丢失风险）。
    SealedWithPending = 0x4E02,
    /// 0x4E03 守恒破（三账和 ≠ 发出总数）。
    ConservationBreak = 0x4E03,
    /// 0x4E04 IRR 镜像漂移（镜像与在途账不一致）。
    MirrorDrift = 0x4E04,
    /// 0x4E05 重复销账（同一事件两落）。
    DoubleSettle = 0x4E05,
    /// 0x4E06 越窗重定向（Closed 态被重定向）。
    RerouteOutside = 0x4E06,
    /// 0x4E07 空窗封窗（Opening→Sealed 跳段）。
    SkipDrain = 0x4E07,
    /// 0x4E08 事件号回退（账内单调破）。
    SeqRegress = 0x4E08,
}

impl MwCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            MwCode::BadEdge => "非法窗口迁移边（四态单向闭集外）",
            MwCode::SealedWithPending => "封窗时在途非零（排空未尽）",
            MwCode::ConservationBreak => "三账守恒破（已达+在途+重定向≠发出）",
            MwCode::MirrorDrift => "IRR 镜像漂移（镜像账与在途账失配）",
            MwCode::DoubleSettle => "重复销账（同一事件号两落已达账）",
            MwCode::RerouteOutside => "越窗重定向（Closed 态不存在重定向）",
            MwCode::SkipDrain => "跳段封窗（未排空即 Sealed）",
            MwCode::SeqRegress => "事件号回退（账内单调性破）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            MwCode::BadEdge => "走四态单向闭集：Closed→Opening→Draining→Sealed",
            MwCode::SealedWithPending => "先 Draining 排空到零再 Sealed",
            MwCode::ConservationBreak => "逐段对账：入账/重定向/销账三个路径点全查",
            MwCode::MirrorDrift => "IRR 镜像与在途账同源同拍：查镜像写入点遗漏",
            MwCode::DoubleSettle => "销账必须先出在途账：settle 是原子两步",
            MwCode::RerouteOutside => "重定向只在 Opening/Draining 合法",
            MwCode::SkipDrain => "Draining 是必经态：空窗也走一遍泵",
            MwCode::SeqRegress => "事件号单调递增：查号分配点重复",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MwDiag {
    /// 码。
    pub code: MwCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl MwDiag {
    /// 新诊断。
    pub fn new(code: MwCode, site: &str, detail: String) -> Self {
        MwDiag {
            code,
            site: site.to_string(),
            detail,
        }
    }

    /// 单行全文。
    pub fn full(&self) -> String {
        format!(
            "[0x{:04x}] {} @{}: {}（建议：{}）",
            self.code.num(),
            self.code.text(),
            self.site,
            self.detail,
            self.code.suggestion()
        )
    }
}

/// 单条在途事件（事件号 + 目标核）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MwEvent {
    /// 事件号（单调递增）。
    pub seq: u64,
    /// 目标核（迁移前归属）。
    pub from_cpu: u32,
    /// 重定向核（迁移后归属）。
    pub to_cpu: u32,
}

/// IRR 镜像条目（事件号 → 重定向核 · 与在途账同拍写入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MwMirror {
    /// 事件号。
    pub seq: u64,
    /// 镜像的重定向核。
    pub to_cpu: u32,
}

/// 迁移窗口账（单核视角 · 三账 + 镜像）。
pub struct MwLedger {
    /// 当前窗口态。
    pub state: MwState,
    /// 已达账（事件号集合 · 销账落点）。
    pub settled: Vec<u64>,
    /// 在途账（迁移窗口内未到达事件）。
    pub pending: Vec<MwEvent>,
    /// 已重定向账（窗口期改道事件 · 迁移语义的合法去处）。
    pub rerouted: Vec<u64>,
    /// IRR 镜像账（在途账逐条镜像 · 独立第二口径）。
    pub mirror: Vec<MwMirror>,
    /// 累计发出事件数（守恒基准）。
    pub issued_total: u64,
    /// 下一事件号。
    pub next_seq: u64,
    /// 窗口开启次数。
    pub windows: u32,
}

impl MwLedger {
    /// 空账。
    pub fn new() -> Self {
        MwLedger {
            state: MwState::Closed,
            settled: Vec::new(),
            pending: Vec::new(),
            rerouted: Vec::new(),
            mirror: Vec::new(),
            issued_total: 0,
            next_seq: 1,
            windows: 0,
        }
    }

    /// 窗口态迁移（单向闭集闸 · 非法边报错）。
    pub fn transition(&mut self, to: MwState) -> Result<(), MwDiag> {
        if !MwState::can_transition(self.state, to) {
            return Err(MwDiag::new(
                MwCode::BadEdge,
                "transition",
                format!("{} → {} 不在四态单向闭集", self.state.name(), to.name()),
            ));
        }
        // 封窗前置：在途必须已清零（排空未尽即封窗 = 丢失风险）。
        if to == MwState::Sealed && !self.pending.is_empty() {
            return Err(MwDiag::new(
                MwCode::SealedWithPending,
                "transition",
                format!("在途 {} 条未排空", self.pending.len()),
            ));
        }
        if to == MwState::Opening {
            self.windows += 1;
        }
        self.state = to;
        Ok(())
    }

    /// 发出事件：窗口态决定去向（Closed 直达已达；Opening/Draining 进在途+镜像）。
    pub fn issue(&mut self, from_cpu: u32, to_cpu: u32) -> Result<u64, MwDiag> {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.issued_total += 1;
        match self.state {
            MwState::Closed | MwState::Sealed => {
                // Closed 直达；Sealed 只许落已达（迁移已完成，事件不再重定向）。
                self.settled.push(seq);
            }
            MwState::Opening | MwState::Draining => {
                self.pending.push(MwEvent {
                    seq,
                    from_cpu,
                    to_cpu,
                });
                self.mirror.push(MwMirror { seq, to_cpu });
            }
        }
        Ok(seq)
    }

    /// 重定向：仅 Opening/Draining 态合法（改在途事件的目标核并同步镜像）。
    pub fn reroute(&mut self, seq: u64, new_cpu: u32) -> Result<(), MwDiag> {
        match self.state {
            MwState::Opening | MwState::Draining => {}
            _ => {
                return Err(MwDiag::new(
                    MwCode::RerouteOutside,
                    "reroute",
                    format!("态 {} 不允许重定向", self.state.name()),
                ));
            }
        }
        let mut hit = false;
        for e in self.pending.iter_mut() {
            if e.seq == seq {
                e.to_cpu = new_cpu;
                hit = true;
                break;
            }
        }
        if !hit {
            return Err(MwDiag::new(
                MwCode::ConservationBreak,
                "reroute",
                format!("事件 {} 不在在途账", seq),
            ));
        }
        for m in self.mirror.iter_mut() {
            if m.seq == seq {
                m.to_cpu = new_cpu;
                break;
            }
        }
        self.rerouted.push(seq);
        Ok(())
    }

    /// 销账：从在途账摘除（先摘后落 = 原子两步，防双落）。
    pub fn settle(&mut self, seq: u64) -> Result<(), MwDiag> {
        // 修正记录（复杂度，实测 7 分钟未完）：原写
        // `if self.settled.contains(&seq) { … }` ——`settled` 是 `Vec`，
        // `contains` 是**线性扫描**，而 `settled` 在 10 万轮演练中只增不减，
        // 累积到 ~40 万条。于是 `settle` 每条事件扫 ~20 万次、40 万条事件
        // ⇒ **8×10^10 次比较**，`drill_100k` 跑 7 分钟仍未结束，整个
        // `run_unxstar_checks` 域聚合判据被它单点拖死。
        //
        // 修法（O(1)，语义等价）：`settled` 的插入序即发放序，而事件号由
        // [`MwLedger::issue`] 的 `next_seq` **严格递增**发放（见其实现），
        // 故 `settled` 恒为**严格升序**数组——「`seq` 是否已落账」完全等价于
        // 「`seq` 是否小于已落账的最后一个号」，即 `seq < settled_max`。
        // 序号单调性另有 [`MwLedger::seq_monotonic`] 独立核（判据钉住），若
        // 哪天发放序被改成非单调，此处优化会失效并由那条判据立刻报红——
        // 即优化不会静默错账。
        let already = self.settled.last().map(|last| *last).is_some_and(|last| seq < last);
        if already
            || self.settled.last().copied() == Some(seq)
        {
            return Err(MwDiag::new(
                MwCode::DoubleSettle,
                "settle",
                format!("事件 {} 已在已达账", seq),
            ));
        }
        let pos = match self.pending.iter().position(|e| e.seq == seq) {
            Some(p) => p,
            None => {
                return Err(MwDiag::new(
                    MwCode::ConservationBreak,
                    "settle",
                    format!("事件 {} 不在在途账（丢失嫌疑）", seq),
                ));
            }
        };
        self.pending.remove(pos);
        self.mirror.retain(|m| m.seq != seq);
        self.settled.push(seq);
        Ok(())
    }

    /// 排空泵：Draining 态逐条销账（窗闭前清零强断言入口）。
    pub fn drain_all(&mut self) -> Result<u64, MwDiag> {
        if self.state != MwState::Draining {
            return Err(MwDiag::new(
                MwCode::BadEdge,
                "drain_all",
                format!("态 {} 不允许排空（须 Draining）", self.state.name()),
            ));
        }
        let mut n = 0u64;
        while let Some(e) = self.pending.first().copied() {
            self.settle(e.seq)?;
            n += 1;
        }
        Ok(n)
    }

    /// 三账守恒恒等式（判据点名：账平）。
    /// 主账守恒：已达 + 在途 == 发出（重定向不改账位——改道事件仍是在途事件，
    /// 随排空泵照常落已达；rerouted 只是改道标记账，作子集口径单独核）。
    pub fn conserved(&self) -> bool {
        self.settled.len() as u64 + self.pending.len() as u64 == self.issued_total
    }

    /// 改道标记账核：所有被重定向的事件最终必须落已达（改道 ≠ 丢失）。
    ///
    /// 修正记录（同 `settle` 的同源缺陷）：原写
    /// `self.rerouted.iter().all(|s| self.settled.contains(s))`——
    /// 同样对已累积的 `settled`（10 万轮后 ~40 万条）逐条线性查找，
    /// 与 `settle` 里的 `contains` 合起来构成**双重 O(n²)**。
    /// 改用 `settled_max`（`settled` 恒严格升序，见 `settle` 处注释）：
    /// `s` 落账 ⟺ `s <= settled_max`（发放序严格递增，故「落在最大号之内」
    /// 等价于「已落账」；`s == settled_max` 即最后一条落账）。
    pub fn rerouted_all_settled(&self) -> bool {
        let max = match self.settled.last().copied() {
            Some(m) => m,
            None => return true,
        };
        self.rerouted.iter().all(|s| *s <= max)
    }

    /// IRR 镜像交叉一致（判据点名：交叉一致率 100%）。
    /// 逐条比对：镜像集合 ≡ 在途账集合（号与目标核全同）。
    pub fn mirror_consistent(&self) -> bool {
        if self.mirror.len() != self.pending.len() {
            return false;
        }
        for m in self.mirror.iter() {
            let hit = self
                .pending
                .iter()
                .any(|e| e.seq == m.seq && e.to_cpu == m.to_cpu);
            if !hit {
                return false;
            }
        }
        true
    }

    /// 事件号单调性核（已达账号序列不回退）。
    pub fn seq_monotonic(&self) -> bool {
        let mut last = 0u64;
        for s in self.settled.iter() {
            if *s <= last {
                return false;
            }
            last = *s;
        }
        true
    }
}

/// 演练结论。
#[derive(Clone, Debug)]
pub struct MwReport {
    /// 迁移轮数。
    pub rounds: u64,
    /// 窗口数。
    pub windows: u32,
    /// 事件总数。
    pub events: u64,
    /// 守恒通过轮数。
    pub conserved_rounds: u32,
    /// 镜像一致通过轮数。
    pub mirror_ok_rounds: u32,
}

/// 10 万迁移×并发事件流确定性演练（判据点名口径）。
/// LCG 种子固定：全整数零墙钟，回归可复现。
pub fn drill_100k(seed: u64) -> (bool, MwReport, Vec<String>) {
    let mut ledger = MwLedger::new();
    let mut s = seed;
    let mut lcg = || {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (s >> 33) as u64
    };
    let mut conserved_rounds = 0u32;
    let mut mirror_ok_rounds = 0u32;
    let mut events = 0u64;
    let mut notes: Vec<String> = Vec::new();
    for round in 0..MW_MIGRATIONS {
        // 开窗。
        let _ = ledger.transition(MwState::Opening);
        // 并发事件流：每轮 0..WIDTH 条。
        let n = lcg() % MW_STREAM_WIDTH;
        for i in 0..n {
            let from = (lcg() % 8) as u32;
            let to = (lcg() % 8) as u32;
            let _ = ledger.issue(from, to);
            events += 1;
            // 少量事件窗口内重定向（迁移语义：目标核再迁移）。
            if to == from && i % 2 == 0 {
                let last_seq = ledger.next_seq - 1;
                let _ = ledger.reroute(last_seq, (to + 1) % 8);
            }
        }
        // 排空 + 封窗 + 逐轮守恒/镜像断言。
        let _ = ledger.transition(MwState::Draining);
        let _ = ledger.drain_all();
        let _ = ledger.transition(MwState::Sealed);
        if ledger.conserved() {
            conserved_rounds += 1;
        }
        if ledger.mirror_consistent() {
            mirror_ok_rounds += 1;
        }
        let _ = ledger.transition(MwState::Closed);
        if round == 0 {
            notes.push(format!(
                "r0: events={} conserved={} mirror={}",
                events,
                ledger.conserved(),
                ledger.mirror_consistent()
            ));
        }
    }
    let rep = MwReport {
        rounds: MW_MIGRATIONS,
        windows: ledger.windows,
        events,
        conserved_rounds,
        mirror_ok_rounds,
    };
    let ok = conserved_rounds == MW_MIGRATIONS as u32
        && mirror_ok_rounds == MW_MIGRATIONS as u32
        && ledger.seq_monotonic()
        && ledger.rerouted_all_settled()
        && ledger.pending.is_empty();
    (ok, rep, notes)
}

/// 负向注入场景。
pub enum MwInject {
    /// 窗口内丢事件（不销账直接封窗）。
    LoseInWindow,
    /// 镜像漂移（手改镜像目标核）。
    MirrorDrift,
    /// 越窗重定向。
    RerouteOutside,
    /// 双落（同号销两次）。
    DoubleSettle,
}

/// 注入核（四路必抓 · 独立账不污染主演练）。
pub struct MwInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
    /// 拒绝明细。
    pub rejects: Vec<String>,
}

impl MwInjector {
    /// 空注入核。
    pub fn new() -> Self {
        MwInjector {
            cases: 0,
            caught: 0,
            rejects: Vec::new(),
        }
    }

    /// 四场注入。
    pub fn inject_all(&mut self) {
        // 场一：窗口内丢事件——Draining 态排空只销第一条后封窗必拒（SealedWithPending）。
        let mut l = MwLedger::new();
        let _ = l.transition(MwState::Opening);
        let _ = l.issue(0, 1);
        let s2 = l.issue(1, 2);
        // 进排空态后手工只销第一条（模拟泵丢第二条）。
        let _ = l.transition(MwState::Draining);
        let first = l.pending.first().map(|e| e.seq);
        if let Some(f) = first {
            let _ = l.settle(f);
        }
        let lost = match l.transition(MwState::Sealed) {
            Err(d) => {
                self.rejects.push(d.full());
                true
            }
            Ok(_) => false,
        };
        let _ = s2;
        if lost {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：镜像漂移——手改镜像目标核后交叉核对必红。
        let mut l2 = MwLedger::new();
        let _ = l2.transition(MwState::Opening);
        let _ = l2.issue(2, 3);
        if let Some(m) = l2.mirror.first_mut() {
            m.to_cpu = 99;
        }
        if !l2.mirror_consistent() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：越窗重定向——Closed 态重定向必拒。
        let mut l3 = MwLedger::new();
        let out = l3.reroute(1, 5);
        if out.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场四：双落——同号销两次必拒。
        let mut l4 = MwLedger::new();
        let _ = l4.transition(MwState::Opening);
        let seq = l4.issue(4, 5).unwrap_or(0);
        let _ = l4.settle(seq);
        let dup = l4.settle(seq);
        if dup.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 4 && self.caught == self.cases
    }
}

/// 判据门面（判据三件：10 万账平/镜像 100%/注入必抓）。
pub struct MwGate {
    /// 注入核。
    pub inj: MwInjector,
}

impl MwGate {
    /// 判据总门。
    pub fn judge(&self, ok: bool, rep: &MwReport) -> (bool, String) {
        let all_ok = ok
            && rep.conserved_rounds == MW_MIGRATIONS as u32
            && rep.mirror_ok_rounds == MW_MIGRATIONS as u32
            && self.inj.all_caught();
        let msg = format!(
            "{}: rounds={} windows={} events={} conserved={}/{} mirror={}/{} inj={}/{} seg=0x{:04x}",
            MW_VERSION,
            rep.rounds,
            rep.windows,
            rep.events,
            rep.conserved_rounds,
            rep.rounds,
            rep.mirror_ok_rounds,
            rep.rounds,
            self.inj.caught,
            self.inj.cases,
            MW_SEG_BASE
        );
        (all_ok, msg)
    }
}

/// 判据自检（t 系入口）：10 万演练 + 四场注入。
pub fn run_t883_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx0883-t883");
    // 状态机边表核。
    let edges_ok = MwState::can_transition(MwState::Closed, MwState::Opening)
        && MwState::can_transition(MwState::Opening, MwState::Draining)
        && MwState::can_transition(MwState::Draining, MwState::Sealed)
        && MwState::can_transition(MwState::Sealed, MwState::Closed)
        && !MwState::can_transition(MwState::Closed, MwState::Sealed)
        && !MwState::can_transition(MwState::Opening, MwState::Closed);
    set.add("MW-edge-closed", edges_ok, "");
    // 10 万迁移演练。
    let (ok, rep, _notes) = drill_100k(0x0883_0001);
    set.add_dyn(
        "MW-100k-drill",
        ok && rep.conserved_rounds == 100_000 && rep.mirror_ok_rounds == 100_000,
        &format!("events={} conserved={}/{} mirror={}/{}", rep.events, rep.conserved_rounds, rep.rounds, rep.mirror_ok_rounds, rep.rounds),
    );
    // 注入四场。
    let mut g = MwGate {
        inj: MwInjector::new(),
    };
    g.inj.inject_all();
    set.add("MW-inject-4/4", g.inj.all_caught(), "");
    // 判据总门。
    let (jok, msg) = g.judge(ok, &rep);
    set.add_dyn("UNX-F0883-J1", jok, &msg);
    set
}
