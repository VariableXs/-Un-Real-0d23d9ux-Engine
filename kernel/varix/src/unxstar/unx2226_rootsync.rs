//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2226 · 冻结账-根台账对回（§三 A3 行对表） · 0x72B00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B32）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F2226-J1 根台账对回全过（冻结账批次计数/域账累计
//!     与根台账 §三 A3 行数字逐项对表——批数/条数/行数三对一致），
//!     数字漂移注入 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 双账三对：冻结账（B01–B40，每批 20 条，共 800 条）按域归并出
//!     「批数/条数/行数」三项累计；根台账 §三 A3 八域行独立闭式直算
//!     同三项；对回 = 逐域逐项对表（8 域 × 3 项 = 24 对）；
//!   · 双路独立：归并路径（逐批累加）与建根路径（闭式直算）代码路径
//!     不同，同源事实两条路必须数字一致，任何漂移即拒；
//!   · 数字漂移注入 10 路：批数漂移/条数漂移/行数漂移/缺行/换行/改名/
//!     多行/批次改归属/全表漂移/尾行漂移，对回器 10/10 检出；
//!   · 状态机 Drafting→Frozen→Reconciled 单调推进，未冻结对回显性拒；
//!   · 批次区间连续性守卫：批号 1..=40 首尾相接无洞无重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72B00，VX3-1103 根台账对回窗口独占）。
pub const SEG_BASE: u32 = 0x72B00;
/// 冻结批数（B01–B40，冻结 40）。
pub const BATCH_N: usize = 40;
/// 每批条数（冻结 20）。
pub const PER_BATCH: usize = 20;
/// 冻结账总条数（40×20=800）。
pub const TOTAL_ENTRIES: usize = BATCH_N * PER_BATCH;
/// §三 A3 域行数（八域，冻结 8）。
pub const DOMAIN_N: usize = 8;
/// 每域覆盖批数（40/8=5）。
pub const BATCHES_PER_DOMAIN: usize = BATCH_N / DOMAIN_N;
/// 冻结账纪元（冻结 3）。
pub const EPOCH: u64 = 3;
/// 数字漂移注入路数（判据 10/10，冻结）。
pub const DRIFT_ROUNDS: u32 = 10;
/// 每条基线行数（行权值 = 基线 + 全局下标 %7，冻结 18）。
pub const LINE_BASE: u32 = 18;
/// 对回总对数（8 域 × 3 项 = 24 对）。
pub const PAIRS_TOTAL: u32 = (DOMAIN_N * 3) as u32;

/// 八域行名（§三 A3 行序冻结）。
pub const DOMAIN_NAMES: [&str; DOMAIN_N] =
    ["调度", "内存", "IPC", "ACPI", "安全", "文件", "时序", "导出"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncCode {
    /// 批数漂移：域行批数与冻结账归并批数对不上
    BatchCountDrift,
    /// 条数漂移：域行条数与冻结账归并条数对不上
    EntryCountDrift,
    /// 行数漂移：域行行数与冻结账归并行数对不上
    LineCountDrift,
    /// 行缺失：根账域行少于八域，§三 A3 表面残缺
    RowMissing,
    /// 行多余：根账出现八域之外的幽灵域行
    RowExtra,
    /// 域名未知：行名不在八域闭集内或行序错位
    DomainUnknown,
    /// 状态拒：未冻结对回或状态机倒退，先冻结再对回
    StateRej,
    /// 双跑不一致：复现红线，golden_run 两次应同值
    NotReproducible,
}

impl SyncCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SyncCode::BatchCountDrift => "批数漂移：域行批数与冻结账归并值对不上，逐批核对归属",
            SyncCode::EntryCountDrift => "条数漂移：域行条数与冻结账归并值对不上，逐条清点",
            SyncCode::LineCountDrift => "行数漂移：域行行数与冻结账归并值对不上，重算行权值",
            SyncCode::RowMissing => "行缺失：根账域行不足八行，按 §三 A3 补齐域行",
            SyncCode::RowExtra => "行多余：根账出现未知域行，删除幽灵行或并入八域",
            SyncCode::DomainUnknown => "域名未知：行名不在八域闭集内，按 §三 A3 域名表改正",
            SyncCode::StateRej => "状态拒：未冻结对回或状态倒退，先走冻结再对回",
            SyncCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [SyncCode; 8] = [
        SyncCode::BatchCountDrift,
        SyncCode::EntryCountDrift,
        SyncCode::LineCountDrift,
        SyncCode::RowMissing,
        SyncCode::RowExtra,
        SyncCode::DomainUnknown,
        SyncCode::StateRej,
        SyncCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SyncCode::ALL.len() {
        if SyncCode::ALL[i].val() == code {
            return SyncCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72B00 段闭集内"
}

// ── 状态机（Drafting→Frozen→Reconciled，单调推进） ───────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncState {
    /// 起草：批次尚未冻结，禁止对回。
    Drafting,
    /// 已冻结：B01–B40 逐批封账，可对回。
    Frozen,
    /// 已对回：24 对全过，对回结论落账。
    Reconciled,
}

impl SyncState {
    /// 单调序值：Drafting=0 < Frozen=1 < Reconciled=2。
    pub fn rank(self) -> u8 {
        match self {
            SyncState::Drafting => 0,
            SyncState::Frozen => 1,
            SyncState::Reconciled => 2,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: SyncState) -> bool {
        to.rank() > self.rank()
    }
}

// ── 行权值与批域映射（同源事实的唯一定义点） ────────────────────
/// 第 global_idx 条（0 起）的行权值：基线 + 下标 %7，确定性可复算。
pub fn entry_line_weight(global_idx: usize) -> u32 {
    LINE_BASE + (global_idx % 7) as u32
}

/// 批号（1 起）→ 域下标：每域连片 5 批（B01–B05 属调度域…）。
pub fn batch_domain(batch_id: u64) -> usize {
    ((batch_id - 1) as usize) / BATCHES_PER_DOMAIN
}

// ── 冻结账（对回一侧账本） ──────────────────────────────────────
/// 冻结批：批号、域归属、首条全局号、条数、行数、封账旗。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenBatch {
    pub id: u64,
    pub domain_idx: usize,
    pub first_entry: u64,
    pub entries: u32,
    pub lines: u32,
    pub frozen: bool,
}

/// 冻结账：纪元 + 批表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenLedger {
    pub epoch: u64,
    pub batches: Vec<FrozenBatch>,
}

/// 建冻结账（草稿态：frozen 全 false，行数按行权值逐条累加）。
pub fn build_frozen_ledger(epoch: u64) -> FrozenLedger {
    let mut batches: Vec<FrozenBatch> = Vec::new();
    let mut b = 0usize;
    while b < BATCH_N {
        let mut lines = 0u32;
        let mut j = 0usize;
        while j < PER_BATCH {
            lines += entry_line_weight(b * PER_BATCH + j);
            j += 1;
        }
        batches.push(FrozenBatch {
            id: (b + 1) as u64,
            domain_idx: b / BATCHES_PER_DOMAIN,
            first_entry: (b * PER_BATCH + 1) as u64,
            entries: PER_BATCH as u32,
            lines,
            frozen: false,
        });
        b += 1;
    }
    FrozenLedger { epoch, batches }
}

/// 封账：逐批 frozen 置位；返回实际封账批数。
pub fn freeze_ledger(led: &mut FrozenLedger) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < led.batches.len() {
        if let Some(bt) = led.batches.get_mut(i) {
            if !bt.frozen {
                bt.frozen = true;
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 批次区间连续性：批号 1..=40 首尾相接，无洞无重叠，首条号/条数/域归属逐批对。
pub fn batch_continuity(led: &FrozenLedger) -> bool {
    if led.batches.len() != BATCH_N {
        return false;
    }
    let mut b = 0usize;
    while b < BATCH_N {
        let bt = match led.batches.get(b) {
            Some(x) => x,
            None => return false,
        };
        let want_id = (b + 1) as u64;
        if bt.id != want_id
            || bt.first_entry != want_id * PER_BATCH as u64 - (PER_BATCH as u64 - 1)
            || bt.entries != PER_BATCH as u32
            || bt.domain_idx != b / BATCHES_PER_DOMAIN
        {
            return false;
        }
        b += 1;
    }
    true
}

// ── 域账归并（路径一：逐批累加） ────────────────────────────────
/// 域归并行：域下标 + 批数/条数/行数三项累计。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainRollup {
    pub domain_idx: usize,
    pub batches: u32,
    pub entries: u32,
    pub lines: u32,
}

/// 冻结账按域归并（要求全账已封；任一批未封即拒）。
pub fn domain_rollup(led: &FrozenLedger) -> Result<Vec<DomainRollup>, u32> {
    let mut out: Vec<DomainRollup> = Vec::new();
    let mut d = 0usize;
    while d < DOMAIN_N {
        out.push(DomainRollup { domain_idx: d, batches: 0, entries: 0, lines: 0 });
        d += 1;
    }
    let mut i = 0usize;
    while i < led.batches.len() {
        let bt = match led.batches.get(i) {
            Some(x) => x,
            None => return Err(SyncCode::RowMissing.val()),
        };
        if !bt.frozen {
            return Err(SyncCode::StateRej.val());
        }
        if bt.domain_idx >= DOMAIN_N {
            return Err(SyncCode::DomainUnknown.val());
        }
        if let Some(r) = out.get_mut(bt.domain_idx) {
            r.batches += 1;
            r.entries += bt.entries;
            r.lines += bt.lines;
        }
        i += 1;
    }
    Ok(out)
}

// ── 根台账 §三 A3（路径二：闭式直算） ───────────────────────────
/// 根账域行：域名 + 批数/条数/行数三项（A3 表三列数字）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootRow {
    pub domain: String,
    pub batches: u32,
    pub entries: u32,
    pub lines: u32,
}

/// 根台账：纪元 + 八域行表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootLedger {
    pub epoch: u64,
    pub rows: Vec<RootRow>,
}

/// 建根台账：逐域闭式直算三项（不经批表，与归并路径结构独立）。
pub fn build_root_ledger(epoch: u64) -> RootLedger {
    let mut rows: Vec<RootRow> = Vec::new();
    let mut d = 0usize;
    while d < DOMAIN_N {
        // 条数闭式：批数 × 每批条数。
        let entries = (BATCHES_PER_DOMAIN * PER_BATCH) as u32;
        // 行数闭式：对域内每条全局下标直算行权值求和。
        let mut lines = 0u32;
        let mut k = 0usize;
        while k < entries as usize {
            lines += entry_line_weight(d * entries as usize + k);
            k += 1;
        }
        rows.push(RootRow {
            domain: DOMAIN_NAMES[d].to_string(),
            batches: BATCHES_PER_DOMAIN as u32,
            entries,
            lines,
        });
        d += 1;
    }
    RootLedger { epoch, rows }
}

// ── 对回器（三对逐项对表） ──────────────────────────────────────
/// 对回报告：已核域数、已核对数（每域 3 对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconcileReport {
    pub domains_checked: u32,
    pub pairs_checked: u32,
}

/// 对回：根账行 vs 域归并行，逐域批数/条数/行数三对一致才放行。
/// 任何一行缺失/多余/改名/数字漂移，均返回首个命中诊断码。
pub fn reconcile(root: &RootLedger, rollup: &[DomainRollup]) -> Result<ReconcileReport, u32> {
    if root.rows.len() < DOMAIN_N {
        return Err(SyncCode::RowMissing.val());
    }
    if root.rows.len() > DOMAIN_N {
        return Err(SyncCode::RowExtra.val());
    }
    if rollup.len() != DOMAIN_N {
        return Err(SyncCode::RowMissing.val());
    }
    let mut pairs = 0u32;
    let mut d = 0usize;
    while d < DOMAIN_N {
        let row = match root.rows.get(d) {
            Some(x) => x,
            None => return Err(SyncCode::RowMissing.val()),
        };
        let acc = match rollup.get(d) {
            Some(x) => x,
            None => return Err(SyncCode::RowMissing.val()),
        };
        if row.domain != DOMAIN_NAMES[d] {
            return Err(SyncCode::DomainUnknown.val());
        }
        if row.batches != acc.batches {
            return Err(SyncCode::BatchCountDrift.val());
        }
        pairs += 1;
        if row.entries != acc.entries {
            return Err(SyncCode::EntryCountDrift.val());
        }
        pairs += 1;
        if row.lines != acc.lines {
            return Err(SyncCode::LineCountDrift.val());
        }
        pairs += 1;
        d += 1;
    }
    Ok(ReconcileReport { domains_checked: DOMAIN_N as u32, pairs_checked: pairs })
}

// ── 对回引擎（状态机 + 对回记账） ───────────────────────────────
/// 对回引擎：状态机单调推进 + 对回次数/检出漂移次数记账。
#[derive(Clone, Debug)]
pub struct RootSync {
    pub state: SyncState,
    pub epoch: u64,
    /// 已完成对回轮数。
    pub reconciles: u32,
    /// 累计检出的漂移注入轮数。
    pub drift_caught: u32,
}

impl RootSync {
    pub fn new(epoch: u64) -> RootSync {
        RootSync { state: SyncState::Drafting, epoch, reconciles: 0, drift_caught: 0 }
    }

    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance(&mut self, to: SyncState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(SyncCode::StateRej.val());
        }
        self.state = to;
        Ok(())
    }

    /// 冻结：封批表 + 状态推进 Frozen；草稿态重复冻结平移拒。
    pub fn freeze(&mut self, led: &mut FrozenLedger) -> Result<u32, u32> {
        let n = freeze_ledger(led);
        self.advance(SyncState::Frozen)?;
        Ok(n)
    }

    /// 对回执行口：仅 Frozen 及以后可对回；结论落账。
    pub fn reconcile_run(&mut self, root: &RootLedger, led: &FrozenLedger) -> Result<ReconcileReport, u32> {
        if self.state.rank() < SyncState::Frozen.rank() {
            return Err(SyncCode::StateRej.val());
        }
        let acc = domain_rollup(led)?;
        let rep = reconcile(root, &acc)?;
        self.reconciles += 1;
        if self.state.rank() < SyncState::Reconciled.rank() {
            self.state = SyncState::Reconciled;
        }
        Ok(rep)
    }

    /// 漂移检出记账（注入轮用）。
    pub fn note_drift(&mut self) {
        self.drift_caught += 1;
    }
}

// ── 数字漂移注入（10 路） ───────────────────────────────────────
/// 对黄金双账注入第 kind 路漂移，返回漂移后的 (根账, 冻结账)。
/// 十路语义：
///   0 批数漂移（域 0 批数 +1）；
///   1 条数漂移（域 1 条数 -1）；
///   2 行数漂移（域 2 行数 +7）；
///   3 缺行（删域 3 行）；
///   4 换行（域 4/5 行数互换）；
///   5 改名（域 5 域名改幽灵）；
///   6 多行（追加幽灵域行）；
///   7 批次改归属（批 30 从域 5 改挂域 6，动冻结账）；
///   8 全表漂移（八行条数全 +20）；
///   9 尾行漂移（域 7 行数 -1）。
pub fn inject_drift(root: &RootLedger, led: &FrozenLedger, kind: usize) -> (RootLedger, FrozenLedger) {
    let mut r = root.clone();
    let mut l = led.clone();
    match kind % 10 {
        0 => {
            if let Some(row) = r.rows.get_mut(0) {
                row.batches += 1;
            }
        }
        1 => {
            if let Some(row) = r.rows.get_mut(1) {
                row.entries = row.entries.saturating_sub(1);
            }
        }
        2 => {
            if let Some(row) = r.rows.get_mut(2) {
                row.lines += 7;
            }
        }
        3 => {
            let mut keep: Vec<RootRow> = Vec::new();
            let mut i = 0usize;
            while i < r.rows.len() {
                if i != 3 {
                    if let Some(row) = r.rows.get(i) {
                        keep.push(row.clone());
                    }
                }
                i += 1;
            }
            r.rows = keep;
        }
        4 => {
            let a = match r.rows.get(4) {
                Some(x) => x.lines,
                None => 0,
            };
            let b = match r.rows.get(5) {
                Some(x) => x.lines,
                None => 0,
            };
            if let Some(row) = r.rows.get_mut(4) {
                row.lines = b;
            }
            if let Some(row) = r.rows.get_mut(5) {
                row.lines = a;
            }
        }
        5 => {
            if let Some(row) = r.rows.get_mut(5) {
                row.domain = String::from("幽灵域");
            }
        }
        6 => {
            r.rows.push(RootRow {
                domain: String::from("幽灵域"),
                batches: 1,
                entries: PER_BATCH as u32,
                lines: LINE_BASE * PER_BATCH as u32,
            });
        }
        7 => {
            // 批 30（下标 29，原属域 5）改挂域 6：归并值随之漂移。
            if let Some(bt) = l.batches.get_mut(29) {
                bt.domain_idx = 6;
            }
        }
        8 => {
            let mut i = 0usize;
            while i < r.rows.len() {
                if let Some(row) = r.rows.get_mut(i) {
                    row.entries += PER_BATCH as u32;
                }
                i += 1;
            }
        }
        _ => {
            if let Some(row) = r.rows.get_mut(7) {
                row.lines = row.lines.saturating_sub(1);
            }
        }
    }
    (r, l)
}

// ── 注入检出账 ──────────────────────────────────────────────────
/// 漂移注入检出账： tries/caught + 漏抓路号。
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的漂移路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮先验黄金双账（对回必须通过，防注入器污染基准），
/// 再注入一路漂移，对回器必须检出。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut out = InjectLedger::new();
    let led0 = build_frozen_ledger(EPOCH);
    let root0 = build_root_ledger(EPOCH);
    let mut r = 0u32;
    while r < rounds {
        let mut led = led0.clone();
        let _ = freeze_ledger(&mut led);
        let acc = domain_rollup(&led)?;
        reconcile(&root0, &acc)?;
        let kind = (r % 10) as usize;
        let (rd, ld) = inject_drift(&root0, &led, kind);
        let accd = domain_rollup(&ld)?;
        out.record(kind, reconcile(&rd, &accd).is_err());
        r += 1;
    }
    Ok(out)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：建双账→冻结→归并→对回 24 对→连续性→四拒探→注入 10/10。
/// 返回 (正旗数, 拒旗数, 对回对数)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = build_frozen_ledger(EPOCH);
    let root = build_root_ledger(EPOCH);
    let mut eng = RootSync::new(EPOCH);
    // 冻结 + 归并 + 对回。
    eng.freeze(&mut led)?;
    let acc = domain_rollup(&led)?;
    let rep = eng.reconcile_run(&root, &led)?;
    // 连续性 + 总量对平。
    let cont = batch_continuity(&led);
    let total_ok = {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < acc.len() {
            if let Some(a) = acc.get(i) {
                t += a.entries;
            }
            i += 1;
        }
        t == TOTAL_ENTRIES as u32
    };
    // 四拒探：未冻结对回 / 状态倒退 / 空根账 / 批数漂移。
    let mut eng2 = RootSync::new(EPOCH);
    let draft_rej = eng2.reconcile_run(&root, &led).is_err();
    let back_rej = eng.advance(SyncState::Drafting).is_err();
    let empty = RootLedger { epoch: EPOCH, rows: Vec::new() };
    let empty_rej = reconcile(&empty, &acc).is_err();
    let (rd, ld) = inject_drift(&root, &led, 0);
    let accd = domain_rollup(&ld)?;
    let drift_rej = reconcile(&rd, &accd).is_err();
    // 注入 10 路全检出。
    let inj = inject_rounds(DRIFT_ROUNDS)?;
    let inj_ok = inj.all_caught();
    let flags = (rep.pairs_checked == PAIRS_TOTAL) as u32
        + cont as u32
        + total_ok as u32
        + inj_ok as u32;
    let rejects = draft_rej as u32 + back_rej as u32 + empty_rej as u32 + drift_rej as u32;
    Ok((flags, rejects, rep.pairs_checked))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _)) => Ok(()),
        _ => Err(SyncCode::NotReproducible.val()),
    }
}

/// 序 B：漂移注入 10 路全检出。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(DRIFT_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(SyncCode::BatchCountDrift.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72B00 && SyncCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = BATCH_N == 40
        && PER_BATCH == 20
        && TOTAL_ENTRIES == 800
        && DOMAIN_N == 8
        && BATCHES_PER_DOMAIN == 5
        && EPOCH == 3
        && DRIFT_ROUNDS == 10
        && PAIRS_TOTAL == 24
        && in_window;
    if ok {
        Ok(())
    } else {
        Err(SyncCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SyncCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SyncCode::ALL.len() {
        let v = SyncCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 对回幂等探针（同账重跑同报告） ──────────────────────────────
pub fn reconcile_idempotent() -> bool {
    let mut led = build_frozen_ledger(EPOCH);
    let _ = freeze_ledger(&mut led);
    let root = build_root_ledger(EPOCH);
    let acc = match domain_rollup(&led) {
        Ok(a) => a,
        Err(_) => return false,
    };
    let a = match reconcile(&root, &acc) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let b = match reconcile(&root, &acc) {
        Ok(x) => x,
        Err(_) => return false,
    };
    a == b && a.pairs_checked == PAIRS_TOTAL
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut led = build_frozen_ledger(EPOCH);
    let root = build_root_ledger(EPOCH);
    let mut eng = RootSync::new(EPOCH);
    eng.freeze(&mut led)?;
    let rep = eng.reconcile_run(&root, &led)?;
    let inj = inject_rounds(DRIFT_ROUNDS)?;
    Ok(format!(
        "状态=已完成 pairs={} domains={} drift={}/{} batches={}",
        rep.pairs_checked, rep.domains_checked, inj.caught, inj.tries, BATCH_N
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, p)) => (*f == 4 && *r == 4, format!("flags={} rejects={} pairs={}", f, r, p)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-根台账对回全过", gp, gd));
    // 黄金双账准备（后续分项判据共用）。
    let mut led = build_frozen_ledger(EPOCH);
    let root = build_root_ledger(EPOCH);
    let _ = freeze_ledger(&mut led);
    let acc = match domain_rollup(&led) {
        Ok(a) => a,
        Err(_) => Vec::new(),
    };
    // J1 三对逐项：批数/条数/行数各 8 域全对。
    let mut b_ok = true;
    let mut e_ok = true;
    let mut l_ok = true;
    let mut d = 0usize;
    while d < DOMAIN_N && d < acc.len() && d < root.rows.len() {
        let row = match root.rows.get(d) {
            Some(x) => x,
            None => break,
        };
        let a = match acc.get(d) {
            Some(x) => x,
            None => break,
        };
        if row.batches != a.batches {
            b_ok = false;
        }
        if row.entries != a.entries {
            e_ok = false;
        }
        if row.lines != a.lines {
            l_ok = false;
        }
        d += 1;
    }
    v.push(("J1-批数八域对平", b_ok, format!("域数={} 每域批={}", DOMAIN_N, BATCHES_PER_DOMAIN)));
    v.push(("J1-条数八域对平", e_ok, format!("总条数={} 每域条={}", TOTAL_ENTRIES, BATCHES_PER_DOMAIN * PER_BATCH)));
    v.push(("J1-行数八域对平", l_ok, format!("行权值=基线{}+下标%7", LINE_BASE)));
    // J1 漂移注入 10/10。
    let inj = inject_rounds(DRIFT_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-漂移注入10/10", ip, id));
    // J1 批次区间连续（无洞无重叠）。
    v.push(("J1-批次区间连续", batch_continuity(&led), format!("B01–B{} 逐批首条号/条数/域归属对", BATCH_N)));
    // J1 总量对平：归并条数合计 = 800。
    let total_ok = {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < acc.len() {
            if let Some(a) = acc.get(i) {
                t += a.entries;
            }
            i += 1;
        }
        t == TOTAL_ENTRIES as u32
    };
    v.push(("J1-总量对平800条", total_ok, format!("归并合计={} 应={}", TOTAL_ENTRIES, TOTAL_ENTRIES)));
    // J2 状态机单调守卫：推进/平移/倒退。
    let mut eng = RootSync::new(EPOCH);
    let m1 = eng.advance(SyncState::Frozen).is_ok();
    let m2 = eng.advance(SyncState::Frozen).is_err();
    let m3 = eng.advance(SyncState::Drafting).is_err();
    let m4 = eng.advance(SyncState::Reconciled).is_ok();
    v.push((
        "J2-状态机单调守卫",
        m1 && m2 && m3 && m4,
        format!("进={} 平拒={} 倒拒={} 终进={}", m1, m2, m3, m4),
    ));
    // J2 未冻结对回拒 + 空根账拒。
    let mut eng2 = RootSync::new(EPOCH);
    let draft_rej = eng2.reconcile_run(&root, &led).is_err();
    let empty = RootLedger { epoch: EPOCH, rows: Vec::new() };
    let empty_rej = reconcile(&empty, &acc).is_err();
    v.push(("J2-未冻结对回拒", draft_rej, String::from("草稿态对回一律拒，先冻结再对回")));
    v.push(("J2-空根账拒", empty_rej, String::from("零行根账按行缺失拒，补齐 §三 A3 八行")));
    // J2 对回幂等。
    v.push(("J2-对回幂等", reconcile_idempotent(), String::from("同账重跑同报告")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72B00 窗口")));
    v
}

pub fn run_unx2226_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2226 冻结账-根台账对回（§三 A3 行对表）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
