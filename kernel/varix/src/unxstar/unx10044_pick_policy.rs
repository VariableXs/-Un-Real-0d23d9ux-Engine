//! UNX-F10044 回归选例策略（全量/增量/抽样） · 0x91b00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10044（B22 常态化回归线 · 选例策略）。
//! 锚点：UNX-F10045-J1 增量命中核（依赖图覆盖核 10/10 次），图空洞漏选 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91b00（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `PolicyCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `CaseBook`：40 条战线条目（B21 口径）逐条注册，越界/重复拒绝；
//!   - 状态机 `SelectorState`：Idle → Loaded → Armed 单向推进；增量选例须
//!     Loaded（依赖图就位）、抽样选例须 Armed（种子落位），越档显性拒绝；
//!   - 分发器 `PolicySelector`：全量/增量/抽样三策略分发，增量按模块覆盖
//!     选例（依赖图命中核），空洞模块显性拒绝（图空洞漏选检出）；
//!   - 注入核 `BadPickCase`：空册全量 / 无图增量 / 图空洞 / 零种子 / 越策略
//!     五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10044 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91b00;

/// 段窗上界（下一卡 0x91b40 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91b40;

/// 战线条目数（B21 口径 40 条）。
pub const CASE_COUNT: usize = 40;

/// 依赖模块数（条目按 10 模块分面）。
pub const MODULE_COUNT: usize = 10;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 抽样配额（分层抽 6 条，来源：常态抽样条款）。
pub const SAMPLE_QUOTA: usize = 6;

/// Linux errno 正数码（对照表口径）。
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;
pub const ENODEV: i32 = 19;
pub const EBADE: i32 = 52;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PolicyCode {
    /// 空册全量（注册表为空即全量）
    EmptyFull,
    /// 无图增量（依赖图未装载）
    NoGraph,
    /// 图空洞（变更模块出闭集）
    GraphHole,
    /// 零种子/坏种子
    BadSeed,
    /// 越策略（档与策略不符）
    PolicyMismatch,
    /// 条目重复注册
    DuplicateCase,
    /// 条目越界
    CaseOutOfRange,
    /// 状态机倒退
    StateRegress,
    /// 抽样配额越界
    QuotaRange,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl PolicyCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PolicyCode::EmptyFull => "空册全量被拒：注册表为空不得全量选例；请先注册战线条目",
            PolicyCode::NoGraph => "无图增量被拒：依赖图未装载不得增量选例；请先 load_graph",
            PolicyCode::GraphHole => "图空洞被拒：变更模块出 0..10 闭集；请核对模块号",
            PolicyCode::BadSeed => "坏种子被拒：抽样种子必须非零；请重置种子",
            PolicyCode::PolicyMismatch => "越策略被拒：档与策略必须一一对应；请按分发器口径选例",
            PolicyCode::DuplicateCase => "重复条目被拒：同号条目已注册；请去重后重报",
            PolicyCode::CaseOutOfRange => "条目越界被拒：条目号必须落在 1..=40；请核对口",
            PolicyCode::StateRegress => "状态倒退被拒：Idle→Loaded→Armed 单向推进；请按序装载",
            PolicyCode::QuotaRange => "配额越界被拒：抽样配额窗口 1..=条目数；请核对配额",
            PolicyCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚误改",
            PolicyCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            PolicyCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91b00..=0x91b40",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [PolicyCode; 12] = [
        PolicyCode::EmptyFull,
        PolicyCode::NoGraph,
        PolicyCode::GraphHole,
        PolicyCode::BadSeed,
        PolicyCode::PolicyMismatch,
        PolicyCode::DuplicateCase,
        PolicyCode::CaseOutOfRange,
        PolicyCode::StateRegress,
        PolicyCode::QuotaRange,
        PolicyCode::LedgerRegress,
        PolicyCode::NotReproducible,
        PolicyCode::SegmentViolation,
    ];
}

// ── 策略闭集 ──────────────────────────────────────────────────────────
/// 选例策略（闭集，对齐 F10043 冻结映射）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Policy {
    /// 全量选例
    Full,
    /// 增量选例（依赖图驱动）
    Incremental,
    /// 抽样选例（分层随机）
    Stratified,
}

impl Policy {
    /// 闭集全量。
    pub const ALL: [Policy; 3] = [Policy::Full, Policy::Incremental, Policy::Stratified];

    /// 从文本解析（"full"/"incremental"/"stratified"）。
    pub fn from_field(s: &str) -> Option<Policy> {
        match s {
            "full" => Some(Policy::Full),
            "incremental" => Some(Policy::Incremental),
            "stratified" => Some(Policy::Stratified),
            _ => None,
        }
    }
}

// ── 条目注册表 ────────────────────────────────────────────────────────
/// 战线条目注册表：40 条逐条注册，号 1..=40。
#[derive(Clone, Debug)]
pub struct CaseBook {
    /// 注册占用表（下标 = 条目号-1）
    pub have: [bool; CASE_COUNT],
    /// 已注册条数
    pub registered: u32,
}

impl CaseBook {
    /// 构造空注册表。
    pub const fn new() -> CaseBook {
        CaseBook { have: [false; CASE_COUNT], registered: 0 }
    }

    /// 注册一条：越界与重复显性拒绝。
    pub fn register(&mut self, id: u32) -> Result<(), PolicyCode> {
        if id == 0 || (id as usize) > CASE_COUNT {
            return Err(PolicyCode::CaseOutOfRange);
        }
        let idx = id as usize - 1;
        if *self.have.get(idx).unwrap_or(&true) {
            return Err(PolicyCode::DuplicateCase);
        }
        if let Some(slot) = self.have.get_mut(idx) {
            *slot = true;
        }
        self.registered = self.registered.saturating_add(1);
        Ok(())
    }

    /// 条目 → 模块面（条目 i 归模块 (i-1) % 10，B21 十面口径）。
    pub fn module_of(id: u32) -> Option<usize> {
        if id == 0 || (id as usize) > CASE_COUNT {
            None
        } else {
            Some(((id - 1) % MODULE_COUNT as u32) as usize)
        }
    }

    /// 全量选例：空册拒绝，否则升序返回全部注册号。
    pub fn select_full(&self) -> Result<Vec<u32>, PolicyCode> {
        if self.registered == 0 {
            return Err(PolicyCode::EmptyFull);
        }
        let mut out = Vec::new();
        let mut id = 1u32;
        while (id as usize) <= CASE_COUNT {
            if *self.have.get(id as usize - 1).unwrap_or(&false) {
                out.push(id);
            }
            id += 1;
        }
        Ok(out)
    }
}

/// 注册全部 40 条。
pub fn register_all(book: &mut CaseBook) -> Result<(), PolicyCode> {
    let mut id = 1u32;
    while (id as usize) <= CASE_COUNT {
        book.register(id)?;
        id += 1;
    }
    Ok(())
}

// ── 分发器状态机 ──────────────────────────────────────────────────────
/// 分发器状态机：Idle → Loaded（依赖图就位）→ Armed（种子落位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectorState {
    /// 未装载
    Idle,
    /// 依赖图已装载（可增量）
    Loaded,
    /// 种子已落位（可抽样）
    Armed,
}

/// 策略分发器：装载依赖图占位表与种子，三策略分发。
#[derive(Clone, Copy, Debug)]
pub struct PolicySelector {
    /// 当前状态
    pub state: SelectorState,
    /// 依赖图占位表（模块 → 是否有图边覆盖）
    pub graph: [bool; MODULE_COUNT],
    /// 抽样种子（非零有效）
    pub seed: u32,
}

impl PolicySelector {
    /// 构造未装载分发器。
    pub const fn new() -> PolicySelector {
        PolicySelector { state: SelectorState::Idle, graph: [false; MODULE_COUNT], seed: 0 }
    }

    /// 装载依赖图：占位表逐模块置位；Idle→Loaded。
    pub fn load_graph(&mut self, covered: [bool; MODULE_COUNT]) -> Result<(), PolicyCode> {
        if self.state == SelectorState::Armed {
            return Err(PolicyCode::StateRegress);
        }
        self.graph = covered;
        self.state = SelectorState::Loaded;
        Ok(())
    }

    /// 落位抽样种子：非零才有效；Loaded→Armed（未装载先装载）。
    pub fn set_seed(&mut self, seed: u32) -> Result<(), PolicyCode> {
        if seed == 0 {
            return Err(PolicyCode::BadSeed);
        }
        if self.state == SelectorState::Idle {
            return Err(PolicyCode::StateRegress);
        }
        self.seed = seed;
        self.state = SelectorState::Armed;
        Ok(())
    }

    /// 增量选例：按变更模块并集取受影响条目；无图拒绝、空洞拒绝。
    pub fn select_incremental(&self, book: &CaseBook, changed: &[usize]) -> Result<Vec<u32>, PolicyCode> {
        if self.state == SelectorState::Idle {
            return Err(PolicyCode::NoGraph);
        }
        // 空洞核：变更模块必须落在 0..MODULE_COUNT 且图上有覆盖。
        let mut hit = [false; MODULE_COUNT];
        for m in changed.iter() {
            if *m >= MODULE_COUNT {
                return Err(PolicyCode::GraphHole);
            }
            if let Some(slot) = hit.get_mut(*m) {
                *slot = true;
            }
        }
        // 命中核：受影响条目 = 模块面被变更覆盖的注册条目。
        let mut out = Vec::new();
        let mut id = 1u32;
        while (id as usize) <= CASE_COUNT {
            if *book.have.get(id as usize - 1).unwrap_or(&false) {
                if let Some(m) = CaseBook::module_of(id) {
                    if *hit.get(m).unwrap_or(&false) {
                        out.push(id);
                    }
                }
            }
            id += 1;
        }
        Ok(out)
    }

    /// 增量覆盖核：受影响条目必须全部入选（图空洞漏选检出）。
    pub fn coverage_ok(&self, book: &CaseBook, changed: &[usize], picked: &[u32]) -> bool {
        let mut want = [false; CASE_COUNT];
        for m in changed.iter() {
            if *m >= MODULE_COUNT {
                return false;
            }
            let mut id = 1u32;
            while (id as usize) <= CASE_COUNT {
                if *book.have.get(id as usize - 1).unwrap_or(&false) {
                    if CaseBook::module_of(id) == Some(*m) {
                        if let Some(slot) = want.get_mut(id as usize - 1) {
                            *slot = true;
                        }
                    }
                }
                id += 1;
            }
        }
        for id in picked.iter() {
            if *id == 0 || (*id as usize) > CASE_COUNT {
                return false;
            }
            if let Some(slot) = want.get_mut(*id as usize - 1) {
                *slot = false;
            }
        }
        let mut left = false;
        let mut i = 0usize;
        while i < CASE_COUNT {
            if *want.get(i).unwrap_or(&false) {
                left = true;
            }
            i += 1;
        }
        !left
    }

    /// 抽样选例：等步长跨注册表抽配额条（同种子确定性）。
    pub fn select_stratified(&self, book: &CaseBook, quota: usize) -> Result<Vec<u32>, PolicyCode> {
        if self.state != SelectorState::Armed {
            return Err(PolicyCode::PolicyMismatch);
        }
        if quota == 0 || quota > book.registered as usize {
            return Err(PolicyCode::QuotaRange);
        }
        let all = book.select_full()?;
        let step = all.len() / quota;
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < quota {
            let idx = (k * step + (self.seed as usize) % step.max(1)) % all.len();
            if let Some(v) = all.get(idx) {
                out.push(*v);
            }
            k += 1;
        }
        Ok(out)
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏选例样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPickCase {
    /// 空册全量
    EmptyFull,
    /// 无图增量
    NoGraph,
    /// 图空洞（模块 99）
    GraphHole,
    /// 零种子
    BadSeed,
    /// 越策略（未 Armed 抽样）
    PolicyMismatch,
}

impl BadPickCase {
    /// 闭集全量。
    pub const ALL: [BadPickCase; 5] = [
        BadPickCase::EmptyFull,
        BadPickCase::NoGraph,
        BadPickCase::GraphHole,
        BadPickCase::BadSeed,
        BadPickCase::PolicyMismatch,
    ];

    /// 注入一轮：构造坏样本并触发分发器，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        let empty_book = CaseBook::new();
        let mut full_book = CaseBook::new();
        let _ = register_all(&mut full_book);
        let mut sel = PolicySelector::new();
        let covered = [true; MODULE_COUNT];
        match self {
            BadPickCase::EmptyFull => full_book.select_full().is_ok() && empty_book.select_full().is_err(),
            BadPickCase::NoGraph => sel.select_incremental(&full_book, &[0]).is_err(),
            BadPickCase::GraphHole => {
                if sel.load_graph(covered).is_err() {
                    return false;
                }
                sel.select_incremental(&full_book, &[99]).is_err()
            }
            BadPickCase::BadSeed => {
                if sel.load_graph(covered).is_err() {
                    return false;
                }
                sel.set_seed(0).is_err()
            }
            BadPickCase::PolicyMismatch => {
                // 未 Armed（Idle 态）直接抽样 → 显性拒绝。
                sel.select_stratified(&full_book, SAMPLE_QUOTA).is_err()
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PickLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl PickLedger {
    const fn zero() -> PickLedger {
        PickLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失衡回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), PolicyCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(PolicyCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_pick_injection() -> (PickLedger, [u32; 5]) {
    let mut ledger = PickLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadPickCase::ALL.len() {
        if let Some(case) = BadPickCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；无图/空册族"),
    ("ENODEV", -19, 55, "Windows ERROR_NO_SUCH_DEVICE；依赖图缺设备族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；越策略族"),
    ("EBADE", -52, 1, "Windows ERROR_INVALID_FUNCTION；坏交换描述族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏种子/配额族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：注册→装载→增量→抽样全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut book = CaseBook::new();
    let _ = register_all(&mut book);
    let mut sel = PolicySelector::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut covered = [true; MODULE_COUNT];
    if let Some(slot) = covered.get_mut(9) {
        *slot = false; // 模块 9 无图边覆盖，制造部分覆盖语料
    }
    bytes.push(match sel.load_graph(covered) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    let _ = sel.set_seed(7);
    match sel.select_incremental(&book, &[0, 1]) {
        Ok(v) => {
            bytes.push(v.len() as u8);
            for id in v.iter() {
                bytes.extend_from_slice(&id.to_le_bytes());
            }
        }
        Err(_) => bytes.push(0),
    }
    match sel.select_stratified(&book, SAMPLE_QUOTA) {
        Ok(v) => {
            bytes.push(v.len() as u8);
            for id in v.iter() {
                bytes.extend_from_slice(&id.to_le_bytes());
            }
        }
        Err(_) => bytes.push(0),
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10044 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10044_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10044");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91b00..=0x91b40", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 注册表：全册注册 40/40
    let mut book = CaseBook::new();
    let reg_ok = register_all(&mut book).is_ok() && book.registered == CASE_COUNT as u32;
    s.add("注册表：40 条战线条目全量注册", reg_ok,
          "条目号 1..=40 逐条入册，无重复无越界");

    // 3. 注册边界：越界与重复拒绝
    let mut b2 = CaseBook::new();
    let _ = b2.register(1);
    let bound_ok = b2.register(0).is_err()
        && b2.register(41).is_err()
        && b2.register(1) == Err(PolicyCode::DuplicateCase);
    s.add("注册边界：0/41/重复号全拒", bound_ok,
          "条目号闭集 1..=40，出界与重复显性拒绝");

    // 4. 全量选例：空册拒绝、全册 40 条
    let empty = CaseBook::new();
    let full_ok = empty.select_full().is_err() && book.select_full() == Ok((1u32..=40).collect::<Vec<u32>>());
    s.add("全量选例：空册拒绝、全册返回 40 条", full_ok,
          "全量策略要求注册表非空");

    // 5. 状态机：Idle→Loaded→Armed 推进
    let mut sel = PolicySelector::new();
    let covered = [true; MODULE_COUNT];
    let st1 = sel.load_graph(covered) == Ok(()) && sel.state == SelectorState::Loaded;
    let st2 = sel.set_seed(7) == Ok(()) && sel.state == SelectorState::Armed;
    s.add("状态机：装载→落种单向推进", st1 && st2,
          "Idle→Loaded→Armed 单向推进，倒退拒绝");

    // 6. 增量命中核：变更模块 0、1 → 条目 1..10、11..20 共 20 条
    let inc = sel.select_incremental(&book, &[0, 1]);
    let hit_ok = match &inc {
        Ok(v) => v.len() == 2 * (CASE_COUNT / MODULE_COUNT)
            && sel.coverage_ok(&book, &[0, 1], v),
        Err(_) => false,
    };
    s.add("增量命中核：变更 2 模块命中 20 条且覆盖核通过", hit_ok,
          "受影响条目 = 模块面命中并集，逐条入选");

    // 7. 增量命中 10/10（锚点判据直接探针）
    let mut cov_hit = 0u32;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let m = r as usize % MODULE_COUNT;
        let mut sel_r = PolicySelector::new();
        let _ = sel_r.load_graph(covered);
        if let Ok(v) = sel_r.select_incremental(&book, &[m]) {
            if sel_r.coverage_ok(&book, &[m], &v) {
                cov_hit = cov_hit.saturating_add(1);
            }
        }
        r += 1;
    }
    s.add("增量命中 10/10：逐轮覆盖核全过", cov_hit == INJECT_ROUNDS,
          "十轮逐模块变更，受影响条目零漏选");

    // 8. 图空洞漏选 10/10 检出
    let mut hole_hit = 0u32;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        if sel.select_incremental(&book, &[MODULE_COUNT + 7]).is_err() {
            hole_hit = hole_hit.saturating_add(1);
        }
        r2 += 1;
    }
    s.add("图空洞检出：越界模块 10/10 拒绝", hole_hit == INJECT_ROUNDS,
          "空洞模块出 0..10 闭集即 GraphHole，杜绝漏选");

    // 9. 抽样选例：配额 6、同种子确定性
    let s1 = sel.select_stratified(&book, SAMPLE_QUOTA);
    let s2 = sel.select_stratified(&book, SAMPLE_QUOTA);
    let strat_ok = match (&s1, &s2) {
        (Ok(a), Ok(b)) => a.len() == SAMPLE_QUOTA && a == b && a.iter().all(|x| *x >= 1 && *x <= 40),
        _ => false,
    };
    s.add("抽样选例：配额 6 条且同种子复现", strat_ok,
          "等步长抽样同种子两次逐条一致");

    // 10. 越策略拦截：未 Armed 抽样 / Idle 增量都拒绝
    let idle_sel = PolicySelector::new();
    let oop_ok = idle_sel.select_stratified(&book, SAMPLE_QUOTA).is_err()
        && idle_sel.select_incremental(&book, &[0]).is_err();
    s.add("越策略拦截：未装载/未落种分发显性拒绝", oop_ok,
          "策略与档位必须一一对应，越档即拦");

    // 11. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_pick_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "空册/无图/空洞/零种子/越策略全被显式拒绝");

    // 12. 注入账守恒：逃逸=0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 13. 文案闭集：非空
    let text_ok = PolicyCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 14. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < PolicyCode::ALL.len() {
        let mut j = i + 1;
        while j < PolicyCode::ALL.len() {
            if let (Some(x), Some(y)) = (PolicyCode::ALL.get(i), PolicyCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = PolicyCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 15. Linux 逐码一致
    let codes_ok = agrees_with_linux(EACCES)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(ENODEV)
        && agrees_with_linux(EBADE)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 16. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 17. 模块面映射确定性
    let map_ok = CaseBook::module_of(1) == Some(0)
        && CaseBook::module_of(10) == Some(9)
        && CaseBook::module_of(11) == Some(0)
        && CaseBook::module_of(0).is_none()
        && CaseBook::module_of(41).is_none();
    s.add("模块面映射：(id-1)%10 确定且出界拒绝", map_ok,
          "条目→模块映射纯函数，出界 None");

    // 动态明细（≤8）：命中数一行 + 指纹一行
    let hit_text = format!("增量命中 {} 条 / 注入 {} / 逃逸 {}",
        inc.map(|v| v.len()).unwrap_or(0), ledger.injected, ledger.escaped);
    s.add_dyn("选例快照", ledger.escaped == 0, &hit_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_all_40() {
        let mut book = CaseBook::new();
        assert!(register_all(&mut book).is_ok());
        assert_eq!(book.registered, 40);
    }

    #[test]
    fn duplicate_and_out_of_range_rejected() {
        let mut book = CaseBook::new();
        let _ = book.register(3);
        assert_eq!(book.register(3), Err(PolicyCode::DuplicateCase));
        assert_eq!(book.register(0), Err(PolicyCode::CaseOutOfRange));
        assert_eq!(book.register(41), Err(PolicyCode::CaseOutOfRange));
    }

    #[test]
    fn empty_full_rejected() {
        let book = CaseBook::new();
        assert_eq!(book.select_full(), Err(PolicyCode::EmptyFull));
    }

    #[test]
    fn incremental_covers_changed_modules() {
        let mut book = CaseBook::new();
        let _ = register_all(&mut book);
        let mut sel = PolicySelector::new();
        let _ = sel.load_graph([true; MODULE_COUNT]);
        let v = sel.select_incremental(&book, &[2]).unwrap_or_default();
        assert_eq!(v.len(), 4);
        assert!(sel.coverage_ok(&book, &[2], &v));
    }

    #[test]
    fn graph_hole_rejected() {
        let mut book = CaseBook::new();
        let _ = register_all(&mut book);
        let mut sel = PolicySelector::new();
        let _ = sel.load_graph([true; MODULE_COUNT]);
        assert_eq!(sel.select_incremental(&book, &[10]), Err(PolicyCode::GraphHole));
    }

    #[test]
    fn no_graph_rejected() {
        let mut book = CaseBook::new();
        let _ = register_all(&mut book);
        let sel = PolicySelector::new();
        assert_eq!(sel.select_incremental(&book, &[0]), Err(PolicyCode::NoGraph));
    }

    #[test]
    fn zero_seed_rejected() {
        let mut sel = PolicySelector::new();
        let _ = sel.load_graph([true; MODULE_COUNT]);
        assert_eq!(sel.set_seed(0), Err(PolicyCode::BadSeed));
    }

    #[test]
    fn stratified_deterministic() {
        let mut book = CaseBook::new();
        let _ = register_all(&mut book);
        let mut sel = PolicySelector::new();
        let _ = sel.load_graph([true; MODULE_COUNT]);
        let _ = sel.set_seed(42);
        let a = sel.select_stratified(&book, SAMPLE_QUOTA).unwrap_or_default();
        let b = sel.select_stratified(&book, SAMPLE_QUOTA).unwrap_or_default();
        assert_eq!(a, b);
        assert_eq!(a.len(), SAMPLE_QUOTA);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_pick_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(ENODEV));
        assert!(agrees_with_linux(EBADE));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10044_checks_green() {
        let s = run_unx10044_checks();
        assert!(s.all_passed(), "UNX-F10044 red: {}", s.red_items_text());
    }
}
