//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1665 · 放置策略总纲（唤醒放置决策树） · 0x723C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B04）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1665-J1 放置决策树冻结 v1（空闲核→缓存亲和→
//!     唤醒者跟随→回落本核四级），每级落账可回放，树结构与账目一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 树冻结 v1 四级线序：L0 空闲核 → L1 缓存亲和（同 LLC、排除唤醒
//!     者本核）→ L2 唤醒者跟随（wake_ok 旗标）→ L3 回落本核（必达）；
//!   · 逐级游标 cursor 单调：跳级/回退评估显性拒（LevelOrder）；
//!   · 每级落账：passed_mask 逐位累积；回放 = 同上下文按落账视图重走
//!     树逐项全等（LogReplay），树结构一致审计（PickInvalid）；
//!   · 账本状态机 Unfrozen→Frozen→Sealed 单调推进，越态放置显性拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x723C0，VX3-1103 放置树窗口独占）。
pub const SEG_BASE: u32 = 0x723C0;

/// 决策树版本（冻结 v1）。
pub const TREE_VERSION: u32 = 1;
/// 决策树级数（冻结 4）。
pub const TREE_LEVELS: usize = 4;
/// 放置账核容量上限（冻结 64）。
pub const TREE_CORE_CAP: usize = 64;
/// 放置账目容量上限（防账面膨胀，冻结 256）。
pub const TREE_LOG_CAP: usize = 256;
/// 真机档核数（冻结 4）。
pub const TREE_CPUS: usize = 4;
/// 放行级落账标记（该级评估过但未命中）。
pub const PASS_MARK: u32 = u32::MAX;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TreeCode {
    /// 核号越界（home/waker/选中核不在注册表）
    CoreOver,
    /// 树版本失配（非冻结 v1）
    TreeVersion,
    /// 级序破坏（跳级/回退评估，游标单调被破坏）
    LevelOrder,
    /// 非法选中（选中核不满足所在级谓词，账目被篡改面）
    PickInvalid,
    /// 账目回放失配（同上下文重走树结果不同）
    LogReplay,
    /// 账目为空（未放置即要求回放/导出）
    LogEmpty,
    /// 状态/上下文非法（未冻结即放置 / 封账后放置）
    CtxBad,
    /// 双跑不一致
    NotReproducible,
}

impl TreeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TreeCode::CoreOver => "核号越界：home/waker/选中核先注册再放置",
            TreeCode::TreeVersion => "树版本失配：只认冻结 v1，版本被改即拒",
            TreeCode::LevelOrder => "级序破坏：L0→L1→L2→L3 逐级游标单调，跳级/回退即拒",
            TreeCode::PickInvalid => "非法选中：选中核不满足该级谓词，查账目是否被篡改",
            TreeCode::LogReplay => "回放失配：同上下文重走树必须逐项全等，查账目/视图漂移",
            TreeCode::LogEmpty => "账目为空：先放置落账再回放/导出",
            TreeCode::CtxBad => "状态非法：Unfrozen 不可放置、Sealed 只读，先 freeze",
            TreeCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [TreeCode; 8] = [
        TreeCode::CoreOver,
        TreeCode::TreeVersion,
        TreeCode::LevelOrder,
        TreeCode::PickInvalid,
        TreeCode::LogReplay,
        TreeCode::LogEmpty,
        TreeCode::CtxBad,
        TreeCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TreeCode::ALL.len() {
        if TreeCode::ALL[i].val() == code {
            return TreeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x723C0 段闭集内"
}

// ── 冻结树结构（v1 四级线序，名字即账目键） ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TreeLevel {
    /// L0：空闲核优先。
    Idle,
    /// L1：缓存亲和（与唤醒者同 LLC，排除唤醒者本核）。
    CacheAffine,
    /// L2：唤醒者跟随（唤醒核带 wake_ok 旗标才可落）。
    WakeFollow,
    /// L3：回落本核（home 兜底，必达）。
    HomeFall,
}

impl TreeLevel {
    pub const ALL: [TreeLevel; 4] = [
        TreeLevel::Idle,
        TreeLevel::CacheAffine,
        TreeLevel::WakeFollow,
        TreeLevel::HomeFall,
    ];
    pub fn rank(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            TreeLevel::Idle => "空闲核",
            TreeLevel::CacheAffine => "缓存亲和",
            TreeLevel::WakeFollow => "唤醒者跟随",
            TreeLevel::HomeFall => "回落本核",
        }
    }
}

// ── 账本状态机（单调推进，倒退显性拒） ───────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TreeState {
    /// 未冻结：树在起草，不可放置。
    Unfrozen,
    /// 已冻结：可放置落账、可回放。
    Frozen,
    /// 已封账：回放复核通过的只读快照。
    Sealed,
}

impl TreeState {
    /// 单调序值：Unfrozen=0 < Frozen=1 < Sealed=2。
    pub fn rank(self) -> u8 {
        match self {
            TreeState::Unfrozen => 0,
            TreeState::Frozen => 1,
            TreeState::Sealed => 2,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: TreeState) -> bool {
        to.rank() > self.rank()
    }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 核视图：放置谓词的输入面（idle→L0，llc→L1，wake_ok→L2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuRow {
    pub core: u32,
    pub idle: bool,
    pub llc: u32,
    pub wake_ok: bool,
}

/// 放置上下文：任务本核 + 唤醒者核。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskCtx {
    pub tid: u32,
    /// 本核（上次运行核，L3 兜底目标）。
    pub home: u32,
    /// 唤醒者核（L1/L2 依据）。
    pub waker: u32,
}

/// 单笔放置账：命中级 + 选中核 + 逐级放行掩码 + 上下文与视图快照
/// （视图快照随账落存：回放按落账时的视图重走树，免受视图漂移影响）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceLog {
    pub tid: u32,
    pub ctx: TaskCtx,
    /// 命中级 rank（0..=3）。
    pub level: u8,
    /// 选中核号。
    pub chosen: u32,
    /// 放行掩码：bit k = 第 k 级评估过未命中；bit level = 命中。
    pub passed_mask: u8,
    /// 落账时的核视图快照（回放准绳）。
    pub view: Vec<CpuRow>,
}

/// 放置决策树账本：视图 + 游标 + 账目。
#[derive(Clone, Debug)]
pub struct TreeBook {
    pub state: TreeState,
    pub version: u32,
    pub cpus: Vec<CpuRow>,
    /// 逐级评估游标（0..=TREE_LEVELS，单调）。
    pub cursor: usize,
    /// 当前决策的放行掩码（place 内逐级累积）。
    pub passed_mask: u8,
    pub logs: Vec<PlaceLog>,
}

impl TreeBook {
    /// 建账本并注册 0..cpus 核视图（初始 Unfrozen）。
    pub fn new(rows: Vec<CpuRow>) -> TreeBook {
        let mut trimmed = Vec::new();
        let mut i = 0;
        while i < rows.len() && i < TREE_CORE_CAP {
            if let Some(r) = rows.get(i) {
                trimmed.push(*r);
            }
            i += 1;
        }
        TreeBook {
            state: TreeState::Unfrozen,
            version: TREE_VERSION,
            cpus: trimmed,
            cursor: 0,
            passed_mask: 0,
            logs: Vec::new(),
        }
    }

    /// 按核号定位视图下标（任意视图切片通用）。
    pub fn cpu_index_in(cpus: &[CpuRow], core: u32) -> Option<usize> {
        let mut i = 0;
        while i < cpus.len() {
            if let Some(r) = cpus.get(i) {
                if r.core == core {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按核号定位本账视图下标。
    pub fn cpu_index(&self, core: u32) -> Option<usize> {
        TreeBook::cpu_index_in(&self.cpus, core)
    }

    /// 核视图读取。
    pub fn row(&self, core: u32) -> Option<CpuRow> {
        let i = self.cpu_index(core)?;
        match self.cpus.get(i) {
            Some(r) => Some(*r),
            None => None,
        }
    }

    /// 冻结树（版本 v1 闸 + 非空闸 + 单调推进）。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.version != TREE_VERSION {
            return Err(TreeCode::TreeVersion.val());
        }
        if self.cpus.is_empty() {
            return Err(TreeCode::CtxBad.val());
        }
        if !self.state.can_advance(TreeState::Frozen) {
            return Err(TreeCode::CtxBad.val());
        }
        self.state = TreeState::Frozen;
        Ok(())
    }

    /// 单级评估：级序闸（游标单调）→ 谓词求值 → 放行落账。
    /// 命中返回 Some(核号)，未命中返回 None。
    pub fn eval_level(&mut self, level: usize, ctx: &TaskCtx) -> Result<Option<u32>, u32> {
        if self.version != TREE_VERSION {
            return Err(TreeCode::TreeVersion.val());
        }
        if level != self.cursor {
            return Err(TreeCode::LevelOrder.val());
        }
        if level >= TREE_LEVELS {
            return Err(TreeCode::LevelOrder.val());
        }
        if self.cpu_index(ctx.home).is_none() || self.cpu_index(ctx.waker).is_none() {
            return Err(TreeCode::CoreOver.val());
        }
        let pick = TreeBook::level_pick(level, ctx, &self.cpus);
        self.passed_mask |= 1u8 << level;
        self.cursor += 1;
        Ok(pick)
    }

    /// 按核号取视图行（任意切片通用）。
    pub fn row_of(cpus: &[CpuRow], core: u32) -> Option<CpuRow> {
        let mut i = 0;
        while i < cpus.len() {
            if let Some(r) = cpus.get(i) {
                if r.core == core {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 级谓词求值（纯函数）：按冻结线序扫描注册视图。
    pub fn level_pick(level: usize, ctx: &TaskCtx, cpus: &[CpuRow]) -> Option<u32> {
        match level {
            // L0 空闲核：注册序第一个 idle 核。
            0 => {
                let mut i = 0;
                while i < cpus.len() {
                    if let Some(r) = cpus.get(i) {
                        if r.idle {
                            return Some(r.core);
                        }
                    }
                    i += 1;
                }
                None
            }
            // L1 缓存亲和：与唤醒者同 LLC 且非唤醒者本核的第一个核。
            1 => {
                let wl = match TreeBook::row_of(cpus, ctx.waker) { Some(r) => r.llc, None => return None };
                let mut j = 0;
                while j < cpus.len() {
                    if let Some(r) = cpus.get(j) {
                        if r.llc == wl && r.core != ctx.waker {
                            return Some(r.core);
                        }
                    }
                    j += 1;
                }
                None
            }
            // L2 唤醒者跟随：唤醒核 wake_ok 才可落。
            2 => match TreeBook::row_of(cpus, ctx.waker) {
                Some(r) if r.wake_ok => Some(r.core),
                _ => None,
            },
            // L3 回落本核：home 必达。
            3 => Some(ctx.home),
            _ => None,
        }
    }

    /// 级谓词复核（账目一致面）：选中核必须真满足该级谓词。
    pub fn level_pick_valid(
        level: usize,
        ctx: &TaskCtx,
        chosen: u32,
        cpus: &[CpuRow],
    ) -> Result<(), u32> {
        match TreeBook::level_pick(level, ctx, cpus) {
            Some(c) if c == chosen => Ok(()),
            _ => Err(TreeCode::PickInvalid.val()),
        }
    }

    /// 放置一次：Frozen 态闸 → 重置游标 → 逐级评估 → 首中即落账。
    pub fn place(&mut self, ctx: &TaskCtx) -> Result<(u32, u8), u32> {
        if self.state != TreeState::Frozen {
            return Err(TreeCode::CtxBad.val());
        }
        if self.version != TREE_VERSION {
            return Err(TreeCode::TreeVersion.val());
        }
        if self.logs.len() >= TREE_LOG_CAP {
            return Err(TreeCode::LogEmpty.val());
        }
        self.cursor = 0;
        self.passed_mask = 0;
        let mut level = 0usize;
        let hit = loop {
            if level >= TREE_LEVELS {
                // 四级全空不现实（L3 必达），防御性收口：回落本核。
                break (ctx.home, TREE_LEVELS as u8 - 1);
            }
            let pick = self.eval_level(level, ctx)?;
            match pick {
                Some(c) => break (c, level as u8),
                None => level += 1,
            }
        };
        TreeBook::level_pick_valid(hit.1 as usize, ctx, hit.0, &self.cpus)?;
        let log = PlaceLog {
            tid: ctx.tid,
            ctx: *ctx,
            level: hit.1,
            chosen: hit.0,
            passed_mask: self.passed_mask,
            view: self.cpus.clone(),
        };
        self.logs.push(log);
        Ok(hit)
    }

    /// 账目回放：按落账视图快照同上下文重走树，逐项全等 + 防篡改。
    pub fn replay(&self) -> Result<(), u32> {
        if self.logs.is_empty() {
            return Err(TreeCode::LogEmpty.val());
        }
        let mut i = 0;
        while i < self.logs.len() {
            let lg = match self.logs.get(i) {
                Some(l) => l.clone(),
                None => return Err(TreeCode::LogEmpty.val()),
            };
            // 落账视图核合法性 + 命中级谓词复核（篡改即 PickInvalid）。
            if TreeBook::cpu_index_in(&lg.view, lg.ctx.home).is_none()
                || TreeBook::cpu_index_in(&lg.view, lg.ctx.waker).is_none()
            {
                return Err(TreeCode::CoreOver.val());
            }
            TreeBook::level_pick_valid(lg.level as usize, &lg.ctx, lg.chosen, &lg.view)?;
            // 重走树：游标从 0 逐级推进到命中级（按落账视图快照）。
            let mut cursor = 0usize;
            let mut mask = 0u8;
            let hit = loop {
                if cursor >= TREE_LEVELS {
                    break (lg.ctx.home, TREE_LEVELS as u8 - 1);
                }
                let pick = TreeBook::level_pick(cursor, &lg.ctx, &lg.view);
                mask |= 1u8 << cursor;
                match pick {
                    Some(c) => break (c, cursor as u8),
                    None => cursor += 1,
                }
            };
            if hit.1 != lg.level || hit.0 != lg.chosen || mask != lg.passed_mask {
                return Err(TreeCode::LogReplay.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 树结构一致审计：版本 v1 + 账目只引合法级 + 掩码从 0 连续 + 谓词成立。
    pub fn structure_ok(&self) -> bool {
        if self.version != TREE_VERSION {
            return false;
        }
        if self.logs.is_empty() {
            return false;
        }
        let mut i = 0;
        while i < self.logs.len() {
            let lg = match self.logs.get(i) {
                Some(l) => l.clone(),
                None => return false,
            };
            if (lg.level as usize) >= TREE_LEVELS {
                return false;
            }
            // 放行掩码从 bit0 连续到命中位、其后全零。
            let mut k = 0usize;
            while k < 8 {
                let want = k <= lg.level as usize;
                if (lg.passed_mask & (1u8 << k) != 0) != want {
                    return false;
                }
                k += 1;
            }
            // 落账视图核合法 + 命中级谓词成立（按落账视图快照）。
            let ctx_ok = TreeBook::cpu_index_in(&lg.view, lg.ctx.home).is_some()
                && TreeBook::cpu_index_in(&lg.view, lg.ctx.waker).is_some();
            let pick_ok = TreeBook::level_pick_valid(lg.level as usize, &lg.ctx, lg.chosen, &lg.view)
                .is_ok();
            if !ctx_ok || !pick_ok {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 封账：回放 + 结构审计双过才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        self.replay()?;
        if !self.structure_ok() {
            return Err(TreeCode::PickInvalid.val());
        }
        if !self.state.can_advance(TreeState::Sealed) {
            return Err(TreeCode::CtxBad.val());
        }
        self.state = TreeState::Sealed;
        Ok(())
    }

    /// 导出放置账摘要（非空 + 逐笔可对回）。
    pub fn export_tree(&self) -> Result<String, u32> {
        if self.logs.is_empty() {
            return Err(TreeCode::LogEmpty.val());
        }
        let mut s = String::from("place[");
        let mut i = 0;
        while i < self.logs.len() {
            let lg = match self.logs.get(i) {
                Some(l) => l.clone(),
                None => return Err(TreeCode::LogEmpty.val()),
            };
            if i > 0 { s.push(','); }
            let lname = match TreeLevel::ALL.get(lg.level as usize) { Some(lv) => lv.name(), None => "?" };
            s.push_str(&format!("t{}@{}:{}", lg.tid, lname, lg.chosen));
            i += 1;
        }
        s.push_str(&format!("] v{} n={}", self.version, self.logs.len()));
        if s.is_empty() {
            return Err(TreeCode::LogEmpty.val());
        }
        Ok(s)
    }
}

// ── 标准视图与剧本：四级各命中一次 ──────────────────────────────
/// 视组构造器：按 (idle, llc, wake_ok) 三表生成 0..4 核视图。
pub fn rows_of(idle: [bool; 4], llc: [u32; 4], wok: [bool; 4]) -> Vec<CpuRow> {
    let mut v: Vec<CpuRow> = Vec::new();
    let mut i = 0usize;
    while i < 4 {
        let (di, dl, dw) = (idle.get(i), llc.get(i), wok.get(i));
        v.push(CpuRow {
            core: i as u32,
            idle: match di { Some(x) => *x, None => false },
            llc: match dl { Some(x) => *x, None => 0 },
            wake_ok: match dw { Some(x) => *x, None => false },
        });
        i += 1;
    }
    v
}

/// 视组 A：c2 空闲 → L0 命中。
pub fn view_a() -> Vec<CpuRow> {
    rows_of([false, false, true, false], [0, 0, 1, 1], [true, true, true, true])
}

/// 视组 B：全忙、waker=1 与 c0 同 LLC → L1 命中 c0。
pub fn view_b() -> Vec<CpuRow> {
    rows_of([false; 4], [0, 0, 1, 1], [true, true, true, true])
}

/// 视组 C：waker=0 独占 LLC9 → L1 落空，wake_ok → L2 命中 c0。
pub fn view_c() -> Vec<CpuRow> {
    rows_of([false; 4], [9, 0, 1, 1], [true, true, true, true])
}

/// 视组 D：waker=0 独占 LLC9 且 wake_ok=false → L3 命中 home。
pub fn view_d() -> Vec<CpuRow> {
    rows_of([false; 4], [9, 0, 1, 1], [false, true, true, true])
}

/// 建冻结账本（给视组）。
pub fn frozen_book(rows: Vec<CpuRow>) -> Result<TreeBook, u32> {
    let mut b = TreeBook::new(rows);
    b.freeze()?;
    Ok(b)
}

// ── LCG（确定性伪随机，供注入轮步进） ───────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：版本路/级序路/越界路/回放路） ─────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 标准放置剧本：四笔放置，四级各命中一次（Frozen 态，账目 4 笔）。
pub fn run_place_script() -> Result<TreeBook, u32> {
    let mut b = frozen_book(view_a())?;
    b.place(&TaskCtx { tid: 11, home: 0, waker: 1 })?; // P1：L0 空闲核 c2。
    b.cpus = view_b();
    b.place(&TaskCtx { tid: 12, home: 2, waker: 1 })?; // P2：L1 缓存亲和 c0。
    b.cpus = view_c();
    b.place(&TaskCtx { tid: 13, home: 1, waker: 0 })?; // P3：L2 唤醒者跟随 c0。
    b.cpus = view_d();
    b.place(&TaskCtx { tid: 14, home: 3, waker: 0 })?; // P4：L3 回落本核 c3。
    Ok(b)
}

// ── 黄金全链：冻结→四级各一 placement→回放→结构审计→四拒→封账 ─
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut b = run_place_script()?;
    if b.logs.len() != 4 {
        return Err(TreeCode::LogEmpty.val());
    }
    // 命中级分布：L0/L1/L2/L3 各恰一次。
    let mut seen = [0u32; 4];
    let mut i = 0;
    while i < b.logs.len() {
        if let Some(lg) = b.logs.get(i) {
            if (lg.level as usize) < 4 { seen[lg.level as usize] += 1; }
        }
        i += 1;
    }
    if seen != [1, 1, 1, 1] { return Err(TreeCode::LevelOrder.val()); }
    b.replay()?;
    if !b.structure_ok() {
        return Err(TreeCode::PickInvalid.val());
    }
    let s = b.export_tree()?;
    if s.is_empty() {
        return Err(TreeCode::LogEmpty.val());
    }
    // 注入 1：home 越界（未注册核 9）必拒。
    let over_rej = b.place(&TaskCtx { tid: 90, home: 9, waker: 0 }).is_err();
    // 注入 2：树版本被改（v2）必拒。
    let mut bad = run_place_script()?;
    bad.version = 2;
    let ver_rej = bad.place(&TaskCtx { tid: 91, home: 0, waker: 1 }).is_err();
    // 注入 3：账目篡改（L0 原选 c2 空闲，改成 c0 忙）回放必抓。
    let mut bad2 = run_place_script()?;
    if let Some(lg) = bad2.logs.get_mut(0) {
        lg.chosen = 0;
    }
    let pick_rej = bad2.replay() == Err(TreeCode::PickInvalid.val());
    // 注入 4：账目级漂移（L1 账改成 L3 级）回放必抓。
    let mut bad3 = run_place_script()?;
    if let Some(lg) = bad3.logs.get_mut(1) {
        lg.level = 3;
    }
    let replay_rej = bad3.replay().is_err();
    b.seal()?;
    let flags = 1u32; // 导出摘要非空旗。
    let rejects = over_rej as u32 + ver_rej as u32 + pick_rej as u32 + replay_rej as u32;
    Ok((flags, rejects, b.logs.len() as u32))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链 + 双跑一致。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 4, 4)) => {
            let a = golden_run()?;
            let bb = golden_run()?;
            if a != bb {
                return Err(TreeCode::NotReproducible.val());
            }
            Ok(())
        }
        _ => Err(TreeCode::NotReproducible.val()),
    }
}

/// 序 B：注入面四拒（未冻结/级序跳级/重复冻结/封账后放置）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut b = TreeBook::new(view_a());
    let r1 = b.place(&TaskCtx { tid: 1, home: 0, waker: 1 }).is_err();
    let mut b = frozen_book(view_a())?;
    // 跳级：游标在 0 直接评估 L1。
    let r2 = b.eval_level(1, &TaskCtx { tid: 1, home: 0, waker: 1 }).is_err();
    let r3 = b.freeze().is_err();
    let _ = b.place(&TaskCtx { tid: 1, home: 0, waker: 1 });
    b.seal()?;
    let r4 = b.place(&TaskCtx { tid: 2, home: 0, waker: 1 }).is_err();
    match (r1, r2, r3, r4) {
        (true, true, true, true) => Ok(()),
        _ => Err(TreeCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（v1 / 四级 / 4 核 / 账目上限 256）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        TREE_VERSION == 1, TREE_LEVELS == 4, TREE_CPUS == 4,
        TREE_CORE_CAP == 64, TREE_LOG_CAP == 256,
        PASS_MARK == u32::MAX, TreeLevel::ALL.len() == 4,
    ) {
        (true, true, true, true, true, true, true) => Ok(()),
        _ => Err(TreeCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：版本路（版本改 2 后放置/冻结必拒）。
        let mut b1 = frozen_book(view_a())?;
        b1.version = 2;
        let a = b1.place(&TaskCtx { tid: 1, home: 0, waker: 1 }).is_err();
        let mut b1b = TreeBook::new(view_a());
        b1b.version = 2;
        let bb = b1b.freeze().is_err();
        led.record(0, a && bb);
        // 路 2：级序路（跳级 / 回退评估必拒）。
        let mut b2 = frozen_book(view_a())?;
        let _ = b2.eval_level(0, &TaskCtx { tid: 1, home: 0, waker: 1 });
        let back = b2.eval_level(0, &TaskCtx { tid: 1, home: 0, waker: 1 }).is_err();
        let mut b2b = frozen_book(view_a())?;
        let skip = b2b.eval_level(3, &TaskCtx { tid: 1, home: 0, waker: 1 }).is_err();
        led.record(1, back && skip);
        // 路 3：越界路（home/waker 未注册必拒）。
        let mut b3 = frozen_book(view_a())?;
        let a = b3.place(&TaskCtx { tid: 1, home: 42, waker: 0 }).is_err();
        let bb = b3.place(&TaskCtx { tid: 1, home: 0, waker: 42 }).is_err();
        led.record(2, a && bb);
        // 路 4：回放路（随机篡改选中核为账面外核 9，回放必抓）。
        let mut b4 = run_place_script()?;
        let pick = rng.below(4) as usize;
        let mut tampered = false;
        if let Some(lg) = b4.logs.get_mut(pick) {
            lg.chosen = 9;
            tampered = true;
        }
        let caught = match b4.replay() {
            Err(e) => tampered && (e == TreeCode::PickInvalid.val() || e == TreeCode::LogReplay.val()),
            Ok(_) => false,
        };
        led.record(3, caught);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TreeCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TreeCode::ALL.len() {
        let v = TreeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut b = run_place_script()?;
    b.seal()?;
    let s = b.export_tree()?;
    if s.is_empty() || b.state != TreeState::Sealed {
        return Err(TreeCode::LogEmpty.val());
    }
    Ok(format!("状态=已完成 {}", s))
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（1 旗 + 4 拒 + 4 笔账）。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((f, r, n)) => *f == 1 && *r == 4 && *n == 4,
            Err(_) => false,
        },
        match &g {
            Ok((f, r, n)) => format!("flags={} rejects={} logs={}", f, r, n),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1 四级分布：L0/L1/L2/L3 各命中一次。
    let spread = match run_place_script() {
        Ok(b) => {
            let mut seen = [0u32; 4];
            let mut i = 0;
            while i < b.logs.len() {
                if let Some(lg) = b.logs.get(i) {
                    if (lg.level as usize) < 4 { seen[lg.level as usize] += 1; }
                }
                i += 1;
            }
            seen == [1, 1, 1, 1]
        }
        Err(_) => false,
    };
    v.push((
        "J1-四级各中一次",
        spread,
        String::from("空闲核→缓存亲和→唤醒者跟随→回落本核"),
    ));
    // J1 每级落账可回放 + 树结构与账目一致。
    let consistent = match run_place_script() {
        Ok(b) => b.replay().is_ok() && b.structure_ok(),
        Err(_) => false,
    };
    v.push((
        "J1-树账一致可回放",
        consistent,
        String::from("回放逐项全等 + 掩码连续 + 谓词成立"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 段位守卫。
    v.push((
        "J4-段位守卫",
        segment_guard(),
        String::from("0x723C0 窗口"),
    ));
    v
}

pub fn run_unx1665_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1665 放置策略总纲（唤醒放置决策树）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
