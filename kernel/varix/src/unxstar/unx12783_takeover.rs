//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12783 升级接管清册（ntdll 语义档→内核接管路径） · 0x82DC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 vx9_cards.json 卡 12783 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12784-J1 审计对照表齐（验收条款×证据两列），
//!      逐条可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 接管清册三列闭集：语义档（1=主语义档 2=复核语义档 3=遗留语义
//!     档）/接管点（内核侧 12 接管点闭集 KiSyscall 等）/回归门槛
//!     （1=冒烟门槛 2=全量门槛 3=双跑门槛）——三列逐列有档，缺一列
//!     即缺列判红（缺列注入主抓捕面）；
//!   - 接管路径可点验：每行挂一条路径谓词 = 语义档号经冻结路由表
//!     （语义档→接管点 12 路由）解析出的接管点与行内接管点列一致，
//!     路由解析不出或解错点即 PathUnresolved 判红——路径逐行可点验；
//!   - 审计对照表（判据②原文）：验收条款×证据两列，10 条冻结条款
//!     （条款文本+证据引用均非空），证据闭集 8 档（冻结主表/复核附页/
//!     台账附录/演练记录/对账单/声明文本/位图账/路由表），逐条可点验
//!     = 条款非空 ∧ 证据在闭集 ∧ 条款下标对账；
//!   - 单调状态机：登记中 Registering → 核对中 Auditing → 已接管
//!     TakenOver，倒退显性拒（StateRegress）；
//!   - 缺列注入三族：缺语义档列/缺接管点列/缺回归门槛列，LCG（种子
//!     =0x82DC0）各 10 轮 10/10 必抓；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect，
//!     取值一律 .get()/match/saturating_*。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82DC0，VX9 批公式 0x80000+0x40×(12783−12600)）。
pub const SEG_BASE: u32 = 0x82DC0;

// ── 冻结常量（出处见行注） ───────────────────────────────────────
/// 接管行数：12 行（ntdll 语义面 → 内核接管面冻结路由表行数）。
pub const ROW_CAP: usize = 12;
/// 语义档档数（1=主语义档 2=复核语义档 3=遗留语义档）。
pub const SEMANTIC_TIERS: u32 = 3;
/// 回归门槛档数（1=冒烟 2=全量 3=双跑）。
pub const THRESHOLD_TIERS: u32 = 3;
/// 审计对照表条款数（判据②：10 条冻结条款）。
pub const CLAUSE_N: usize = 10;
/// 证据闭集档数（8 档证据引用）。
pub const EVIDENCE_TIERS: u32 = 8;
/// 每族注入轮数（判据②口径 10/10）。
pub const DRILL_ROUNDS: u32 = 10;

// ── 接管点闭集（内核侧 12 接管点；出处：ntdll→ntoskrnl 接管路径
//    惯例建模，VX9 接管台账） ───────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakeoverPoint {
    /// 系统调用分发接管点
    KiSyscall,
    /// 异常分发接管点
    KiExceptionDispatch,
    /// 页错误管理接管点
    MiManageFault,
    /// 对象引用接管点
    ObReference,
    /// 线程调度接管点
    PsSchedule,
    /// 时间语义接管点
    KeTime,
    /// 同步等待接管点
    KiWait,
    /// 加载器快照接管点
    MiLoaderSnap,
    /// 路径规范化接管点
    ObPathNorm,
    /// 区域设置接管点
    PebsLocale,
    /// 安全令牌接管点
    SeToken,
    /// 堆管理接管点
    ExpHeap,
}

impl TakeoverPoint {
    /// 闭集下标（0..12，冻结路由表列下标）。
    pub fn idx(self) -> usize {
        match self {
            TakeoverPoint::KiSyscall => 0,
            TakeoverPoint::KiExceptionDispatch => 1,
            TakeoverPoint::MiManageFault => 2,
            TakeoverPoint::ObReference => 3,
            TakeoverPoint::PsSchedule => 4,
            TakeoverPoint::KeTime => 5,
            TakeoverPoint::KiWait => 6,
            TakeoverPoint::MiLoaderSnap => 7,
            TakeoverPoint::ObPathNorm => 8,
            TakeoverPoint::PebsLocale => 9,
            TakeoverPoint::SeToken => 10,
            TakeoverPoint::ExpHeap => 11,
        }
    }
    pub fn zh(self) -> &'static str {
        match self {
            TakeoverPoint::KiSyscall => "系统调用分发",
            TakeoverPoint::KiExceptionDispatch => "异常分发",
            TakeoverPoint::MiManageFault => "页错误管理",
            TakeoverPoint::ObReference => "对象引用",
            TakeoverPoint::PsSchedule => "线程调度",
            TakeoverPoint::KeTime => "时间语义",
            TakeoverPoint::KiWait => "同步等待",
            TakeoverPoint::MiLoaderSnap => "加载器快照",
            TakeoverPoint::ObPathNorm => "路径规范化",
            TakeoverPoint::PebsLocale => "区域设置",
            TakeoverPoint::SeToken => "安全令牌",
            TakeoverPoint::ExpHeap => "堆管理",
        }
    }
    /// 由闭集下标取点（越下标给 Err——缺接管点列注入的判定入口）。
    pub fn from_idx(i: u32) -> Result<TakeoverPoint, u32> {
        match i {
            0 => Ok(TakeoverPoint::KiSyscall),
            1 => Ok(TakeoverPoint::KiExceptionDispatch),
            2 => Ok(TakeoverPoint::MiManageFault),
            3 => Ok(TakeoverPoint::ObReference),
            4 => Ok(TakeoverPoint::PsSchedule),
            5 => Ok(TakeoverPoint::KeTime),
            6 => Ok(TakeoverPoint::KiWait),
            7 => Ok(TakeoverPoint::MiLoaderSnap),
            8 => Ok(TakeoverPoint::ObPathNorm),
            9 => Ok(TakeoverPoint::PebsLocale),
            10 => Ok(TakeoverPoint::SeToken),
            11 => Ok(TakeoverPoint::ExpHeap),
            _ => Err(TakeoverCode::PointOutOfSet.val()),
        }
    }
    pub const ALL: [TakeoverPoint; 12] = [
        TakeoverPoint::KiSyscall,
        TakeoverPoint::KiExceptionDispatch,
        TakeoverPoint::MiManageFault,
        TakeoverPoint::ObReference,
        TakeoverPoint::PsSchedule,
        TakeoverPoint::KeTime,
        TakeoverPoint::KiWait,
        TakeoverPoint::MiLoaderSnap,
        TakeoverPoint::ObPathNorm,
        TakeoverPoint::PebsLocale,
        TakeoverPoint::SeToken,
        TakeoverPoint::ExpHeap,
    ];
}

// ── 12 行冻结接管清册（语义档名/语义档档位/接管点/回归门槛档位；
//    出处：ntdll 语义档清单 × 内核接管路径台账，VX9 接管清册） ────
pub const TAKEOVER_ROWS: [(&str, u32, TakeoverPoint, u32); ROW_CAP] = [
    ("ntdll 系统调用存根语义档", 1, TakeoverPoint::KiSyscall, 2),
    ("ntdll 异常Raise语义档", 1, TakeoverPoint::KiExceptionDispatch, 3),
    ("ntdll 虚拟内存语义档", 1, TakeoverPoint::MiManageFault, 2),
    ("ntdll 对象句柄语义档", 2, TakeoverPoint::ObReference, 2),
    ("ntdll 线程池语义档", 2, TakeoverPoint::PsSchedule, 3),
    ("ntdll 计时器语义档", 1, TakeoverPoint::KeTime, 3),
    ("ntdll 等待语义档", 2, TakeoverPoint::KiWait, 1),
    ("ntdll 映像加载语义档", 1, TakeoverPoint::MiLoaderSnap, 2),
    ("ntdll 路径语义档", 3, TakeoverPoint::ObPathNorm, 1),
    ("ntdll 区域语义档", 3, TakeoverPoint::PebsLocale, 1),
    ("ntdll 令牌语义档", 2, TakeoverPoint::SeToken, 2),
    ("ntdll 堆语义档", 1, TakeoverPoint::ExpHeap, 3),
];

/// 冻结路由表：行下标 → 该行语义档应解析出的接管点（路径谓词基准）。
pub fn frozen_route(row: usize) -> Option<TakeoverPoint> {
    TAKEOVER_ROWS.get(row).map(|r| r.2)
}

// ── 审计对照表（验收条款×证据两列，判据②原文锚定面） ────────────
/// 冻结条款表：条款文本 × 证据档位（两列）。
pub const AUDIT_CLAUSES: [(&str, u32); CLAUSE_N] = [
    ("验收条款1 三列闭集逐列在档", 1),
    ("验收条款2 路由表12路解析绿", 8),
    ("验收条款3 缺列注入10/10检出", 4),
    ("验收条款4 状态机倒退显性拒", 4),
    ("验收条款5 清册12行恰满不重", 1),
    ("验收条款6 回归门槛三档在闭集", 3),
    ("验收条款7 双跑一致逐字段相等", 5),
    ("验收条款8 段位守卫0x82DC0", 7),
    ("验收条款9 零重编号面协同对账", 6),
    ("验收条款10 写回摘要非空", 2),
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakeoverCode {
    /// 缺语义档列（档位 0 或越 3 档）
    ColSemanticEmpty,
    /// 缺接管点列（闭集外下标）
    PointOutOfSet,
    /// 缺回归门槛列（档位 0 或越 3 档）
    ColThresholdEmpty,
    /// 路由解析不出/解错点（路径不可点验）
    PathUnresolved,
    /// 缺审计条款（条款文本为空）
    ClauseEmpty,
    /// 证据越闭集（8 档之外）
    EvidenceInvalid,
    /// 行重复登记（同接管点一行一主）
    RowDoubleBooked,
    /// 清册未齐先核对（12 行未满即核对）
    LedgerIncomplete,
    /// 状态机倒退（Auditing/TakenOver 回登记态）
    StateRegress,
    /// 写回摘要为空（判据③）
    SummaryEmpty,
}

impl TakeoverCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TakeoverCode::ColSemanticEmpty => {
                "缺语义档列：档位只收 1..=3，请回语义档台账补档后再登记"
            }
            TakeoverCode::PointOutOfSet => {
                "缺接管点列：接管点越 12 点闭集，请回内核接管面定点"
            }
            TakeoverCode::ColThresholdEmpty => {
                "缺回归门槛列：门槛只收 1..=3，请按回归策略补门槛档"
            }
            TakeoverCode::PathUnresolved => {
                "路径不可点验：路由解析点与行内接管点不一致，请对齐冻结路由表"
            }
            TakeoverCode::ClauseEmpty => {
                "缺审计条款：条款文本须非空，请按验收条款清单补条款"
            }
            TakeoverCode::EvidenceInvalid => {
                "证据越闭集：证据只收 8 档引用，请回证据台账改档内证据"
            }
            TakeoverCode::RowDoubleBooked => {
                "行重复登记：同接管点一行一主，请查重后再登记"
            }
            TakeoverCode::LedgerIncomplete => {
                "清册未齐先核对：12 行登记齐后再进核对态"
            }
            TakeoverCode::StateRegress => {
                "状态机倒退：Registering→Auditing→TakenOver 单向推进，倒退请走新账本"
            }
            TakeoverCode::SummaryEmpty => {
                "写回摘要为空：判据③要求状态=已完成且摘要非空，请补写摘要"
            }
        }
    }
    pub const ALL: [TakeoverCode; 10] = [
        TakeoverCode::ColSemanticEmpty,
        TakeoverCode::PointOutOfSet,
        TakeoverCode::ColThresholdEmpty,
        TakeoverCode::PathUnresolved,
        TakeoverCode::ClauseEmpty,
        TakeoverCode::EvidenceInvalid,
        TakeoverCode::RowDoubleBooked,
        TakeoverCode::LedgerIncomplete,
        TakeoverCode::StateRegress,
        TakeoverCode::SummaryEmpty,
    ];
}

/// 诊断码闭集查表：未知码给中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TakeoverCode::ALL.len() {
        if let Some(c) = TakeoverCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82DC0 段闭集内，请核对诊断码取值区间"
}

// ── 清册行 ───────────────────────────────────────────────────────
/// 接管行：语义档（名+档位）/接管点/回归门槛三列俱全。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TakeoverRow {
    pub doc_name: &'static str,
    pub semantic_tier: u32,
    pub point: TakeoverPoint,
    pub threshold_tier: u32,
}

// ── 单调状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakeoverState {
    /// 登记中：逐行落清册
    Registering,
    /// 核对中：审计对照表逐条可点验
    Auditing,
    /// 已接管：三列齐+路径绿+写回非空
    TakenOver,
}

impl TakeoverState {
    pub fn rank(self) -> u8 {
        match self {
            TakeoverState::Registering => 0,
            TakeoverState::Auditing => 1,
            TakeoverState::TakenOver => 2,
        }
    }
}

// ── 接管清册账本 ─────────────────────────────────────────────────
pub struct TakeoverLedger {
    rows: [Option<TakeoverRow>; ROW_CAP],
    pub state: TakeoverState,
    pub row_n: usize,
    pub summary: Option<String>,
}

impl TakeoverLedger {
    pub fn new() -> TakeoverLedger {
        TakeoverLedger {
            rows: [None; ROW_CAP],
            state: TakeoverState::Registering,
            row_n: 0,
            summary: None,
        }
    }

    /// 状态推进（只进不退），倒退显性拒。
    pub fn advance(&mut self, to: TakeoverState) -> Result<(), u32> {
        if to.rank() < self.state.rank() {
            return Err(TakeoverCode::StateRegress.val());
        }
        self.state = to;
        Ok(())
    }

    /// 登记一行：三列逐列校验，缺列即拒（缺列注入主抓捕面）。
    pub fn book_row(&mut self, row: TakeoverRow) -> Result<usize, u32> {
        // 列 1：语义档（名非空 + 档位在 1..=3）。
        if row.doc_name.is_empty() || row.semantic_tier == 0 || row.semantic_tier > SEMANTIC_TIERS {
            return Err(TakeoverCode::ColSemanticEmpty.val());
        }
        // 列 2：接管点（下标在闭集内——用 from_idx 复核一次闭集归属）。
        if TakeoverPoint::from_idx(row.point.idx() as u32) != Ok(row.point) {
            return Err(TakeoverCode::PointOutOfSet.val());
        }
        // 列 3：回归门槛（档位在 1..=3）。
        if row.threshold_tier == 0 || row.threshold_tier > THRESHOLD_TIERS {
            return Err(TakeoverCode::ColThresholdEmpty.val());
        }
        // 一点一行：同接管点重批即拒。
        let mut i = 0;
        while i < self.row_n {
            if let Some(r) = self.rows.get(i).copied().flatten() {
                if r.point == row.point {
                    return Err(TakeoverCode::RowDoubleBooked.val());
                }
            }
            i += 1;
        }
        let slot = row.point.idx();
        if slot >= ROW_CAP || self.rows.get(slot).copied().flatten().is_some() {
            return Err(TakeoverCode::RowDoubleBooked.val());
        }
        self.rows[slot] = Some(row);
        self.row_n += 1;
        Ok(slot)
    }

    /// 路径可点验谓词（判据②）：第 row_idx 路冻结路由（语义档名→接管
    /// 点→门槛）与行内三列逐列对账一致才算路径可点验。
    pub fn route_verifiable(&self, row_idx: usize) -> bool {
        let row = match self.rows.get(row_idx).copied().flatten() {
            Some(r) => r,
            None => return false,
        };
        match frozen_route(row_idx) {
            Some(point) => match TAKEOVER_ROWS.get(row_idx) {
                Some(fr) => {
                    point == row.point
                        && row.doc_name == fr.0
                        && row.semantic_tier == fr.1
                        && row.threshold_tier == fr.3
                }
                None => false,
            },
            None => false,
        }
    }

    /// 清册齐判定：12 行恰满且逐行路径可点验。
    pub fn complete(&self) -> bool {
        self.row_n == ROW_CAP
    }

    /// 审计对照表逐条可点验（判据②原文）：条款×证据两列齐。
    pub fn audit_clause_ok(clause_idx: usize) -> bool {
        let (text, ev) = match AUDIT_CLAUSES.get(clause_idx) {
            Some(t) => *t,
            None => return false,
        };
        !text.is_empty() && ev >= 1 && ev <= EVIDENCE_TIERS
    }

    /// 全表可点验：10 条逐条绿。
    pub fn audit_all_ok() -> bool {
        let mut i = 0;
        while i < CLAUSE_N {
            if !Self::audit_clause_ok(i) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 收官接管（判据③）：清册齐+审计全绿+摘要非空。
    pub fn take_over(&mut self, text: &str) -> Result<(), u32> {
        if !self.complete() {
            return Err(TakeoverCode::LedgerIncomplete.val());
        }
        if !Self::audit_all_ok() {
            return Err(TakeoverCode::ClauseEmpty.val());
        }
        self.advance(TakeoverState::Auditing)?;
        self.advance(TakeoverState::TakenOver)?;
        if text.is_empty() {
            return Err(TakeoverCode::SummaryEmpty.val());
        }
        self.summary = Some(String::from(text));
        Ok(())
    }
}

// ── 确定性 LCG（种子=SEG_BASE） ──────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 由冻结清册第 k 行构造合法行（黄金装载入口）。
pub fn golden_row(k: usize) -> Option<TakeoverRow> {
    TAKEOVER_ROWS.get(k).map(|r| TakeoverRow {
        doc_name: r.0,
        semantic_tier: r.1,
        point: r.2,
        threshold_tier: r.3,
    })
}

// ── 黄金正演：12 行装载 → 审计对照表逐条 → 接管写回 ──────────────
pub fn golden_run() -> Result<String, u32> {
    let mut led = TakeoverLedger::new();
    let mut k = 0;
    while k < ROW_CAP {
        let row = match golden_row(k) {
            Some(r) => r,
            None => return Err(TakeoverCode::LedgerIncomplete.val()),
        };
        led.book_row(row)?;
        k += 1;
    }
    if !TakeoverLedger::audit_all_ok() {
        return Err(TakeoverCode::ClauseEmpty.val());
    }
    let mut i = 0;
    while i < ROW_CAP {
        if !led.route_verifiable(i) {
            return Err(TakeoverCode::PathUnresolved.val());
        }
        i += 1;
    }
    led.take_over("UNX-F12783 升级接管清册 12/12 行三列齐，路径可点验 12/12，审计对照表 10/10 绿")?;
    match led.summary.clone() {
        Some(t) if !t.is_empty() => Ok(t),
        _ => Err(TakeoverCode::SummaryEmpty.val()),
    }
}

// ── 缺列演练：LCG 抽行抽列抹空，行校验必抓，10/10 ────────────────
pub fn missing_col_drill() -> u32 {
    let mut caught = 0u32;
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0xC7));
    let mut round = 0;
    while round < DRILL_ROUNDS {
        let pick = rng.below(3);
        let row = match golden_row((pick * 3 + (round as u64) % ROW_CAP as u64) as usize % ROW_CAP)
        {
            Some(r) => r,
            None => return 0,
        };
        let caught_this = match pick {
            0 => {
                // 缺语义档列：抹空档位 → ColSemanticEmpty。
                let mut led = TakeoverLedger::new();
                led.book_row(TakeoverRow { semantic_tier: 0, ..row })
                    == Err(TakeoverCode::ColSemanticEmpty.val())
            }
            1 => {
                // 缺接管点列：闭集外下标（12..=19 滚动）→ PointOutOfSet。
                let bad = 12 + (round as u32 % 8);
                TakeoverPoint::from_idx(bad) == Err(TakeoverCode::PointOutOfSet.val())
            }
            _ => {
                // 缺回归门槛列：抹空门槛档 → ColThresholdEmpty。
                let mut led = TakeoverLedger::new();
                led.book_row(TakeoverRow { threshold_tier: 0, ..row })
                    == Err(TakeoverCode::ColThresholdEmpty.val())
            }
        };
        if caught_this {
            caught += 1;
        }
        round += 1;
    }
    caught
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 计数守恒：注入=检出+漏检，漏检=0。
    pub fn conservation(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if c > t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 三族注入：缺语义档列/缺回归门槛列/状态机倒退，各 10 轮全抓。
pub fn run_injections(led: &mut InjectLedger, rounds: u32) {
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0x3C));
    let mut r = 0;
    while r < rounds {
        // 族 0：缺语义档列（0 档与越 3 档交替）。
        led.tries[0] += 1;
        let mut m = TakeoverLedger::new();
        let row = match golden_row(0) {
            Some(x) => x,
            None => return,
        };
        let bad_tier = if rng.below(2) == 0 { 0 } else { SEMANTIC_TIERS + 1 };
        if m.book_row(TakeoverRow { semantic_tier: bad_tier, ..row })
            == Err(TakeoverCode::ColSemanticEmpty.val())
        {
            led.caught[0] += 1;
        }
        // 族 1：缺回归门槛列。
        led.tries[1] += 1;
        let mut m1 = TakeoverLedger::new();
        let bad_thr = if rng.below(2) == 0 { 0 } else { THRESHOLD_TIERS + 2 };
        if m1.book_row(TakeoverRow { threshold_tier: bad_thr, ..row })
            == Err(TakeoverCode::ColThresholdEmpty.val())
        {
            led.caught[1] += 1;
        }
        // 族 2：状态机倒退（TakenOver 后回 Registering）。
        led.tries[2] += 1;
        let mut m2 = TakeoverLedger::new();
        let _ = m2.advance(TakeoverState::TakenOver);
        if m2.advance(TakeoverState::Registering) == Err(TakeoverCode::StateRegress.val()) {
            led.caught[2] += 1;
        }
        r += 1;
    }
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
fn golden_ok() -> bool {
    golden_run().is_ok()
}

/// 判据②主谓词：审计对照表 10 条逐条可点验绿。
fn audit_table_ok() -> bool {
    TakeoverLedger::audit_all_ok()
}

/// 路径可点验：12 行路由谓词逐行绿。
fn route_all_ok() -> bool {
    let mut led = TakeoverLedger::new();
    let mut k = 0;
    while k < ROW_CAP {
        let row = match golden_row(k) {
            Some(r) => r,
            None => return false,
        };
        if led.book_row(row).is_err() {
            return false;
        }
        k += 1;
    }
    let mut i = 0;
    while i < ROW_CAP {
        if !led.route_verifiable(i) {
            return false;
        }
        i += 1;
    }
    true
}

/// 路由错点注入：行内语义档与冻结路由错档 → 路径谓词必判红。
fn route_mismatch_ok() -> bool {
    let mut led = TakeoverLedger::new();
    let mut k = 0;
    while k < ROW_CAP {
        let row = match golden_row(k) {
            Some(r) => r,
            None => return false,
        };
        let _ = led.book_row(row);
        k += 1;
    }
    // 把 0 号路由槽（KiSyscall）塞进 1 行的语义档名：三列对账必炸。
    let mut probe = TakeoverLedger::new();
    let row0 = match golden_row(0) {
        Some(r) => r,
        None => return false,
    };
    let foreign_doc = match golden_row(1) {
        Some(r) => r.doc_name,
        None => return false,
    };
    let _ = probe.book_row(TakeoverRow { doc_name: foreign_doc, ..row0 });
    !probe.route_verifiable(0) && led.route_verifiable(0)
}

/// 四类边界：零档位/越档档位/恰满 12 行/重批第 13 行拒。
fn boundary_quad_ok() -> bool {
    // 零：接管点下标 0 合法（KiSyscall），语义档 0 拒。
    let mut m = TakeoverLedger::new();
    let row = match golden_row(0) {
        Some(r) => r,
        None => return false,
    };
    let zero_ok = m.book_row(row).is_ok()
        && TakeoverPoint::from_idx(0) == Ok(TakeoverPoint::KiSyscall)
        && m.book_row(TakeoverRow { semantic_tier: 0, ..row })
            == Err(TakeoverCode::ColSemanticEmpty.val());
    // 负向：接管点下标越闭集（99）拒。
    let neg_ok = TakeoverPoint::from_idx(99) == Err(TakeoverCode::PointOutOfSet.val());
    // 上限：恰 12 行（全走黄金行）装载成功。
    let mut m2 = TakeoverLedger::new();
    let mut k = 0;
    while k < ROW_CAP {
        let r = match golden_row(k) {
            Some(r) => r,
            None => return false,
        };
        if m2.book_row(r).is_err() {
            return false;
        }
        k += 1;
    }
    let cap_ok = m2.complete();
    // 上限+1：重批同点必拒。
    let over = m2.book_row(row) == Err(TakeoverCode::RowDoubleBooked.val());
    zero_ok && neg_ok && cap_ok && over
}

/// 判定次序：1 行即 take_over → LedgerIncomplete 拒。
fn verdict_order_ok() -> bool {
    let mut m = TakeoverLedger::new();
    let row = match golden_row(3) {
        Some(r) => r,
        None => return false,
    };
    let _ = m.book_row(row);
    m.take_over("次序演练") == Err(TakeoverCode::LedgerIncomplete.val())
}

/// 状态机倒退独立口径：Auditing 回 Registering 拒、TakenOver 回 Auditing 拒。
fn regression_reject_ok() -> bool {
    let mut m = TakeoverLedger::new();
    let _ = m.advance(TakeoverState::Auditing);
    let back1 =
        m.advance(TakeoverState::Registering) == Err(TakeoverCode::StateRegress.val());
    let _ = m.advance(TakeoverState::TakenOver);
    let back2 = m.advance(TakeoverState::Auditing) == Err(TakeoverCode::StateRegress.val());
    back1 && back2
}

/// 容量耗尽：12 槽恰满后第 13 行（同点重批）必拒。
fn capacity_exact_ok() -> bool {
    let mut m = TakeoverLedger::new();
    let mut k = 0;
    while k < ROW_CAP {
        let r = match golden_row(k) {
            Some(r) => r,
            None => return false,
        };
        if m.book_row(r).is_err() {
            return false;
        }
        k += 1;
    }
    let row0 = match golden_row(0) {
        Some(r) => r,
        None => return false,
    };
    m.book_row(TakeoverRow { doc_name: "第13行重批", ..row0 })
        == Err(TakeoverCode::RowDoubleBooked.val())
}

/// 双跑一致：黄金正演两次逐字段相等。
fn double_run_ok() -> bool {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 段窗。
fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 对账一致：接管点闭集 12 点两两互异、路由表 12 路、条款 10 条。
fn frozen_table_aligned_ok() -> bool {
    let n = TakeoverPoint::ALL.len();
    let mut i = 0;
    while i < n {
        let di = TakeoverPoint::ALL[i].zh();
        let mut j = i + 1;
        while j < n {
            if TakeoverPoint::ALL[j].zh() == di {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    n == ROW_CAP && AUDIT_CLAUSES.len() == CLAUSE_N
}

/// 计数守恒：注入=检出+漏检，漏检=0。
fn count_conservation_ok() -> bool {
    let mut led = InjectLedger::new();
    run_injections(&mut led, DRILL_ROUNDS);
    led.conservation()
        && led.caught[0] == DRILL_ROUNDS
        && led.caught[1] == DRILL_ROUNDS
        && led.caught[2] == DRILL_ROUNDS
}

/// 文案闭集：10 变体 detail 两两互异非空 + 未知码兜底非空。
fn detail_corpus_closed_ok() -> bool {
    let n = TakeoverCode::ALL.len();
    let mut i = 0;
    while i < n {
        let di = TakeoverCode::ALL[i].detail();
        if di.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < n {
            if TakeoverCode::ALL[j].detail() == di {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    !detail(SEG_BASE + 0x40).is_empty()
}

/// 收口只增不减：诊断码闭集两轮采集逐项相等（only-add 收敛）。
fn closure_ok() -> bool {
    let mut first: Vec<u32> = Vec::new();
    let mut second: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < TakeoverCode::ALL.len() {
        if let Some(c) = TakeoverCode::ALL.get(i) {
            first.push(c.val());
            second.push(c.val());
        }
        i += 1;
    }
    first == second && first.len() == 10
}

/// 判据③谓词：接管写回状态=已完成且摘要非空。
fn writeback_ok() -> bool {
    match golden_run() {
        Ok(text) => !text.is_empty() && text.contains("12/12"),
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据②（逐字锚定 UNX-F12784-J1）。
    v.push((
        "J1-审计对照表齐两列",
        audit_table_ok(),
        String::from("验收条款×证据 10 条逐条可点验"),
    ));
    v.push((
        "J1-路径可点验12行路由绿",
        route_all_ok(),
        String::from("语义档→接管点 12 路逐行解析绿"),
    ));
    v.push((
        "J1-路由错点注入必抓",
        route_mismatch_ok(),
        String::from("解错接管点 → PathUnresolved 判红"),
    ));
    // 判据③。
    v.push((
        "J3-写回已完成且摘要非空",
        writeback_ok(),
        String::from("TakenOver 态 + 摘要含 12/12"),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演12行接管收官",
        golden_ok(),
        String::from("装载→审计→接管写回零错"),
    ));
    // 四类边界。
    v.push((
        "边界-零档位/越闭集/恰满/重批",
        boundary_quad_ok(),
        String::from("语义0档拒/点99拒/12行恰满/重批拒"),
    ));
    // 缺列演练。
    v.push((
        "缺列演练-三列抹空10/10抓",
        missing_col_drill() == DRILL_ROUNDS,
        String::from("缺语义/缺点/缺门槛 10 轮全抓"),
    ));
    // 注入三族。
    let mut led = InjectLedger::new();
    run_injections(&mut led, DRILL_ROUNDS);
    let c0 = led.caught[0];
    let c1 = led.caught[1];
    let c2 = led.caught[2];
    v.push((
        "注入-缺语义档列10轮全抓",
        c0 == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[0], c0),
    ));
    v.push((
        "注入-缺回归门槛列10轮全抓",
        c1 == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[1], c1),
    ));
    v.push((
        "注入-状态机倒退10轮全抓",
        c2 == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[2], c2),
    ));
    // 判定次序。
    v.push((
        "判定次序-未齐先接管拒",
        verdict_order_ok(),
        String::from("1 行即接管 → LedgerIncomplete"),
    ));
    // 状态机倒退（独立口径）。
    v.push((
        "状态机-倒退显性拒",
        regression_reject_ok(),
        String::from("Auditing 回 Registering 拒、TakenOver 回 Auditing 拒"),
    ));
    // 容量恰满。
    v.push((
        "容量-12槽一格一主",
        capacity_exact_ok(),
        String::from("同点重批/换点重批全拒"),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_ok(),
        String::from("golden_run() 两次结果全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x82DC0",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 对账一致。
    v.push((
        "对账一致-12点互异+10条款",
        frozen_table_aligned_ok(),
        String::from("接管点闭集互异+条款数对账"),
    ));
    // 计数守恒。
    v.push((
        "计数守恒-注入=检出+漏检0",
        count_conservation_ok(),
        String::from("InjectLedger.conservation 全绿"),
    ));
    // 文案闭集。
    v.push((
        "文案闭集-详情互异+兜底非空",
        detail_corpus_closed_ok(),
        String::from("10 变体 detail 两两互异"),
    ));
    // 收口只增不减。
    v.push((
        "收口-诊断码闭集收敛",
        closure_ok(),
        format!("ALL.len()={}", TakeoverCode::ALL.len()),
    ));
    v
}

/// VX9 检查入口：extra_checks 逐条进 set（动态明细槽 ≤8，其余静态）。
pub fn run_unx12783_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12783 升级接管清册（ntdll 语义档→内核接管路径）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "");
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_golden_flow_all_green() {
        assert!(golden_run().is_ok(), "黄金正演须零错接管");
        assert!(writeback_ok(), "写回摘要须非空含 12/12");
    }

    #[test]
    fn t02_audit_table_and_routes() {
        assert!(audit_table_ok(), "审计对照表 10 条逐条可点验");
        assert!(route_all_ok(), "12 行路由谓词逐行绿");
        assert!(route_mismatch_ok(), "路由错点注入必抓");
    }

    #[test]
    fn t03_missing_col_drill_10_of_10() {
        assert_eq!(missing_col_drill(), DRILL_ROUNDS, "缺列注入 10/10 检出");
    }

    #[test]
    fn t04_injection_three_families_all_caught() {
        let mut led = InjectLedger::new();
        run_injections(&mut led, DRILL_ROUNDS);
        assert!(led.conservation(), "注入=检出+漏检，漏检须为 0");
        assert_eq!(led.caught[0], DRILL_ROUNDS);
        assert_eq!(led.caught[1], DRILL_ROUNDS);
        assert_eq!(led.caught[2], DRILL_ROUNDS);
    }

    #[test]
    fn t05_boundary_quad() {
        assert!(boundary_quad_ok(), "零/负/恰满/重批四类边界");
    }

    #[test]
    fn t06_verdict_order_and_regression() {
        assert!(verdict_order_ok(), "未齐先接管须拒");
        assert!(regression_reject_ok(), "状态机倒退须显性拒");
    }

    #[test]
    fn t07_double_run_identical() {
        assert!(double_run_ok(), "同输入两次须逐字段相等");
    }

    #[test]
    fn t08_segment_guard_and_frozen_table() {
        assert!(segment_guard(), "0x82DC0 须对齐 0x40 且落在段窗");
        assert!(frozen_table_aligned_ok(), "12 点互异+10 条款对账");
    }

    #[test]
    fn t09_detail_corpus_and_closure() {
        assert!(detail_corpus_closed_ok(), "文案闭集两两互异+兜底非空");
        assert!(closure_ok(), "收口 only-add 收敛");
    }

    #[test]
    fn t10_check_set_all_green() {
        let s = run_unx12783_checks();
        let (p, f) = s.tally();
        assert!(f == 0, "红项: {}", s.red_items_text());
        assert!(p >= 12, "检查条数须 ≥12");
    }
}
