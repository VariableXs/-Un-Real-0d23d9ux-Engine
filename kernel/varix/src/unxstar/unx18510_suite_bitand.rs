//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18510 · SuiteMask 比较特例（位与语义）
//! · 0x7FB40 段 · 批 06 · 配额 260
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18510-J1 SuiteMask 位与特例断言全过（请求位任一在场
//!   即过），与 F18408 口径一致。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 位全集口径冻结（与 F18408 一致）：KNOWN_MASK = 0x27FF（12 位
//!     闭集：0x0001..=0x0400 连续 11 位 + 0x2000）；位全集外 = 保留位
//!     （请求携带保留位必拒）；
//!   - 特例双语义：VER_AND（6）= (sys & req) == req（请求位**全部**
//!     在场）/ VER_OR（7）= (sys & req) != 0（请求位**任一**在场即过
//!     ——J1 口径）；其它算子一律 OpForbidden（套件维无序比较语义）；
//!   - 冻结表 20 例：AND 道 10 例 + OR 道 10 例，期望逐例手推；
//!   - 门规：req=0 空请求拒 / 保留位拒 / 非法算子拒；
//!   - 注入四类 × 10 轮：AND 误作 OR（部分在场例必偏）/ OR 误作 AND
//!     （部分在场例必偏）/ 保留位请求 / 空请求——逐类逐轮必抓；
//!   - 探针状态机：Open → Sweeping → Sealed；行序严格升序（乱序/
//!     重复/越界三闸）；账本守恒（问=过+偏）；
//!   - 对账：与 F18408 的 KNOWN_MASK 冻结值与 12 位计数交叉核对；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] SmkCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   SUITE_ROWS 20 例冻结表（AND/OR 双道）
//!   [数据结构] 位名冻结表（12 位，与 F18408 同口径）
//!   [状态机]   SmkPhase 三态 + SuiteProbe（三闸 + 零偏差对拍）
//!   [账本]     SmkLedger（问=过+偏 守恒）
//!   [注入核]   四类 × 10 轮（AND误OR/OR误AND/保留位/空请求）
//!   [收口]     golden_run + run_unx18510_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FB40，AI-VX11-1105 G1 批06 独占区）。
pub const SEG_BASE: u32 = 0x7FB40;

// ── 域冻结常量（与 F18408 口径一致） ────────────────────────────
/// VER_AND（冻结）。
pub const VER_AND: u8 = 6;
/// VER_OR（冻结）。
pub const VER_OR: u8 = 7;
/// 已知位全集（12 位闭集，与 F18408 冻结值一致）。
pub const KNOWN_MASK: u16 = 0x27FF;
/// 已知位数（冻结：12）。
pub const KNOWN_BITS: usize = 12;
/// 每道行数（10 例 × 2 道 = 20 例）。
pub const LANE_ROWS: usize = 10;
/// 总行数。
pub const SUITE_ROWS: usize = 20;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 位名冻结表（与 F18408 同口径 12 位，bit 升序）。
pub const SUITE_NAMES: [&str; KNOWN_BITS] = [
    "VER_SUITE_SMALLBUSINESS",             // 0x0001
    "VER_SUITE_ENTERPRISE",                // 0x0002
    "VER_SUITE_BACKOFFICE",                // 0x0004
    "VER_SUITE_COMMUNICATIONS",            // 0x0008
    "VER_SUITE_TERMINAL",                  // 0x0010
    "VER_SUITE_SMALLBUSINESS_RESTRICTED",  // 0x0020
    "VER_SUITE_EMBEDDEDNT",                // 0x0040
    "VER_SUITE_DATACENTER",                // 0x0080
    "VER_SUITE_SINGLEUSERTS",              // 0x0100
    "VER_SUITE_PERSONAL",                  // 0x0200
    "VER_SUITE_SERVERFORSMALLBUSINESS",    // 0x0400
    "VER_SUITE_EMBEDDEDAE",                // 0x2000
];

/// 第 i 已知位值（越界 None；与 SUITE_NAMES 同序）。
pub fn suite_bit(i: usize) -> Option<u16> {
    match i {
        0 => Some(0x0001),
        1 => Some(0x0002),
        2 => Some(0x0004),
        3 => Some(0x0008),
        4 => Some(0x0010),
        5 => Some(0x0020),
        6 => Some(0x0040),
        7 => Some(0x0080),
        8 => Some(0x0100),
        9 => Some(0x0200),
        10 => Some(0x0400),
        11 => Some(0x2000),
        _ => None,
    }
}

/// 保留位样本（KNOWN_MASK 外四例，注入面）。
pub const RESERVED_SAMPLES: [u16; 4] = [0x0800, 0x1000, 0x4000, 0x8000];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmkCode {
    /// 算子非法（套件维只认 VER_AND/VER_OR）。
    OpForbidden,
    /// 空请求（req=0）。
    EmptyRequest,
    /// 保留位请求（req 携带 KNOWN_MASK 外位）。
    ReservedBit,
    /// 系统值越界（sys 携带 KNOWN_MASK 外位）。
    SysOutOfRange,
    /// 行号越界（≥20）。
    RowIdOutOfRange,
    /// 行乱序。
    RowOutOfOrder,
    /// 行重复。
    RowDuplicate,
    /// 裁决偏差（got ≠ expect 零容忍）。
    VerdictDeviation,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 空表收口。
    TableEmpty,
    /// 口径失配（与 F18408 KNOWN_MASK 不等）。
    CrossCheckMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl SmkCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SmkCode::OpForbidden => "算子非法：套件维只认 VER_AND/VER_OR",
            SmkCode::EmptyRequest => "空请求：req=0 无位可比",
            SmkCode::ReservedBit => "保留位请求：位全集外必拒",
            SmkCode::SysOutOfRange => "系统值越界：KNOWN_MASK 外位",
            SmkCode::RowIdOutOfRange => "行号越界：冻结表仅 0..=19",
            SmkCode::RowOutOfOrder => "行乱序：必须严格升序",
            SmkCode::RowDuplicate => "行重复：同行只探一次",
            SmkCode::VerdictDeviation => "裁决偏差：got≠expect 零容忍",
            SmkCode::LedgerBroken => "账本守恒破坏：问≠过+偏",
            SmkCode::TableEmpty => "空表收口：零探针不得封账",
            SmkCode::CrossCheckMismatch => "口径失配：与 F18408 不等",
            SmkCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SmkCode; 12] = [
        SmkCode::OpForbidden,
        SmkCode::EmptyRequest,
        SmkCode::ReservedBit,
        SmkCode::SysOutOfRange,
        SmkCode::RowIdOutOfRange,
        SmkCode::RowOutOfOrder,
        SmkCode::RowDuplicate,
        SmkCode::VerdictDeviation,
        SmkCode::LedgerBroken,
        SmkCode::TableEmpty,
        SmkCode::CrossCheckMismatch,
        SmkCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SmkCode::ALL.len() {
        if SmkCode::ALL[i].code() == code {
            return SmkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FB40 段闭集内"
}

// ── 特例比较核 ──────────────────────────────────────────────────
/// 套件特例比较：AND=全在场 / OR=任一在场（J1 口径）。
pub fn suite_compare(op: u8, sys: u16, req: u16) -> Result<bool, u32> {
    // 算子闸：只认 AND/OR。
    if op != VER_AND && op != VER_OR {
        return Err(SmkCode::OpForbidden.code());
    }
    // 空请求闸。
    if req == 0 {
        return Err(SmkCode::EmptyRequest.code());
    }
    // 保留位闸（请求侧）。
    if req as u16 & !KNOWN_MASK != 0 {
        return Err(SmkCode::ReservedBit.code());
    }
    // 系统值越界闸。
    if sys as u16 & !KNOWN_MASK != 0 {
        return Err(SmkCode::SysOutOfRange.code());
    }
    if op == VER_AND {
        Ok((sys & req) == req)
    } else {
        Ok((sys & req) != 0)
    }
}

/// 误评核①：AND 语义误作 OR（注入面）。
pub fn suite_as_or(sys: u16, req: u16) -> bool {
    (sys & req) != 0
}

/// 误评核②：OR 语义误作 AND（注入面）。
pub fn suite_as_and(sys: u16, req: u16) -> bool {
    (sys & req) == req
}

// ── 冻结表 ──────────────────────────────────────────────────────
/// 冻结行：(lane 0=AND 1=OR, sys, req, expect)。
pub type SmkRow = (u8, u16, u16, bool);

/// AND 道第 i 例（期望 = (sys & req) == req，手推）。
pub fn and_row(i: usize) -> Option<SmkRow> {
    match i {
        0 => Some((0, 0x0100, 0x0100, true)),   // 自含
        1 => Some((0, 0x0100, 0x0101, false)),  // 缺 0x0001
        2 => Some((0, 0x0104, 0x0104, true)),   // 自含（双位）
        3 => Some((0, 0x0104, 0x0100, true)),   // 子集在场
        4 => Some((0, 0x0001, 0x0001, true)),   // 单位自含
        5 => Some((0, 0x0001, 0x0003, false)),  // 缺 0x0002
        6 => Some((0, 0x0300, 0x0300, true)),   // 双位自含
        7 => Some((0, 0x0300, 0x0200, true)),   // 子集在场
        8 => Some((0, 0x0300, 0x0101, false)),  // 缺 0x0001
        9 => Some((0, 0x27FF, 0x27FF, true)),   // 全集自含
        _ => None,
    }
}

/// OR 道第 i 例（期望 = (sys & req) != 0，手推）。
pub fn or_row(i: usize) -> Option<SmkRow> {
    match i {
        0 => Some((1, 0x0100, 0x0100, true)),   // 自含
        1 => Some((1, 0x0100, 0x0101, true)),   // 任一在场
        2 => Some((1, 0x0100, 0x0001, false)),  // 无交
        3 => Some((1, 0x0104, 0x0004, true)),   // 子位在场
        4 => Some((1, 0x0104, 0x0200, false)),  // 无交
        5 => Some((1, 0x0001, 0x0003, true)),   // 任一在场
        6 => Some((1, 0x0300, 0x0400, false)),  // 无交
        7 => Some((1, 0x0300, 0x0300, true)),   // 全在场
        8 => Some((1, 0x0104, 0x0800, false)),  // 无交（EMBEDDEDAE 位外）
        9 => Some((1, 0x27FF, 0x27FF, true)),   // 全集
        _ => None,
    }
}

/// 按 (lane, idx) 取例。
pub fn smk_row(lane: usize, idx: usize) -> Option<SmkRow> {
    match lane {
        0 => and_row(idx),
        1 => or_row(idx),
        _ => None,
    }
}

/// 表指纹（道/系统/请求/期望全敏感折叠）。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x1850A;
    let mut lane = 0;
    while lane < 2 {
        let mut i = 0;
        while i < LANE_ROWS {
            match smk_row(lane, i) {
                Some((l, s, r, e)) => {
                    acc = acc.wrapping_mul(31).wrapping_add(l as u32);
                    acc = acc.wrapping_mul(31).wrapping_add(s as u32);
                    acc = acc.wrapping_mul(31).wrapping_add(r as u32);
                    acc = acc.wrapping_mul(31).wrapping_add(if e { 1 } else { 2 });
                }
                None => {
                    acc = acc.wrapping_mul(31).wrapping_add(0xFFFF);
                }
            }
            i += 1;
        }
        lane += 1;
    }
    acc
}

// ── 账本 ────────────────────────────────────────────────────────
/// 套件账（问 = 过 + 偏）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmkLedger {
    pub queries: u32,
    pub passes: u32,
    pub deviations: u32,
    pub rejects: u32,
}

impl SmkLedger {
    pub const fn new() -> SmkLedger {
        SmkLedger { queries: 0, passes: 0, deviations: 0, rejects: 0 }
    }
    pub fn conserve(&self) -> bool {
        self.queries == self.passes + self.deviations
    }
}

// ── 状态机（Open → Sweeping → Sealed） ──────────────────────────
/// 探针阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmkPhase {
    /// 未开探。
    Open,
    /// 扫描中。
    Sweeping,
    /// 已封账。
    Sealed,
}

/// 套件探针：双道顺序（先 AND 后 OR）+ 三闸 + 零偏差对拍。
pub struct SuiteProbe {
    pub phase: SmkPhase,
    /// 全局游标（0..19，AND 道在前）。
    cursor: usize,
    pub ledger: SmkLedger,
    /// 评值偏置（None=正常 1=AND误OR 2=OR误AND）。
    bias: Option<u8>,
}

impl SuiteProbe {
    pub fn new() -> SuiteProbe {
        SuiteProbe {
            phase: SmkPhase::Open,
            cursor: 0,
            ledger: SmkLedger::new(),
            bias: None,
        }
    }

    pub fn set_bias(&mut self, b: Option<u8>) {
        self.bias = b;
    }

    /// 全局下标 → (lane, idx)。
    fn locate(pos: usize) -> Option<(usize, usize)> {
        if pos < LANE_ROWS {
            Some((0, pos))
        } else if pos < SUITE_ROWS {
            Some((1, pos - LANE_ROWS))
        } else {
            None
        }
    }

    /// 偏置评值。
    fn eval(&self, lane: usize, sys: u16, req: u16) -> bool {
        match self.bias {
            None => {
                if lane == 0 {
                    (sys & req) == req
                } else {
                    (sys & req) != 0
                }
            }
            Some(1) => suite_as_or(sys, req),
            _ => suite_as_and(sys, req),
        }
    }

    /// 单行探针。
    pub fn probe(&mut self, pos: usize) -> Result<(), u32> {
        if self.phase == SmkPhase::Sealed {
            return Err(SmkCode::RowIdOutOfRange.code());
        }
        self.ledger.queries = self.ledger.queries.saturating_add(1);
        let (lane, idx) = match Self::locate(pos) {
            Some(x) => x,
            None => {
                self.ledger.deviations = self.ledger.deviations.saturating_add(1);
                return Err(SmkCode::RowIdOutOfRange.code());
            }
        };
        if pos < self.cursor {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            if self.cursor == pos + 1 {
                return Err(SmkCode::RowDuplicate.code());
            }
            return Err(SmkCode::RowOutOfOrder.code());
        }
        if pos > self.cursor {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            return Err(SmkCode::RowOutOfOrder.code());
        }
        let (_l, sys, req, expect) = match smk_row(lane, idx) {
            Some(x) => x,
            None => {
                self.ledger.deviations = self.ledger.deviations.saturating_add(1);
                return Err(SmkCode::RowIdOutOfRange.code());
            }
        };
        // 门规校验（与比较核同闸：偏差面探针也要过门规）。
        let gate = suite_compare(if lane == 0 { VER_AND } else { VER_OR }, sys, req);
        let got = match gate {
            Ok(_) => self.eval(lane, sys, req),
            Err(e) => return Err(e),
        };
        if got != expect {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            return Err(SmkCode::VerdictDeviation.code());
        }
        self.cursor = pos + 1;
        self.ledger.passes = self.ledger.passes.saturating_add(1);
        self.phase = SmkPhase::Sweeping;
        Ok(())
    }

    /// 全表顺探（AND 道先）。
    pub fn probe_all(&mut self) -> Result<usize, u32> {
        let mut n = 0;
        let mut pos = 0usize;
        while pos < SUITE_ROWS {
            self.probe(pos)?;
            n += 1;
            pos += 1;
        }
        Ok(n)
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == SmkPhase::Open {
            return Err(SmkCode::TableEmpty.code());
        }
        if !self.ledger.conserve() {
            return Err(SmkCode::LedgerBroken.code());
        }
        self.phase = SmkPhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（过账数 / 账 / 指纹）。
pub type GoldenOut = (usize, SmkLedger, u32);

/// 黄金全链一次：20 例顺探全过 → 封账 → F18408 口径对账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut p = SuiteProbe::new();
    let n = p.probe_all()?;
    if n != SUITE_ROWS {
        return Err(SmkCode::VerdictDeviation.code());
    }
    p.seal()?;
    if !p.ledger.conserve() {
        return Err(SmkCode::LedgerBroken.code());
    }
    if p.ledger.passes != SUITE_ROWS as u32 || p.ledger.deviations != 0 {
        return Err(SmkCode::VerdictDeviation.code());
    }
    // F18408 口径：KNOWN_MASK = 0x27FF 且 12 位。
    let mut bits = 0usize;
    let mut i = 0;
    while i < KNOWN_BITS {
        if suite_bit(i).is_some() {
            bits += 1;
        }
        i += 1;
    }
    if bits != KNOWN_BITS || KNOWN_MASK != 0x27FF {
        return Err(SmkCode::CrossCheckMismatch.code());
    }
    Ok((n, p.ledger, table_fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SmkCode::NotReproducible.code());
    }
    Ok(())
}

// ── 交叉对账 ────────────────────────────────────────────────────
/// 交叉核对：位全集逐位值 + KNOWN_MASK 折叠 + 双语义分野。
pub fn cross_check() -> Result<(), u32> {
    // 逐位折叠 == KNOWN_MASK。
    let mut fold = 0u16;
    let mut i = 0;
    while i < KNOWN_BITS {
        let b = match suite_bit(i) {
            Some(b) => b,
            None => return Err(SmkCode::CrossCheckMismatch.code()),
        };
        fold |= b;
        i += 1;
    }
    if fold != KNOWN_MASK {
        return Err(SmkCode::CrossCheckMismatch.code());
    }
    // 双语义分野：全在场例 AND=T、OR=T；部分在场例 AND=F、OR=T；
    // 无交例 AND=F、OR=F。
    if !(suite_compare(VER_AND, 0x0100, 0x0100)? == true
        && suite_compare(VER_OR, 0x0100, 0x0100)? == true
        && suite_compare(VER_AND, 0x0100, 0x0101)? == false
        && suite_compare(VER_OR, 0x0100, 0x0101)? == true
        && suite_compare(VER_AND, 0x0100, 0x0001)? == false
        && suite_compare(VER_OR, 0x0100, 0x0001)? == false)
    {
        return Err(SmkCode::CrossCheckMismatch.code());
    }
    Ok(())
}

// ── 注入面统计 ──────────────────────────────────────────────────
/// AND 误作 OR 偏差面：AND 道中 (sys&req)!=0 但 !=req 的例数
/// （部分在场例——OR 会说 T 而 AND 期望 F）。
pub fn and_as_or_dev_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < LANE_ROWS {
        match and_row(i) {
            Some((_l, s, r, e)) => {
                if suite_as_or(s, r) != e {
                    n += 1;
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

/// OR 误作 AND 偏差面：OR 道中 (sys&req)==req 但 !=0 判不同的例数
/// （部分在场例——AND 会说 F 而 OR 期望 T）。
pub fn or_as_and_dev_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < LANE_ROWS {
        match or_row(i) {
            Some((_l, s, r, e)) => {
                if suite_as_and(s, r) != e {
                    n += 1;
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

// ── 注入核（四类 × 10 轮） ──────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 AND 误作 OR。
    pub andor_caught: u32,
    /// 类2 OR 误作 AND。
    pub orand_caught: u32,
    /// 类3 保留位请求。
    pub reserved_caught: u32,
    /// 类4 空请求。
    pub empty_caught: u32,
}

/// 四类注入 rounds 轮；每类每轮必抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1：AND 道偏置探针（AND 误作 OR）→ 部分在场例必偏。
        let mut p1 = SuiteProbe::new();
        p1.set_bias(Some(1));
        let mut caught1 = 0u32;
        let mut pos = 0usize;
        while pos < LANE_ROWS {
            match p1.probe(pos) {
                Err(c) if c == SmkCode::VerdictDeviation.code() => {
                    caught1 = caught1.saturating_add(1);
                    p1.cursor = pos + 1;
                }
                Ok(()) => {}
                _ => return Err(SmkCode::VerdictDeviation.code()),
            }
            pos += 1;
        }
        if caught1 != and_as_or_dev_rows() as u32 {
            return Err(SmkCode::VerdictDeviation.code());
        }
        led.andor_caught = led.andor_caught.saturating_add(caught1);
        // 类2：OR 道偏置探针（OR 误作 AND）→ 部分在场例必偏。
        // （AND 道先顺探过闸——偏置不影响 AND 道正确性。）
        let mut p2 = SuiteProbe::new();
        p2.set_bias(Some(2));
        let mut caught2 = 0u32;
        let mut pos2 = 0usize;
        while pos2 < SUITE_ROWS {
            match p2.probe(pos2) {
                Err(c) if c == SmkCode::VerdictDeviation.code() => {
                    caught2 = caught2.saturating_add(1);
                    p2.cursor = pos2 + 1;
                }
                Ok(()) => {}
                _ => return Err(SmkCode::VerdictDeviation.code()),
            }
            pos2 += 1;
        }
        if caught2 != or_as_and_dev_rows() as u32 {
            return Err(SmkCode::VerdictDeviation.code());
        }
        led.orand_caught = led.orand_caught.saturating_add(caught2);
        // 类3：保留位请求（四例轮换）→ ReservedBit。
        let bad = RESERVED_SAMPLES[(r % 4) as usize];
        match suite_compare(VER_AND, 0x0100, bad) {
            Err(c) if c == SmkCode::ReservedBit.code() => {
                led.reserved_caught = led.reserved_caught.saturating_add(1);
            }
            _ => return Err(SmkCode::ReservedBit.code()),
        }
        // 类4：空请求（AND/OR 轮换）→ EmptyRequest。
        let op4 = match r % 2 {
            0 => VER_AND,
            _ => VER_OR,
        };
        match suite_compare(op4, 0x0100, 0) {
            Err(c) if c == SmkCode::EmptyRequest.code() => {
                led.empty_caught = led.empty_caught.saturating_add(1);
            }
            _ => return Err(SmkCode::EmptyRequest.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：20 例全过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：非法算子（EQ）拒。
pub fn probe_seq_b() -> Result<(), u32> {
    suite_compare(1, 0x0100, 0x0100).map(|_| ())
}

/// 探针 C：保留位请求拒。
pub fn probe_seq_c() -> Result<(), u32> {
    suite_compare(VER_OR, 0x0100, 0x1000).map(|_| ())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < SmkCode::ALL.len() {
        let v = SmkCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（8–20 条；动态明细 ≤8） ────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：20 例全过（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == SUITE_ROWS && led.deviations == 0 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-20例位与全过", g_ok, format!("{:?}", g)));

    // 2. F18408 口径：KNOWN_MASK=0x27FF、12 位、逐位折叠一致。
    let cc = cross_check().is_ok();
    v.push(("J1-F18408口径对账", cc, String::from("KNOWN=0x27FF 12位")));

    // 3. VER_OR 任一在场即过（J1 口径直查）。
    let any_ok = suite_compare(VER_OR, 0x0100, 0x0101) == Ok(true)
        && suite_compare(VER_OR, 0x0001, 0x0003) == Ok(true)
        && suite_compare(VER_OR, 0x0100, 0x0001) == Ok(false);
    v.push(("J1-OR任一在场即过", any_ok, String::from("2真1假")));

    // 4. VER_AND 全在场才过（与 OR 分野）。
    let all_ok = suite_compare(VER_AND, 0x0100, 0x0101) == Ok(false)
        && suite_compare(VER_AND, 0x0104, 0x0100) == Ok(true)
        && suite_compare(VER_AND, 0x27FF, 0x27FF) == Ok(true);
    v.push(("J1-AND全在场才过", all_ok, String::from("1假2真")));

    // 5. AND 误 OR 注入（面额 × 10 轮）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let c1 = match led.as_ref() {
        Ok(l) => l.andor_caught,
        Err(_) => 0,
    };
    let e1 = and_as_or_dev_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-AND误OR全偏", c1 == e1, format!("{}/{}", c1, e1)));

    // 6. OR 误 AND 注入（面额 × 10 轮）。
    let c2 = match led.as_ref() {
        Ok(l) => l.orand_caught,
        Err(_) => 0,
    };
    let e2 = or_as_and_dev_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-OR误AND全偏", c2 == e2, format!("{}/{}", c2, e2)));

    // 7. 保留位注入 10/10。
    let c3 = match led.as_ref() {
        Ok(l) => l.reserved_caught,
        Err(_) => 0,
    };
    v.push(("J1-保留位注入10/10", c3 == JUDGE_ROUNDS, format!("caught={}", c3)));

    // 8. 空请求注入 10/10。
    let c4 = match led.as_ref() {
        Ok(l) => l.empty_caught,
        Err(_) => 0,
    };
    v.push(("J1-空请求注入10/10", c4 == JUDGE_ROUNDS, format!("caught={}", c4)));

    // 9. 注入账守恒。
    let cons_ok = match led.as_ref() {
        Ok(l) => l.andor_caught + l.orand_caught + l.reserved_caught + l.empty_caught
            == e1 + e2 + 2 * JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons_ok, String::from("4类面额×10轮")));

    // 10. 非法算子拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-非法算子拒", pb, format!("err={}", pb)));

    // 11. 保留位拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-保留位拒", pc, format!("err={}", pc)));

    // 12. 系统值越界拒（sys 携带保留位）。
    let sys_gate = suite_compare(VER_AND, 0x1000, 0x0100).is_err();
    v.push(("J1-系统值越界拒", sys_gate, String::from("sys=0x1000")));

    // 13. 行三闸：乱序/重复/越界。
    let gates = {
        let mut p1 = SuiteProbe::new();
        let _ = p1.probe(5);
        let ooo = p1.probe(2).is_err();
        let mut p2 = SuiteProbe::new();
        let _ = p2.probe(0);
        let dup = p2.probe(0).is_err();
        let mut p3 = SuiteProbe::new();
        let oob = p3.probe(SUITE_ROWS).is_err();
        ooo && dup && oob
    };
    v.push(("J1-行三闸乱重越", gates, String::from("乱序/重复/越界")));

    // 14. 空表收口拒。
    let empty = {
        let mut p = SuiteProbe::new();
        p.seal().is_err()
    };
    v.push(("J1-空表收口拒", empty, String::from("零探针")));

    // 15. 表指纹双跑一致。
    let fp = table_fingerprint();
    let fp2 = table_fingerprint();
    v.push(("J2-表指纹双跑", fp == fp2, format!("{:08X}", fp)));

    // 16. 双跑一致（复现红线）。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 17. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 18. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = SmkCode::ALL.len() == 12;
        let mut i = 0;
        while i < SmkCode::ALL.len() {
            if SmkCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 18 条；前 8 条动态明细）。
pub fn run_unx18510_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18510 SuiteMask 位与特例");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18510_checks();
        assert!(set.all_passed(), "UNX-F18510 判据集应全绿");
    }

    /// 黄金链：20 例全过。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 20);
                assert_eq!(led.passes, 20);
                assert_eq!(led.deviations, 0);
                assert!(led.conserve());
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// F18408 口径：位全集逐位值 + 折叠。
    #[test]
    fn test_known_mask() {
        assert_eq!(KNOWN_MASK, 0x27FF);
        let mut fold = 0u16;
        let mut i = 0;
        while i < KNOWN_BITS {
            match suite_bit(i) {
                Some(b) => {
                    assert_eq!(KNOWN_MASK & b, b, "位 {} 不在全集内", i);
                    fold |= b;
                }
                None => assert!(false, "位 {} 不应越界", i),
            }
            i += 1;
        }
        assert_eq!(fold, KNOWN_MASK);
        // 保留位样本全部在全集外。
        let mut k = 0;
        while k < RESERVED_SAMPLES.len() {
            assert_eq!(RESERVED_SAMPLES[k] & KNOWN_MASK, 0, "保留样本应全集外");
            k += 1;
        }
    }

    /// AND/OR 双语义分野逐例。
    #[test]
    fn test_and_or_semantics() {
        // 全在场：AND=T OR=T。
        assert_eq!(suite_compare(VER_AND, 0x0104, 0x0104), Ok(true));
        assert_eq!(suite_compare(VER_OR, 0x0104, 0x0104), Ok(true));
        // 部分在场：AND=F OR=T（J1「任一在场即过」）。
        assert_eq!(suite_compare(VER_AND, 0x0100, 0x0101), Ok(false));
        assert_eq!(suite_compare(VER_OR, 0x0100, 0x0101), Ok(true));
        // 无交：AND=F OR=F。
        assert_eq!(suite_compare(VER_AND, 0x0100, 0x0001), Ok(false));
        assert_eq!(suite_compare(VER_OR, 0x0100, 0x0001), Ok(false));
    }

    /// 门规：非法算子/空请求/保留位/系统越界 四闸。
    #[test]
    fn test_gates() {
        match suite_compare(1, 0x0100, 0x0100) {
            Err(c) => assert_eq!(c, SmkCode::OpForbidden.code()),
            Ok(_) => assert!(false, "非法算子必须拒"),
        }
        match suite_compare(VER_AND, 0x0100, 0) {
            Err(c) => assert_eq!(c, SmkCode::EmptyRequest.code()),
            Ok(_) => assert!(false, "空请求必须拒"),
        }
        match suite_compare(VER_OR, 0x0100, 0x8000) {
            Err(c) => assert_eq!(c, SmkCode::ReservedBit.code()),
            Ok(_) => assert!(false, "保留位必须拒"),
        }
        match suite_compare(VER_AND, 0x8000, 0x0100) {
            Err(c) => assert_eq!(c, SmkCode::SysOutOfRange.code()),
            Ok(_) => assert!(false, "系统越界必须拒"),
        }
    }

    /// 注入四类全抓（10 轮）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.andor_caught, and_as_or_dev_rows() as u32 * 10);
                assert_eq!(l.orand_caught, or_as_and_dev_rows() as u32 * 10);
                assert_eq!(l.reserved_caught, 10);
                assert_eq!(l.empty_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 行三闸。
    #[test]
    fn test_row_gates() {
        let mut p1 = SuiteProbe::new();
        assert!(p1.probe(5).is_ok());
        match p1.probe(2) {
            Err(c) => assert_eq!(c, SmkCode::RowOutOfOrder.code()),
            Ok(()) => assert!(false, "乱序必须拒"),
        }
        let mut p2 = SuiteProbe::new();
        assert!(p2.probe(0).is_ok());
        match p2.probe(0) {
            Err(c) => assert_eq!(c, SmkCode::RowDuplicate.code()),
            Ok(()) => assert!(false, "重复必须拒"),
        }
        let mut p3 = SuiteProbe::new();
        match p3.probe(SUITE_ROWS) {
            Err(c) => assert_eq!(c, SmkCode::RowIdOutOfRange.code()),
            Ok(()) => assert!(false, "越界必须拒"),
        }
    }

    /// 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
