//! UNX-F49075 延迟账本运营与 O1 对照（判据主轴运营）错误码族（15 条）· 0x73440
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定）：
//!   UNX-F49075 延迟账本运营与 O1 对照（判据主轴运营）错误码族（15 条）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49076-J1 四点 100% 覆盖；异常零静默；
//!             判据锚定 体验十三·补。
//!   UNX-F49076 延迟账本运营与 O1 对照（判据主轴运营）观测埋点
//!           （入口/出口/耗时/结果四点全覆盖）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49077-J1 fuzz ×10^4 零崩；
//!             判据锚定 体验十。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（码族）：窗口 → 占位 → 兜底 → 触达。
//! 窗口先于占位：先证明 14 条码各自落在 [SEG_BASE+1, SEG_BASE+14] 这个闭区间里，
//!   再谈它们是否互相占重——落窗之外的码连「本段诊断码」都不是，占位与否无意义；
//! 占位先于兜底：14 条必须恰好占满 14 个码位且无重复，占位一有空洞，
//!   「越界哨」就会被迫去填洞，兜底语义被污染（哨从此既是哨又是已知码）；
//! 兜底先于触达：哨位口径（段外一律 UNKNOWN）先钉死，触达统计才有分母——
//!   否则「15 条都出现过」这句话里的 15 到底含不含哨，无法核验；
//! 触达最后：只有前三级都成立，「14 条已知码 + 1 条哨各自至少命中一次」才是
//!   对码族完备性的证明，而不是对一张错表的统计。
//! 闸门次序（埋点）：取号 → 入口 → 出口 → 耗时 → 结果。
//! 取号先于入口：没有票据就无法把「这一次观测」与「另一次观测」区分开，
//!   后四个点会退化成四个孤立数字（账本上认不出它们属于同一次调用）；
//! 入口先于出口：出口时间戳单独存在时，耗时的被减数还没落地；
//! 出口先于耗时：耗时是对「出口 − 入口」的复算，两个端点不齐就不该有耗时；
//! 耗时先于结果：耗时是结果可信度的前提——结果码写完再补耗时，
//!   等于允许一条「先判生死、后补证据」的记录入账；
//! 结果最后：四点闭环的终点，只有它落地这一次观测才可被审计。
//! 关键差异：码族面只回答「这个码是不是本段的、说明是什么、有没有被用过」；
//! 埋点面只回答「这一次调用四个点齐不齐、次序对不对、耗时对得上账上的差值吗」。
//! 两面共用同一套账行格式（tag|k1..k4|digest），但摘要域各自独立：
//! 码族摘要折 (op, ticket, enter, exit, cost, result, stage)——埋点全字段，
//! 账行摘要折 (tag, k1, k2, k3, k4)——行内可见字段。二者都不含摘要自身，
//! 否则一位数字的篡改就能把摘要一并带走，等于没有摘要。
//! 口径说明（「15 条码族」到底怎么数）：
//!   段内 14 条已知码恰好占满 [SEG_BASE+0x01, SEG_BASE+0x0E]（闭集）；
//!   第 15 条是「越界哨」，它**不占任何码位**，只在 detail() 的默认分支里
//!   兜底 UNKNOWN，并在 CodeHist 里占用第 15 号统计槽。故
//!   FAMILY_SIZE = 15 = KNOWN_CODES(14) + SENTINEL_SLOT(1)，
//!   凡本文件说「15 条」，指的都是这个口径——哨算一条「口径」不算一条「已知码」；
//!   is_known() 对哨恒为假，任何把哨当已知码用的路径都会被 audit_family 打回。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::unx48941_wheel_core as p_core;
use crate::checks::CheckSet;

/// 诊断段基址（0x73440，B14 族第七槽）。
pub const SEG_BASE: u32 = 0x73440;

/// 段内已知码条数（闭集 14 条）。
pub const KNOWN_CODES: usize = 14;
/// 越界哨统计槽（不占码位的第 15 条）。
pub const SENTINEL_SLOT: u8 = 15;
/// 码族口径条数（已知 14 + 哨 1）。
pub const FAMILY_SIZE: usize = 15;
/// 统计槽总数（0 号槽留空不用，1..=14 已知码，15 哨）。
pub const SLOTS: usize = 16;
/// 四点埋点数（入口/出口/耗时/结果）。
pub const POINTS: usize = 4;
/// 埋点阶段数（取号 + 四点）。
pub const STAGES: usize = 5;
/// 时间戳上限（1 秒，纳秒）。
pub const NS_LIMIT: u32 = 1_000_000_000;
/// 耗时预算（微秒级 50µs；超过即记「未达时」）。
pub const COST_BUDGET_NS: u32 = 50_000;
/// 票据容量（取号上界，含）。
pub const TICKET_CAP: u32 = 4_096;
/// 单本账的操作容量。
pub const OP_CAP: usize = 64;
/// 单条观测导出的账行数（取号 1 + 四点 4）。
pub const TRACE_ROWS: usize = 5;
/// 账行数字键（固定列序，不随标签变化）。
pub const ROW_KEYS: [&str; 4] = ["k1", "k2", "k3", "k4"];
/// 账行字段数（tag + 4 键 + digest）。
pub const ROW_FIELDS: usize = 6;
/// 未知诊断码说明（越界哨的文案出口）。
pub const UNKNOWN: &str = "未知 B14 错误码族/埋点诊断码";
/// 阶段序号：取号。
pub const STAGE_TICKET: u8 = 0;
/// 阶段序号：入口。
pub const STAGE_ENTER: u8 = 1;
/// 阶段序号：出口。
pub const STAGE_EXIT: u8 = 2;
/// 阶段序号：耗时。
pub const STAGE_COST: u8 = 3;
/// 阶段序号：结果。
pub const STAGE_RESULT: u8 = 4;
/// 阶段序号：闭环完成。
pub const STAGE_CLOSED: u8 = 5;
/// 四点齐全掩码。
pub const FOUR_POINT_MASK: u8 = 0x0F;
/// 取号账行标签。
pub const TICKET_TAG: &str = "TKT";
/// 入口账行标签。
pub const ENTER_TAG: &str = "PTEN";
/// 出口账行标签。
pub const EXIT_TAG: &str = "PTEX";
/// 耗时账行标签。
pub const COST_TAG: &str = "PTCT";
/// 结果账行标签。
pub const RESULT_TAG: &str = "PTRS";
/// 五阶标签（下标即阶段序号）。
pub const STAGE_TAGS: [&str; STAGES] = [
    TICKET_TAG, ENTER_TAG, EXIT_TAG, COST_TAG, RESULT_TAG,
];
/// 五阶中文名（下标即阶段序号）。
pub const STAGE_NAMES: [&str; STAGES] = ["取号", "入口", "出口", "耗时", "结果"];

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 错误码族/埋点诊断码（段内 14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Code {
    /// 码落在本段窗口之外
    CodeOutOfSegment,
    /// 段内码位被重复占用
    CodeDuplicated,
    /// 段内码位出现空洞（未占满 14 位）
    CodeGap,
    /// 票据为零、超容或重号
    TicketBad,
    /// 埋点阶段序号越出五阶
    StageBad,
    /// 四点次序断裂
    OrderBad,
    /// 四点缺项即入账
    PointMissing,
    /// 耗时或时间戳与进出口不自洽
    CostBad,
    /// 结果码不在已知码族
    ResultBad,
    /// 操作号为零、重复或超上限
    OpBad,
    /// 摘要与行内/账内字段不符
    DigestBad,
    /// 账行键序、数字或分隔符非法
    RenderBad,
    /// 账本容量已满
    CapacityBad,
    /// 异常未入码账（零静默违约）
    SilentBad,
}

impl Code {
    /// 诊断码数值（段内 0x01..0x0E）。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 码的说明（走 detail，保证与 detail 同源）。
    pub fn message(self) -> &'static str {
        detail(self.val())
    }

    /// 全码表（14 条，段内闭集）。
    pub const ALL: [Code; KNOWN_CODES] = [
        Code::CodeOutOfSegment,
        Code::CodeDuplicated,
        Code::CodeGap,
        Code::TicketBad,
        Code::StageBad,
        Code::OrderBad,
        Code::PointMissing,
        Code::CostBad,
        Code::ResultBad,
        Code::OpBad,
        Code::DigestBad,
        Code::RenderBad,
        Code::CapacityBad,
        Code::SilentBad,
    ];
}

/// 码值到说明；段外一律 UNKNOWN，不泄漏邻段文案。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => "诊断码落在本段窗口之外",
        2 => "段内码位被重复占用",
        3 => "段内码位出现空洞未占满十四位",
        4 => "票据为零、超容或在本账内重号",
        5 => "埋点阶段序号越出取号至结果五阶",
        6 => "四点次序断裂（未取号先入口等）",
        7 => "四点缺项即被要求入账",
        8 => "耗时或时间戳与进出口差值不自洽",
        9 => "结果码不在本段已知码族内",
        10 => "操作号为零、本账重复或超出容量上限",
        11 => "摘要与行内或账内字段不符",
        12 => "账行键序、数字或分隔符非法",
        13 => "账本操作容量已满",
        14 => "异常未入码账（零静默被违约）",
        _ => UNKNOWN,
    }
}

/// 段位守卫：14 码恰好占满 [SEG_BASE+1, SEG_BASE+14]，且互不重复。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0E;
    let mut i = 0usize;
    while i < Code::ALL.len() {
        let v = match Code::ALL.get(i) {
            Some(c) => c.val(),
            None => return false,
        };
        if v < lo || v > hi {
            return false;
        }
        let mut j = i + 1;
        while j < Code::ALL.len() {
            if let Some(other) = Code::ALL.get(j) {
                if other.val() == v {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 段内窗口下界（含）。
pub fn window_lo() -> u32 {
    SEG_BASE + 0x01
}

/// 段内窗口上界（含）。
pub fn window_hi() -> u32 {
    SEG_BASE + 0x0E
}

/// 已知码判定：只有段内 14 位算已知，哨不算。
pub fn is_known(code: u32) -> bool {
    code >= window_lo() && code <= window_hi()
}

/// 统计槽归属：段内 1..=14，段外一律第 15 槽（哨）。
pub fn family_slot(code: u32) -> u8 {
    if is_known(code) {
        (code - SEG_BASE) as u8
    } else {
        SENTINEL_SLOT
    }
}

/// 码族口径条数（14 已知 + 1 哨 = 15）。
pub fn family_len() -> usize {
    KNOWN_CODES + 1
}

/// 段内 14 个码值（升序，供表审计）。
pub fn family_values() -> [u32; KNOWN_CODES] {
    let mut out = [0u32; KNOWN_CODES];
    let mut i = 0usize;
    while i < KNOWN_CODES {
        out[i] = SEG_BASE + 0x01 + i as u32;
        i += 1;
    }
    out
}

/// 对任意一张码表做窗口/重复/空洞审计：段外 → CodeOutOfSegment，
/// 重复 → CodeDuplicated，条数不足 14（含空表）→ CodeGap。
pub fn audit_family(codes: &[u32]) -> Result<(), u32> {
    let lo = window_lo();
    let hi = window_hi();
    let mut i = 0usize;
    while i < codes.len() {
        let c = match codes.get(i) {
            Some(v) => *v,
            None => return Err(Code::CodeGap.val()),
        };
        if c < lo || c > hi {
            return Err(Code::CodeOutOfSegment.val());
        }
        let mut j = i + 1;
        while j < codes.len() {
            if let Some(other) = codes.get(j) {
                if *other == c {
                    return Err(Code::CodeDuplicated.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    if codes.len() != KNOWN_CODES {
        return Err(Code::CodeGap.val());
    }
    Ok(())
}

/// 把 64 位摘要折入 64 位累加器（高低两半各折一次）。
fn fold_digest(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

// ── 码账（触达直方图） ───────────────────────────────────────────
/// 15 槽码账：1..=14 记已知码，15 记哨。0 号槽恒为 0，只作对齐占位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeHist {
    counts: [u32; SLOTS],
}

impl CodeHist {
    /// 空账。
    pub const fn new() -> CodeHist {
        CodeHist { counts: [0u32; SLOTS] }
    }

    /// 记一次命中，返回落入的槽号。
    pub fn bump(&mut self, code: u32) -> u8 {
        let slot = family_slot(code) as usize;
        if let Some(c) = self.counts.get_mut(slot) {
            *c = c.saturating_add(1);
        }
        slot as u8
    }

    /// 某槽计数（越界槽返回 0）。
    pub fn slot_count(&self, slot: u8) -> u32 {
        match self.counts.get(slot as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 全部槽总计数（不含 0 号占位槽）。
    pub fn total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 1usize;
        while i < SLOTS {
            if let Some(v) = self.counts.get(i) {
                sum = sum.saturating_add(*v);
            }
            i += 1;
        }
        sum
    }

    /// 已知码计数（1..=14）。
    pub fn known_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 1usize;
        while i <= KNOWN_CODES {
            if let Some(v) = self.counts.get(i) {
                sum = sum.saturating_add(*v);
            }
            i += 1;
        }
        sum
    }

    /// 哨计数（第 15 槽）。
    pub fn sentinel_total(&self) -> u32 {
        self.slot_count(SENTINEL_SLOT)
    }

    /// 被触达的已知码个数（计数 > 0 的槽数，1..=14）。
    pub fn touched_known(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 1usize;
        while i <= KNOWN_CODES {
            if let Some(v) = self.counts.get(i) {
                if *v > 0 {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 被触达的总槽数（含哨）。
    pub fn touched_slots(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 1usize;
        while i < SLOTS {
            if let Some(v) = self.counts.get(i) {
                if *v > 0 {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 0 号占位槽必须恒零——它一旦有数，说明 family_slot 出了负数回绕。
    pub fn guard_slot_zero(&self) -> bool {
        match self.counts.first() {
            Some(v) => *v == 0,
            None => false,
        }
    }

    /// 合并两本账（逐槽饱和相加）。
    pub fn merge(&mut self, other: &CodeHist) {
        let mut i = 0usize;
        while i < SLOTS {
            if let (Some(a), Some(b)) = (self.counts.get_mut(i), other.counts.get(i)) {
                *a = a.saturating_add(*b);
            }
            i += 1;
        }
    }
}

/// 双记账对账：独立计数器 observed 与码账 total 必须逐值相等。
/// 少记 → 静默吞异常；多记 → 虚报。两种都归 SilentBad，不区分委屈哪一方。
pub fn reconcile(observed: u32, hist: &CodeHist) -> Result<(), u32> {
    let booked = hist.total();
    if booked < observed {
        return Err(Code::SilentBad.val());
    }
    if booked > observed {
        return Err(Code::SilentBad.val());
    }
    Ok(())
}

// ── 埋点阶段 ─────────────────────────────────────────────────────
/// 五阶埋点（取号 + 四点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Point {
    /// 取号
    Ticket,
    /// 入口
    Enter,
    /// 出口
    Exit,
    /// 耗时
    Cost,
    /// 结果
    Result,
}

impl Point {
    /// 阶段序号（0..=4）。
    pub fn index(self) -> u8 {
        match self {
            Point::Ticket => STAGE_TICKET,
            Point::Enter => STAGE_ENTER,
            Point::Exit => STAGE_EXIT,
            Point::Cost => STAGE_COST,
            Point::Result => STAGE_RESULT,
        }
    }

    /// 阶段中文名。
    pub fn name(self) -> &'static str {
        match STAGE_NAMES.get(self.index() as usize) {
            Some(v) => v,
            None => "?",
        }
    }

    /// 阶段账行标签。
    pub fn tag(self) -> &'static str {
        match STAGE_TAGS.get(self.index() as usize) {
            Some(v) => v,
            None => "?",
        }
    }

    /// 是否属于「四点」（取号不算四点）。
    pub fn is_four_point(self) -> bool {
        self.index() >= STAGE_ENTER
    }

    /// 由序号反查；越界为 StageBad。
    pub fn from_index(i: u8) -> Result<Point, u32> {
        match i {
            0 => Ok(Point::Ticket),
            1 => Ok(Point::Enter),
            2 => Ok(Point::Exit),
            3 => Ok(Point::Cost),
            4 => Ok(Point::Result),
            _ => Err(Code::StageBad.val()),
        }
    }
}

/// 四点对应的位（入口 bit0，出口 bit1，耗时 bit2，结果 bit3）。
pub fn point_bit(p: Point) -> u8 {
    match p {
        Point::Ticket => 0,
        Point::Enter => 1 << 0,
        Point::Exit => 1 << 1,
        Point::Cost => 1 << 2,
        Point::Result => 1 << 3,
    }
}

// ── 单次观测 ─────────────────────────────────────────────────────
/// 一次埋点观测：操作号 + 票据 + 四点 + 阶段游标 + 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpTrace {
    /// 操作号（非零）。
    pub op: u32,
    /// 票据号（1..=TICKET_CAP）。
    pub ticket: u32,
    /// 入口时间戳（纳秒）。
    pub enter_ns: u32,
    /// 出口时间戳（纳秒）。
    pub exit_ns: u32,
    /// 耗时（必须等于 exit − enter）。
    pub cost_ns: u32,
    /// 结果码（必须是段内已知码）。
    pub result_code: u32,
    /// 阶段游标（0..=5）。
    pub stage: u8,
    /// 摘要（折全字段，不含自身）。
    pub digest: u64,
}

/// 观测摘要：折 (op, ticket, enter, exit, cost, result, stage)。
pub fn trace_digest(
    op: u32,
    ticket: u32,
    enter_ns: u32,
    exit_ns: u32,
    cost_ns: u32,
    result_code: u32,
    stage: u8,
) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, op);
    h = p_core::fold_u32(h, ticket);
    h = p_core::fold_u32(h, enter_ns);
    h = p_core::fold_u32(h, exit_ns);
    h = p_core::fold_u32(h, cost_ns);
    h = p_core::fold_u32(h, result_code);
    h = p_core::fold_byte(h, stage);
    h
}

impl OpTrace {
    /// 开一次观测：只接受非零操作号。
    pub fn new(op: u32) -> Result<OpTrace, u32> {
        if op == 0 {
            return Err(Code::OpBad.val());
        }
        Ok(OpTrace {
            op,
            ticket: 0,
            enter_ns: 0,
            exit_ns: 0,
            cost_ns: 0,
            result_code: 0,
            stage: STAGE_TICKET,
            digest: trace_digest(op, 0, 0, 0, 0, 0, STAGE_TICKET),
        })
    }

    /// 摘要在每次成功推进后重算（保持 verify 恒可用）。
    fn refresh(&mut self) {
        self.digest = trace_digest(
            self.op,
            self.ticket,
            self.enter_ns,
            self.exit_ns,
            self.cost_ns,
            self.result_code,
            self.stage,
        );
    }

    /// 阶段守卫：只允许从 expected 推进。
    fn expect_stage(&self, expected: u8) -> Result<(), u32> {
        if self.stage != expected {
            return Err(Code::OrderBad.val());
        }
        Ok(())
    }

    /// 取号：票据不得为零、不得超容。
    pub fn take_ticket(&mut self, ticket: u32) -> Result<(), u32> {
        self.expect_stage(STAGE_TICKET)?;
        if ticket == 0 || ticket > TICKET_CAP {
            return Err(Code::TicketBad.val());
        }
        self.ticket = ticket;
        self.stage = STAGE_ENTER;
        self.refresh();
        Ok(())
    }

    /// 入口：时间戳必须落在 (0, 1s]。
    pub fn enter(&mut self, ns: u32) -> Result<(), u32> {
        self.expect_stage(STAGE_ENTER)?;
        if ns == 0 || ns > NS_LIMIT {
            return Err(Code::CostBad.val());
        }
        self.enter_ns = ns;
        self.stage = STAGE_EXIT;
        self.refresh();
        Ok(())
    }

    /// 出口：必须严格晚于入口且不超 1s。
    pub fn exit(&mut self, ns: u32) -> Result<(), u32> {
        self.expect_stage(STAGE_EXIT)?;
        if ns <= self.enter_ns || ns > NS_LIMIT {
            return Err(Code::CostBad.val());
        }
        self.exit_ns = ns;
        self.stage = STAGE_COST;
        self.refresh();
        Ok(())
    }

    /// 耗时：必须恰好等于出口 − 入口（复算，不接受自报值）。
    pub fn cost(&mut self, ns: u32) -> Result<(), u32> {
        self.expect_stage(STAGE_COST)?;
        if self.exit_ns <= self.enter_ns {
            return Err(Code::CostBad.val());
        }
        let want = self.exit_ns - self.enter_ns;
        if ns != want || ns > NS_LIMIT {
            return Err(Code::CostBad.val());
        }
        self.cost_ns = ns;
        self.stage = STAGE_RESULT;
        self.refresh();
        Ok(())
    }

    /// 结果：码必须落在段内已知族（哨不算已知）。
    pub fn result(&mut self, code: u32) -> Result<(), u32> {
        self.expect_stage(STAGE_RESULT)?;
        if !is_known(code) {
            return Err(Code::ResultBad.val());
        }
        self.result_code = code;
        self.stage = STAGE_CLOSED;
        self.refresh();
        Ok(())
    }

    /// 四点是否齐全。
    pub fn four_points(&self) -> bool {
        self.points_mask() == FOUR_POINT_MASK
    }

    /// 四点掩码（入口 bit0 … 结果 bit3）。
    pub fn points_mask(&self) -> u8 {
        let mut m = 0u8;
        if self.stage >= STAGE_EXIT {
            m |= point_bit(Point::Enter);
        }
        if self.stage >= STAGE_COST {
            m |= point_bit(Point::Exit);
        }
        if self.stage >= STAGE_RESULT {
            m |= point_bit(Point::Cost);
        }
        if self.stage >= STAGE_CLOSED {
            m |= point_bit(Point::Result);
        }
        m
    }

    /// 已落地点个数（0..=4）。
    pub fn point_count(&self) -> u32 {
        let mut n = 0u32;
        let mut m = self.points_mask();
        while m != 0 {
            n = n.saturating_add((m & 1) as u32);
            m >>= 1;
        }
        n
    }

    /// 是否在耗时预算内。
    pub fn on_time(&self) -> bool {
        self.cost_ns <= COST_BUDGET_NS
    }

    /// 耗时与进出口是否对得上账。
    pub fn cost_balanced(&self) -> bool {
        if self.stage < STAGE_RESULT {
            return false;
        }
        self.exit_ns > self.enter_ns && self.cost_ns == self.exit_ns - self.enter_ns
    }

    /// 闭环完成。
    pub fn closed(&self) -> bool {
        self.stage == STAGE_CLOSED
    }

    /// 摘要自洽。
    pub fn verify(&self) -> bool {
        self.digest
            == trace_digest(
                self.op,
                self.ticket,
                self.enter_ns,
                self.exit_ns,
                self.cost_ns,
                self.result_code,
                self.stage,
            )
    }

    /// 阶段名。
    pub fn stage_name(&self) -> &'static str {
        if self.stage >= STAGE_CLOSED {
            return "闭环";
        }
        match STAGE_NAMES.get(self.stage as usize) {
            Some(v) => v,
            None => "?",
        }
    }
}

/// 闭环闭合性：四点齐 + 次序正确 + 耗时对账 + 结果在族 + 摘要自洽。
pub fn trace_closed_ok(t: &OpTrace) -> bool {
    t.closed()
        && t.four_points()
        && t.cost_balanced()
        && is_known(t.result_code)
        && t.verify()
}

// ── 账行渲染与回读 ───────────────────────────────────────────────
/// 16 位小写十六进制。
fn hex16(v: u64) -> String {
    format!("{:016x}", v)
}

/// 十进制无符号解析（拒绝空串、非数字、溢出）。
fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 10 {
        return None;
    }
    let mut v = 0u64;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.saturating_mul(10).saturating_add((b - b'0') as u64);
        if v > u32::MAX as u64 {
            return None;
        }
    }
    Some(v as u32)
}

/// 16 位十六进制解析（大小写均收，长度必须恰为 16）。
fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v = 0u64;
    for b in s.bytes() {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        v = (v << 4) | d as u64;
    }
    Some(v)
}

/// 账行摘要：只折行内可见字段（tag + 四键），不含摘要自身。
pub fn row_digest(tag: &str, a: u32, b: u32, c: u32, d: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    for byte in tag.as_bytes() {
        h = p_core::fold_byte(h, *byte);
    }
    h = p_core::fold_u32(h, a);
    h = p_core::fold_u32(h, b);
    h = p_core::fold_u32(h, c);
    h = p_core::fold_u32(h, d);
    h
}

/// 渲染一条账行：`TAG|k1=..|k2=..|k3=..|k4=..|digest=16位小写hex`。
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|k1={}|k2={}|k3={}|k4={}|digest={}",
        tag,
        a,
        b,
        c,
        d,
        hex16(digest)
    )
}

/// 按固定标签与键序切行，返回各键值 + 摘要串（末位）。
pub fn row_fields<'a>(row: &'a str, tag: &str, keys: &[&str]) -> Option<Vec<&'a str>> {
    let mut it = row.split('|');
    if it.next() != Some(tag) {
        return None;
    }
    let mut vals: Vec<&'a str> = Vec::with_capacity(keys.len() + 1);
    let mut i = 0usize;
    while i < keys.len() {
        let part = it.next()?;
        let (k, v) = part.split_once('=')?;
        if k != *keys.get(i)? {
            return None;
        }
        vals.push(v);
        i += 1;
    }
    let tail = it.next()?;
    let (dk, dv) = tail.split_once('=')?;
    if dk != "digest" || it.next().is_some() {
        return None;
    }
    vals.push(dv);
    Some(vals)
}

/// 通用账行审计：键序 → 数字 → 摘要重算 → 往返恒等。
pub fn audit_render(row: &str, tag: &str) -> bool {
    let vals = match row_fields(row, tag, &ROW_KEYS) {
        Some(v) => v,
        None => return false,
    };
    let a = match vals.first().and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    let b = match vals.get(1).and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    let c = match vals.get(2).and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    let d = match vals.get(3).and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    let digest = match vals.get(4).and_then(|s| parse_hex16(s)) {
        Some(v) => v,
        None => return false,
    };
    if digest != row_digest(tag, a, b, c, d) {
        return false;
    }
    render_row(tag, a, b, c, d, digest) == row
}

/// 四列语义：某标签下 k1..k4 分别代表什么（回读时逐列按此校验）。
pub fn row_columns(tag: &str) -> Option<[&'static str; ROW_KEYS.len()]> {
    if tag == TICKET_TAG {
        return Some(["op", "ticket", "ticket", "reserved"]);
    }
    if tag == ENTER_TAG {
        return Some(["op", "ticket", "enter_ns", "stage"]);
    }
    if tag == EXIT_TAG {
        return Some(["op", "ticket", "exit_ns", "stage"]);
    }
    if tag == COST_TAG {
        return Some(["op", "ticket", "cost_ns", "on_time"]);
    }
    if tag == RESULT_TAG {
        return Some(["op", "ticket", "result_code", "slot"]);
    }
    None
}

/// 埋点账行审计：通用结构 + 逐标签范围/列间一致性。
pub fn audit_point(row: &str) -> bool {
    let mut tag = "";
    let mut k = 0usize;
    while k < STAGES {
        match STAGE_TAGS.get(k) {
            Some(t) => {
                if row.len() >= t.len() && row.get(0..t.len()) == Some(*t) {
                    tag = t;
                    break;
                }
            }
            None => break,
        }
        k += 1;
    }
    if tag.is_empty() {
        return false;
    }
    if !audit_render(row, tag) {
        return false;
    }
    let vals = match row_fields(row, tag, &ROW_KEYS) {
        Some(v) => v,
        None => return false,
    };
    let a = match vals.first().and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    let b = match vals.get(1).and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    let c = match vals.get(2).and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    let d = match vals.get(3).and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    if a == 0 {
        return false;
    }
    if b == 0 || b > TICKET_CAP {
        return false;
    }
    if tag == TICKET_TAG {
        // 票据双写对账：k3 必须等于 k2，k4 必须为 0
        return c == b && d == 0;
    }
    if c > NS_LIMIT {
        return false;
    }
    if tag == ENTER_TAG {
        return c > 0 && d == STAGE_ENTER as u32;
    }
    if tag == EXIT_TAG {
        return c > 0 && d == STAGE_EXIT as u32;
    }
    if tag == COST_TAG {
        let want = if c <= COST_BUDGET_NS { 1u32 } else { 0u32 };
        return c > 0 && d == want;
    }
    if tag == RESULT_TAG {
        return is_known(c) && d == family_slot(c) as u32;
    }
    false
}

/// 导出一条观测的五行账（取号 + 四点，固定次序）；未闭环返回空表。
pub fn trace_rows(t: &OpTrace) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(TRACE_ROWS);
    if !t.four_points() || t.ticket == 0 || t.op == 0 {
        return out;
    }
    out.push(render_row(
        TICKET_TAG,
        t.op,
        t.ticket,
        t.ticket,
        0,
        row_digest(TICKET_TAG, t.op, t.ticket, t.ticket, 0),
    ));
    out.push(render_row(
        ENTER_TAG,
        t.op,
        t.ticket,
        t.enter_ns,
        STAGE_ENTER as u32,
        row_digest(ENTER_TAG, t.op, t.ticket, t.enter_ns, STAGE_ENTER as u32),
    ));
    out.push(render_row(
        EXIT_TAG,
        t.op,
        t.ticket,
        t.exit_ns,
        STAGE_EXIT as u32,
        row_digest(EXIT_TAG, t.op, t.ticket, t.exit_ns, STAGE_EXIT as u32),
    ));
    let on_time = if t.on_time() { 1u32 } else { 0u32 };
    out.push(render_row(
        COST_TAG,
        t.op,
        t.ticket,
        t.cost_ns,
        on_time,
        row_digest(COST_TAG, t.op, t.ticket, t.cost_ns, on_time),
    ));
    out.push(render_row(
        RESULT_TAG,
        t.op,
        t.ticket,
        t.result_code,
        family_slot(t.result_code) as u32,
        row_digest(
            RESULT_TAG,
            t.op,
            t.ticket,
            t.result_code,
            family_slot(t.result_code) as u32,
        ),
    ));
    out
}

/// 逐行审计一组账行，首个坏行返回 RenderBad。
pub fn audit_rows(rows: &[String]) -> Result<(), u32> {
    let mut i = 0usize;
    while i < rows.len() {
        match rows.get(i) {
            Some(r) => {
                if !audit_point(r) {
                    return Err(Code::RenderBad.val());
                }
            }
            None => return Err(Code::RenderBad.val()),
        }
        i += 1;
    }
    Ok(())
}

/// 一组五行账的跨行对账：标签次序、op/ticket 一致、进出口耗时三点成账。
pub fn audit_trace(rows: &[String]) -> bool {
    if rows.len() != TRACE_ROWS {
        return false;
    }
    if audit_rows(rows).is_err() {
        return false;
    }
    let mut k = 0usize;
    while k < TRACE_ROWS {
        match rows.get(k) {
            Some(r) => {
                let tag = match STAGE_TAGS.get(k) {
                    Some(t) => *t,
                    None => return false,
                };
                let vals = match row_fields(r, tag, &ROW_KEYS) {
                    Some(v) => v,
                    None => return false,
                };
                let a = match vals.first().and_then(|s| parse_u32(s)) {
                    Some(v) => v,
                    None => return false,
                };
                let b = match vals.get(1).and_then(|s| parse_u32(s)) {
                    Some(v) => v,
                    None => return false,
                };
                let first = match rows.first() {
                    Some(f) => f,
                    None => return false,
                };
                let fv = match row_fields(first, TICKET_TAG, &ROW_KEYS) {
                    Some(v) => v,
                    None => return false,
                };
                let fa = match fv.first().and_then(|s| parse_u32(s)) {
                    Some(v) => v,
                    None => return false,
                };
                let fb = match fv.get(1).and_then(|s| parse_u32(s)) {
                    Some(v) => v,
                    None => return false,
                };
                if a != fa || b != fb {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    let ent = match rows.get(1).and_then(|r| row_fields(r, ENTER_TAG, &ROW_KEYS)) {
        Some(v) => v,
        None => return false,
    };
    let ext = match rows.get(2).and_then(|r| row_fields(r, EXIT_TAG, &ROW_KEYS)) {
        Some(v) => v,
        None => return false,
    };
    let cst = match rows.get(3).and_then(|r| row_fields(r, COST_TAG, &ROW_KEYS)) {
        Some(v) => v,
        None => return false,
    };
    let enter = match ent.get(2).and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    let exit = match ext.get(2).and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    let cost = match cst.get(2).and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    if exit <= enter {
        return false;
    }
    if cost != exit - enter {
        return false;
    }
    let on_time = match cst.get(3).and_then(|s| parse_u32(s)) {
        Some(v) => v,
        None => return false,
    };
    let want = if cost <= COST_BUDGET_NS { 1u32 } else { 0u32 };
    on_time == want
}

// ── 观测账本 ─────────────────────────────────────────────────────
/// 观测账本：操作唯一、票据唯一、容量受控、码账双记账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OpLedger {
    ops: Vec<OpTrace>,
    tickets: Vec<u32>,
    rejected: u32,
    hist: CodeHist,
    digest: u64,
}

impl OpLedger {
    /// 空账。
    pub fn new() -> OpLedger {
        OpLedger {
            ops: Vec::new(),
            tickets: Vec::new(),
            rejected: 0,
            hist: CodeHist::new(),
            digest: p_core::FNV_OFF,
        }
    }

    /// 记一笔拒收：独立计数器与码账必须同步，缺一即为静默。
    fn reject(&mut self, code: u32) -> u32 {
        self.rejected = self.rejected.saturating_add(1);
        self.hist.bump(code);
        code
    }

    /// 入账：闭环 → 摘要 → 操作唯一 → 票据唯一 → 容量。
    pub fn push(&mut self, t: OpTrace) -> Result<(), u32> {
        if !t.four_points() {
            return Err(self.reject(Code::PointMissing.val()));
        }
        if !t.verify() {
            return Err(self.reject(Code::DigestBad.val()));
        }
        if !is_known(t.result_code) {
            return Err(self.reject(Code::ResultBad.val()));
        }
        if self.find(t.op).is_some() {
            return Err(self.reject(Code::OpBad.val()));
        }
        if self.tickets.contains(&t.ticket) {
            return Err(self.reject(Code::TicketBad.val()));
        }
        if self.ops.len() >= OP_CAP {
            return Err(self.reject(Code::CapacityBad.val()));
        }
        self.ops.push(t);
        self.tickets.push(t.ticket);
        self.digest = fold_digest(self.digest, t.digest);
        Ok(())
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// 按下标取。
    pub fn get(&self, i: usize) -> Option<&OpTrace> {
        self.ops.get(i)
    }

    /// 按操作号查。
    pub fn find(&self, op: u32) -> Option<&OpTrace> {
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(t) = self.ops.get(i) {
                if t.op == op {
                    return Some(t);
                }
            }
            i += 1;
        }
        None
    }

    /// 票据是否已被占用。
    pub fn ticket_used(&self, ticket: u32) -> bool {
        self.tickets.contains(&ticket)
    }

    /// 票据条数。
    pub fn tickets_len(&self) -> usize {
        self.tickets.len()
    }

    /// 拒收计数（独立于码账）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 码账。
    pub fn hist(&self) -> &CodeHist {
        &self.hist
    }

    /// 链式摘要。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 链摘要重算。
    pub fn recompute(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(t) = self.ops.get(i) {
                h = fold_digest(h, t.digest);
            }
            i += 1;
        }
        h
    }

    /// 四点覆盖计数（按点独立统计）。
    pub fn coverage(&self) -> [u32; POINTS] {
        let mut out = [0u32; POINTS];
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(t) = self.ops.get(i) {
                let m = t.points_mask();
                let mut p = 0usize;
                while p < POINTS {
                    if (m & (1u8 << p)) != 0 {
                        if let Some(c) = out.get_mut(p) {
                            *c = c.saturating_add(1);
                        }
                    }
                    p += 1;
                }
            }
            i += 1;
        }
        out
    }

    /// 四点是否 100% 覆盖（每个点都覆盖到每一条观测）。
    pub fn coverage_full(&self) -> bool {
        if self.ops.is_empty() {
            return false;
        }
        let c = self.coverage();
        let mut i = 0usize;
        while i < POINTS {
            match c.get(i) {
                Some(v) => {
                    if (*v as usize) != self.ops.len() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 账本自洽：逐条闭环 + 摘要自洽 + 操作/票据唯一 + 链摘要可重算 + 双记账平。
    pub fn verify(&self) -> bool {
        let mut ops: Vec<u32> = Vec::new();
        let mut tks: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.ops.len() {
            match self.ops.get(i) {
                Some(t) => {
                    if !trace_closed_ok(t) {
                        return false;
                    }
                    if ops.contains(&t.op) || tks.contains(&t.ticket) {
                        return false;
                    }
                    ops.push(t.op);
                    tks.push(t.ticket);
                }
                None => return false,
            }
            i += 1;
        }
        if self.digest != self.recompute() {
            return false;
        }
        if !self.hist.guard_slot_zero() {
            return false;
        }
        reconcile(self.rejected, &self.hist).is_ok()
    }

    /// 平均耗时（整数纳秒，避免浮点）。
    pub fn mean_cost_ns(&self) -> u32 {
        if self.ops.is_empty() {
            return 0;
        }
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(t) = self.ops.get(i) {
                sum = sum.saturating_add(t.cost_ns as u64);
            }
            i += 1;
        }
        let n = self.ops.len() as u64;
        match sum.checked_div(n) {
            Some(v) => {
                if v > u32::MAX as u64 {
                    u32::MAX
                } else {
                    v as u32
                }
            }
            None => 0,
        }
    }

    /// 最大耗时。
    pub fn max_cost_ns(&self) -> u32 {
        let mut best = 0u32;
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(t) = self.ops.get(i) {
                if t.cost_ns > best {
                    best = t.cost_ns;
                }
            }
            i += 1;
        }
        best
    }

    /// 未达时条数。
    pub fn late_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(t) = self.ops.get(i) {
                if !t.on_time() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

/// 导出整本账的账行（按入账序 × 五行）。
pub fn ledger_rows(l: &OpLedger) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(l.len().saturating_mul(TRACE_ROWS));
    let mut i = 0usize;
    while i < l.len() {
        if let Some(t) = l.get(i) {
            let mut rows = trace_rows(t);
            let mut k = 0usize;
            while k < rows.len() {
                if let Some(r) = rows.get(k) {
                    out.push(String::from(r.as_str()));
                }
                k += 1;
            }
            rows.clear();
        }
        i += 1;
    }
    out
}

/// 导行在 `times` 次之间逐字一致。
pub fn rows_stable(l: &OpLedger, times: u32) -> bool {
    if times == 0 {
        return false;
    }
    let first = ledger_rows(l);
    let mut t = 1u32;
    while t < times {
        let again = ledger_rows(l);
        if again.len() != first.len() {
            return false;
        }
        let mut i = 0usize;
        while i < first.len() {
            if first.get(i) != again.get(i) {
                return false;
            }
            i += 1;
        }
        t = t.saturating_add(1);
    }
    true
}

// ── 合成 ─────────────────────────────────────────────────────────
/// 按种子合成一次闭环观测（票据/时间戳/结果码均由 LCG 决定）。
///
/// 注意 LCG 的低位退化：`below(n)` 取的是 `next() % n`，而
/// `state' = state·A + C` 的低 k 位只由 state 的低 k 位决定。故这里绝不能
/// 用 `seed ^ (op << 17)` 派生种子——op 只动高位，`below(512)` 会为所有 op
/// 返回同一个票据（实测：六个 op 全拿到 189）。用奇数乘子把 op 摊到低位，
/// 每个 op 才是独立序列。
pub fn synth_trace(seed: u64, op: u32) -> Result<OpTrace, u32> {
    let mix = seed ^ (op as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut rng = p_core::Lcg::new(mix);
    let mut t = OpTrace::new(op)?;
    let ticket = 1 + rng.below(TICKET_CAP / 8);
    t.take_ticket(ticket)?;
    let enter = 1 + rng.below(NS_LIMIT / 4);
    t.enter(enter)?;
    let span = 1 + rng.below(COST_BUDGET_NS.saturating_mul(3));
    let exit = enter.saturating_add(span).min(NS_LIMIT);
    if exit <= enter {
        return Err(Code::CostBad.val());
    }
    t.exit(exit)?;
    let cost = exit - enter;
    t.cost(cost)?;
    let slot = 1 + rng.below(KNOWN_CODES as u32);
    t.result(SEG_BASE + slot)?;
    Ok(t)
}

/// 合成一本六条观测的账（操作号 1..=6，票据由操作号派生以保证账内唯一）。
pub fn synth_ledger(seed: u64) -> Result<OpLedger, u32> {
    let mut l = OpLedger::new();
    let mut op = 1u32;
    while op <= 6 {
        let mut t = synth_trace(seed, op)?;
        t.ticket = op;
        t.digest = trace_digest(
            t.op,
            t.ticket,
            t.enter_ns,
            t.exit_ns,
            t.cost_ns,
            t.result_code,
            t.stage,
        );
        l.push(t)?;
        op = op.saturating_add(1);
    }
    Ok(l)
}

// ── 码族触达演练 ─────────────────────────────────────────────────
/// 把 14 条已知码与哨各触发至少一次：全部经真实拒绝/审计路径产生，不伪造计数。
pub fn exercise_codes() -> CodeHist {
    let mut hist = CodeHist::new();

    // 1) 段外码：对一张含越界码的表做族审计
    if let Err(c) = audit_family(&[SEG_BASE + 0x0F]) {
        hist.bump(c);
    }
    // 2) 重复码：同一码位占两次
    if let Err(c) = audit_family(&[SEG_BASE + 0x01, SEG_BASE + 0x01]) {
        hist.bump(c);
    }
    // 3) 空洞码：表长不足 14（含空表）
    if let Err(c) = audit_family(&[SEG_BASE + 0x01, SEG_BASE + 0x03]) {
        hist.bump(c);
    }
    if let Err(c) = audit_family(&[]) {
        hist.bump(c);
    }

    // 4) 票据非法：零票据 + 超容票据
    if let Ok(mut t) = OpTrace::new(1) {
        if let Err(c) = t.take_ticket(0) {
            hist.bump(c);
        }
        if let Err(c) = t.take_ticket(TICKET_CAP + 1) {
            hist.bump(c);
        }
        // 5) 阶段非法：由序号反查越界
        if let Err(c) = Point::from_index(STAGES as u8) {
            hist.bump(c);
        }
        // 6) 次序非法：未取号先入口
        if let Err(c) = t.enter(1) {
            hist.bump(c);
        }
        if t.take_ticket(7).is_ok() {
            // 7) 四点缺项：三点即入账
            let mut l = OpLedger::new();
            if let Err(c) = l.push(t) {
                hist.bump(c);
            }
        }
        // 8) 耗时非法：出口不晚于入口
        if t.enter(500).is_ok() {
            if let Err(c) = t.exit(500) {
                hist.bump(c);
            }
        }
        if t.exit(900).is_ok() {
            // 9) 结果码非法：段外码（哨）
            if let Err(c) = t.result(SEG_BASE + 0x0F) {
                hist.bump(c);
            }
            if t.cost(400).is_ok() {
                if let Err(c) = t.result(SEG_BASE + 0x0F) {
                    hist.bump(c);
                }
            }
        }
        // 10) 耗时非法：自报值与差值不符
        if let Ok(mut u) = OpTrace::new(2) {
            if u.take_ticket(11).is_ok() && u.enter(1_000).is_ok() && u.exit(2_000).is_ok() {
                if let Err(c) = u.cost(999) {
                    hist.bump(c);
                }
            }
        }
    }

    // 11) 操作号非法
    if let Err(c) = OpTrace::new(0) {
        hist.bump(c);
    }
    // 12) 摘要非法：篡改字段后入账
    if let Ok(mut t) = synth_trace(0x7351, 1) {
        t.enter_ns = t.enter_ns.saturating_add(1);
        let mut l = OpLedger::new();
        if let Err(c) = l.push(t) {
            hist.bump(c);
        }
    }
    // 13) 操作重复
    if let Ok(t) = synth_trace(0x7353, 5) {
        let mut l = OpLedger::new();
        if l.push(t).is_ok() {
            if let Err(c) = l.push(t) {
                hist.bump(c);
            }
        }
    }
    // 14) 账行非法：坏行入审
    let mut bad: Vec<String> = Vec::new();
    bad.push(String::from("TKT|k1=1|k2=2|k3=2|k4=0|digest=0000000000000000"));
    if let Err(c) = audit_rows(&bad) {
        hist.bump(c);
    }
    // 15) 容量已满：填满 OP_CAP 后再入一条（票据强制唯一，避免被 TicketBad 抢先）
    let mut full = OpLedger::new();
    let mut i = 1u32;
    while i <= OP_CAP as u32 {
        if let Ok(mut t) = synth_trace(0x7361, i) {
            t.ticket = i;
            t.digest = trace_digest(
                t.op,
                t.ticket,
                t.enter_ns,
                t.exit_ns,
                t.cost_ns,
                t.result_code,
                t.stage,
            );
            let _ = full.push(t);
        }
        i = i.saturating_add(1);
    }
    if let Ok(mut t) = synth_trace(0x7363, OP_CAP as u32 + 1) {
        t.ticket = OP_CAP as u32 + 1;
        t.digest = trace_digest(
            t.op,
            t.ticket,
            t.enter_ns,
            t.exit_ns,
            t.cost_ns,
            t.result_code,
            t.stage,
        );
        if let Err(c) = full.push(t) {
            hist.bump(c);
        }
    }
    // 16) 零静默违约：观测数比码账多一
    let mut silent = CodeHist::new();
    silent.bump(SEG_BASE + 0x01);
    if let Err(c) = reconcile(2, &silent) {
        hist.bump(c);
    }
    // 17) 越界哨：域外码（本段上下界之外、邻段码）只入第 15 槽，不冒充已知码。
    //     哨计数是「见过多少个域外码」的口径，故必须由真实域外码驱动。
    hist.bump(SEG_BASE + 0x0F);
    hist.bump(0x73381);
    hist.bump(0);

    hist
}

/// 触达账体检：14 条已知码全触达、哨触达、0 号槽恒零、双记账平。
pub fn exercise_all_green(h: &CodeHist) -> bool {
    h.touched_known() as usize == KNOWN_CODES
        && h.sentinel_total() >= 1
        && h.touched_slots() as usize == FAMILY_SIZE
        && h.guard_slot_zero()
        && h.total() >= FAMILY_SIZE as u32
}

// ── 自检 ─────────────────────────────────────────────────────────
/// 八项检查（前四项锚 F49075 码族，后四项锚 F49076 埋点）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::with_capacity(8);

    // 1) 码族满窗：窗口/占位/口径/说明互不重复。
    let mut ok = segment_guard();
    let mut bits = String::new();
    if !ok {
        bits.push_str("guard red; ");
    }
    if audit_family(&family_values()).is_err() {
        ok = false;
        bits.push_str("family audit red; ");
    }
    if family_len() != FAMILY_SIZE || KNOWN_CODES + 1 != FAMILY_SIZE {
        ok = false;
        bits.push_str("口径漂移; ");
    }
    let mut s = 0usize;
    while s < KNOWN_CODES {
        let code = SEG_BASE + 0x01 + s as u32;
        if family_slot(code) as usize != s + 1 || !is_known(code) {
            ok = false;
            bits.push_str("slot 漂移; ");
            break;
        }
        s += 1;
    }
    if family_slot(SEG_BASE) != SENTINEL_SLOT
        || family_slot(SEG_BASE + 0x0F) != SENTINEL_SLOT
        || family_slot(u32::MAX) != SENTINEL_SLOT
    {
        ok = false;
        bits.push_str("哨位漂移; ");
    }
    let mut i = 0usize;
    while i < Code::ALL.len() {
        match Code::ALL.get(i) {
            Some(c) => {
                if c.message() == UNKNOWN || c.message().is_empty() {
                    ok = false;
                    bits.push_str("文案缺; ");
                }
                let mut j = i + 1;
                while j < Code::ALL.len() {
                    if let Some(o) = Code::ALL.get(j) {
                        if o.message() == c.message() {
                            ok = false;
                            bits.push_str("文案重复; ");
                        }
                    }
                    j += 1;
                }
            }
            None => ok = false,
        }
        i += 1;
    }
    if ok && bits.is_empty() {
        bits = format!(
            "codes={} family={} lo={:#x} hi={:#x}",
            KNOWN_CODES,
            FAMILY_SIZE,
            window_lo(),
            window_hi()
        );
    }
    out.push(("F49075/码族满窗", ok, bits));

    // 2) 越界哨兜底：段外一律 UNKNOWN，邻段码不泄漏。
    let probes: [u32; 9] = [
        0,
        SEG_BASE,
        SEG_BASE - 0x01,
        SEG_BASE + 0x0F,
        SEG_BASE + 0x10,
        0x73381,
        0x73481,
        0x73281,
        u32::MAX,
    ];
    let mut ok = true;
    let mut leaks = 0u32;
    let mut k = 0usize;
    while k < probes.len() {
        if let Some(code) = probes.get(k) {
            if detail(*code) != UNKNOWN {
                leaks = leaks.saturating_add(1);
            }
            if is_known(*code) {
                leaks = leaks.saturating_add(1);
            }
            if family_slot(*code) != SENTINEL_SLOT {
                leaks = leaks.saturating_add(1);
            }
        }
        k += 1;
    }
    if leaks != 0 {
        ok = false;
    }
    // 已知码必须给出非 UNKNOWN 且互不相同
    let mut known_ok = true;
    let mut i = 0usize;
    while i < KNOWN_CODES {
        let code = SEG_BASE + 0x01 + i as u32;
        if detail(code) == UNKNOWN || family_slot(code) as usize != i + 1 {
            known_ok = false;
        }
        i += 1;
    }
    if !known_ok {
        ok = false;
    }
    out.push((
        "F49075/越界哨兜底",
        ok && leaks == 0,
        format!("probes={} leaks={} sentinel={}", probes.len(), leaks, SENTINEL_SLOT),
    ));

    // 3) 账行结构：往返恒等 + 键序/数字/摘要/多余列一律拒收。
    let ok;
    let mut info = String::from("导行失败");
    match synth_ledger(0x7355) {
        Ok(l) => {
            let rows = ledger_rows(&l);
            let mut all = rows.len() == l.len().saturating_mul(TRACE_ROWS) && !rows.is_empty();
            let mut i = 0usize;
            while i < rows.len() {
                match rows.get(i) {
                    Some(r) => {
                        if !audit_point(r) {
                            all = false;
                        }
                        let mut longer = String::from(r.as_str());
                        longer.push('x');
                        if audit_point(&longer) {
                            all = false;
                        }
                    }
                    None => all = false,
                }
                i += 1;
            }
            if audit_rows(&rows).is_err() {
                all = false;
            }
            let bad: [&str; 10] = [
                "",
                "TKT|k1=1|k2=2|k3=2",
                "TKT|k2=2|k1=1|k3=2|k4=0|digest=0000000000000000",
                "TKT|k1=1|k2=2|k3=2|k4=0|digest=000000000000000z",
                "TKT|k1=1|k2=2|k3=2|k4=0|digest=0000000000000000",
                "TKT|k1=1|k2=2|k3=2|k4=0|digest=0000000000000000|x=1",
                "PTZZ|k1=1|k2=2|k3=2|k4=0|digest=0000000000000000",
                "TKT|k1=|k2=2|k3=2|k4=0|digest=0000000000000000",
                "TKT|k1=99999999999|k2=2|k3=2|k4=0|digest=0000000000000000",
                "PTCT|k1=1|k2=2|k3=50001|k4=1|digest=0000000000000000",
            ];
            let mut k = 0usize;
            while k < bad.len() {
                if let Some(r) = bad.get(k) {
                    if audit_point(r) {
                        all = false;
                    }
                }
                k += 1;
            }
            // 往返恒等：解析再渲染必须逐字相同
            let mut k = 0usize;
            while k < rows.len() {
                match rows.get(k) {
                    Some(r) => {
                        let mut tag = "";
                        let mut q = 0usize;
                        while q < STAGES {
                            if let Some(t) = STAGE_TAGS.get(q) {
                                if r.len() >= t.len() && r.get(0..t.len()) == Some(*t) {
                                    tag = t;
                                    break;
                                }
                            }
                            q += 1;
                        }
                        match row_fields(r, tag, &ROW_KEYS) {
                            Some(v) => {
                                let a = v.first().and_then(|s| parse_u32(s));
                                let b = v.get(1).and_then(|s| parse_u32(s));
                                let c = v.get(2).and_then(|s| parse_u32(s));
                                let d = v.get(3).and_then(|s| parse_u32(s));
                                let dg = v.get(4).and_then(|s| parse_hex16(s));
                                match (a, b, c, d, dg) {
                                    (Some(a), Some(b), Some(c), Some(d), Some(dg)) => {
                                        if render_row(tag, a, b, c, d, dg) != *r {
                                            all = false;
                                        }
                                    }
                                    _ => all = false,
                                }
                            }
                            None => all = false,
                        }
                    }
                    None => all = false,
                }
                k += 1;
            }
            all = all && rows_stable(&l, 10);
            ok = all;
            info = format!("rows={} bad={} stable={}", rows.len(), bad.len(), all);
        }
        Err(_) => ok = false,
    }
    out.push(("F49075/账行结构", ok, info));

    // 4) 码族触达：15 条口径全部命中，双记账平，哨不冒充已知。
    let hist = exercise_codes();
    let ok = exercise_all_green(&hist);
    let mut bits = format!(
        "known={} slots={} total={} sent={}",
        hist.touched_known(),
        hist.touched_slots(),
        hist.total(),
        hist.sentinel_total()
    );
    if !ok {
        bits.push_str(" 触达不全");
    }
    out.push(("F49075/码族触达", ok, bits));

    // 5) 四点覆盖：整本账 100% 覆盖；缺一点即 PointMissing。
    let ok;
    let mut info = String::from("覆盖失败");
    match synth_ledger(0x7357) {
        Ok(l) => {
            let cov = l.coverage();
            let mut all = l.coverage_full() && !l.is_empty() && l.verify();
            let mut i = 0usize;
            while i < POINTS {
                match cov.get(i) {
                    Some(v) => {
                        if (*v as usize) != l.len() {
                            all = false;
                        }
                    }
                    None => all = false,
                }
                i += 1;
            }
            let mut i = 0usize;
            while i < l.len() {
                match l.get(i) {
                    Some(t) => {
                        if t.points_mask() != FOUR_POINT_MASK
                            || t.point_count() as usize != POINTS
                            || !trace_closed_ok(t)
                        {
                            all = false;
                        }
                    }
                    None => all = false,
                }
                i += 1;
            }
            // 缺一点必须被拒
            if let Ok(mut t) = OpTrace::new(97) {
                if t.take_ticket(9_000).is_err() {
                    // 票据超容：这正是 TicketBad 的口径
                }
                if t.take_ticket(900).is_ok()
                    && t.enter(10).is_ok()
                    && t.exit(20).is_ok()
                    && t.cost(10).is_ok()
                {
                    let mut probe = OpLedger::new();
                    if probe.push(t).err() != Some(Code::PointMissing.val()) {
                        all = false;
                    }
                }
            }
            ok = all;
            let mut c0 = 0u32;
            let mut c1 = 0u32;
            let mut c2 = 0u32;
            let mut c3 = 0u32;
            if let Some(v) = cov.first().copied() {
                c0 = v;
            }
            if let Some(v) = cov.get(1).copied() {
                c1 = v;
            }
            if let Some(v) = cov.get(2).copied() {
                c2 = v;
            }
            if let Some(v) = cov.get(3).copied() {
                c3 = v;
            }
            info = format!("ops={} cov={}/{}/{}/{}", l.len(), c0, c1, c2, c3);
        }
        Err(_) => ok = false,
    }
    out.push(("F49076/四点覆盖", ok, info));

    // 6) 次序守卫：五阶游标守卫 + 票据/操作唯一。
    let mut ok = true;
    let mut bits = String::new();
    match OpTrace::new(11) {
        Ok(mut t) => {
            if t.enter(100).err() != Some(Code::OrderBad.val()) {
                ok = false;
                bits.push_str("ticket-late; ");
            }
            if t.exit(200).err() != Some(Code::OrderBad.val()) {
                ok = false;
                bits.push_str("exit-early; ");
            }
            if t.cost(10).err() != Some(Code::OrderBad.val()) {
                ok = false;
                bits.push_str("cost-early; ");
            }
            if t.result(SEG_BASE + 1).err() != Some(Code::OrderBad.val()) {
                ok = false;
                bits.push_str("result-early; ");
            }
            if t.take_ticket(0).err() != Some(Code::TicketBad.val()) {
                ok = false;
                bits.push_str("ticket-zero; ");
            }
            if t.take_ticket(TICKET_CAP + 1).err() != Some(Code::TicketBad.val()) {
                ok = false;
                bits.push_str("ticket-over; ");
            }
            if t.take_ticket(21).is_err() {
                ok = false;
                bits.push_str("ticket-ok; ");
            }
            if t.take_ticket(22).err() != Some(Code::OrderBad.val()) {
                ok = false;
                bits.push_str("ticket-twice; ");
            }
            if t.enter(0).err() != Some(Code::CostBad.val()) {
                ok = false;
                bits.push_str("enter-zero; ");
            }
            if t.enter(NS_LIMIT + 1).err() != Some(Code::CostBad.val()) {
                ok = false;
                bits.push_str("enter-over; ");
            }
            if t.enter(1_000).is_err() {
                ok = false;
                bits.push_str("enter-ok; ");
            }
            if t.exit(1_000).err() != Some(Code::CostBad.val()) {
                ok = false;
                bits.push_str("exit-equal; ");
            }
            if t.exit(999).err() != Some(Code::CostBad.val()) {
                ok = false;
                bits.push_str("exit-back; ");
            }
            if t.exit(1_400).is_err() {
                ok = false;
                bits.push_str("exit-ok; ");
            }
            if t.cost(401).err() != Some(Code::CostBad.val()) {
                ok = false;
                bits.push_str("cost-off; ");
            }
            if t.cost(400).is_err() {
                ok = false;
                bits.push_str("cost-ok; ");
            }
            if t.result(0).err() != Some(Code::ResultBad.val()) {
                ok = false;
                bits.push_str("result-zero; ");
            }
            if t.result(SEG_BASE + 0x0F).err() != Some(Code::ResultBad.val()) {
                ok = false;
                bits.push_str("result-sentinel; ");
            }
            if t.result(0x73381).err() != Some(Code::ResultBad.val()) {
                ok = false;
                bits.push_str("result-neighbor; ");
            }
            if t.result(SEG_BASE + 3).is_err() {
                ok = false;
                bits.push_str("result-ok; ");
            }
            if !trace_closed_ok(&t) {
                ok = false;
                bits.push_str("closed-red; ");
            }
        }
        Err(_) => {
            ok = false;
            bits.push_str("new-red; ");
        }
    }
    if OpTrace::new(0).err() != Some(Code::OpBad.val()) {
        ok = false;
        bits.push_str("op-zero; ");
    }
    if Point::from_index(STAGES as u8).err() != Some(Code::StageBad.val()) {
        ok = false;
        bits.push_str("stage-over; ");
    }
    if Point::from_index(9).err() != Some(Code::StageBad.val()) {
        ok = false;
        bits.push_str("stage-9; ");
    }
    // 票据/操作唯一
    if let (Ok(a), Ok(b)) = (synth_trace(0x7359, 31), synth_trace(0x7359, 31)) {
        let mut l = OpLedger::new();
        if l.push(a).is_err() {
            ok = false;
            bits.push_str("push-red; ");
        }
        if l.push(b).err() != Some(Code::OpBad.val()) {
            ok = false;
            bits.push_str("op-dup; ");
        }
    }
    let mut probes = 0u32;
    if let (Ok(a), Ok(b)) = (synth_trace(0x7359, 32), synth_trace(0x7359, 32)) {
        let mut l = OpLedger::new();
        probes = probes.saturating_add(1);
        if l.push(a).is_err() {
            ok = false;
            bits.push_str("push-red; ");
        }
        probes = probes.saturating_add(1);
        if l.push(b).err() != Some(Code::OpBad.val()) {
            ok = false;
            bits.push_str("op-dup; ");
        }
    }
    if let (Ok(a), Ok(mut b)) = (synth_trace(0x7359, 33), synth_trace(0x7359, 34)) {
        // 票据撞号但摘要未修补 → 摘要码先响（摘要先于唯一性）
        b.ticket = a.ticket;
        let mut l = OpLedger::new();
        if l.push(a).is_err() {
            ok = false;
            bits.push_str("push-red2; ");
        }
        probes = probes.saturating_add(1);
        if l.push(b).err() != Some(Code::DigestBad.val()) {
            ok = false;
            bits.push_str("digest-first; ");
        }
        // 修补摘要后再撞号 → 票据码
        let mut c = b;
        c.digest = trace_digest(
            c.op,
            c.ticket,
            c.enter_ns,
            c.exit_ns,
            c.cost_ns,
            c.result_code,
            c.stage,
        );
        probes = probes.saturating_add(1);
        if l.push(c).err() != Some(Code::TicketBad.val()) {
            ok = false;
            bits.push_str("ticket-dup; ");
        }
        if l.rejected() != 2 {
            ok = false;
            bits.push_str("reject-miscount; ");
        }
        if reconcile(l.rejected(), l.hist()).is_err() {
            ok = false;
            bits.push_str("silent-gap; ");
        }
    }
    if ok && bits.is_empty() {
        bits = format!("order/uniq ok probes={}", probes);
    }
    out.push(("F49076/次序守卫", ok, bits));

    // 7) 耗时对账：cost == exit − enter、预算边界、极值。
    let ok;
    let mut info = String::from("对账失败");
    match synth_ledger(0x7365) {
        Ok(l) => {
            let mut all = !l.is_empty();
            let mut i = 0usize;
            while i < l.len() {
                match l.get(i) {
                    Some(t) => {
                        if !t.cost_balanced() {
                            all = false;
                        }
                        if t.on_time() != (t.cost_ns <= COST_BUDGET_NS) {
                            all = false;
                        }
                    }
                    None => all = false,
                }
                i += 1;
            }
            // 预算边界：等于预算算准时，多一纳秒算未达时
            if let Ok(mut t) = OpTrace::new(41) {
                if t.take_ticket(101).is_ok() && t.enter(1_000).is_ok() {
                    if t.exit(1_000 + COST_BUDGET_NS).is_ok()
                        && t.cost(COST_BUDGET_NS).is_ok()
                        && !t.on_time()
                    {
                        all = false;
                    }
                }
            }
            if let Ok(mut t) = OpTrace::new(42) {
                if t.take_ticket(102).is_ok() && t.enter(1_000).is_ok() {
                    if t.exit(1_001 + COST_BUDGET_NS).is_ok()
                        && t.cost(COST_BUDGET_NS + 1).is_ok()
                        && t.on_time()
                    {
                        all = false;
                    }
                }
            }
            // 极值：出口贴到 1s 上限仍应成账
            if let Ok(mut t) = OpTrace::new(43) {
                if t.take_ticket(103).is_ok()
                    && t.enter(NS_LIMIT - 1).is_ok()
                    && t.exit(NS_LIMIT).is_ok()
                    && t.cost(1).is_ok()
                    && !t.cost_balanced()
                {
                    all = false;
                }
            }
            // 耗时自报值必须被复算打回（多一纳秒）
            if let Ok(mut t) = OpTrace::new(44) {
                if t.take_ticket(104).is_ok() && t.enter(5).is_ok() && t.exit(9).is_ok() {
                    if t.cost(5).err() != Some(Code::CostBad.val()) {
                        all = false;
                    }
                    if t.cost(4).is_err() {
                        all = false;
                    }
                }
            }
            if all {
                info = format!(
                    "ops={} mean={} max={} late={}",
                    l.len(),
                    l.mean_cost_ns(),
                    l.max_cost_ns(),
                    l.late_count()
                );
            }
            // 均值不得越过最大值
            if l.mean_cost_ns() > l.max_cost_ns() && !l.is_empty() {
                all = false;
            }
            ok = all;
        }
        Err(_) => ok = false,
    }
    out.push(("F49076/耗时对账", ok, info));

    // 8) 闭环复跑：同种子双跑逐字一致、链摘要可重算、五行跨行对账、十次导行稳定。
    let ok;
    let mut info = String::from("复跑失败");
    match (synth_ledger(0x7367), synth_ledger(0x7367)) {
        (Ok(a), Ok(b)) => {
            let mut all = a == b && a.digest() == b.digest() && a.verify() && b.verify();
            all = all && a.digest() == a.recompute();
            all = all && !a.is_empty();
            if reconcile(a.rejected(), a.hist()).is_err() {
                all = false;
            }
            let mut i = 0usize;
            while i < a.len() {
                match a.get(i) {
                    Some(t) => {
                        let rows = trace_rows(t);
                        if !audit_trace(&rows) {
                            all = false;
                        }
                        if rows.len() != TRACE_ROWS {
                            all = false;
                        }
                    }
                    None => all = false,
                }
                i += 1;
            }
            all = all && rows_stable(&a, 10);
            // 种子敏感：两个至少相差 2 且不被 |1 合并的种子必须给出不同链摘要
            match synth_ledger(0x7369) {
                Ok(c) => {
                    if c.digest() == a.digest() {
                        all = false;
                    }
                }
                Err(_) => all = false,
            }
            ok = all;
            info = format!(
                "ops={} digest_same={} rows={}",
                a.len(),
                a.digest() == b.digest(),
                a.len().saturating_mul(TRACE_ROWS)
            );
        }
        _ => ok = false,
    }
    out.push(("F49076/闭环复跑", ok, info));

    out
}

/// 域自检入口。
pub fn run_unx49075_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX-F49075");
    let rows = extra_checks();
    let mut i = 0usize;
    while i < rows.len() {
        if let Some((name, passed, detail)) = rows.get(i) {
            set.add_dyn(name, *passed, detail.as_str());
        }
        i += 1;
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger(seed: u64) -> OpLedger {
        match synth_ledger(seed) {
            Ok(v) => v,
            Err(_) => OpLedger::new(),
        }
    }

    fn closed(op: u32, ticket: u32, enter: u32, exit: u32, code: u32) -> OpTrace {
        let mut t = OpTrace::new(op).expect("op");
        t.take_ticket(ticket).expect("ticket");
        t.enter(enter).expect("enter");
        t.exit(exit).expect("exit");
        t.cost(exit - enter).expect("cost");
        t.result(code).expect("result");
        t
    }

    fn refix(t: &mut OpTrace) {
        t.digest = trace_digest(
            t.op,
            t.ticket,
            t.enter_ns,
            t.exit_ns,
            t.cost_ns,
            t.result_code,
            t.stage,
        );
    }

    #[test]
    fn code_vals_in_window() {
        assert_eq!(Code::CodeOutOfSegment.val(), SEG_BASE + 1);
        assert_eq!(Code::SilentBad.val(), SEG_BASE + 14);
        assert!(segment_guard());
    }

    #[test]
    fn code_all_len_fourteen() {
        assert_eq!(Code::ALL.len(), KNOWN_CODES);
        assert_eq!(KNOWN_CODES, 14);
    }

    #[test]
    fn code_details_distinct() {
        let mut i = 0usize;
        while i < Code::ALL.len() {
            let a = Code::ALL[i];
            assert_ne!(a.message(), UNKNOWN);
            let mut j = i + 1;
            while j < Code::ALL.len() {
                assert_ne!(a.message(), Code::ALL[j].message());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn code_detail_matches_table() {
        let mut i = 0usize;
        while i < Code::ALL.len() {
            let c = Code::ALL[i];
            assert_eq!(detail(c.val()), c.message());
            i += 1;
        }
    }

    #[test]
    fn detail_unknown_outside_segment() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 15), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(u32::MAX), UNKNOWN);
        assert_eq!(detail(0x73381), UNKNOWN);
        assert_eq!(detail(0x73481), UNKNOWN);
    }

    #[test]
    fn window_bounds_are_closed() {
        assert_eq!(window_lo(), SEG_BASE + 1);
        assert_eq!(window_hi(), SEG_BASE + 14);
        assert!(is_known(window_lo()));
        assert!(is_known(window_hi()));
        assert!(!is_known(window_lo() - 1));
        assert!(!is_known(window_hi() + 1));
    }

    #[test]
    fn family_audit_accepts_own_table() {
        assert!(audit_family(&family_values()).is_ok());
        assert_eq!(family_values().len(), KNOWN_CODES);
        assert_eq!(family_values()[0], SEG_BASE + 1);
        assert_eq!(family_values()[KNOWN_CODES - 1], SEG_BASE + 14);
    }

    #[test]
    fn family_audit_rejects_out_of_segment() {
        assert_eq!(
            audit_family(&[SEG_BASE + 15]).err(),
            Some(Code::CodeOutOfSegment.val())
        );
        assert_eq!(
            audit_family(&[0]).err(),
            Some(Code::CodeOutOfSegment.val())
        );
    }

    #[test]
    fn family_audit_rejects_duplicated() {
        assert_eq!(
            audit_family(&[SEG_BASE + 1, SEG_BASE + 1]).err(),
            Some(Code::CodeDuplicated.val())
        );
    }

    #[test]
    fn family_audit_rejects_gap() {
        assert_eq!(
            audit_family(&[]).err(),
            Some(Code::CodeGap.val())
        );
        assert_eq!(
            audit_family(&[SEG_BASE + 1, SEG_BASE + 3]).err(),
            Some(Code::CodeGap.val())
        );
    }

    #[test]
    fn family_slot_mapping() {
        let mut i = 0usize;
        while i < KNOWN_CODES {
            let code = SEG_BASE + 1 + i as u32;
            assert_eq!(family_slot(code) as usize, i + 1);
            i += 1;
        }
        assert_eq!(family_slot(SEG_BASE), SENTINEL_SLOT);
        assert_eq!(family_slot(SEG_BASE + 15), SENTINEL_SLOT);
        assert_eq!(family_slot(u32::MAX), SENTINEL_SLOT);
    }

    #[test]
    fn family_len_is_fifteen() {
        assert_eq!(family_len(), FAMILY_SIZE);
        assert_eq!(family_len(), KNOWN_CODES + 1);
        assert_eq!(SENTINEL_SLOT as usize, FAMILY_SIZE);
        assert_eq!(SLOTS, FAMILY_SIZE + 1);
    }

    #[test]
    fn point_index_names_tags() {
        let ps = [
            Point::Ticket,
            Point::Enter,
            Point::Exit,
            Point::Cost,
            Point::Result,
        ];
        let mut i = 0usize;
        while i < ps.len() {
            let p = ps[i];
            assert_eq!(p.index() as usize, i);
            assert_eq!(p.name(), STAGE_NAMES[i]);
            assert_eq!(p.tag(), STAGE_TAGS[i]);
            i += 1;
        }
    }

    #[test]
    fn point_from_index_bounds() {
        let mut i = 0u8;
        while i < STAGES as u8 {
            let p = Point::from_index(i).expect("point");
            assert_eq!(p.index(), i);
            i += 1;
        }
        assert_eq!(
            Point::from_index(STAGES as u8).err(),
            Some(Code::StageBad.val())
        );
        assert_eq!(Point::from_index(99).err(), Some(Code::StageBad.val()));
    }

    #[test]
    fn point_four_point_flag() {
        assert!(!Point::Ticket.is_four_point());
        assert!(Point::Enter.is_four_point());
        assert!(Point::Exit.is_four_point());
        assert!(Point::Cost.is_four_point());
        assert!(Point::Result.is_four_point());
    }

    #[test]
    fn point_bit_masks() {
        assert_eq!(point_bit(Point::Ticket), 0);
        assert_eq!(point_bit(Point::Enter), 1);
        assert_eq!(point_bit(Point::Exit), 2);
        assert_eq!(point_bit(Point::Cost), 4);
        assert_eq!(point_bit(Point::Result), 8);
        assert_eq!(point_bit(Point::Enter) | point_bit(Point::Result), 0x09);
    }

    #[test]
    fn trace_new_basic() {
        let t = OpTrace::new(5).expect("t");
        assert_eq!(t.op, 5);
        assert_eq!(t.stage, STAGE_TICKET);
        assert_eq!(t.ticket, 0);
        assert!(t.verify());
        assert!(!t.four_points());
        assert!(!t.closed());
        assert_eq!(t.stage_name(), "取号");
    }

    #[test]
    fn trace_rejects_zero_op() {
        assert_eq!(OpTrace::new(0).err(), Some(Code::OpBad.val()));
    }

    #[test]
    fn trace_stage_order_guard() {
        let mut t = OpTrace::new(1).expect("t");
        assert_eq!(t.enter(10).err(), Some(Code::OrderBad.val()));
        assert_eq!(t.exit(20).err(), Some(Code::OrderBad.val()));
        assert_eq!(t.cost(10).err(), Some(Code::OrderBad.val()));
        assert_eq!(
            t.result(SEG_BASE + 1).err(),
            Some(Code::OrderBad.val())
        );
        assert!(t.take_ticket(1).is_ok());
        assert_eq!(t.take_ticket(2).err(), Some(Code::OrderBad.val()));
    }

    #[test]
    fn trace_ticket_bounds() {
        let mut t = OpTrace::new(1).expect("t");
        assert_eq!(t.take_ticket(0).err(), Some(Code::TicketBad.val()));
        assert_eq!(
            t.take_ticket(TICKET_CAP + 1).err(),
            Some(Code::TicketBad.val())
        );
        assert!(t.take_ticket(TICKET_CAP).is_ok());
    }

    #[test]
    fn trace_enter_bounds() {
        let mut t = OpTrace::new(1).expect("t");
        assert!(t.take_ticket(3).is_ok());
        assert_eq!(t.enter(0).err(), Some(Code::CostBad.val()));
        assert_eq!(
            t.enter(NS_LIMIT + 1).err(),
            Some(Code::CostBad.val())
        );
        assert!(t.enter(NS_LIMIT).is_ok());
    }

    #[test]
    fn trace_exit_bounds() {
        let mut t = OpTrace::new(1).expect("t");
        assert!(t.take_ticket(3).is_ok());
        assert!(t.enter(100).is_ok());
        assert_eq!(t.exit(100).err(), Some(Code::CostBad.val()));
        assert_eq!(t.exit(99).err(), Some(Code::CostBad.val()));
        assert_eq!(
            t.exit(NS_LIMIT + 1).err(),
            Some(Code::CostBad.val())
        );
        assert!(t.exit(101).is_ok());
    }

    #[test]
    fn trace_cost_is_recomputed() {
        let mut t = OpTrace::new(1).expect("t");
        assert!(t.take_ticket(3).is_ok());
        assert!(t.enter(500).is_ok());
        assert!(t.exit(560).is_ok());
        assert_eq!(t.cost(61).err(), Some(Code::CostBad.val()));
        assert_eq!(t.cost(0).err(), Some(Code::CostBad.val()));
        assert!(t.cost(60).is_ok());
        assert!(t.cost_balanced());
    }

    #[test]
    fn trace_result_family_only() {
        let mut t = OpTrace::new(1).expect("t");
        assert!(t.take_ticket(3).is_ok());
        assert!(t.enter(10).is_ok());
        assert!(t.exit(20).is_ok());
        assert!(t.cost(10).is_ok());
        assert_eq!(t.result(0).err(), Some(Code::ResultBad.val()));
        assert_eq!(
            t.result(SEG_BASE + 15).err(),
            Some(Code::ResultBad.val())
        );
        assert_eq!(t.result(0x73381).err(), Some(Code::ResultBad.val()));
        assert!(t.result(SEG_BASE + 14).is_ok());
        assert!(t.closed());
        assert_eq!(t.stage_name(), "闭环");
    }

    #[test]
    fn trace_points_progress() {
        let mut t = OpTrace::new(1).expect("t");
        assert_eq!(t.points_mask(), 0);
        assert!(t.take_ticket(3).is_ok());
        assert_eq!(t.points_mask(), 0);
        assert!(t.enter(10).is_ok());
        assert_eq!(t.points_mask(), 0x01);
        assert!(t.exit(20).is_ok());
        assert_eq!(t.points_mask(), 0x03);
        assert!(t.cost(10).is_ok());
        assert_eq!(t.points_mask(), 0x07);
        assert!(t.result(SEG_BASE + 2).is_ok());
        assert_eq!(t.points_mask(), FOUR_POINT_MASK);
        assert_eq!(t.point_count(), POINTS as u32);
    }

    #[test]
    fn trace_closed_ok_true() {
        let t = closed(9, 77, 1_000, 1_400, SEG_BASE + 4);
        assert!(trace_closed_ok(&t));
        assert!(t.four_points());
        assert!(t.cost_balanced());
        assert_eq!(t.cost_ns, 400);
    }

    #[test]
    fn trace_verify_detects_tamper() {
        let mut t = closed(9, 77, 1_000, 1_400, SEG_BASE + 4);
        t.cost_ns = 401;
        assert!(!t.verify());
        assert!(!trace_closed_ok(&t));
    }

    #[test]
    fn trace_on_time_boundary() {
        let t = closed(1, 1, 10, 10 + COST_BUDGET_NS, SEG_BASE + 1);
        assert!(t.on_time());
        let u = closed(2, 2, 10, 11 + COST_BUDGET_NS, SEG_BASE + 1);
        assert!(!u.on_time());
    }

    #[test]
    fn trace_extreme_limit() {
        let t = closed(1, 1, NS_LIMIT - 1, NS_LIMIT, SEG_BASE + 14);
        assert!(t.verify());
        assert!(t.cost_balanced());
        assert_eq!(t.cost_ns, 1);
    }

    #[test]
    fn hist_bump_slots() {
        let mut h = CodeHist::new();
        assert_eq!(h.bump(SEG_BASE + 3), 3);
        assert_eq!(h.slot_count(3), 1);
        assert_eq!(h.total(), 1);
        assert_eq!(h.known_total(), 1);
        assert_eq!(h.sentinel_total(), 0);
        assert_eq!(h.touched_known(), 1);
    }

    #[test]
    fn hist_sentinel_slot_counts() {
        let mut h = CodeHist::new();
        h.bump(SEG_BASE);
        h.bump(u32::MAX);
        assert_eq!(h.sentinel_total(), 2);
        assert_eq!(h.known_total(), 0);
        assert_eq!(h.touched_slots(), 1);
        assert_eq!(h.slot_count(0), 0);
        assert!(h.guard_slot_zero());
    }

    #[test]
    fn hist_totals_and_merge() {
        let mut a = CodeHist::new();
        a.bump(SEG_BASE + 1);
        a.bump(SEG_BASE + 2);
        let mut b = CodeHist::new();
        b.bump(SEG_BASE + 2);
        b.bump(SEG_BASE + 2);
        a.merge(&b);
        assert_eq!(a.total(), 4);
        assert_eq!(a.slot_count(2), 3);
        assert_eq!(a.touched_known(), 2);
        assert!(a.guard_slot_zero());
    }

    #[test]
    fn reconcile_ok_and_silent() {
        let mut h = CodeHist::new();
        h.bump(SEG_BASE + 1);
        h.bump(SEG_BASE + 2);
        assert!(reconcile(2, &h).is_ok());
        assert_eq!(reconcile(3, &h).err(), Some(Code::SilentBad.val()));
        assert_eq!(reconcile(1, &h).err(), Some(Code::SilentBad.val()));
        assert_eq!(reconcile(0, &CodeHist::new()).ok(), Some(()));
    }

    #[test]
    fn render_row_shape() {
        let row = render_row(TICKET_TAG, 1, 2, 2, 0, row_digest(TICKET_TAG, 1, 2, 2, 0));
        assert!(row.starts_with("TKT|k1=1|k2=2|k3=2|k4=0|digest="));
        assert_eq!(row.len(), "TKT|k1=1|k2=2|k3=2|k4=0|digest=".len() + 16);
        let tail = row.rsplit('|').next().unwrap_or("");
        assert_eq!(tail.len(), 23);
    }

    #[test]
    fn row_digest_stable_and_sensitive() {
        let a = row_digest(TICKET_TAG, 1, 2, 3, 4);
        let b = row_digest(TICKET_TAG, 1, 2, 3, 4);
        let c = row_digest(TICKET_TAG, 1, 2, 3, 5);
        let d = row_digest(ENTER_TAG, 1, 2, 3, 4);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn parse_u32_edges() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("12a"), None);
        assert_eq!(parse_u32("-1"), None);
        assert_eq!(parse_u32("99999999999"), None);
    }

    #[test]
    fn parse_hex16_edges() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("000000000000000"), None);
        assert_eq!(parse_hex16("000000000000000g"), None);
        assert_eq!(parse_hex16(""), None);
    }

    #[test]
    fn row_fields_key_order() {
        let row = render_row(ENTER_TAG, 7, 8, 9, 1, row_digest(ENTER_TAG, 7, 8, 9, 1));
        let v = row_fields(&row, ENTER_TAG, &ROW_KEYS).expect("fields");
        assert_eq!(v.len(), ROW_KEYS.len() + 1);
        assert_eq!(v.first().copied(), Some("7"));
        assert_eq!(v.get(1).copied(), Some("8"));
        assert_eq!(v.get(2).copied(), Some("9"));
        assert_eq!(v.get(3).copied(), Some("1"));
        assert!(row_fields(&row, EXIT_TAG, &ROW_KEYS).is_none());
    }

    #[test]
    fn audit_render_accepts_good_row() {
        let d = row_digest(COST_TAG, 5, 6, 7, 1);
        let row = render_row(COST_TAG, 5, 6, 7, 1, d);
        assert!(audit_render(&row, COST_TAG));
        assert!(!audit_render(&row, RESULT_TAG));
    }

    #[test]
    fn audit_render_rejects_bad_rows() {
        let d = row_digest(COST_TAG, 5, 6, 7, 1);
        let row = render_row(COST_TAG, 5, 6, 7, 1, d);
        let mut longer = String::from(row.as_str());
        longer.push('x');
        assert!(!audit_render(&longer, COST_TAG));
        assert!(!audit_render("", COST_TAG));
        assert!(!audit_render("COST|k1=5|k2=6|k3=7", COST_TAG));
        assert!(!audit_render("COST|k2=6|k1=5|k3=7|k4=1|digest=0000000000000000", COST_TAG));
        assert!(!audit_render("COST|k1=5|k2=6|k3=7|k4=1|digest=000000000000000z", COST_TAG));
    }

    #[test]
    fn audit_point_tag_dispatch() {
        let t = closed(3, 44, 1_000, 1_500, SEG_BASE + 5);
        let rows = trace_rows(&t);
        assert_eq!(rows.len(), TRACE_ROWS);
        let mut i = 0usize;
        while i < rows.len() {
            assert!(audit_point(rows[i].as_str()));
            i += 1;
        }
        assert!(!audit_point("NOPE|k1=1|k2=2|k3=2|k4=0|digest=0000000000000000"));
    }

    #[test]
    fn audit_point_rejects_semantic_breaks() {
        // 票据双写不一致
        let d = row_digest(TICKET_TAG, 3, 44, 45, 0);
        assert!(!audit_point(&render_row(TICKET_TAG, 3, 44, 45, 0, d)));
        // 入口阶段列写错
        let d = row_digest(ENTER_TAG, 3, 44, 10, 3);
        assert!(!audit_point(&render_row(ENTER_TAG, 3, 44, 10, 3, d)));
        // 耗时与预算标志不符
        let d = row_digest(COST_TAG, 3, 44, 100, 0);
        assert!(!audit_point(&render_row(COST_TAG, 3, 44, 100, 0, d)));
        // 结果槽号写错
        let d = row_digest(RESULT_TAG, 3, 44, SEG_BASE + 5, 6);
        assert!(!audit_point(&render_row(RESULT_TAG, 3, 44, SEG_BASE + 5, 6, d)));
        // 票据超容
        let d = row_digest(TICKET_TAG, 3, TICKET_CAP + 1, TICKET_CAP + 1, 0);
        assert!(!audit_point(&render_row(
            TICKET_TAG,
            3,
            TICKET_CAP + 1,
            TICKET_CAP + 1,
            0,
            d
        )));
    }

    #[test]
    fn trace_rows_empty_when_unclosed() {
        let mut t = OpTrace::new(2).expect("t");
        assert!(trace_rows(&t).is_empty());
        assert!(t.take_ticket(5).is_ok());
        assert!(trace_rows(&t).is_empty());
        assert!(t.enter(10).is_ok());
        assert!(t.exit(20).is_ok());
        assert!(t.cost(10).is_ok());
        assert!(trace_rows(&t).is_empty());
        assert!(t.result(SEG_BASE + 1).is_ok());
        assert_eq!(trace_rows(&t).len(), TRACE_ROWS);
    }

    #[test]
    fn audit_trace_cross_row() {
        let t = closed(4, 55, 2_000, 2_600, SEG_BASE + 6);
        let rows = trace_rows(&t);
        assert!(audit_trace(&rows));
        assert!(audit_rows(&rows).is_ok());
        let mut fewer: Vec<String> = Vec::new();
        fewer.push(String::from(rows[0].as_str()));
        assert!(!audit_trace(&fewer));
        assert!(audit_rows(&[]).is_ok());
    }

    #[test]
    fn audit_trace_rejects_broken_chain() {
        let t = closed(4, 55, 2_000, 2_600, SEG_BASE + 6);
        let rows = trace_rows(&t);
        let mut broken: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < rows.len() {
            if i == 2 {
                // 出口时间戳改成 2_601，摘要仍自洽（重算），但跨行差值对不上
                let d = row_digest(EXIT_TAG, 4, 55, 2_601, STAGE_EXIT as u32);
                broken.push(render_row(
                    EXIT_TAG,
                    4,
                    55,
                    2_601,
                    STAGE_EXIT as u32,
                    d,
                ));
            } else {
                broken.push(String::from(rows[i].as_str()));
            }
            i += 1;
        }
        assert!(!audit_trace(&broken));
    }

    #[test]
    fn ledger_push_and_verify() {
        let mut l = OpLedger::new();
        assert!(l.is_empty());
        assert!(l.verify());
        assert_eq!(l.digest(), p_core::FNV_OFF);
        let t = closed(1, 2, 10, 20, SEG_BASE + 1);
        assert!(l.push(t).is_ok());
        assert_eq!(l.len(), 1);
        assert_eq!(l.tickets_len(), 1);
        assert!(l.verify());
        assert!(l.ticket_used(2));
        assert_ne!(l.digest(), p_core::FNV_OFF);
        assert_eq!(l.recompute(), l.digest());
    }

    #[test]
    fn ledger_rejects_unclosed() {
        let mut l = OpLedger::new();
        let mut t = OpTrace::new(1).expect("t");
        assert!(t.take_ticket(2).is_ok());
        assert_eq!(l.push(t).err(), Some(Code::PointMissing.val()));
        assert_eq!(l.rejected(), 1);
        assert_eq!(l.hist().total(), 1);
        assert!(reconcile(l.rejected(), l.hist()).is_ok());
    }

    #[test]
    fn ledger_rejects_dup_op() {
        let mut l = OpLedger::new();
        let a = closed(1, 2, 10, 20, SEG_BASE + 1);
        let mut b = closed(1, 3, 10, 20, SEG_BASE + 1);
        refix(&mut b);
        assert!(l.push(a).is_ok());
        assert_eq!(l.push(b).err(), Some(Code::OpBad.val()));
    }

    #[test]
    fn ledger_rejects_dup_ticket() {
        let mut l = OpLedger::new();
        let a = closed(1, 9, 10, 20, SEG_BASE + 1);
        let mut b = closed(2, 10, 10, 20, SEG_BASE + 1);
        b.ticket = 9;
        refix(&mut b);
        assert!(l.push(a).is_ok());
        assert_eq!(l.push(b).err(), Some(Code::TicketBad.val()));
    }

    #[test]
    fn ledger_rejects_bad_digest() {
        let mut l = OpLedger::new();
        let mut t = closed(1, 2, 10, 20, SEG_BASE + 1);
        t.exit_ns = 21;
        assert_eq!(l.push(t).err(), Some(Code::DigestBad.val()));
    }

    #[test]
    fn ledger_capacity() {
        let mut l = OpLedger::new();
        let mut i = 1u32;
        while i <= OP_CAP as u32 {
            let mut t = closed(i, i, 10, 20, SEG_BASE + 1);
            refix(&mut t);
            assert!(l.push(t).is_ok());
            i += 1;
        }
        assert_eq!(l.len(), OP_CAP);
        let extra = closed(999, 999, 10, 20, SEG_BASE + 1);
        assert_eq!(l.push(extra).err(), Some(Code::CapacityBad.val()));
        assert!(l.verify());
    }

    #[test]
    fn ledger_coverage_full() {
        let l = ledger(0x7341);
        assert!(!l.is_empty());
        assert!(l.coverage_full());
        let cov = l.coverage();
        let mut i = 0usize;
        while i < POINTS {
            assert_eq!(cov[i] as usize, l.len());
            i += 1;
        }
    }

    #[test]
    fn ledger_stats() {
        let l = ledger(0x7343);
        assert!(l.mean_cost_ns() <= l.max_cost_ns());
        assert!(l.max_cost_ns() > 0);
        assert_eq!(l.late_count() as usize <= l.len(), true);
    }

    #[test]
    fn ledger_find_and_get() {
        let l = ledger(0x7345);
        let mut i = 0usize;
        while i < l.len() {
            let t = l.get(i).expect("t");
            assert_eq!(l.find(t.op).map(|x| x.op), Some(t.op));
            i += 1;
        }
        assert!(l.find(0).is_none());
        assert!(l.get(OP_CAP).is_none());
    }

    #[test]
    fn ledger_rows_stable() {
        let l = ledger(0x7347);
        assert!(rows_stable(&l, 10));
        assert!(!rows_stable(&l, 0));
        let rows = ledger_rows(&l);
        assert_eq!(rows.len(), l.len() * TRACE_ROWS);
    }

    #[test]
    fn synth_deterministic() {
        let a = ledger(0x7349);
        let b = ledger(0x7349);
        assert_eq!(a, b);
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn synth_seed_sensitive() {
        let a = ledger(0x7349);
        let b = ledger(0x734B);
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn synth_ops_and_tickets_unique() {
        let l = ledger(0x7351);
        let mut ops: Vec<u32> = Vec::new();
        let mut tks: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < l.len() {
            let t = l.get(i).expect("t");
            assert!(!ops.contains(&t.op));
            assert!(!tks.contains(&t.ticket));
            ops.push(t.op);
            tks.push(t.ticket);
            i += 1;
        }
        assert_eq!(ops.len(), 6);
    }

    #[test]
    fn exercise_codes_hits_all() {
        let h = exercise_codes();
        assert_eq!(h.touched_known() as usize, KNOWN_CODES);
        assert!(h.sentinel_total() >= 1);
        assert_eq!(h.touched_slots() as usize, FAMILY_SIZE);
        assert!(exercise_all_green(&h));
        assert!(h.guard_slot_zero());
    }

    #[test]
    fn exercise_codes_stable_across_runs() {
        let a = exercise_codes();
        let b = exercise_codes();
        assert_eq!(a, b);
        assert_eq!(a.total(), b.total());
    }

    #[test]
    fn exercise_codes_no_silence() {
        let h = exercise_codes();
        assert!(reconcile(h.total(), &h).is_ok());
        assert_eq!(
            reconcile(h.total() + 1, &h).err(),
            Some(Code::SilentBad.val())
        );
    }

    #[test]
    fn checks_all_green() {
        let set = run_unx49075_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn checks_len_and_truncation() {
        let set = run_unx49075_checks();
        assert_eq!(set.len(), 8);
        assert!(!set.truncated());
        assert_eq!(set.passed_count(), 8);
        assert_eq!(set.failed_count(), 0);
    }

    #[test]
    fn checks_names_distinct() {
        let set = run_unx49075_checks();
        let mut names: Vec<&'static str> = Vec::new();
        let mut i = 0usize;
        for c in set.iter() {
            assert!(!names.contains(&c.name));
            names.push(c.name);
            i += 1;
        }
        assert_eq!(i, 8);
        assert!(names.contains(&"F49075/码族满窗"));
        assert!(names.contains(&"F49076/四点覆盖"));
    }

    #[test]
    fn checks_detail_nonempty() {
        let rows = extra_checks();
        assert_eq!(rows.len(), 8);
        for (name, passed, det) in rows {
            assert!(passed, "{} red: {}", name, det);
            assert!(!det.is_empty(), "{} detail 为空", name);
        }
    }

    #[test]
    fn checks_prefix_split() {
        let rows = extra_checks();
        let mut f75 = 0usize;
        let mut f76 = 0usize;
        let mut i = 0usize;
        while i < rows.len() {
            if let Some((name, _, _)) = rows.get(i) {
                if name.starts_with("F49075/") {
                    f75 += 1;
                } else if name.starts_with("F49076/") {
                    f76 += 1;
                }
            }
            i += 1;
        }
        assert_eq!(f75, 4);
        assert_eq!(f76, 4);
    }

    #[test]
    fn row_columns_table() {
        assert!(row_columns(TICKET_TAG).is_some());
        assert!(row_columns(ENTER_TAG).is_some());
        assert!(row_columns(EXIT_TAG).is_some());
        assert!(row_columns(COST_TAG).is_some());
        assert!(row_columns(RESULT_TAG).is_some());
        assert!(row_columns("NOPE").is_none());
    }
}
