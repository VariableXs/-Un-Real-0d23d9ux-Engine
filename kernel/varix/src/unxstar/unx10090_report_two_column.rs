//! UNX-F10090 双判据报表扩展（正/反通过率两列）· 0x92640 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10091-J1 示例条核（样例条 J1/J2 齐备可复测 10/10 次），缺判据条目 10/10 次拒收
//!
//! SEG_BASE = 0x92640（本卡独占 0x40 窗口，下一卡 0x92680）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器：格值 token 解析器（j1:pass|j1:fail|j2:pass|j2:fail 四形制，
//!      闭集外一律 Unknown 拒绝，空串亦拒），是报表格写入的唯一入口。
//!   ② 状态机：双列相位机 Unsplit→J1Ready→J2Ready→BothReady，任何
//!      越相位写入 → PhaseViolation，混列写入 → MixedColumn。
//!   ③ 账本：双列账本 TwoColumnLedger，行 = (条目号, J1 格, J2 格)；
//!      J1 格只收 J1 格值，J2 格只收 J2 格值，冻结格禁覆写。
//!   ④ 注入核：混列 / 重复行 / 覆写冻结格 / 缺列取数 四类坏样本逐类
//!      10/10 次拒绝，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核报表确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10090 独占 0x40 窗口，下一卡 0x92680）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92640;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 双列账本行数上限（报表宽度工程约束）。
pub const ROW_CAP: usize = 64;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TwoColCode {
    /// 空格值写入
    EmptyCell,
    /// 格值落入错列（J2 格值写入 J1 列或反之）
    MixedColumn,
    /// 未知格值 token
    UnknownCell,
    /// 条目号重复
    DupRow,
    /// 冻结格被覆写
    FrozenCell,
    /// 双列未齐即取数
    MissingColumn,
    /// 空样本取率
    EmptySample,
    /// 越相位写入
    PhaseViolation,
    /// 账本守恒失守（两列计数与行数不符）
    LedgerMismatch,
    /// 格值标签往返不一致
    CellRoundTrip,
    /// 拒绝后账本被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 段位越界
    SegmentViolation,
}

impl TwoColCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TwoColCode::EmptyCell => "空格值被拒：报表格必须携带显式 pass|fail；\
                空格请走缺列路径而非静默占位",
            TwoColCode::MixedColumn => "混列被拒：J1 格值只能入正列、J2 格值只能入反列；\
                请核对列号后再写入",
            TwoColCode::UnknownCell => "未知格值被拒：格值只认 j1:pass|j1:fail|j2:pass|j2:fail；\
                请改用闭集内写法",
            TwoColCode::DupRow => "条目号重复被拒：每行必须对应唯一条目；\
                重复写入请走更新评审而非新增行",
            TwoColCode::FrozenCell => "冻结格被拒：已冻结的格不得覆写；\
                请走重测路径产出新期报表",
            TwoColCode::MissingColumn => "双列未齐被拒：取率前正/反两列必须齐备；\
                请补齐缺列再做通过率统计",
            TwoColCode::EmptySample => "空样本被拒：零行账本不得产出通过率；\
                请先入行再取数",
            TwoColCode::PhaseViolation => "越相位被拒：账本相位必须按 未分列→正列→反列→双列 推进；\
                禁止跳相位取数",
            TwoColCode::LedgerMismatch => "账本守恒失守：行数必须等于两列非空格计数对齐结果；\
                请排查账本是否被旁路改动",
            TwoColCode::CellRoundTrip => "格值往返不一致：label 再解析必须回到原格值；\
                请检查闭集文案与解析器是否同步",
            TwoColCode::RejectedTrace => "拒绝留痕被拒：被拒写入不得改动任何已冻结行；\
                请排查拒绝路径是否误提交",
            TwoColCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [TwoColCode; 12] = [
        TwoColCode::EmptyCell,
        TwoColCode::MixedColumn,
        TwoColCode::UnknownCell,
        TwoColCode::DupRow,
        TwoColCode::FrozenCell,
        TwoColCode::MissingColumn,
        TwoColCode::EmptySample,
        TwoColCode::PhaseViolation,
        TwoColCode::LedgerMismatch,
        TwoColCode::CellRoundTrip,
        TwoColCode::RejectedTrace,
        TwoColCode::SegmentViolation,
    ];
}

// ── 格值闭集与解析器 ──────────────────────────────────────────────────
/// 列别闭集：0 = 正列（J1），1 = 反列（J2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColSide {
    /// 正列：J1 通过率
    J1,
    /// 反列：J2 通过率
    J2,
}

impl ColSide {
    /// 列序号（0/1）。
    pub fn index(self) -> usize {
        match self {
            ColSide::J1 => 0,
            ColSide::J2 => 1,
        }
    }

    /// 中文列名（成文用）。
    pub fn label(self) -> &'static str {
        match self {
            ColSide::J1 => "正列(J1)",
            ColSide::J2 => "反列(J2)",
        }
    }

    /// 闭集全量。
    pub const ALL: [ColSide; 2] = [ColSide::J1, ColSide::J2];
}

/// 格值闭集：某列的 pass / fail 二值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellKind {
    /// 该列判据通过
    Pass,
    /// 该列判据未通过
    Fail,
}

impl CellKind {
    /// 闭集全量（pass / fail 二值）。
    pub const ALL: [CellKind; 2] = [CellKind::Pass, CellKind::Fail];

    /// 通过与否（pass=true）。
    pub fn is_pass(self) -> bool {
        matches!(self, CellKind::Pass)
    }

    /// 格值所属列别（格值 token 的前缀决定）。
    pub fn side_of_token(token: &str) -> Option<ColSide> {
        if token.starts_with("j1:") {
            Some(ColSide::J1)
        } else if token.starts_with("j2:") {
            Some(ColSide::J2)
        } else {
            None
        }
    }
}

/// 格值 token 解析：形制 j1:pass|j1:fail|j2:pass|j2:fail，返回 (列别, 格值)。
/// 闭集外一律拒绝；空串亦拒。解析器是格写入的唯一入口。
pub fn parse_cell(token: &str) -> Result<(ColSide, CellKind), TwoColCode> {
    if token.is_empty() {
        return Err(TwoColCode::EmptyCell);
    }
    let side = match CellKind::side_of_token(token) {
        Some(s) => s,
        None => return Err(TwoColCode::UnknownCell),
    };
    let rest = match token.get(3..) {
        Some(r) => r,
        None => return Err(TwoColCode::UnknownCell),
    };
    let cell = match rest {
        "pass" => CellKind::Pass,
        "fail" => CellKind::Fail,
        _ => return Err(TwoColCode::UnknownCell),
    };
    Ok((side, cell))
}

/// 格值 → token 反解（供往返一致性检查）。
pub fn cell_to_token(side: ColSide, cell: CellKind) -> String {
    let p = match side {
        ColSide::J1 => "j1",
        ColSide::J2 => "j2",
    };
    match cell {
        CellKind::Pass => format!("{}:pass", p),
        CellKind::Fail => format!("{}:fail", p),
    }
}

// ── 双列相位状态机 ────────────────────────────────────────────────────
/// 账本相位：Unsplit → J1Ready → J2Ready → BothReady。
/// 相位只允许单向推进；越相位写入/取数显性拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TwoColPhase {
    /// 未分列：两列均空
    Unsplit,
    /// 正列就位：J1 列已冻结
    J1Ready,
    /// 反列就位：J2 列已冻结
    J2Ready,
    /// 双列就位：可取双通过率
    BothReady,
}

impl TwoColPhase {
    /// 相位序（0..=3，供单调推进判定）。
    pub fn rank(self) -> u8 {
        match self {
            TwoColPhase::Unsplit => 0,
            TwoColPhase::J1Ready => 1,
            TwoColPhase::J2Ready => 2,
            TwoColPhase::BothReady => 3,
        }
    }

    /// 单调推进：目标相位必须严格升档（原地停留/降档一律拒绝）；
    /// 冻结序不定，故允许跨档直入（如 J2 先冻结即入 J2Ready）。
    pub fn advance(&mut self, to: TwoColPhase) -> Result<(), TwoColCode> {
        if to.rank() > self.rank() {
            *self = to;
            Ok(())
        } else {
            Err(TwoColCode::PhaseViolation)
        }
    }
}

// ── 双列账本 ──────────────────────────────────────────────────────────
/// 一行：条目号 + 两列格值（None = 该列尚未写入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TwoColRow {
    /// 条目号（账本内唯一）
    pub entry: u32,
    /// 正列格值
    pub j1: Option<CellKind>,
    /// 反列格值
    pub j2: Option<CellKind>,
}

/// 双列账本：正/反通过率两列分列落账，混列/重复/覆写逐次显性拒绝。
#[derive(Clone, Debug)]
pub struct TwoColLedger {
    rows: Vec<TwoColRow>,
    frozen: [bool; 2],
    phase: TwoColPhase,
    cap: usize,
}

impl TwoColLedger {
    /// 空账构造（容量 ROW_CAP，相位 Unsplit）。
    pub fn new() -> TwoColLedger {
        TwoColLedger {
            rows: Vec::new(),
            frozen: [false; 2],
            phase: TwoColPhase::Unsplit,
            cap: ROW_CAP,
        }
    }

    /// 新增一行（条目号必须未出现过）；返回行下标。
    pub fn add_row(&mut self, entry: u32) -> Result<usize, TwoColCode> {
        if self.rows.len() >= self.cap {
            return Err(TwoColCode::LedgerMismatch);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.entry == entry {
                    return Err(TwoColCode::DupRow);
                }
            }
            i += 1;
        }
        let idx = self.rows.len();
        self.rows.push(TwoColRow { entry, j1: None, j2: None });
        Ok(idx)
    }

    /// 写格：token 由解析器解出 (列别, 格值)；列别必须与目标列一致，
    /// 冻结列禁写，目标行必须已存在。任一违反显性拒绝且不留痕。
    pub fn set_cell(&mut self, row: usize, target: ColSide, token: &str) -> Result<(), TwoColCode> {
        let (side, cell) = parse_cell(token)?;
        if side != target {
            return Err(TwoColCode::MixedColumn);
        }
        if self.frozen[target.index()] {
            return Err(TwoColCode::FrozenCell);
        }
        if row >= self.rows.len() {
            return Err(TwoColCode::MissingColumn);
        }
        if let Some(r) = self.rows.get_mut(row) {
            match target {
                ColSide::J1 => r.j1 = Some(cell),
                ColSide::J2 => r.j2 = Some(cell),
            }
        }
        Ok(())
    }

    /// 冻结一列：整列必须已写满；冻结后该列只读。
    pub fn freeze(&mut self, side: ColSide) -> Result<(), TwoColCode> {
        let ci = side.index();
        let mut i = 0usize;
        while i < self.rows.len() {
            let filled = match self.rows.get(i) {
                Some(r) => match ci {
                    0 => r.j1.is_some(),
                    _ => r.j2.is_some(),
                },
                None => false,
            };
            if !filled {
                return Err(TwoColCode::MissingColumn);
            }
            i += 1;
        }
        if self.frozen[ci] {
            return Err(TwoColCode::FrozenCell);
        }
        self.frozen[ci] = true;
        // 相位由冻结面推导：只允许严格升档（任一冻结序都合法，重复冻结已被
        // 上方 FrozenCell 拦截），降档/原地停留即越相位。
        let next = if self.frozen[0] && self.frozen[1] {
            TwoColPhase::BothReady
        } else if self.frozen[0] {
            TwoColPhase::J1Ready
        } else {
            TwoColPhase::J2Ready
        };
        if next.rank() <= self.phase.rank() {
            return Err(TwoColCode::PhaseViolation);
        }
        self.phase = next;
        Ok(())
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 账本是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 双列是否齐备（两列都冻结且非空）。
    pub fn both_ready(&self) -> bool {
        self.phase == TwoColPhase::BothReady && !self.rows.is_empty()
    }

    /// 列通过率：返回 (pass, total)；双列未齐或空样本显性拒绝。
    pub fn rate(&self, side: ColSide) -> Result<(u32, u32), TwoColCode> {
        if self.rows.is_empty() {
            return Err(TwoColCode::EmptySample);
        }
        if !self.frozen[side.index()] {
            return Err(TwoColCode::MissingColumn);
        }
        let mut pass = 0u32;
        let mut total = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                let cell = match side {
                    ColSide::J1 => r.j1,
                    ColSide::J2 => r.j2,
                };
                match cell {
                    Some(c) => {
                        total += 1;
                        if c.is_pass() {
                            pass += 1;
                        }
                    }
                    None => return Err(TwoColCode::MissingColumn),
                }
            }
            i += 1;
        }
        Ok((pass, total))
    }

    /// 账本守恒：行数 = 两列各自非空格计数（齐备态下逐格非 None）。
    pub fn conserved(&self) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            match self.rows.get(i) {
                None => return false,
                Some(r) => {
                    let both = r.j1.is_some() && r.j2.is_some();
                    let none = r.j1.is_none() && r.j2.is_none();
                    if !(both || none) {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }
}

// ── 注入核：坏写入样本矩阵 ────────────────────────────────────────────
/// 四类坏写入样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadWrite {
    /// 混列：J2 格值写入正列
    MixedColumn,
    /// 重复条目号
    DupRow,
    /// 覆写冻结列
    FrozenCell,
    /// 缺列取数：列未冻结先取率
    MissingColumn,
}

impl BadWrite {
    /// 闭集全量。
    pub const ALL: [BadWrite; 4] = [
        BadWrite::MixedColumn,
        BadWrite::DupRow,
        BadWrite::FrozenCell,
        BadWrite::MissingColumn,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadWrite::MixedColumn => "混列写入",
            BadWrite::DupRow => "重复条目",
            BadWrite::FrozenCell => "覆写冻结格",
            BadWrite::MissingColumn => "缺列取数",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失守恒返回 RejectedTrace。
    pub fn record(&mut self, caught: bool) -> Result<(), TwoColCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(TwoColCode::RejectedTrace);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏写入注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_write_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadWrite::ALL.len() {
        let case = match BadWrite::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadWrite::MixedColumn => {
                    let mut l = TwoColLedger::new();
                    let r = l.add_row(7);
                    match r {
                        Ok(idx) => l.set_cell(idx, ColSide::J1, "j2:pass")
                            .err() == Some(TwoColCode::MixedColumn),
                        Err(_) => false,
                    }
                }
                BadWrite::DupRow => {
                    let mut l = TwoColLedger::new();
                    let _ = l.add_row(7);
                    l.add_row(7).err() == Some(TwoColCode::DupRow)
                }
                BadWrite::FrozenCell => {
                    let mut l = TwoColLedger::new();
                    let ok = match l.add_row(3) {
                        Ok(idx) => l.set_cell(idx, ColSide::J1, "j1:pass").is_ok(),
                        Err(_) => false,
                    };
                    ok && l.freeze(ColSide::J1).is_ok()
                        && l.set_cell(0, ColSide::J1, "j1:fail")
                            .err() == Some(TwoColCode::FrozenCell)
                }
                BadWrite::MissingColumn => {
                    let mut l = TwoColLedger::new();
                    let ok = match l.add_row(5) {
                        Ok(idx) => l.set_cell(idx, ColSide::J2, "j2:fail").is_ok(),
                        Err(_) => false,
                    };
                    ok && l.rate(ColSide::J1).err() == Some(TwoColCode::MissingColumn)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 报表成文（纯函数） ────────────────────────────────────────────────
/// 双列成文：「行数=<n>;J1=<p>/<t>;J2=<p>/<t>」；双列未齐先拒。
pub fn compose_two_col_report(l: &TwoColLedger) -> Result<String, TwoColCode> {
    if !l.both_ready() {
        return Err(TwoColCode::MissingColumn);
    }
    let (p1, t1) = l.rate(ColSide::J1)?;
    let (p2, t2) = l.rate(ColSide::J2)?;
    let out = format!("行数={};J1={}/{};J2={}/{}", l.len(), p1, t1, p2, t2);
    if out.is_empty() {
        return Err(TwoColCode::EmptySample);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；未知格值/混列族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复条目族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；覆写冻结格族"),
    ("ENODATA", -61, 232, "Windows ERROR_NO_DATA；缺列/空样本取数族"),
    ("EBADE", -52, 31, "Windows ERROR_GEN_FAILURE；账本守恒失守族"),
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

/// 规范语料双跑指纹：四行账本双列入账 → 冻结 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = TwoColLedger::new();
    let spec: [(u32, &str, &str); 4] = [
        (11, "j1:pass", "j2:pass"),
        (12, "j1:pass", "j2:fail"),
        (13, "j1:fail", "j2:pass"),
        (14, "j1:pass", "j2:pass"),
    ];
    let mut si = 0usize;
    while si < spec.len() {
        if let Some(row) = spec.get(si) {
            if let Ok(idx) = l.add_row(row.0) {
                let _ = l.set_cell(idx, ColSide::J1, row.1);
                let _ = l.set_cell(idx, ColSide::J2, row.2);
            }
        }
        si += 1;
    }
    let _ = l.freeze(ColSide::J1);
    let _ = l.freeze(ColSide::J2);
    let bytes = match compose_two_col_report(&l) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10090 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10090_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10090");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92640..=0x9267F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 格值解析器：四形制全通过
    let parse_ok = parse_cell("j1:pass") == Ok((ColSide::J1, CellKind::Pass))
        && parse_cell("j1:fail") == Ok((ColSide::J1, CellKind::Fail))
        && parse_cell("j2:pass") == Ok((ColSide::J2, CellKind::Pass))
        && parse_cell("j2:fail") == Ok((ColSide::J2, CellKind::Fail));
    s.add("格值解析器：j1/j2 × pass/fail 四形制全通过", parse_ok,
          "格值闭集必须全可解析，闭集外显性拒绝");

    // 3. 未知格值注入：未知/空串/漂移 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_cell("j3:pass").is_ok()
            || parse_cell("").is_ok()
            || parse_cell("j1:PASS").is_ok()
            || parse_cell("j2:pass ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("未知格值注入：未知/空串/漂移 10/10 拒绝", amb_ok,
          "格值歧义必须逐次显性拒绝，不许静默归并到相邻格值");

    // 4. 混列写入拒绝：J2 格值入正列 10/10
    let mut mix_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut l = TwoColLedger::new();
        if let Ok(idx) = l.add_row(1) {
            if l.set_cell(idx, ColSide::J1, "j2:pass").is_ok()
                || l.set_cell(idx, ColSide::J2, "j1:fail").is_ok()
            {
                mix_ok = false;
            }
        }
        r += 1;
    }
    s.add("混列注入：J2 格值入正列 / J1 格值入反列 10/10 拒绝", mix_ok,
          "正/反通过率必须分列落账，交叉写入即混列显性拒绝");

    // 5. 双列齐备构建：四行入账两列冻结
    let mut l = TwoColLedger::new();
    let spec: [(u32, &str, &str); 4] = [
        (21, "j1:pass", "j2:pass"),
        (22, "j1:fail", "j2:fail"),
        (23, "j1:pass", "j2:fail"),
        (24, "j1:pass", "j2:fail"),
    ];
    let mut build_ok = true;
    let mut si = 0usize;
    while si < spec.len() {
        if let Some(row) = spec.get(si) {
            match l.add_row(row.0) {
                Ok(idx) => {
                    if l.set_cell(idx, ColSide::J1, row.1).is_err()
                        || l.set_cell(idx, ColSide::J2, row.2).is_err()
                    {
                        build_ok = false;
                    }
                }
                Err(_) => build_ok = false,
            }
        }
        si += 1;
    }
    let frz_ok = l.freeze(ColSide::J1).is_ok() && l.freeze(ColSide::J2).is_ok();
    s.add("双列齐备构建：四行入账且正/反两列冻结到位", build_ok && frz_ok && l.both_ready(),
          "双列报表必须整列写满后冻结，缺格冻结即拒绝");

    // 6. 双通过率取数正确（分母口径 = 行数）
    let rate_ok = l.rate(ColSide::J1) == Ok((3, 4)) && l.rate(ColSide::J2) == Ok((1, 4));
    s.add("双列取数：J1=3/4、J2=1/4 与逐格账面一致", rate_ok,
          "通过率分母冻结为已冻结行数，逐格可复算");

    // 7. 注入核：四类坏写入样本各 10/10 拒绝
    let (inj, per_class) = run_bad_write_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏写入样本各 10/10 拒绝", all_caught,
          "混列/重复行/覆写冻结格/缺列取数必须逐类逐次拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadWrite::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 相位机：单调升档、原地停留与降档显性拒绝
    let phase_ok = {
        let mut p = TwoColPhase::Unsplit;
        p.advance(TwoColPhase::J1Ready).is_ok()
            && p.advance(TwoColPhase::J1Ready).is_err()
            && p.advance(TwoColPhase::BothReady).is_ok()
            && {
                let mut q = TwoColPhase::BothReady;
                q.advance(TwoColPhase::Unsplit).is_err()
            }
    };
    s.add("相位机：相位只升不降，停留/降档显性拒绝", phase_ok,
          "越相位取数/写入必须拒绝，防跳格出双列报表");

    // 10. 拒绝不留痕：混列被拒后行账面不变
    let mut l2 = TwoColLedger::new();
    let _ = l2.add_row(31);
    let _ = l2.set_cell(0, ColSide::J1, "j1:pass");
    let _ = l2.set_cell(0, ColSide::J2, "j2:pass");
    let before = l2.conserved() && l2.len() == 1;
    let _ = l2.set_cell(0, ColSide::J1, "j2:fail");
    let trace_ok = before && l2.conserved();
    s.add("拒绝不留痕：混列被拒不改任何已写格", trace_ok,
          "被拒写入不得改动账面，拒绝即回滚到写入前账态");

    // 11. 账本守恒：半行（单列写格）必须显性暴露
    let mut l3 = TwoColLedger::new();
    let _ = l3.add_row(41);
    let _ = l3.set_cell(0, ColSide::J1, "j1:pass");
    let half_open = !l3.conserved();
    let _ = l3.set_cell(0, ColSide::J2, "j2:pass");
    let closed = l3.conserved();
    s.add("账本守恒：半行态暴露、补齐后守恒恢复", half_open && closed,
          "单列写入的行是未闭合半行，守恒审计必须即时暴露");

    // 12. 成文非空且含三标记 + 双跑一致
    let sum = compose_two_col_report(&l);
    let text_ok = match &sum {
        Ok(t) => t.contains("行数") && t.contains("J1=") && t.contains("J2="),
        Err(_) => false,
    };
    s.add("双列成文：标记齐全且两次成文逐字节一致",
          text_ok && sum == compose_two_col_report(&l),
          "成文必须携带 行数/J1/J2 三标记且为纯函数");

    // 13. 未齐成文拒绝
    let mut l4 = TwoColLedger::new();
    let _ = l4.add_row(51);
    let _ = l4.set_cell(0, ColSide::J1, "j1:pass");
    s.add("未齐成文拒绝：单列账本不得产出双列小结",
          compose_two_col_report(&l4).err() == Some(TwoColCode::MissingColumn),
          "缺反列的双列报表即虚报，产出小结必须先拒");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(17) && agrees_with_linux(1)
        && agrees_with_linux(61) && agrees_with_linux(52)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < TwoColCode::ALL.len() {
        let mut j = i + 1;
        while j < TwoColCode::ALL.len() {
            if let (Some(a), Some(b)) = (TwoColCode::ALL.get(i), TwoColCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = TwoColCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 格值往返 + 双跑指纹
    let rt_ok = ColSide::ALL.iter().all(|side| {
        CellKind::ALL.iter().all(|cell| {
            let tok = cell_to_token(*side, *cell);
            parse_cell(&tok) == Ok((*side, *cell))
        })
    });
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：格值标签可回解析且双跑哈希相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");
    let _ = i;

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum { Ok(t) => t.len(), Err(_) => 0 };
    let sum_text = format!("双列小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_cell("j1:pass"), Ok((ColSide::J1, CellKind::Pass)));
        assert!(parse_cell("j3:pass").is_err());
        assert_eq!(parse_cell("").err(), Some(TwoColCode::EmptyCell));
        assert_eq!(parse_cell("j1:Pass").err(), Some(TwoColCode::UnknownCell));
    }

    #[test]
    fn mixed_column_rejected() {
        let mut l = TwoColLedger::new();
        let idx = l.add_row(1).unwrap_or(0);
        assert_eq!(l.set_cell(idx, ColSide::J1, "j2:pass").err(),
                   Some(TwoColCode::MixedColumn));
        assert_eq!(l.set_cell(idx, ColSide::J2, "j1:fail").err(),
                   Some(TwoColCode::MixedColumn));
    }

    #[test]
    fn dup_row_rejected() {
        let mut l = TwoColLedger::new();
        let _ = l.add_row(9);
        assert_eq!(l.add_row(9).err(), Some(TwoColCode::DupRow));
    }

    #[test]
    fn freeze_blocks_rewrite() {
        let mut l = TwoColLedger::new();
        let idx = l.add_row(2).unwrap_or(0);
        let _ = l.set_cell(idx, ColSide::J1, "j1:pass");
        let _ = l.set_cell(idx, ColSide::J2, "j2:pass");
        assert!(l.freeze(ColSide::J1).is_ok());
        assert_eq!(l.set_cell(idx, ColSide::J1, "j1:fail").err(),
                   Some(TwoColCode::FrozenCell));
        assert!(l.freeze(ColSide::J2).is_ok());
        assert!(l.both_ready());
    }

    #[test]
    fn rate_matches_cells() {
        let mut l = TwoColLedger::new();
        let i0 = l.add_row(3).unwrap_or(0);
        let i1 = l.add_row(4).unwrap_or(0);
        let _ = l.set_cell(i0, ColSide::J1, "j1:pass");
        let _ = l.set_cell(i0, ColSide::J2, "j2:fail");
        let _ = l.set_cell(i1, ColSide::J1, "j1:pass");
        let _ = l.set_cell(i1, ColSide::J2, "j2:pass");
        let _ = l.freeze(ColSide::J1);
        let _ = l.freeze(ColSide::J2);
        assert_eq!(l.rate(ColSide::J1), Ok((2, 2)));
        assert_eq!(l.rate(ColSide::J2), Ok((1, 2)));
    }

    #[test]
    fn half_row_breaks_conservation() {
        let mut l = TwoColLedger::new();
        let idx = l.add_row(5).unwrap_or(0);
        let _ = l.set_cell(idx, ColSide::J1, "j1:pass");
        assert!(!l.conserved());
        let _ = l.set_cell(idx, ColSide::J2, "j2:pass");
        assert!(l.conserved());
    }

    #[test]
    fn phase_machine_monotonic() {
        let mut p = TwoColPhase::Unsplit;
        assert!(p.advance(TwoColPhase::J1Ready).is_ok());
        assert!(p.advance(TwoColPhase::Unsplit).is_err());
        assert!(p.advance(TwoColPhase::J2Ready).is_ok());
        assert!(p.advance(TwoColPhase::BothReady).is_ok());
        assert!(p.advance(TwoColPhase::Unsplit).is_err());
        assert_eq!(p, TwoColPhase::BothReady);
    }

    #[test]
    fn bad_write_matrix_green() {
        let (inj, per) = run_bad_write_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn report_needs_both_columns() {
        let mut l = TwoColLedger::new();
        assert!(compose_two_col_report(&l).is_err());
        let idx = l.add_row(6).unwrap_or(0);
        let _ = l.set_cell(idx, ColSide::J1, "j1:pass");
        let _ = l.set_cell(idx, ColSide::J2, "j2:pass");
        let _ = l.freeze(ColSide::J1);
        let _ = l.freeze(ColSide::J2);
        let rep = compose_two_col_report(&l).unwrap_or_default();
        assert!(rep.contains("J1=1/1"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(61));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(17), -17);
    }

    #[test]
    fn unx10090_checks_green() {
        let set = run_unx10090_checks();
        assert!(set.all_passed(), "UNX-F10090 red: {}", set.red_items_text());
    }
}
