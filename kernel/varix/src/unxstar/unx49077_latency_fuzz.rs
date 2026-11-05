//! UNX-F49077 延迟账本运营与 O1 对照（判据主轴运营） fuzz 面（乱参数注入不崩）· 0x73480
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定）：
//!   UNX-F49077 延迟账本运营与 O1 对照（判据主轴运营） fuzz 面（乱参数注入不崩）:
//!          ① 纯功能代码 ≥280 行；
//!          ② 判据原文：UNX-F49078-J1 ×20 全绿入库；
//!             判据锚定 §AI-62 §8。
//!   UNX-F49078 延迟账本运营与 O1 对照（判据主轴运营）判据用例集（20 例入库可复跑）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49079-J1 批收口防重留痕零撞号；
//!             判据锚定 §6.1。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（fuzz）：参数合法 → 单帧守恒 → 万帧累积 → 码族触达。
//! 参数合法先于单帧：种子为零、跨度为零、故障位越界时，「跑了一万帧」这件事
//!   本身没有意义——那是在跑一张非法参数表，不是在跑 fuzz；
//! 单帧守恒先于万帧累积：一帧内 attempts = accepted + rejected 必须成立，
//!   否则累积到一万帧只是把一次漏记放大一万倍；
//! 万帧累积先于码族触达：先证明 ×10^4 帧不崩且零静默，再谈每条码是否被触达——
//!   反过来的话，「码全触达」可能只是某几百帧的功劳，其余九千多帧根本没跑；
//! 码族触达最后，且分两面看：fuzz 面自然只产出票据/次序/耗时/结果/重号 5 类，
//!   其余 9 类（帧/种子/参数/阶段/摘要/行面/用例/复跑/编号）由 `exercise_codes()`
//!   逐条用真实接口触发；触发不到即记 `misses`，失手不为零就红——触达不许靠猜。
//!   这样「14 条满窗」是几何事实而不是统计巧合，同时保留了 fuzz 面的随机性。
//! 闸门次序（用例集）：表形 → 单例 → 全表 → 入库。
//! 表形先于单例：序号 #01..#20 必须连续唯一，标签不得重号，这是「20 例」的前提；
//! 单例先于全表：每例先自证（各自的判据成立且摘要可重算），全表绿才有意义；
//! 全表先于入库：二十例同时全绿且两次复跑摘要一致，才允许写账；
//! 入库先于收口：账行逐条可回读之后，才谈批收口留痕与跨模块零撞号。
//! 关键差异：fuzz 面只回答「参数再乱，一帧之内账还平不平、有没有静默吞异常」；
//! 用例集只回答「二十条判据各自成立吗、二十次复跑逐字一致吗、编号撞不撞」。
//! 两面共用同一套账行格式（tag|k1..k4|digest），摘要域互相独立：
//! 帧摘要折 (frame, accepted, rejected, digit)——只折行内可见四列；
//! 用例摘要折 (index, kind, passed, payload)——同样只折可见列。
//! 二者都不含摘要自身，否则一位数字的篡改就能把摘要一并带走。
//! 编号口径：本模块所有「编号」都落在本段 [SEG_BASE+0x01, SEG_BASE+0x3F) 内——
//!   诊断码取 +0x01..+0x0E，用例号取 +0x21..+0x34；两者不重叠，
//!   且与 B14 前五模块的段 (0x73280/0x732C0/0x73300/0x73340/0x73380) 互不相交。
//!   跨模块防重的根不是「登记时查重」而是「各段区间互斥」：只要一段只肯承认
//!   自己区间里的号，撞号在几何上就不可能发生；登记器的查重只是第二道闸。
//!   本段可编号槽为 +0x01..+0x3F（63 个），登记器容量取 48：
//!   14 码 + 20 例 = 34 条留痕，余 14 个位给后续批次，且容量闸真能触发（不是死代码）。
//! 另：帧行/用例行/编号行三种 tag（FZRS/CASE/IDRG）共用同一渲染与回读口径，
//!   任何一行读不回来都是 RenderBad——行面同样零静默。
//!   码账 16 槽：0 号结构性留空（任何码都落不进去）、1..=14 记已知码、
//!   15 号哨槽只吃域外码；哨计数必须恒等于「送进来的域外码条数」，绝不外溢。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::unx48941_wheel_core as p_core;
use crate::checks::CheckSet;

/// 诊断段基址（0x73480，B14 族第八槽）。
pub const SEG_BASE: u32 = 0x73480;

/// fuzz 帧数（判据口径 ×10^4）。
pub const FUZZ_FRAMES: u32 = 10_000;
/// 帧数上限（防失控复跑）。
pub const FRAME_CAP: u32 = 100_000;
/// 单帧操作数上限。
pub const OPS_CAP: u32 = 8;
/// 票据容量（含）。
pub const TICKET_CAP: u32 = 4_096;
/// 时间戳上限（1 秒，纳秒）。
pub const NS_LIMIT: u32 = 1_000_000_000;
/// 耗时预算（50µs）。
pub const COST_BUDGET_NS: u32 = 50_000;
/// 段内已知诊断码条数。
pub const KNOWN_CODES: usize = 14;
/// 越界哨统计槽。
pub const SENTINEL_SLOT: u8 = 15;
/// 统计槽总数（0 号留空）。
pub const SLOTS: usize = 16;
/// 用例条数（判据口径 ×20）。
pub const CASE_COUNT: usize = 20;
/// 编号登记器容量（14 条诊断码 + 20 个用例号 + 余量，且不超过段内可编号槽数-1）。
pub const ID_CAP: usize = 48;
/// 每段占用跨度（半开区间 [base, base+SPAN)）。
pub const SEG_SPAN: u32 = 0x40;
/// 用例号局部基址（+0x20，避开 +0x01..+0x0E 的诊断码）。
pub const CASE_ID_BASE: u32 = SEG_BASE + 0x20;
/// 账行数字键（固定列序）。
pub const ROW_KEYS: [&str; 4] = ["k1", "k2", "k3", "k4"];
/// 账行字段数（tag + 4 键 + digest）。
pub const ROW_FIELDS: usize = 6;
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B14 fuzz/用例库诊断码";
/// 帧账行标签。
pub const FUZZ_TAG: &str = "FZRS";
/// 用例账行标签。
pub const CASE_TAG: &str = "CASE";
/// 编号账行标签。
pub const ID_TAG: &str = "IDRG";
/// 段表标签。
pub const SEG_TAG: &str = "SEGL";

// 故障注入位（fuzz 的「乱」是有刻度的乱，不是无参数乱跑）
/// 注入「零票据」。
pub const F_TICKET_ZERO: u32 = 1 << 0;
/// 注入「超容票据」。
pub const F_TICKET_OVER: u32 = 1 << 1;
/// 注入「次序颠倒」（未入口先出口）。
pub const F_ORDER: u32 = 1 << 2;
/// 注入「耗时自报偏差」。
pub const F_COST: u32 = 1 << 3;
/// 注入「域外结果码」。
pub const F_RESULT: u32 = 1 << 4;
/// 注入「操作号重复」。
pub const F_OP_DUP: u32 = 1 << 5;
/// 全部故障位。
pub const F_ALL: u32 = 0x3F;
/// 故障位数（掩码合法上界校验用）。
pub const FAULT_BITS: usize = 6;

/// 段表：B14 前五模块 + 本批两个模块（半开区间）。
/// 只登记「谁占了哪一段」，不登记内容——防重只需要区间互斥这一条事实。
pub const SEG_LEDGER: [(&str, u32); 7] = [
    ("UNX-F49061", 0x73280),
    ("UNX-F49063", 0x732C0),
    ("UNX-F49065", 0x73300),
    ("UNX-F49067", 0x73340),
    ("UNX-F49069", 0x73380),
    ("UNX-F49075", 0x73440),
    ("UNX-F49077", 0x73480),
];

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// fuzz/用例库诊断码（段内 14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Code {
    /// 帧号越出 0..10^4
    FrameBad,
    /// 种子未注入（为零）
    SeedBad,
    /// 参数跨度或故障掩码越界
    ParamBad,
    /// 操作号为零、批内重复或超上限
    OpBad,
    /// 票据为零、超容或帧内重号
    TicketBad,
    /// 阶段序号越出取号至结果五阶
    StageBad,
    /// 阶段次序断裂
    OrderBad,
    /// 耗时与进出口不自洽
    CostBad,
    /// 结果码不在段内已知族
    ResultBad,
    /// 摘要与行内字段不符
    DigestBad,
    /// 账行键序、数字或分隔符非法
    RenderBad,
    /// 用例序号越界、重号或库容已满
    CaseBad,
    /// 复跑摘要不一致
    ReplayBad,
    /// 跨模块编号撞号或空号
    IdBad,
}

impl Code {
    /// 诊断码数值（段内 0x01..0x0E）。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 码的说明（与 detail 同源）。
    pub fn message(self) -> &'static str {
        detail(self.val())
    }

    /// 全码表（14 条）。
    pub const ALL: [Code; KNOWN_CODES] = [
        Code::FrameBad,
        Code::SeedBad,
        Code::ParamBad,
        Code::OpBad,
        Code::TicketBad,
        Code::StageBad,
        Code::OrderBad,
        Code::CostBad,
        Code::ResultBad,
        Code::DigestBad,
        Code::RenderBad,
        Code::CaseBad,
        Code::ReplayBad,
        Code::IdBad,
    ];
}

/// 码值到说明；段外一律 UNKNOWN，不泄漏邻段文案。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => "fuzz 帧号越出零至一万的闭开区间",
        2 => "种子未注入（种子为零）",
        3 => "参数跨度或故障注入掩码越界",
        4 => "操作号为零、单帧重复或超上限",
        5 => "票据为零、超容或在单帧内重号",
        6 => "阶段序号越出取号至结果五阶",
        7 => "阶段次序断裂（未入口先出口等）",
        8 => "耗时与进出口差值不自洽",
        9 => "结果码不在本段已知码族内",
        10 => "摘要与行内字段不符",
        11 => "账行键序、数字或分隔符非法",
        12 => "用例序号越界、重号或用例库容量已满",
        13 => "复跑摘要不一致",
        14 => "跨模块编号撞号或空号",
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
            if let Some(o) = Code::ALL.get(j) {
                if o.val() == v {
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

/// 已知码判定。
pub fn is_known(code: u32) -> bool {
    code >= window_lo() && code <= window_hi()
}

/// 统计槽归属：段内 1..=14，段外一律哨槽。
pub fn family_slot(code: u32) -> u8 {
    if is_known(code) {
        (code - SEG_BASE) as u8
    } else {
        SENTINEL_SLOT
    }
}

/// 段内 14 个码值（升序）。
pub fn family_values() -> [u32; KNOWN_CODES] {
    let mut out = [0u32; KNOWN_CODES];
    let mut i = 0usize;
    while i < KNOWN_CODES {
        out[i] = SEG_BASE + 0x01 + i as u32;
        i += 1;
    }
    out
}

/// 段表审计：条数、升序、两两半开区间不相交。
pub fn segments_disjoint() -> bool {
    let mut i = 0usize;
    while i < SEG_LEDGER.len() {
        let (_, bi) = match SEG_LEDGER.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let lo_i = bi;
        let hi_i = bi.saturating_add(SEG_SPAN);
        let mut j = i + 1;
        while j < SEG_LEDGER.len() {
            let (_, bj) = match SEG_LEDGER.get(j) {
                Some(v) => *v,
                None => return false,
            };
            let lo_j = bj;
            let hi_j = bj.saturating_add(SEG_SPAN);
            if lo_i < hi_j && lo_j < hi_i {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 段归属：返回持有该编号的段表下标。
pub fn classify(code: u32) -> Option<usize> {
    let mut i = 0usize;
    while i < SEG_LEDGER.len() {
        if let Some((_, base)) = SEG_LEDGER.get(i) {
            if code >= *base && code < base.saturating_add(SEG_SPAN) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 本模块段表下标。
pub fn own_slot() -> usize {
    let mut i = 0usize;
    while i < SEG_LEDGER.len() {
        if let Some((_, base)) = SEG_LEDGER.get(i) {
            if *base == SEG_BASE {
                return i;
            }
        }
        i += 1;
    }
    usize::MAX
}

/// 用例号（局部 +0x21..+0x34）。
pub fn case_id(index: u8) -> u32 {
    CASE_ID_BASE + index as u32
}

/// 把 64 位摘要折入 64 位累加器（高低两半各折一次）。
fn fold_digest(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

// ── 码账 ─────────────────────────────────────────────────────────
/// 15 槽码账（0 号留空，1..=14 已知码，15 哨）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeHist {
    counts: [u32; SLOTS],
}

impl CodeHist {
    /// 空账。
    pub const fn new() -> CodeHist {
        CodeHist { counts: [0u32; SLOTS] }
    }

    /// 记一次命中，返回槽号。
    pub fn bump(&mut self, code: u32) -> u8 {
        let slot = family_slot(code) as usize;
        if let Some(c) = self.counts.get_mut(slot) {
            *c = c.saturating_add(1);
        }
        slot as u8
    }

    /// 某槽计数。
    pub fn slot_count(&self, slot: u8) -> u32 {
        match self.counts.get(slot as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 总计数（不含 0 号槽）。
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

    /// 已知码计数。
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

    /// 哨计数。
    pub fn sentinel_total(&self) -> u32 {
        self.slot_count(SENTINEL_SLOT)
    }

    /// 被触达的已知码个数。
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

    /// 0 号占位槽必须恒零。
    pub fn guard_slot_zero(&self) -> bool {
        match self.counts.first() {
            Some(v) => *v == 0,
            None => false,
        }
    }

    /// 合并另一本账。
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

// ── fuzz 参数面 ──────────────────────────────────────────────────
/// fuzz 面能自然触达的已知码种数下界（票据/次序/耗时/结果/重号五类）。
pub const FAULT_MIN: u32 = 5;

/// fuzz 面参数：种子 + 单帧操作数 + 票据/时间跨度 + 故障掩码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzParams {
    /// 种子（非零）。
    pub seed: u64,
    /// 单帧操作数（1..=OPS_CAP）。
    pub ops: u32,
    /// 票据跨度（1..=TICKET_CAP）。
    pub ticket_span: u32,
    /// 时间跨度（1..=NS_LIMIT）。
    pub ns_span: u32,
    /// 故障注入掩码（只能是 F_ALL 的子集）。
    pub fault_mask: u32,
}

impl FuzzParams {
    /// 校验并构造。
    pub fn new(
        seed: u64,
        ops: u32,
        ticket_span: u32,
        ns_span: u32,
        fault_mask: u32,
    ) -> Result<FuzzParams, u32> {
        if seed == 0 {
            return Err(Code::SeedBad.val());
        }
        if ops == 0 || ops > OPS_CAP {
            return Err(Code::ParamBad.val());
        }
        if ticket_span == 0 || ticket_span > TICKET_CAP {
            return Err(Code::ParamBad.val());
        }
        if ns_span == 0 || ns_span > NS_LIMIT {
            return Err(Code::ParamBad.val());
        }
        if (fault_mask & !F_ALL) != 0 {
            return Err(Code::ParamBad.val());
        }
        Ok(FuzzParams {
            seed,
            ops,
            ticket_span,
            ns_span,
            fault_mask,
        })
    }

    /// 标准参数（全故障位开，4 op/帧）。
    pub fn standard(seed: u64) -> FuzzParams {
        FuzzParams {
            seed,
            ops: 4,
            ticket_span: 256,
            ns_span: COST_BUDGET_NS.saturating_mul(4),
            fault_mask: F_ALL,
        }
    }

    /// 自洽判定（与 new 同一口径，供已构造值复查）。
    pub fn verify(&self) -> bool {
        self.seed != 0
            && self.ops >= 1
            && self.ops <= OPS_CAP
            && self.ticket_span >= 1
            && self.ticket_span <= TICKET_CAP
            && self.ns_span >= 1
            && self.ns_span <= NS_LIMIT
            && (self.fault_mask & !F_ALL) == 0
    }

    /// 故障位个数。
    pub fn fault_bits(&self) -> u32 {
        let mut m = self.fault_mask;
        let mut n = 0u32;
        while m != 0 {
            n = n.saturating_add((m & 1) as u32);
            m >>= 1;
        }
        n
    }
}

// ── 单次观测（fuzz 版状态机） ────────────────────────────────────
/// fuzz 里的一次观测：五阶游标 + 四值 + 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzOp {
    /// 操作号（非零）。
    pub op: u32,
    /// 票据。
    pub ticket: u32,
    /// 入口时间戳。
    pub enter_ns: u32,
    /// 出口时间戳。
    pub exit_ns: u32,
    /// 耗时。
    pub cost_ns: u32,
    /// 结果码。
    pub code: u32,
    /// 阶段游标（0..=5）。
    pub stage: u8,
    /// 摘要。
    pub digest: u64,
}

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
/// 阶段序号：闭环。
pub const STAGE_CLOSED: u8 = 5;
/// 阶段总数。
pub const STAGES: usize = 5;
/// 四点齐全掩码。
pub const FOUR_POINT_MASK: u8 = 0x0F;
/// 五阶中文名。
pub const STAGE_NAMES: [&str; STAGES] = ["取号", "入口", "出口", "耗时", "结果"];

/// 阶段名反查：五阶之外一律 StageBad（日志面绝不静默落成「未知」）。
/// 注意 `STAGE_CLOSED` 是终态而非阶段，故它在反查里同样是 StageBad。
pub fn stage_name(stage: u8) -> Result<&'static str, u32> {
    match STAGE_NAMES.get(stage as usize) {
        Some(n) => {
            if n.is_empty() {
                Err(Code::StageBad.val())
            } else {
                Ok(n)
            }
        }
        None => Err(Code::StageBad.val()),
    }
}

/// 观测摘要：折 (op, ticket, enter, exit, cost, code, stage)。
pub fn op_digest(
    op: u32,
    ticket: u32,
    enter_ns: u32,
    exit_ns: u32,
    cost_ns: u32,
    code: u32,
    stage: u8,
) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, op);
    h = p_core::fold_u32(h, ticket);
    h = p_core::fold_u32(h, enter_ns);
    h = p_core::fold_u32(h, exit_ns);
    h = p_core::fold_u32(h, cost_ns);
    h = p_core::fold_u32(h, code);
    h = p_core::fold_byte(h, stage);
    h
}

impl FuzzOp {
    /// 开一次观测。
    pub fn new(op: u32) -> Result<FuzzOp, u32> {
        if op == 0 {
            return Err(Code::OpBad.val());
        }
        Ok(FuzzOp {
            op,
            ticket: 0,
            enter_ns: 0,
            exit_ns: 0,
            cost_ns: 0,
            code: 0,
            stage: STAGE_TICKET,
            digest: op_digest(op, 0, 0, 0, 0, 0, STAGE_TICKET),
        })
    }

    fn refresh(&mut self) {
        self.digest = op_digest(
            self.op,
            self.ticket,
            self.enter_ns,
            self.exit_ns,
            self.cost_ns,
            self.code,
            self.stage,
        );
    }

    fn expect_stage(&self, expected: u8) -> Result<(), u32> {
        if self.stage != expected {
            return Err(Code::OrderBad.val());
        }
        Ok(())
    }

    /// 取号。
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

    /// 入口。
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

    /// 出口。
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

    /// 耗时（必须等于出口 − 入口）。
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

    /// 结果。
    pub fn result(&mut self, code: u32) -> Result<(), u32> {
        self.expect_stage(STAGE_RESULT)?;
        if !is_known(code) {
            return Err(Code::ResultBad.val());
        }
        self.code = code;
        self.stage = STAGE_CLOSED;
        self.refresh();
        Ok(())
    }

    /// 四点掩码。
    pub fn points(&self) -> u8 {
        let mut m = 0u8;
        if self.stage >= STAGE_EXIT {
            m |= 1;
        }
        if self.stage >= STAGE_COST {
            m |= 2;
        }
        if self.stage >= STAGE_RESULT {
            m |= 4;
        }
        if self.stage >= STAGE_CLOSED {
            m |= 8;
        }
        m
    }

    /// 四点齐全。
    pub fn four_points(&self) -> bool {
        self.points() == FOUR_POINT_MASK
    }

    /// 闭环。
    pub fn closed(&self) -> bool {
        self.stage == STAGE_CLOSED
    }

    /// 摘要自洽。
    pub fn verify(&self) -> bool {
        self.digest
            == op_digest(
                self.op,
                self.ticket,
                self.enter_ns,
                self.exit_ns,
                self.cost_ns,
                self.code,
                self.stage,
            )
    }
}

/// 单帧账：一批观测 + 票据/操作去重 + 码账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FrameState {
    frame: u32,
    ops: Vec<FuzzOp>,
    tickets: Vec<u32>,
    rejected: u32,
    external: u32,
    hist: CodeHist,
    digest: u64,
    max_cost: u32,
    total_cost: u64,
}

impl FrameState {
    /// 空帧。
    pub fn new(frame: u32) -> FrameState {
        FrameState {
            frame,
            ops: Vec::new(),
            tickets: Vec::new(),
            rejected: 0,
            external: 0,
            hist: CodeHist::new(),
            digest: p_core::FNV_OFF,
            max_cost: 0,
            total_cost: 0,
        }
    }

    fn reject(&mut self, code: u32) -> u32 {
        self.rejected = self.rejected.saturating_add(1);
        self.hist.bump(code);
        code
    }

    /// 登记一个域外码（只入哨槽，绝不冒充已知码）。
    pub fn note_external(&mut self, code: u32) {
        self.external = self.external.saturating_add(1);
        self.hist.bump(code);
    }

    /// 入帧：摘要 → 操作唯一 → 票据唯一 → 容量。
    pub fn push(&mut self, o: FuzzOp) -> Result<(), u32> {
        if !o.four_points() {
            return Err(self.reject(Code::OrderBad.val()));
        }
        if !o.verify() {
            return Err(self.reject(Code::DigestBad.val()));
        }
        if !is_known(o.code) {
            return Err(self.reject(Code::ResultBad.val()));
        }
        let mut found_op = false;
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(v) = self.ops.get(i) {
                if v.op == o.op {
                    found_op = true;
                }
            }
            i += 1;
        }
        if found_op {
            return Err(self.reject(Code::OpBad.val()));
        }
        if self.tickets.contains(&o.ticket) {
            return Err(self.reject(Code::TicketBad.val()));
        }
        if self.ops.len() >= OPS_CAP as usize {
            return Err(self.reject(Code::ParamBad.val()));
        }
        self.ops.push(o);
        self.tickets.push(o.ticket);
        self.digest = fold_digest(self.digest, o.digest);
        self.total_cost = self.total_cost.saturating_add(o.cost_ns as u64);
        if o.cost_ns > self.max_cost {
            self.max_cost = o.cost_ns;
        }
        Ok(())
    }

    /// 帧号。
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// 已入帧条数。
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// 空帧判定。
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// 按下标取。
    pub fn get(&self, i: usize) -> Option<&FuzzOp> {
        self.ops.get(i)
    }

    /// 拒收计数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 域外码计数。
    pub fn external(&self) -> u32 {
        self.external
    }

    /// 码账。
    pub fn hist(&self) -> &CodeHist {
        &self.hist
    }

    /// 链摘要。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 最大耗时。
    pub fn max_cost(&self) -> u32 {
        self.max_cost
    }

    /// 平均耗时。
    pub fn mean_cost(&self) -> u32 {
        if self.ops.is_empty() {
            return 0;
        }
        match self.total_cost.checked_div(self.ops.len() as u64) {
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

    /// 链摘要重算。
    pub fn recompute(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(o) = self.ops.get(i) {
                h = fold_digest(h, o.digest);
            }
            i += 1;
        }
        h
    }

    /// 帧自洽：逐条闭环 + 操作/票据唯一 + 链摘要 + 双记账（已知码计数 == 拒收数）。
    pub fn verify(&self) -> bool {
        let mut ops: Vec<u32> = Vec::new();
        let mut tks: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.ops.len() {
            match self.ops.get(i) {
                Some(o) => {
                    if !o.closed() || !o.four_points() || !o.verify() || !is_known(o.code) {
                        return false;
                    }
                    if ops.contains(&o.op) || tks.contains(&o.ticket) {
                        return false;
                    }
                    ops.push(o.op);
                    tks.push(o.ticket);
                }
                None => return false,
            }
            i += 1;
        }
        if self.digest != self.recompute() {
            return false;
        }
        if self.hist.known_total() != self.rejected {
            return false;
        }
        if self.hist.sentinel_total() != self.external {
            return false;
        }
        self.hist.guard_slot_zero()
    }
}

/// 单帧结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameOutcome {
    /// 帧号。
    pub frame: u32,
    /// 尝试次数。
    pub attempts: u32,
    /// 入帧条数。
    pub accepted: u32,
    /// 拒收次数。
    pub rejected: u32,
    /// 域外码次数。
    pub external: u32,
    /// 链摘要。
    pub digest: u64,
    /// 码账。
    pub hist: CodeHist,
    /// 最大耗时。
    pub max_cost: u32,
}

impl FrameOutcome {
    /// 静默条数（守恒缺口）。
    pub fn silent(&self) -> u32 {
        self.attempts
            .saturating_sub(self.accepted)
            .saturating_sub(self.rejected)
    }

    /// 守恒判定。
    pub fn balanced(&self) -> bool {
        self.silent() == 0 && self.accepted.saturating_add(self.rejected) == self.attempts
    }
}

/// 跑一帧：参数乱、故障位乱，但每一阶的错误都必须落进码账。
pub fn fuzz_frame(params: &FuzzParams, frame: u32) -> Result<FrameOutcome, u32> {
    if frame >= FUZZ_FRAMES {
        return Err(Code::FrameBad.val());
    }
    if !params.verify() {
        return Err(Code::ParamBad.val());
    }
    let mut st = FrameState::new(frame);
    let mix = params
        .seed
        .wrapping_add((frame as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let mut rng = p_core::Lcg::new(mix);
    let mut attempts = 0u32;
    let mut i = 0u32;
    while i < params.ops {
        attempts = attempts.saturating_add(1);
        let roll = rng.next();
        let fault = ((roll & 0x3F) as u32) & params.fault_mask;
        let ticket = if (fault & F_TICKET_ZERO) != 0 {
            0
        } else if (fault & F_TICKET_OVER) != 0 {
            TICKET_CAP.saturating_add(1)
        } else {
            1u32.saturating_add(rng.below(params.ticket_span))
        };
        let enter = 1u32.saturating_add(rng.below(params.ns_span));
        let span = 1u32.saturating_add(rng.below(params.ns_span / 2 + 1));
        let exit = enter.saturating_add(span).min(NS_LIMIT);
        let cost = if (fault & F_COST) != 0 {
            span.saturating_add(1)
        } else {
            exit.saturating_sub(enter)
        };
        let foreign = (fault & F_RESULT) != 0;
        let code = if foreign {
            SEG_BASE + 0x0F
        } else {
            SEG_BASE + 1 + rng.below(KNOWN_CODES as u32)
        };

        let mut o = match FuzzOp::new(i.saturating_add(1)) {
            Ok(v) => v,
            Err(c) => {
                st.reject(c);
                i = i.saturating_add(1);
                continue;
            }
        };
        let mut first = 0u32;
        if first == 0 {
            if let Err(c) = o.take_ticket(ticket) {
                first = c;
            }
        }
        if first == 0 && (fault & F_ORDER) != 0 {
            if let Err(c) = o.exit(exit) {
                first = c;
            }
        }
        if first == 0 {
            if let Err(c) = o.enter(enter) {
                first = c;
            }
        }
        if first == 0 {
            if let Err(c) = o.exit(exit) {
                first = c;
            }
        }
        if first == 0 {
            if let Err(c) = o.cost(cost) {
                first = c;
            }
        }
        if first == 0 {
            if let Err(c) = o.result(code) {
                first = c;
            }
        }
        if foreign {
            // 域外结果码是「送进来的坏值」，它只能进哨槽
            st.note_external(code);
        }
        if first != 0 {
            st.reject(first);
        } else {
            // push 失败时帧内已双记账（rejected 与码账同步 +1），返回值不再需要
            let pushed = st.push(o).is_ok();
            if pushed && (fault & F_OP_DUP) != 0 {
                // 重号故障：把刚入账的那条原样再推一次，也算一次尝试，必被操作号唯一闸拦下
                attempts = attempts.saturating_add(1);
                let _ = st.push(o);
            }
        }
        i = i.saturating_add(1);
    }

    Ok(FrameOutcome {
        frame,
        attempts,
        accepted: st.len() as u32,
        rejected: st.rejected(),
        external: st.external(),
        digest: st.digest(),
        hist: *st.hist(),
        max_cost: st.max_cost(),
    })
}

/// 万帧累积报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzReport {
    /// 帧数。
    pub frames: u32,
    /// 尝试次数合计。
    pub attempts: u32,
    /// 入帧条数合计。
    pub accepted: u32,
    /// 拒收合计。
    pub rejected: u32,
    /// 域外码合计。
    pub external: u32,
    /// 链摘要。
    pub digest: u64,
    /// 码账。
    pub hist: CodeHist,
    /// 最大耗时。
    pub max_cost: u32,
}

impl FuzzReport {
    /// 静默条数。
    pub fn silent(&self) -> u32 {
        self.attempts
            .saturating_sub(self.accepted)
            .saturating_sub(self.rejected)
    }

    /// 守恒判定。
    pub fn balanced(&self) -> bool {
        self.silent() == 0 && self.accepted.saturating_add(self.rejected) == self.attempts
    }

    /// 故障面触达判定：六个故障位能落到的已知码至少 `FAULT_MIN` 种全命中，
    /// 且哨不冒充已知码。14 条满窗由 `exercise_codes()` 的全扫面负责——
    /// fuzz 面只会自然产出票据/次序/耗时/结果/重号这五类。
    pub fn codes_covered(&self) -> bool {
        self.hist.touched_known() >= FAULT_MIN
            && self.hist.guard_slot_zero()
            && self.hist.known_total() == self.rejected
            && self.hist.sentinel_total() == self.external
    }
}

/// 跑 `frames` 帧并合并。
pub fn fuzz_frames(params: &FuzzParams, frames: u32) -> Result<FuzzReport, u32> {
    if frames == 0 || frames > FRAME_CAP {
        return Err(Code::FrameBad.val());
    }
    if !params.verify() {
        return Err(Code::ParamBad.val());
    }
    let mut report = FuzzReport {
        frames: 0,
        attempts: 0,
        accepted: 0,
        rejected: 0,
        external: 0,
        digest: p_core::FNV_OFF,
        hist: CodeHist::new(),
        max_cost: 0,
    };
    let mut f = 0u32;
    while f < frames {
        let out = fuzz_frame(params, f)?;
        report.frames = report.frames.saturating_add(1);
        report.attempts = report.attempts.saturating_add(out.attempts);
        report.accepted = report.accepted.saturating_add(out.accepted);
        report.rejected = report.rejected.saturating_add(out.rejected);
        report.external = report.external.saturating_add(out.external);
        report.digest = fold_digest(report.digest, out.digest);
        report.hist.merge(&out.hist);
        if out.max_cost > report.max_cost {
            report.max_cost = out.max_cost;
        }
        f = f.saturating_add(1);
    }
    Ok(report)
}

/// 判据口径：×10^4 帧。
pub fn fuzz_ten_thousand(params: &FuzzParams) -> Result<FuzzReport, u32> {
    fuzz_frames(params, FUZZ_FRAMES)
}

/// 复跑守卫：不一致一律 ReplayBad（复跑失败不许只丢一个 false 就走）。
pub fn replay_guard(consistent: bool) -> Result<(), u32> {
    if consistent {
        Ok(())
    } else {
        Err(Code::ReplayBad.val())
    }
}

/// 复跑（带码）：连续重建 `times` 次，守恒、尝试数与链摘要逐次一致；次数为零即 ReplayBad。
pub fn fuzz_replay_checked(params: &FuzzParams, times: u32) -> Result<u64, u32> {
    if times == 0 {
        return Err(Code::ReplayBad.val());
    }
    let mut want: Option<u64> = None;
    let mut t = 0u32;
    while t < times {
        let r = fuzz_frames(params, 64)?;
        replay_guard(r.balanced())?;
        replay_guard(r.attempts >= 64u32.saturating_mul(params.ops))?;
        replay_guard(r.attempts <= 128u32.saturating_mul(params.ops))?;
        match want {
            None => want = Some(r.digest),
            Some(d) => replay_guard(d == r.digest)?,
        }
        t = t.saturating_add(1);
    }
    match want {
        Some(d) => Ok(d),
        None => Err(Code::ReplayBad.val()),
    }
}

/// 复跑（布尔面）：判据用例直接取用。
pub fn fuzz_replay(params: &FuzzParams, times: u32) -> bool {
    fuzz_replay_checked(params, times).is_ok()
}

// ── 账行渲染与回读 ───────────────────────────────────────────────
/// 16 位小写十六进制。
fn hex16(v: u64) -> String {
    format!("{:016x}", v)
}

/// 十进制无符号解析（拒绝空/非数字/溢出）。
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

/// 16 位十六进制解析。
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

/// 账行摘要：只折行内可见字段。
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

/// 按固定标签与键序切行。
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

/// 通用账行解析：键序 → 数字 → 摘要重算 → 往返恒等。
/// 任一步不成立一律 RenderBad（行面绝不静默返回「空」）。
pub fn parse_row(row: &str, tag: &str) -> Result<([u32; ROW_KEYS.len()], u64), u32> {
    let bad = Code::RenderBad.val();
    let vals = match row_fields(row, tag, &ROW_KEYS) {
        Some(v) => v,
        None => return Err(bad),
    };
    let mut nums = [0u32; ROW_KEYS.len()];
    let mut i = 0usize;
    while i < ROW_KEYS.len() {
        let v = match vals.get(i).and_then(|s| parse_u32(s)) {
            Some(v) => v,
            None => return Err(bad),
        };
        nums[i] = v;
        i += 1;
    }
    let digest = match vals.get(ROW_KEYS.len()).and_then(|s| parse_hex16(s)) {
        Some(v) => v,
        None => return Err(bad),
    };
    if digest != row_digest(tag, nums[0], nums[1], nums[2], nums[3]) {
        return Err(bad);
    }
    if render_row(tag, nums[0], nums[1], nums[2], nums[3], digest) != row {
        return Err(bad);
    }
    Ok((nums, digest))
}

/// 通用账行审计：等价于 `parse_row(..).is_ok()`。
pub fn audit_render(row: &str, tag: &str) -> bool {
    parse_row(row, tag).is_ok()
}

/// 帧账行：`FZRS|k1=frame|k2=accepted|k3=rejected|k4=digit|digest=..`。
pub fn frame_row(out: &FrameOutcome) -> String {
    let digit = (out.digest & 0xFFFF_FFFF) as u32;
    render_row(
        FUZZ_TAG,
        out.frame,
        out.accepted,
        out.rejected,
        digit,
        row_digest(FUZZ_TAG, out.frame, out.accepted, out.rejected, digit),
    )
}

/// 帧账行审计：结构 + 帧号上界 + 单帧操作数上界。
pub fn audit_frame_row(row: &str) -> bool {
    match parse_row(row, FUZZ_TAG) {
        Ok((v, _)) => {
            v[0] < FUZZ_FRAMES
                && v[1] <= OPS_CAP
                && v[2] <= OPS_CAP
                && v[1].saturating_add(v[2]) <= OPS_CAP
        }
        Err(_) => false,
    }
}

// ── 用例库（20 例） ──────────────────────────────────────────────
/// 二十例的判据种类（每例一类，不重样）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseKind {
    /// 码族满窗
    FamilyWindow,
    /// 越界哨兜底
    Sentinel,
    /// 账行往返
    RowRoundtrip,
    /// 坏行全拒
    RowReject,
    /// 单帧守恒
    FuzzFrame,
    /// 零静默
    FuzzSilent,
    /// 同种子确定性
    FuzzDeterminism,
    /// 种子敏感
    FuzzSeed,
    /// 次序守卫
    StageOrder,
    /// 四点覆盖
    StageCoverage,
    /// 耗时对账
    CostLedger,
    /// 票据唯一
    TicketUnique,
    /// 操作唯一
    OpUnique,
    /// 结果码族
    ResultFamily,
    /// 摘要链
    DigestChain,
    /// 复跑一致
    Replay,
    /// 用例序号
    CaseIndex,
    /// 编号防重
    IdDedup,
    /// 段区间不撞
    SegDisjoint,
    /// 批收口留痕
    BatchSeal,
}

impl CaseKind {
    /// 序号（0..=19）。
    pub fn index(self) -> u8 {
        match self {
            CaseKind::FamilyWindow => 0,
            CaseKind::Sentinel => 1,
            CaseKind::RowRoundtrip => 2,
            CaseKind::RowReject => 3,
            CaseKind::FuzzFrame => 4,
            CaseKind::FuzzSilent => 5,
            CaseKind::FuzzDeterminism => 6,
            CaseKind::FuzzSeed => 7,
            CaseKind::StageOrder => 8,
            CaseKind::StageCoverage => 9,
            CaseKind::CostLedger => 10,
            CaseKind::TicketUnique => 11,
            CaseKind::OpUnique => 12,
            CaseKind::ResultFamily => 13,
            CaseKind::DigestChain => 14,
            CaseKind::Replay => 15,
            CaseKind::CaseIndex => 16,
            CaseKind::IdDedup => 17,
            CaseKind::SegDisjoint => 18,
            CaseKind::BatchSeal => 19,
        }
    }

    /// 判据名。
    pub fn name(self) -> &'static str {
        match self {
            CaseKind::FamilyWindow => "码族满窗",
            CaseKind::Sentinel => "越界哨兜底",
            CaseKind::RowRoundtrip => "账行往返",
            CaseKind::RowReject => "坏行全拒",
            CaseKind::FuzzFrame => "单帧守恒",
            CaseKind::FuzzSilent => "零静默",
            CaseKind::FuzzDeterminism => "同种子确定性",
            CaseKind::FuzzSeed => "种子敏感",
            CaseKind::StageOrder => "次序守卫",
            CaseKind::StageCoverage => "四点覆盖",
            CaseKind::CostLedger => "耗时对账",
            CaseKind::TicketUnique => "票据唯一",
            CaseKind::OpUnique => "操作唯一",
            CaseKind::ResultFamily => "结果码族",
            CaseKind::DigestChain => "摘要链",
            CaseKind::Replay => "复跑一致",
            CaseKind::CaseIndex => "用例序号",
            CaseKind::IdDedup => "编号防重",
            CaseKind::SegDisjoint => "段区间不撞",
            CaseKind::BatchSeal => "批收口留痕",
        }
    }

    /// 由序号反查。
    pub fn from_index(i: u8) -> Option<CaseKind> {
        match i {
            0 => Some(CaseKind::FamilyWindow),
            1 => Some(CaseKind::Sentinel),
            2 => Some(CaseKind::RowRoundtrip),
            3 => Some(CaseKind::RowReject),
            4 => Some(CaseKind::FuzzFrame),
            5 => Some(CaseKind::FuzzSilent),
            6 => Some(CaseKind::FuzzDeterminism),
            7 => Some(CaseKind::FuzzSeed),
            8 => Some(CaseKind::StageOrder),
            9 => Some(CaseKind::StageCoverage),
            10 => Some(CaseKind::CostLedger),
            11 => Some(CaseKind::TicketUnique),
            12 => Some(CaseKind::OpUnique),
            13 => Some(CaseKind::ResultFamily),
            14 => Some(CaseKind::DigestChain),
            15 => Some(CaseKind::Replay),
            16 => Some(CaseKind::CaseIndex),
            17 => Some(CaseKind::IdDedup),
            18 => Some(CaseKind::SegDisjoint),
            19 => Some(CaseKind::BatchSeal),
            _ => None,
        }
    }
}

/// 一条用例：内部序号 #01..#20 + 判据种类 + 种子。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseDef {
    /// 内部序号（1..=20）。
    pub index: u8,
    /// 内部标签（"#01".."#20"）。
    pub label: &'static str,
    /// 判据种类。
    pub kind: CaseKind,
    /// 该例的种子。
    pub seed: u64,
}

/// 二十例用例表（序号连续、标签不重、种类不重）。
pub const CASE_TABLE: [CaseDef; CASE_COUNT] = [
    CaseDef { index: 1, label: "#01", kind: CaseKind::FamilyWindow, seed: 0x7401 },
    CaseDef { index: 2, label: "#02", kind: CaseKind::Sentinel, seed: 0x7403 },
    CaseDef { index: 3, label: "#03", kind: CaseKind::RowRoundtrip, seed: 0x7405 },
    CaseDef { index: 4, label: "#04", kind: CaseKind::RowReject, seed: 0x7407 },
    CaseDef { index: 5, label: "#05", kind: CaseKind::FuzzFrame, seed: 0x7409 },
    CaseDef { index: 6, label: "#06", kind: CaseKind::FuzzSilent, seed: 0x740B },
    CaseDef { index: 7, label: "#07", kind: CaseKind::FuzzDeterminism, seed: 0x740D },
    CaseDef { index: 8, label: "#08", kind: CaseKind::FuzzSeed, seed: 0x740F },
    CaseDef { index: 9, label: "#09", kind: CaseKind::StageOrder, seed: 0x7411 },
    CaseDef { index: 10, label: "#10", kind: CaseKind::StageCoverage, seed: 0x7413 },
    CaseDef { index: 11, label: "#11", kind: CaseKind::CostLedger, seed: 0x7415 },
    CaseDef { index: 12, label: "#12", kind: CaseKind::TicketUnique, seed: 0x7417 },
    CaseDef { index: 13, label: "#13", kind: CaseKind::OpUnique, seed: 0x7419 },
    CaseDef { index: 14, label: "#14", kind: CaseKind::ResultFamily, seed: 0x741B },
    CaseDef { index: 15, label: "#15", kind: CaseKind::DigestChain, seed: 0x741D },
    CaseDef { index: 16, label: "#16", kind: CaseKind::Replay, seed: 0x741F },
    CaseDef { index: 17, label: "#17", kind: CaseKind::CaseIndex, seed: 0x7421 },
    CaseDef { index: 18, label: "#18", kind: CaseKind::IdDedup, seed: 0x7423 },
    CaseDef { index: 19, label: "#19", kind: CaseKind::SegDisjoint, seed: 0x7425 },
    CaseDef { index: 20, label: "#20", kind: CaseKind::BatchSeal, seed: 0x7427 },
];

/// 按序号查用例：越界或表内错位一律 CaseBad（用例库只有 #01..#20）。
pub fn case_lookup(index: u8) -> Result<&'static CaseDef, u32> {
    let i = match index.checked_sub(1) {
        Some(v) => v as usize,
        None => return Err(Code::CaseBad.val()),
    };
    match CASE_TABLE.get(i) {
        Some(d) => {
            if d.index == index {
                Ok(d)
            } else {
                Err(Code::CaseBad.val())
            }
        }
        None => Err(Code::CaseBad.val()),
    }
}

/// 单例结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseOutcome {
    /// 内部序号。
    pub index: u8,
    /// 内部标签。
    pub label: &'static str,
    /// 判据种类。
    pub kind: CaseKind,
    /// 是否通过。
    pub passed: bool,
    /// 证据载荷（该例实际验到的数量/值）。
    pub payload: u32,
    /// 账行摘要。
    pub digest: u64,
}

impl CaseOutcome {
    /// 该例的账行。
    pub fn row(&self) -> String {
        render_row(
            CASE_TAG,
            self.index as u32,
            self.kind.index() as u32,
            if self.passed { 1 } else { 0 },
            self.payload,
            self.digest,
        )
    }
}

/// 用例账行审计：结构 + 序号 1..=20 + 种类 0..=19 + 通过位 0/1。
pub fn audit_case_row(row: &str) -> bool {
    match parse_row(row, CASE_TAG) {
        Ok((v, _)) => {
            v[0] >= 1
                && v[0] <= CASE_COUNT as u32
                && v[1] < CASE_COUNT as u32
                && v[2] <= 1
                && CaseKind::from_index(v[1] as u8).is_some()
        }
        Err(_) => false,
    }
}

// 二十例的判据实现，一例一函数（返回值 = 通过? , 证据载荷, 说明）
fn case_window(_seed: u64) -> (bool, u32, &'static str) {
    let mut ok = segment_guard();
    let vals = family_values();
    if vals.len() != KNOWN_CODES {
        ok = false;
    }
    // 表必须升序且首尾贴住窗口
    let mut i = 1usize;
    while i < vals.len() {
        if vals[i] <= vals[i - 1] {
            ok = false;
        }
        i += 1;
    }
    if vals.first().copied() != Some(window_lo()) || vals.last().copied() != Some(window_hi()) {
        ok = false;
    }
    // 说明互不重复
    let mut a = 0usize;
    while a < Code::ALL.len() {
        let mut b = a + 1;
        while b < Code::ALL.len() {
            if Code::ALL[a].message() == Code::ALL[b].message() {
                ok = false;
            }
            b += 1;
        }
        a += 1;
    }
    (ok, KNOWN_CODES as u32, "窗口/升序/文案")
}

fn case_sentinel(_seed: u64) -> (bool, u32, &'static str) {
    let probes: [u32; 8] = [
        0,
        SEG_BASE,
        SEG_BASE - 1,
        SEG_BASE + 0x0F,
        0x73381,
        0x73441,
        SEG_BASE + SEG_SPAN,
        u32::MAX,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < probes.len() {
        if let Some(c) = probes.get(i) {
            if detail(*c) != UNKNOWN {
                ok = false;
            }
            if is_known(*c) {
                ok = false;
            }
            if family_slot(*c) != SENTINEL_SLOT {
                ok = false;
            }
        }
        i += 1;
    }
    (ok, probes.len() as u32, "段外一律哨槽")
}

fn case_row_roundtrip(_seed: u64) -> (bool, u32, &'static str) {
    let tags: [&str; 4] = [FUZZ_TAG, CASE_TAG, ID_TAG, SEG_TAG];
    let mut ok = true;
    let mut n = 0u32;
    let mut t = 0usize;
    while t < tags.len() {
        let tag = match tags.get(t) {
            Some(v) => *v,
            None => return (false, 0, "标签表缺"),
        };
        let d = row_digest(tag, 7, 8, 9, 10);
        let row = render_row(tag, 7, 8, 9, 10, d);
        if !audit_render(&row, tag) {
            ok = false;
        }
        match row_fields(&row, tag, &ROW_KEYS) {
            Some(v) => {
                let a = v.first().and_then(|s| parse_u32(s));
                let b = v.get(1).and_then(|s| parse_u32(s));
                let c = v.get(2).and_then(|s| parse_u32(s));
                let dd = v.get(3).and_then(|s| parse_u32(s));
                let dg = v.get(4).and_then(|s| parse_hex16(s));
                match (a, b, c, dd, dg) {
                    (Some(a), Some(b), Some(c), Some(dd), Some(dg)) => {
                        if render_row(tag, a, b, c, dd, dg) != row {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
            }
            None => ok = false,
        }
        n = n.saturating_add(1);
        t += 1;
    }
    (ok, n, "四标签往返恒等")
}

fn case_row_reject(_seed: u64) -> (bool, u32, &'static str) {
    let bad: [&str; 9] = [
        "",
        "FZRS|k1=0|k2=1|k3=1",
        "FZRS|k2=1|k1=0|k3=1|k4=0|digest=0000000000000000",
        "FZRS|k1=0|k2=1|k3=1|k4=0|digest=000000000000000z",
        "FZRS|k1=0|k2=1|k3=1|k4=0|digest=0000000000000000",
        "FZRS|k1=0|k2=1|k3=1|k4=0|digest=0000000000000000|extra=1",
        "CASE|k1=0|k2=1|k3=1|k4=0|digest=0000000000000000",
        "FZRS|k1=|k2=1|k3=1|k4=0|digest=0000000000000000",
        "FZRS|k1=99999999999|k2=1|k3=1|k4=0|digest=0000000000000000",
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < bad.len() {
        if let Some(r) = bad.get(i) {
            if audit_render(r, FUZZ_TAG) {
                ok = false;
            }
        }
        i += 1;
    }
    (ok, bad.len() as u32, "九类坏行全拒")
}

fn case_fuzz_frame(seed: u64) -> (bool, u32, &'static str) {
    let p = FuzzParams::standard(seed);
    match fuzz_frame(&p, 7) {
        Ok(out) => {
            // 重号故障会额外记一次尝试，故尝试数落在 [ops, 2*ops]
            let ok = out.balanced()
                && out.frame == 7
                && out.attempts >= p.ops
                && out.attempts <= p.ops.saturating_mul(2)
                && out.hist.guard_slot_zero()
                && out.hist.known_total() == out.rejected;
            (ok, out.accepted, "单帧守恒 attempts=acc+rej")
        }
        Err(_) => (false, 0, "单帧失败"),
    }
}

fn case_fuzz_silent(seed: u64) -> (bool, u32, &'static str) {
    let p = FuzzParams::standard(seed);
    match fuzz_frames(&p, 64) {
        Ok(r) => (
            r.balanced() && r.silent() == 0 && r.frames == 64,
            64,
            "64 帧零静默",
        ),
        Err(_) => (false, 0, "累积失败"),
    }
}

fn case_fuzz_determinism(seed: u64) -> (bool, u32, &'static str) {
    let p = FuzzParams::standard(seed);
    match (fuzz_frames(&p, 32), fuzz_frames(&p, 32)) {
        (Ok(a), Ok(b)) => (a == b && a.digest == b.digest, 32, "同参数双跑逐字一致"),
        _ => (false, 0, "双跑失败"),
    }
}

fn case_fuzz_seed(seed: u64) -> (bool, u32, &'static str) {
    // 两个种子至少相差 2 且不被 |1 合并（0xXX01 与 0xXX03）
    let a = match FuzzParams::new(seed, 4, 256, 200_000, F_ALL) {
        Ok(v) => v,
        Err(_) => return (false, 0, "参数构造失败"),
    };
    let b = match FuzzParams::new(seed.wrapping_add(2), 4, 256, 200_000, F_ALL) {
        Ok(v) => v,
        Err(_) => return (false, 0, "参数构造失败"),
    };
    match (fuzz_frames(&a, 16), fuzz_frames(&b, 16)) {
        (Ok(x), Ok(y)) => (x.digest != y.digest, 16, "异种子异摘要"),
        _ => (false, 0, "双种子失败"),
    }
}

fn case_stage_order(seed: u64) -> (bool, u32, &'static str) {
    let mut ok = true;
    let mut o = match FuzzOp::new(1) {
        Ok(v) => v,
        Err(_) => return (false, 0, "构造失败"),
    };
    if o.enter(10).err() != Some(Code::OrderBad.val()) {
        ok = false;
    }
    if o.exit(20).err() != Some(Code::OrderBad.val()) {
        ok = false;
    }
    if o.cost(1).err() != Some(Code::OrderBad.val()) {
        ok = false;
    }
    if o.result(SEG_BASE + 1).err() != Some(Code::OrderBad.val()) {
        ok = false;
    }
    if o.take_ticket(0).err() != Some(Code::TicketBad.val()) {
        ok = false;
    }
    if o.take_ticket(TICKET_CAP + 1).err() != Some(Code::TicketBad.val()) {
        ok = false;
    }
    if o.take_ticket(1 + (seed & 0x3F) as u32).is_err() {
        ok = false;
    }
    if o.take_ticket(2).err() != Some(Code::OrderBad.val()) {
        ok = false;
    }
    if o.enter(0).err() != Some(Code::CostBad.val()) {
        ok = false;
    }
    // StageBad：五阶之外没有名字，反查必拒；五阶之内名字必须非空
    if stage_name(STAGES as u8).err() != Some(Code::StageBad.val()) {
        ok = false;
    }
    if stage_name(STAGE_CLOSED).err() != Some(Code::StageBad.val()) {
        ok = false;
    }
    let mut k = 0u8;
    while k < STAGES as u8 {
        match stage_name(k) {
            Ok(n) => {
                if n.is_empty() {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        k = k.saturating_add(1);
    }
    (ok, 5, "五阶游标守卫")
}

fn case_stage_coverage(seed: u64) -> (bool, u32, &'static str) {
    let mut st = FrameState::new(0);
    let mut ok = true;
    let mut i = 0u32;
    while i < 3 {
        let mut o = match FuzzOp::new(i + 1) {
            Ok(v) => v,
            Err(_) => return (false, 0, "构造失败"),
        };
        let ticket = 1 + (i as u32 + 1) * 7 + (seed & 0x07) as u32;
        let enter = 100 + i * 10;
        if o.take_ticket(ticket).is_err() {
            ok = false;
        }
        if o.enter(enter).is_err() {
            ok = false;
        }
        if o.exit(enter + 20).is_err() {
            ok = false;
        }
        if o.cost(20).is_err() {
            ok = false;
        }
        if o.result(SEG_BASE + 1 + i).is_err() {
            ok = false;
        }
        if o.points() != FOUR_POINT_MASK || !o.four_points() || !o.closed() {
            ok = false;
        }
        if st.push(o).is_err() {
            ok = false;
        }
        i += 1;
    }
    (ok && st.len() == 3 && st.verify(), st.len() as u32, "三条四点全覆盖")
}

fn case_cost_ledger(seed: u64) -> (bool, u32, &'static str) {
    let mut ok = true;
    let mut o = match FuzzOp::new(1) {
        Ok(v) => v,
        Err(_) => return (false, 0, "构造失败"),
    };
    if o.take_ticket(1 + (seed & 0x0F) as u32).is_err() {
        ok = false;
    }
    if o.enter(1_000).is_err() {
        ok = false;
    }
    if o.exit(1_500).is_err() {
        ok = false;
    }
    if o.cost(501).err() != Some(Code::CostBad.val()) {
        ok = false;
    }
    if o.cost(499).err() != Some(Code::CostBad.val()) {
        ok = false;
    }
    if o.cost(500).is_err() {
        ok = false;
    }
    // 出口不晚于入口
    let mut b = match FuzzOp::new(2) {
        Ok(v) => v,
        Err(_) => return (false, 0, "构造失败"),
    };
    if b.take_ticket(9_000).is_err() {
        // 超容先被票据闸拦下（这里只确认它没有逃过闸）
    }
    if b.take_ticket(9).is_err() {
        ok = false;
    }
    if b.enter(2_000).is_err() {
        ok = false;
    }
    if b.exit(2_000).err() != Some(Code::CostBad.val()) {
        ok = false;
    }
    (ok, 500, "耗时复算与边界")
}

fn case_ticket_unique(seed: u64) -> (bool, u32, &'static str) {
    let mut st = FrameState::new(1);
    let mut ok = true;
    let base = 1 + (seed & 0x1F) as u32;
    let mut a = match FuzzOp::new(1) {
        Ok(v) => v,
        Err(_) => return (false, 0, "构造失败"),
    };
    if a.take_ticket(base).is_err() || a.enter(10).is_err() || a.exit(20).is_err() {
        ok = false;
    }
    if a.cost(10).is_err() || a.result(SEG_BASE + 1).is_err() {
        ok = false;
    }
    let mut b = match FuzzOp::new(2) {
        Ok(v) => v,
        Err(_) => return (false, 0, "构造失败"),
    };
    if b.take_ticket(base).is_err() || b.enter(10).is_err() || b.exit(20).is_err() {
        ok = false;
    }
    if b.cost(10).is_err() || b.result(SEG_BASE + 2).is_err() {
        ok = false;
    }
    if st.push(a).is_err() {
        ok = false;
    }
    if st.push(b).err() != Some(Code::TicketBad.val()) {
        ok = false;
    }
    (ok && st.len() == 1, 1, "帧内票据唯一")
}

fn case_op_unique(seed: u64) -> (bool, u32, &'static str) {
    let mut st = FrameState::new(2);
    let mut ok = true;
    let mut a = match FuzzOp::new(5) {
        Ok(v) => v,
        Err(_) => return (false, 0, "构造失败"),
    };
    if a.take_ticket(1 + (seed & 0x0F) as u32).is_err()
        || a.enter(10).is_err()
        || a.exit(30).is_err()
        || a.cost(20).is_err()
        || a.result(SEG_BASE + 3).is_err()
    {
        ok = false;
    }
    let mut b = match FuzzOp::new(5) {
        Ok(v) => v,
        Err(_) => return (false, 0, "构造失败"),
    };
    if b.take_ticket(100).is_err()
        || b.enter(10).is_err()
        || b.exit(30).is_err()
        || b.cost(20).is_err()
        || b.result(SEG_BASE + 3).is_err()
    {
        ok = false;
    }
    if st.push(a).is_err() {
        ok = false;
    }
    if st.push(b).err() != Some(Code::OpBad.val()) {
        ok = false;
    }
    (ok && st.len() == 1, 1, "帧内操作唯一")
}

fn case_result_family(seed: u64) -> (bool, u32, &'static str) {
    let mut ok = true;
    let mut o = match FuzzOp::new(1) {
        Ok(v) => v,
        Err(_) => return (false, 0, "构造失败"),
    };
    if o.take_ticket(1 + (seed & 0x07) as u32).is_err()
        || o.enter(10).is_err()
        || o.exit(20).is_err()
        || o.cost(10).is_err()
    {
        return (false, 0, "前置失败");
    }
    if o.result(0).err() != Some(Code::ResultBad.val()) {
        ok = false;
    }
    if o.result(SEG_BASE + 0x0F).err() != Some(Code::ResultBad.val()) {
        ok = false;
    }
    if o.result(0x73381).err() != Some(Code::ResultBad.val()) {
        ok = false;
    }
    if o.result(SEG_BASE + KNOWN_CODES as u32).is_err() {
        ok = false;
    }
    (ok && o.closed(), KNOWN_CODES as u32, "结果码仅限段内")
}

fn case_digest_chain(seed: u64) -> (bool, u32, &'static str) {
    let mut st = FrameState::new(3);
    let mut ok = true;
    let mut i = 0u32;
    while i < 3 {
        let mut o = match FuzzOp::new(i + 1) {
            Ok(v) => v,
            Err(_) => return (false, 0, "构造失败"),
        };
        let t = 1 + i * 3 + (seed & 0x03) as u32;
        if o.take_ticket(t).is_err()
            || o.enter(10 + i).is_err()
            || o.exit(30 + i).is_err()
            || o.cost(20).is_err()
            || o.result(SEG_BASE + 1 + i).is_err()
        {
            ok = false;
        }
        if st.push(o).is_err() {
            ok = false;
        }
        i += 1;
    }
    if st.digest() != st.recompute() {
        ok = false;
    }
    if st.digest() == p_core::FNV_OFF {
        ok = false;
    }
    if !st.verify() {
        ok = false;
    }
    // 篡改一条的耗时就地重推：摘要闸必须先拦，不许静默收下
    if let Some(mut first) = st.get(0).copied() {
        let clean = first.digest;
        first.cost_ns = first.cost_ns.saturating_add(1);
        // 字段变了但摘要没刷新 → 篡改版摘要与原始必须不同
        let tampered = FuzzOp {
            op: first.op,
            ticket: first.ticket,
            enter_ns: first.enter_ns,
            exit_ns: first.exit_ns,
            cost_ns: first.cost_ns,
            code: first.code,
            stage: first.stage,
            digest: op_digest(
                first.op,
                first.ticket,
                first.enter_ns,
                first.exit_ns,
                first.cost_ns,
                first.code,
                first.stage,
            ),
        };
        if tampered.digest == clean {
            ok = false;
        }
        // 原摘要栏位的老值推回账里，必须被 DigestBad 拒收
        let mut guard = FrameState::new(3);
        if guard.push(first).err() != Some(Code::DigestBad.val()) {
            ok = false;
        }
        // 拒收也要双记账：已知码计数 == 拒收数
        if guard.rejected() != 1 || guard.hist().known_total() != 1 {
            ok = false;
        }
        // 刷新摘要后同一条可以入账，且新链摘要与老链必然不同
        let mut fresh = FrameState::new(3);
        if fresh.push(tampered).is_err() {
            ok = false;
        }
        if fresh.digest() == st.digest() {
            ok = false;
        }
    } else {
        ok = false;
    }
    (ok, st.len() as u32, "链摘要可重算")
}

fn case_replay(seed: u64) -> (bool, u32, &'static str) {
    let p = FuzzParams::standard(seed);
    (fuzz_replay(&p, 3), 3, "三次复跑同摘要")
}

fn case_case_index(_seed: u64) -> (bool, u32, &'static str) {
    let mut ok = true;
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        // 反查必须逐位对上表序，越界或错位都是 CaseBad
        match case_lookup((i + 1) as u8) {
            Ok(d) => {
                if d.index as usize != i + 1 {
                    ok = false;
                }
                if !d.label.starts_with('#') {
                    ok = false;
                }
                let mut j = i + 1;
                while j < CASE_TABLE.len() {
                    if let Some(e) = CASE_TABLE.get(j) {
                        if e.label == d.label
                            || e.index == d.index
                            || e.kind.index() == d.kind.index()
                        {
                            ok = false;
                        }
                    }
                    j += 1;
                }
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    if case_lookup(0).err() != Some(Code::CaseBad.val()) {
        ok = false;
    }
    if case_lookup((CASE_COUNT + 1) as u8).err() != Some(Code::CaseBad.val()) {
        ok = false;
    }
    if case_lookup(u8::MAX).err() != Some(Code::CaseBad.val()) {
        ok = false;
    }
    (ok && CASE_TABLE.len() == CASE_COUNT, CASE_TABLE.len() as u32, "序号/标签/种类均不重")
}

fn case_id_dedup(seed: u64) -> (bool, u32, &'static str) {
    let mut reg = IdRegistry::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < KNOWN_CODES {
        if reg.register(SEG_BASE + 1 + i as u32).is_err() {
            ok = false;
        }
        i += 1;
    }
    let taken = SEG_BASE + 1 + (seed & 0x07) as u32;
    if reg.register(taken).err() != Some(Code::IdBad.val()) {
        ok = false;
    }
    // 邻段号：几何上不属于本段，必须拒
    if reg.register(0x73381).err() != Some(Code::IdBad.val()) {
        ok = false;
    }
    if reg.register(SEG_BASE).err() != Some(Code::IdBad.val()) {
        ok = false;
    }
    if reg.register(0).err() != Some(Code::IdBad.val()) {
        ok = false;
    }
    if reg.collisions() != 4 {
        ok = false;
    }
    if !reg.verify() {
        ok = false;
    }
    (ok && reg.len() == KNOWN_CODES, reg.collisions(), "撞号四次全部留痕")
}

fn case_seg_disjoint(_seed: u64) -> (bool, u32, &'static str) {
    let mut ok = segments_disjoint();
    if own_slot() == usize::MAX {
        ok = false;
    }
    if classify(SEG_BASE) != Some(own_slot()) {
        ok = false;
    }
    if classify(SEG_BASE + SEG_SPAN) == Some(own_slot()) {
        ok = false;
    }
    // 每个用例号都必须落在本段
    let mut i = 1u8;
    while i <= CASE_COUNT as u8 {
        if classify(case_id(i)) != Some(own_slot()) {
            ok = false;
        }
        i += 1;
    }
    (ok, SEG_LEDGER.len() as u32, "七段互斥")
}

fn case_batch_seal(seed: u64) -> (bool, u32, &'static str) {
    match seal_standard(seed) {
        Ok((reg, seal)) => {
            let ok = seal.green
                && seal.registered == (KNOWN_CODES + CASE_COUNT) as u32
                && seal.collisions == 0
                && reg.verify()
                && seal.digest == reg.digest();
            (ok, seal.registered, "三十四号收口留痕")
        }
        Err(_) => (false, 0, "收口失败"),
    }
}

/// 跑一条用例。
pub fn run_case(def: &CaseDef) -> CaseOutcome {
    let (passed, payload, _note) = match def.kind {
        CaseKind::FamilyWindow => case_window(def.seed),
        CaseKind::Sentinel => case_sentinel(def.seed),
        CaseKind::RowRoundtrip => case_row_roundtrip(def.seed),
        CaseKind::RowReject => case_row_reject(def.seed),
        CaseKind::FuzzFrame => case_fuzz_frame(def.seed),
        CaseKind::FuzzSilent => case_fuzz_silent(def.seed),
        CaseKind::FuzzDeterminism => case_fuzz_determinism(def.seed),
        CaseKind::FuzzSeed => case_fuzz_seed(def.seed),
        CaseKind::StageOrder => case_stage_order(def.seed),
        CaseKind::StageCoverage => case_stage_coverage(def.seed),
        CaseKind::CostLedger => case_cost_ledger(def.seed),
        CaseKind::TicketUnique => case_ticket_unique(def.seed),
        CaseKind::OpUnique => case_op_unique(def.seed),
        CaseKind::ResultFamily => case_result_family(def.seed),
        CaseKind::DigestChain => case_digest_chain(def.seed),
        CaseKind::Replay => case_replay(def.seed),
        CaseKind::CaseIndex => case_case_index(def.seed),
        CaseKind::IdDedup => case_id_dedup(def.seed),
        CaseKind::SegDisjoint => case_seg_disjoint(def.seed),
        CaseKind::BatchSeal => case_batch_seal(def.seed),
    };
    let digest = row_digest(
        CASE_TAG,
        def.index as u32,
        def.kind.index() as u32,
        if passed { 1 } else { 0 },
        payload,
    );
    CaseOutcome {
        index: def.index,
        label: def.label,
        kind: def.kind,
        passed,
        payload,
        digest,
    }
}

/// 全表结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseReport {
    /// 通过数。
    pub green: u32,
    /// 未通过数。
    pub red: u32,
    /// 链摘要（按表序折每例摘要）。
    pub digest: u64,
    /// 逐例结果（表序）。
    pub outcomes: Vec<CaseOutcome>,
    /// 账行（表序，与 outcomes 一一对应）。
    pub rows: Vec<String>,
}

impl CaseReport {
    /// 全绿判定。
    pub fn all_green(&self) -> bool {
        self.red == 0 && self.green as usize == CASE_COUNT
    }

    /// 链摘要重算。
    pub fn recompute(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                h = fold_digest(h, o.digest);
            }
            i += 1;
        }
        h
    }

    /// 逐行审计。
    pub fn rows_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if !audit_case_row(r) {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        self.rows.len() == CASE_COUNT && !self.rows.is_empty()
    }
}

/// 跑全表二十例。
pub fn run_cases() -> CaseReport {
    let mut outcomes: Vec<CaseOutcome> = Vec::with_capacity(CASE_COUNT);
    let mut rows: Vec<String> = Vec::with_capacity(CASE_COUNT);
    let mut green = 0u32;
    let mut red = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        if let Some(def) = CASE_TABLE.get(i) {
            let out = run_case(def);
            if out.passed {
                green = green.saturating_add(1);
            } else {
                red = red.saturating_add(1);
            }
            digest = fold_digest(digest, out.digest);
            rows.push(out.row());
            outcomes.push(out);
        }
        i += 1;
    }
    CaseReport {
        green,
        red,
        digest,
        outcomes,
        rows,
    }
}

/// 复跑（带码）：全表 `times` 次，逐行逐字与链摘要都必须一致；次数为零即 ReplayBad。
pub fn replay_cases_checked(times: u32) -> Result<u64, u32> {
    if times == 0 {
        return Err(Code::ReplayBad.val());
    }
    let mut want: Option<u64> = None;
    let mut first_rows: Vec<String> = Vec::new();
    let mut t = 0u32;
    while t < times {
        let r = run_cases();
        replay_guard(r.all_green())?;
        replay_guard(r.rows_ok())?;
        replay_guard(r.digest == r.recompute())?;
        if t == 0 {
            first_rows = r.rows.clone();
        } else {
            replay_guard(r.rows.len() == first_rows.len())?;
            let mut i = 0usize;
            while i < first_rows.len() {
                replay_guard(first_rows.get(i) == r.rows.get(i))?;
                i += 1;
            }
        }
        match want {
            None => want = Some(r.digest),
            Some(d) => replay_guard(d == r.digest)?,
        }
        t = t.saturating_add(1);
    }
    match want {
        Some(d) => Ok(d),
        None => Err(Code::ReplayBad.val()),
    }
}

/// 复跑（布尔面）：判据用例直接取用。
pub fn replay_cases(times: u32) -> bool {
    replay_cases_checked(times).is_ok()
}

// ── 编号登记与批收口 ─────────────────────────────────────────────
/// 编号登记器：只肯收自己段里的号，撞号留痕不静默。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IdRegistry {
    codes: Vec<u32>,
    slots: Vec<usize>,
    collisions: u32,
    digest: u64,
}

impl IdRegistry {
    /// 空登记器。
    pub fn new() -> IdRegistry {
        IdRegistry {
            codes: Vec::new(),
            slots: Vec::new(),
            collisions: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 撞号留痕：计数 +1 并返回 IdBad。
    fn collide(&mut self) -> u32 {
        self.collisions = self.collisions.saturating_add(1);
        Code::IdBad.val()
    }

    /// 登记一个全局编号。
    pub fn register(&mut self, code: u32) -> Result<(), u32> {
        let slot = match classify(code) {
            Some(s) => s,
            None => return Err(self.collide()),
        };
        if slot != own_slot() {
            return Err(self.collide());
        }
        if code == SEG_BASE {
            return Err(self.collide());
        }
        if self.codes.contains(&code) {
            return Err(self.collide());
        }
        if self.codes.len() >= ID_CAP {
            return Err(self.collide());
        }
        self.codes.push(code);
        self.slots.push(slot);
        self.digest = fold_digest(self.digest, code as u64);
        Ok(())
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.codes.len()
    }

    /// 空登记器判定。
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    /// 按下标取编号。
    pub fn get(&self, i: usize) -> Option<u32> {
        match self.codes.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 是否已登记。
    pub fn contains(&self, code: u32) -> bool {
        self.codes.contains(&code)
    }

    /// 撞号计数。
    pub fn collisions(&self) -> u32 {
        self.collisions
    }

    /// 链摘要。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 链摘要重算。
    pub fn recompute(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.codes.len() {
            if let Some(c) = self.codes.get(i) {
                h = fold_digest(h, *c as u64);
            }
            i += 1;
        }
        h
    }

    /// 自洽：全部落在本段、无重复、链摘要可重算。
    pub fn verify(&self) -> bool {
        let own = own_slot();
        if own == usize::MAX {
            return false;
        }
        let mut i = 0usize;
        while i < self.codes.len() {
            match (self.codes.get(i), self.slots.get(i)) {
                (Some(c), Some(s)) => {
                    if *s != own {
                        return false;
                    }
                    if classify(*c) != Some(own) {
                        return false;
                    }
                    if *c == SEG_BASE {
                        return false;
                    }
                    let mut j = i + 1;
                    while j < self.codes.len() {
                        if self.codes.get(j) == Some(c) {
                            return false;
                        }
                        j += 1;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        self.digest == self.recompute()
    }

    /// 登记账行：编号 / 段槽 / 段内偏移 / 序位。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.codes.len());
        let base = match SEG_LEDGER.get(own_slot()) {
            Some((_, b)) => *b,
            None => return out,
        };
        let mut i = 0usize;
        while i < self.codes.len() {
            if let Some(c) = self.codes.get(i) {
                let local = c.saturating_sub(base);
                let ord = i as u32;
                out.push(render_row(
                    ID_TAG,
                    *c,
                    own_slot() as u32,
                    local,
                    ord,
                    row_digest(ID_TAG, *c, own_slot() as u32, local, ord),
                ));
            }
            i += 1;
        }
        out
    }
}

/// 编号账行审计：结构 + 段槽 + 偏移 + 偏移回算编号。
pub fn audit_id_row(row: &str) -> bool {
    let (code, slot, local) = match parse_row(row, ID_TAG) {
        Ok((v, _)) => (v[0], v[1], v[2]),
        Err(_) => return false,
    };
    let base = match SEG_LEDGER.get(slot as usize) {
        Some((_, b)) => *b,
        None => return false,
    };
    if slot as usize != own_slot() {
        return false;
    }
    if local == 0 || local >= SEG_SPAN {
        return false;
    }
    if code != base.saturating_add(local) {
        return false;
    }
    classify(code) == Some(slot as usize)
}

/// 收口留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Seal {
    /// 已登记条数。
    pub registered: u32,
    /// 撞号次数。
    pub collisions: u32,
    /// 链摘要。
    pub digest: u64,
    /// 账行条数。
    pub rows: u32,
    /// 收口是否绿。
    pub green: bool,
}

/// 标准收口：14 条诊断码 + 20 个用例号，全部留痕后封账。
/// 种子为零即「未注入」，收口直接拒（种子在这里的语义是批号盐，非随机数）。
pub fn seal_standard(seed: u64) -> Result<(IdRegistry, Seal), u32> {
    if seed == 0 {
        return Err(Code::SeedBad.val());
    }
    let mut reg = IdRegistry::new();
    let mut i = 0usize;
    while i < KNOWN_CODES {
        let _ = reg.register(SEG_BASE + 1 + i as u32);
        i += 1;
    }
    let mut k = 1u8;
    while k <= CASE_COUNT as u8 {
        let _ = reg.register(case_id(k));
        k = k.saturating_add(1);
    }
    let rows = reg.rows();
    let mut rows_ok = rows.len() == reg.len();
    let mut j = 0usize;
    while j < rows.len() {
        match rows.get(j) {
            Some(r) => {
                if !audit_id_row(r) {
                    rows_ok = false;
                }
            }
            None => rows_ok = false,
        }
        j += 1;
    }
    let green = reg.verify()
        && rows_ok
        && reg.len() == KNOWN_CODES + CASE_COUNT
        && reg.collisions() == 0
        && segments_disjoint()
        && classify(SEG_BASE) == Some(own_slot());
    let seal = Seal {
        registered: reg.len() as u32,
        collisions: reg.collisions(),
        digest: reg.digest(),
        rows: rows.len() as u32,
        green,
    };
    Ok((reg, seal))
}

// ── 码族全扫 ─────────────────────────────────────────────────────
/// 码族全扫结果：码账 + 失手次数（失手即静默，必须为零）+ 域外码送检数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeSweep {
    /// 码账。
    pub hist: CodeHist,
    /// 失手次数（调了接口却没拿到预期码）。
    pub misses: u32,
    /// 送进哨槽的域外码条数。
    pub external: u32,
}

impl CodeSweep {
    /// 空扫。
    pub const fn new() -> CodeSweep {
        CodeSweep {
            hist: CodeHist::new(),
            misses: 0,
            external: 0,
        }
    }

    /// 记一次触发：拿到的码必须就是预期那个，否则记失手（绝不静默）。
    fn note(&mut self, got: Option<u32>, want: u32) -> bool {
        match got {
            Some(v) => {
                self.hist.bump(v);
                if v == want {
                    true
                } else {
                    self.misses = self.misses.saturating_add(1);
                    false
                }
            }
            None => {
                self.misses = self.misses.saturating_add(1);
                false
            }
        }
    }

    /// 记一次送检：域外码只准进哨槽。
    fn foreign(&mut self, frame: &mut FrameState, code: u32) -> bool {
        let before = self.hist.sentinel_total();
        frame.note_external(code);
        self.hist.bump(code);
        self.external = self.external.saturating_add(1);
        self.hist.sentinel_total() == before.saturating_add(1)
    }

    /// 满窗判定：14 条已知码全命中、零失手、哨只记域外、0 号槽恒零。
    pub fn full(&self) -> bool {
        self.misses == 0
            && self.hist.touched_known() as usize == KNOWN_CODES
            && self.hist.total()
                == self
                    .hist
                    .known_total()
                    .saturating_add(self.hist.sentinel_total())
            && self.hist.guard_slot_zero()
            && self.external > 0
            && self.hist.sentinel_total() == self.external
    }
}

/// 造一条完整闭环的观测（四点齐全、摘要自洽）；任一步失败即 None。
fn closed_op(op: u32, ticket: u32, enter: u32, cost: u32, code: u32) -> Option<FuzzOp> {
    let mut o = FuzzOp::new(op).ok()?;
    o.take_ticket(ticket).ok()?;
    o.enter(enter).ok()?;
    o.exit(enter.checked_add(cost)?).ok()?;
    o.cost(cost).ok()?;
    o.result(code).ok()?;
    Some(o)
}

/// 码族全扫：14 条已知码各由真实接口触发一次，外加域外码送哨。
/// 任何一条触发不到都记进 `misses`，绝不静默。
pub fn exercise_codes() -> CodeSweep {
    let mut sw = CodeSweep::new();
    let p = FuzzParams::standard(0x7401);
    let one = SEG_BASE + 1;

    // 1) FrameBad：帧号越界 / 帧数越界（参数本身合法）
    sw.note(fuzz_frame(&p, FUZZ_FRAMES).err(), Code::FrameBad.val());
    sw.note(fuzz_frames(&p, 0).err(), Code::FrameBad.val());

    // 2) SeedBad：种子为零（参数面 + 收口面各一次）
    sw.note(
        FuzzParams::new(0, 4, 256, 200_000, F_ALL).err(),
        Code::SeedBad.val(),
    );
    sw.note(seal_standard(0).err(), Code::SeedBad.val());

    // 3) ParamBad：操作数为零 + 单帧容量溢出（第 9 条顶到 OPS_CAP）
    sw.note(
        FuzzParams::new(1, 0, 256, 200_000, F_ALL).err(),
        Code::ParamBad.val(),
    );
    let mut pack = FrameState::new(0);
    let mut i = 1u32;
    while i <= OPS_CAP {
        if let Some(o) = closed_op(i, i, 100 + i, 10, one) {
            match pack.push(o) {
                Ok(()) => {}
                Err(_) => sw.misses = sw.misses.saturating_add(1),
            }
        } else {
            sw.misses = sw.misses.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    match closed_op(OPS_CAP + 1, OPS_CAP + 1, 500, 10, one) {
        Some(o) => {
            sw.note(pack.push(o).err(), Code::ParamBad.val());
        }
        None => sw.misses = sw.misses.saturating_add(1),
    }

    // 4) OpBad：操作号为零 + 同帧重号
    sw.note(FuzzOp::new(0).err(), Code::OpBad.val());
    let mut dup = FrameState::new(1);
    if let Some(o) = closed_op(1, 11, 200, 10, one) {
        match dup.push(o) {
            Ok(()) => {}
            Err(_) => sw.misses = sw.misses.saturating_add(1),
        }
    }
    if let Some(o) = closed_op(1, 12, 300, 10, one) {
        sw.note(dup.push(o).err(), Code::OpBad.val());
    }

    // 5) TicketBad：票据为零 + 超容 + 同帧重票
    let mut tk = match FuzzOp::new(2) {
        Ok(v) => v,
        Err(_) => {
            sw.misses = sw.misses.saturating_add(1);
            return sw;
        }
    };
    sw.note(tk.take_ticket(0).err(), Code::TicketBad.val());
    sw.note(
        tk.take_ticket(TICKET_CAP.saturating_add(1)).err(),
        Code::TicketBad.val(),
    );
    let mut tick = FrameState::new(2);
    if let Some(o) = closed_op(1, 21, 400, 10, one) {
        match tick.push(o) {
            Ok(()) => {}
            Err(_) => sw.misses = sw.misses.saturating_add(1),
        }
    }
    if let Some(o) = closed_op(2, 21, 500, 10, one) {
        sw.note(tick.push(o).err(), Code::TicketBad.val());
    }

    // 6) StageBad：五阶之外没有阶段名（终态 closed 也不算阶段）
    sw.note(stage_name(STAGES as u8).err(), Code::StageBad.val());
    sw.note(stage_name(STAGE_CLOSED).err(), Code::StageBad.val());

    // 7) OrderBad：跳阶（未取号就进/出/耗时/报结果）+ 未闭环就入帧
    let mut ord = match FuzzOp::new(3) {
        Ok(v) => v,
        Err(_) => {
            sw.misses = sw.misses.saturating_add(1);
            return sw;
        }
    };
    sw.note(ord.enter(10).err(), Code::OrderBad.val());
    sw.note(ord.cost(10).err(), Code::OrderBad.val());
    let mut half = FrameState::new(3);
    if let Ok(o) = FuzzOp::new(4) {
        sw.note(half.push(o).err(), Code::OrderBad.val());
    }

    // 8) CostBad：入口为零 + 耗时与出口−入口对不上
    let mut cost = match FuzzOp::new(5) {
        Ok(v) => v,
        Err(_) => {
            sw.misses = sw.misses.saturating_add(1);
            return sw;
        }
    };
    if cost.take_ticket(31).is_err() {
        sw.misses = sw.misses.saturating_add(1);
    }
    sw.note(cost.enter(0).err(), Code::CostBad.val());
    if cost.enter(1_000).is_err() || cost.exit(1_500).is_err() {
        sw.misses = sw.misses.saturating_add(1);
    }
    sw.note(cost.cost(501).err(), Code::CostBad.val());

    // 9) ResultBad：结果码为零 / 段外号 / 邻段号
    let mut res = match FuzzOp::new(6) {
        Ok(v) => v,
        Err(_) => {
            sw.misses = sw.misses.saturating_add(1);
            return sw;
        }
    };
    if res.take_ticket(32).is_err()
        || res.enter(2_000).is_err()
        || res.exit(2_010).is_err()
        || res.cost(10).is_err()
    {
        sw.misses = sw.misses.saturating_add(1);
    }
    sw.note(res.result(0).err(), Code::ResultBad.val());
    sw.note(res.result(SEG_BASE + 0x0F).err(), Code::ResultBad.val());
    sw.note(res.result(0x73381).err(), Code::ResultBad.val());
    if res.result(one).is_err() {
        sw.misses = sw.misses.saturating_add(1);
    }

    // 10) DigestBad：字段改了、摘要没跟着改，入帧必被拦
    let mut dg = FrameState::new(4);
    if let Some(mut o) = closed_op(7, 41, 3_000, 10, one) {
        o.cost_ns = o.cost_ns.saturating_add(1);
        sw.note(dg.push(o).err(), Code::DigestBad.val());
    } else {
        sw.misses = sw.misses.saturating_add(1);
    }

    // 11) RenderBad：键序缺项 / 摘要对不上 / 数字位脏
    sw.note(
        parse_row("FZRS|k1=1|k2=2|k3=3|k4=4|digest=0000000000000000", FUZZ_TAG).err(),
        Code::RenderBad.val(),
    );
    sw.note(parse_row("FZRS|k1=1", FUZZ_TAG).err(), Code::RenderBad.val());
    sw.note(
        parse_row("FZRS|k2=2|k1=1|k3=3|k4=4|digest=0000000000000000", FUZZ_TAG).err(),
        Code::RenderBad.val(),
    );

    // 12) CaseBad：序号零 / 越界 / 全表之外的编号
    sw.note(case_lookup(0).err(), Code::CaseBad.val());
    sw.note(case_lookup((CASE_COUNT + 1) as u8).err(), Code::CaseBad.val());
    sw.note(case_lookup(u8::MAX).err(), Code::CaseBad.val());

    // 13) ReplayBad：零次复跑不算复跑
    sw.note(
        fuzz_replay_checked(&p, 0).err(),
        Code::ReplayBad.val(),
    );
    sw.note(replay_cases_checked(0).err(), Code::ReplayBad.val());
    sw.note(replay_guard(false).err(), Code::ReplayBad.val());

    // 14) IdBad：邻段号 / 段基址 / 零号（撞号必须留痕）
    let mut reg = IdRegistry::new();
    sw.note(reg.register(0x73381).err(), Code::IdBad.val());
    sw.note(reg.register(SEG_BASE).err(), Code::IdBad.val());
    sw.note(reg.register(0).err(), Code::IdBad.val());

    // 哨位：域外码只准进 15 号槽，绝不冒充已知码
    let mut watcher = FrameState::new(5);
    if !sw.foreign(&mut watcher, SEG_BASE + 0x0F) {
        sw.misses = sw.misses.saturating_add(1);
    }
    if !sw.foreign(&mut watcher, 0x73381) {
        sw.misses = sw.misses.saturating_add(1);
    }
    if !watcher.verify() {
        sw.misses = sw.misses.saturating_add(1);
    }

    sw
}

// ── 自检 ─────────────────────────────────────────────────────────
/// 八项检查（前四项锚 F49077 fuzz 面，后四项锚 F49078 用例集）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::with_capacity(8);

    // 1) fuzz 零崩：×10^4 帧守恒、零静默、帧数与尝试数对得上。
    let p = FuzzParams::standard(0x7401);
    let ok;
    let mut info = String::from("万帧失败");
    match fuzz_ten_thousand(&p) {
        Ok(r) => {
            let floor = FUZZ_FRAMES.saturating_mul(p.ops);
            let mut all = r.frames == FUZZ_FRAMES
                && r.balanced()
                && r.silent() == 0
                && r.attempts >= floor
                && r.attempts <= floor.saturating_mul(2)
                && r.digest != p_core::FNV_OFF
                && r.accepted > 0
                && r.rejected > 0;
            // 帧号越界与帧数越界必须被拒
            if fuzz_frame(&p, FUZZ_FRAMES).err() != Some(Code::FrameBad.val()) {
                all = false;
            }
            if fuzz_frames(&p, 0).err() != Some(Code::FrameBad.val()) {
                all = false;
            }
            if fuzz_frames(&p, FRAME_CAP + 1).err() != Some(Code::FrameBad.val()) {
                all = false;
            }
            ok = all;
            info = format!(
                "frames={} attempts={} acc={} rej={} silent={}",
                r.frames,
                r.attempts,
                r.accepted,
                r.rejected,
                r.silent()
            );
        }
        Err(_) => ok = false,
    }
    out.push(("F49077/fuzz零崩", ok, info));

    // 2) 码族触达：14 条已知码各由真实接口触发一次、零失手、哨只记域外。
    let sweep = exercise_codes();
    let mut ok = sweep.full() && sweep.hist.known_total() >= KNOWN_CODES as u32;
    let mut info = format!(
        "touched={}/{} misses={} sentinel={} ext={}",
        sweep.hist.touched_known(),
        KNOWN_CODES,
        sweep.misses,
        sweep.hist.sentinel_total(),
        sweep.external
    );
    // 触达不等于账平：fuzz 面自身的双记账也必须自洽
    match fuzz_frames(&FuzzParams::standard(0x7403), 512) {
        Ok(r) => {
            if !r.codes_covered()
                || r.hist.known_total() != r.rejected
                || r.hist.sentinel_total() != r.external
                || !r.hist.guard_slot_zero()
            {
                ok = false;
                info.push_str(" fuzz账不平");
            }
        }
        Err(_) => {
            ok = false;
            info.push_str(" fuzz失败");
        }
    }
    out.push(("F49077/码族触达", ok, info));

    // 3) 种子与复跑：同种子一致、异种子相异、三次复跑同摘要、参数校验码齐。
    let a = FuzzParams::standard(0x7405);
    let b = FuzzParams::standard(0x7407);
    let mut ok = fuzz_replay(&a, 3);
    let mut bits = String::new();
    if !ok {
        bits.push_str("replay red; ");
    }
    match (fuzz_frames(&a, 32), fuzz_frames(&a, 32)) {
        (Ok(x), Ok(y)) => {
            if x.digest != y.digest {
                ok = false;
                bits.push_str("same-seed drift; ");
            }
        }
        _ => {
            ok = false;
            bits.push_str("same-seed err; ");
        }
    }
    match (fuzz_frames(&a, 32), fuzz_frames(&b, 32)) {
        (Ok(x), Ok(y)) => {
            if x.digest == y.digest {
                ok = false;
                bits.push_str("seed-insensitive; ");
            }
        }
        _ => {
            ok = false;
            bits.push_str("diff-seed err; ");
        }
    }
    // 参数面四类非法值
    if FuzzParams::new(0, 4, 256, 200_000, F_ALL).err() != Some(Code::SeedBad.val()) {
        ok = false;
        bits.push_str("seed0 pass; ");
    }
    if FuzzParams::new(1, 0, 256, 200_000, F_ALL).err() != Some(Code::ParamBad.val()) {
        ok = false;
        bits.push_str("ops0 pass; ");
    }
    if FuzzParams::new(1, OPS_CAP + 1, 256, 200_000, F_ALL).err() != Some(Code::ParamBad.val()) {
        ok = false;
        bits.push_str("ops-over pass; ");
    }
    if FuzzParams::new(1, 4, TICKET_CAP + 1, 200_000, F_ALL).err() != Some(Code::ParamBad.val()) {
        ok = false;
        bits.push_str("ticket-over pass; ");
    }
    if FuzzParams::new(1, 4, 256, NS_LIMIT + 1, F_ALL).err() != Some(Code::ParamBad.val()) {
        ok = false;
        bits.push_str("ns-over pass; ");
    }
    if FuzzParams::new(1, 4, 256, 200_000, 0x40).err() != Some(Code::ParamBad.val()) {
        ok = false;
        bits.push_str("fault pass; ");
    }
    if !p.verify() {
        ok = false;
        bits.push_str("standard invalid; ");
    }
    if ok && bits.is_empty() {
        bits = format!("replay=3 frames=32 faults={}", p.fault_bits());
    }
    out.push(("F49077/种子与复跑", ok, bits));

    // 4) 账行结构：帧行往返、坏行全拒、导行稳定。
    let ok;
    let mut info = String::from("帧行失败");
    match fuzz_frames(&p, 16) {
        Ok(_) => {
            let mut all = true;
            let mut rows: Vec<String> = Vec::with_capacity(16);
            let mut f = 0u32;
            while f < 16 {
                match fuzz_frame(&p, f) {
                    Ok(o) => {
                        let row = frame_row(&o);
                        if !audit_frame_row(&row) {
                            all = false;
                        }
                        let mut longer = String::from(row.as_str());
                        longer.push('x');
                        if audit_frame_row(&longer) {
                            all = false;
                        }
                        rows.push(row);
                    }
                    Err(_) => all = false,
                }
                f = f.saturating_add(1);
            }
            // 帧号越界的行必须拒
            let d = row_digest(FUZZ_TAG, FUZZ_FRAMES, 1, 1, 0);
            if audit_frame_row(&render_row(FUZZ_TAG, FUZZ_FRAMES, 1, 1, 0, d)) {
                all = false;
            }
            // 单帧操作数越界的行必须拒
            let d = row_digest(FUZZ_TAG, 0, OPS_CAP + 1, 0, 0);
            if audit_frame_row(&render_row(FUZZ_TAG, 0, OPS_CAP + 1, 0, 0, d)) {
                all = false;
            }
            // 两次导出逐字一致
            let mut again: Vec<String> = Vec::with_capacity(16);
            let mut f = 0u32;
            while f < 16 {
                if let Ok(o) = fuzz_frame(&p, f) {
                    again.push(frame_row(&o));
                }
                f = f.saturating_add(1);
            }
            if again.len() != rows.len() {
                all = false;
            }
            let mut i = 0usize;
            while i < rows.len() {
                if rows.get(i) != again.get(i) {
                    all = false;
                }
                i += 1;
            }
            ok = all && !rows.is_empty();
            info = format!("rows={} stable={}", rows.len(), all);
        }
        Err(_) => ok = false,
    }
    out.push(("F49077/账行结构", ok, info));

    // 5) 用例集二十：表形（序号/标签/种类）+ 全表绿。
    let report = run_cases();
    let mut ok = report.all_green()
        && report.green as usize == CASE_COUNT
        && report.red == 0
        && report.outcomes.len() == CASE_COUNT
        && CASE_TABLE.len() == CASE_COUNT;
    let mut bits = format!(
        "cases={} green={} red={}",
        CASE_TABLE.len(),
        report.green,
        report.red
    );
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        match CASE_TABLE.get(i) {
            Some(d) => {
                if d.index as usize != i + 1 || d.label.len() != 3 {
                    ok = false;
                    bits.push_str(" 表形歪");
                }
                let mut j = i + 1;
                while j < CASE_TABLE.len() {
                    if let Some(e) = CASE_TABLE.get(j) {
                        if e.label == d.label || e.kind.index() == d.kind.index() {
                            ok = false;
                            bits.push_str(" 重号");
                        }
                    }
                    j += 1;
                }
            }
            None => ok = false,
        }
        i += 1;
    }
    out.push(("F49078/用例集二十", ok, bits));

    // 6) 用例复跑：三次全表逐字一致、逐行可审计、链摘要可重算。
    let mut ok = replay_cases(3);
    let mut bits = String::new();
    if !ok {
        bits.push_str("replay red; ");
    }
    if !report.rows_ok() {
        ok = false;
        bits.push_str("rows red; ");
    }
    if report.digest != report.recompute() {
        ok = false;
        bits.push_str("digest red; ");
    }
    // 逐例摘要必须与其账行一致（入库可复跑的前提）
    let mut i = 0usize;
    while i < report.outcomes.len() {
        match (report.outcomes.get(i), report.rows.get(i)) {
            (Some(o), Some(r)) => {
                if !audit_case_row(r) {
                    ok = false;
                    bits.push_str("row audit; ");
                }
                if o.digest
                    != row_digest(
                        CASE_TAG,
                        o.index as u32,
                        o.kind.index() as u32,
                        if o.passed { 1 } else { 0 },
                        o.payload,
                    )
                {
                    ok = false;
                    bits.push_str("outcome digest; ");
                }
            }
            _ => ok = false,
        }
        i += 1;
    }
    if ok && bits.is_empty() {
        bits = format!("replay=3 rows={} digest={:016x}", report.rows.len(), report.digest);
    }
    out.push(("F49078/用例复跑", ok, bits));

    // 7) 编号防重：只收本段号、撞号留痕、邻段号拒收。
    let mut reg = IdRegistry::new();
    let mut ok = true;
    let mut bits = String::new();
    let mut i = 0usize;
    while i < KNOWN_CODES {
        if reg.register(SEG_BASE + 1 + i as u32).is_err() {
            ok = false;
            bits.push_str("code reg; ");
        }
        i += 1;
    }
    let mut k = 1u8;
    while k <= CASE_COUNT as u8 {
        if reg.register(case_id(k)).is_err() {
            ok = false;
            bits.push_str("case reg; ");
        }
        k = k.saturating_add(1);
    }
    let expect = KNOWN_CODES + CASE_COUNT;
    if reg.len() != expect {
        ok = false;
        bits.push_str("len mismatch; ");
    }
    // 四类撞号：重号 / 邻段号 / 段基址 / 零
    if reg.register(SEG_BASE + 0x05).err() != Some(Code::IdBad.val()) {
        ok = false;
        bits.push_str("dup pass; ");
    }
    if reg.register(0x73381).err() != Some(Code::IdBad.val()) {
        ok = false;
        bits.push_str("neighbor pass; ");
    }
    if reg.register(SEG_BASE).err() != Some(Code::IdBad.val()) {
        ok = false;
        bits.push_str("base pass; ");
    }
    if reg.register(0).err() != Some(Code::IdBad.val()) {
        ok = false;
        bits.push_str("zero pass; ");
    }
    if reg.collisions() != 4 {
        ok = false;
        bits.push_str("collision count; ");
    }
    if !reg.verify() {
        ok = false;
        bits.push_str("reg verify; ");
    }
    let rows = reg.rows();
    if rows.len() != expect {
        ok = false;
        bits.push_str("row count; ");
    }
    let mut j = 0usize;
    while j < rows.len() {
        match rows.get(j) {
            Some(r) => {
                if !audit_id_row(r) {
                    ok = false;
                    bits.push_str("id row audit; ");
                }
            }
            None => ok = false,
        }
        j += 1;
    }
    if ok && bits.is_empty() {
        bits = format!("ids={} collisions={}", reg.len(), reg.collisions());
    }
    out.push(("F49078/编号防重", ok, bits));

    // 8) 批收口：段表互斥 + 收口绿 + 摘要可复现 + 留痕行可审计。
    let mut ok = segments_disjoint();
    let mut bits = String::new();
    if !ok {
        bits.push_str("segments overlap; ");
    }
    if own_slot() == usize::MAX {
        ok = false;
        bits.push_str("own slot missing; ");
    }
    match (seal_standard(0x7409), seal_standard(0x7409)) {
        (Ok((r1, s1)), Ok((r2, s2))) => {
            if !s1.green || !s2.green {
                ok = false;
                bits.push_str("seal red; ");
            }
            if s1 != s2 || r1.digest() != r2.digest() {
                ok = false;
                bits.push_str("seal drift; ");
            }
            if s1.registered != (KNOWN_CODES + CASE_COUNT) as u32 || s1.collisions != 0 {
                ok = false;
                bits.push_str("seal count; ");
            }
            if s1.rows != s1.registered {
                ok = false;
                bits.push_str("seal rows; ");
            }
            if !r1.verify() {
                ok = false;
                bits.push_str("seal verify; ");
            }
            let rows = r1.rows();
            let mut j = 0usize;
            while j < rows.len() {
                match rows.get(j) {
                    Some(r) => {
                        if !audit_id_row(r) {
                            ok = false;
                            bits.push_str("seal row audit; ");
                        }
                    }
                    None => ok = false,
                }
                j += 1;
            }
            // 用例号必须全部落在本段且不与诊断码重叠
            let mut k = 1u8;
            while k <= CASE_COUNT as u8 {
                if classify(case_id(k)) != Some(own_slot()) || is_known(case_id(k)) {
                    ok = false;
                    bits.push_str("case id outside; ");
                }
                k = k.saturating_add(1);
            }
            if ok && bits.is_empty() {
                bits = format!(
                    "sealed={} segs={} digest={:016x}",
                    s1.registered,
                    SEG_LEDGER.len(),
                    s1.digest
                );
            }
        }
        _ => {
            ok = false;
            bits.push_str("seal err; ");
        }
    }
    out.push(("F49078/批收口", ok, bits));

    out
}

/// 域自检入口。
pub fn run_unx49077_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX-F49077");
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

    fn op(op_id: u32, ticket: u32, enter: u32, cost: u32, code: u32) -> FuzzOp {
        let mut o = FuzzOp::new(op_id).expect("op");
        o.take_ticket(ticket).expect("ticket");
        o.enter(enter).expect("enter");
        o.exit(enter + cost).expect("exit");
        o.cost(cost).expect("cost");
        o.result(code).expect("result");
        o
    }

    fn frame(n: u32) -> FrameState {
        let mut st = FrameState::new(0);
        let mut i = 1u32;
        while i <= n {
            let o = op(i, i, 100 + i, 10, SEG_BASE + 1);
            st.push(o).expect("push");
            i = i.saturating_add(1);
        }
        st
    }

    // ── 段位与码族 ───────────────────────────────────────────────
    #[test]
    fn seg_base_pinned() {
        assert_eq!(SEG_BASE, 0x73480);
        assert_eq!(own_slot(), 6);
    }

    #[test]
    fn code_window_is_dense_14() {
        assert_eq!(window_lo(), SEG_BASE + 1);
        assert_eq!(window_hi(), SEG_BASE + 14);
        assert_eq!(Code::ALL.len(), KNOWN_CODES);
        assert_eq!(KNOWN_CODES, 14);
    }

    #[test]
    fn code_values_are_base_plus_index() {
        let mut i = 0usize;
        while i < Code::ALL.len() {
            let c = Code::ALL.get(i).expect("code");
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            i += 1;
        }
    }

    #[test]
    fn code_values_distinct_and_described() {
        let mut vals: Vec<u32> = Vec::new();
        let mut texts: Vec<&'static str> = Vec::new();
        let mut i = 0usize;
        while i < Code::ALL.len() {
            let c = Code::ALL.get(i).expect("code");
            assert!(!vals.contains(&c.val()));
            let d = detail(c.val());
            assert_ne!(d, UNKNOWN);
            assert!(!d.is_empty());
            assert!(!texts.contains(&d));
            vals.push(c.val());
            texts.push(d);
            i += 1;
        }
    }

    #[test]
    fn detail_covers_window_and_unknown() {
        let mut i = 1u32;
        while i <= 14 {
            assert_ne!(detail(SEG_BASE + i), UNKNOWN);
            i += 1;
        }
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(u32::MAX), UNKNOWN);
    }

    #[test]
    fn segment_guard_green() {
        assert!(segment_guard());
        assert!(is_known(SEG_BASE + 1));
        assert!(is_known(SEG_BASE + 14));
        assert!(!is_known(SEG_BASE));
        assert!(!is_known(SEG_BASE + 15));
        assert!(!is_known(0));
    }

    #[test]
    fn family_slot_maps_in_and_out() {
        let mut i = 1u32;
        while i <= 14 {
            assert_eq!(family_slot(SEG_BASE + i), i as u8);
            i += 1;
        }
        assert_eq!(family_slot(SEG_BASE), SENTINEL_SLOT);
        assert_eq!(family_slot(SEG_BASE + 0x0F), SENTINEL_SLOT);
        assert_eq!(family_slot(0x73381), SENTINEL_SLOT);
        assert_eq!(family_slot(0), SENTINEL_SLOT);
    }

    #[test]
    fn family_values_sorted_and_unique() {
        let v = family_values();
        assert_eq!(v.len(), KNOWN_CODES);
        let mut i = 0usize;
        while i < v.len() {
            assert_eq!(v[i], SEG_BASE + 1 + i as u32);
            if i > 0 {
                assert!(v[i] > v[i - 1]);
            }
            i += 1;
        }
    }

    #[test]
    fn segments_pairwise_disjoint() {
        assert!(segments_disjoint());
        assert_eq!(SEG_LEDGER.len(), 7);
        assert_eq!(classify(SEG_BASE), Some(own_slot()));
        assert_eq!(classify(SEG_BASE + 0x3F), Some(own_slot()));
        assert_eq!(classify(0x73381), Some(4));
        assert_eq!(classify(SEG_BASE + SEG_SPAN), None);
    }

    #[test]
    fn case_ids_live_in_own_segment_only() {
        let mut k = 1u8;
        while k <= CASE_COUNT as u8 {
            let id = case_id(k);
            assert_eq!(classify(id), Some(own_slot()));
            assert!(id >= SEG_BASE);
            assert!(id < SEG_BASE + SEG_SPAN);
            assert!(!is_known(id));
            k = k.saturating_add(1);
        }
    }

    // ── 码账 ─────────────────────────────────────────────────────
    #[test]
    fn hist_bump_and_totals() {
        let mut h = CodeHist::new();
        assert_eq!(h.total(), 0);
        assert_eq!(h.bump(SEG_BASE + 3), 3);
        assert_eq!(h.slot_count(3), 1);
        assert_eq!(h.total(), 1);
        assert_eq!(h.known_total(), 1);
        assert_eq!(h.bump(0x73381), SENTINEL_SLOT);
        assert_eq!(h.sentinel_total(), 1);
        assert_eq!(h.total(), 2);
        assert_eq!(h.known_total(), 1);
    }

    #[test]
    fn hist_slot_zero_is_structurally_empty() {
        let mut h = CodeHist::new();
        assert!(h.guard_slot_zero());
        // 0 既不是已知码也不是域外码，它只能落进哨槽；0 号槽永远进不去
        h.bump(0);
        assert_eq!(h.slot_count(0), 0);
        assert_eq!(h.sentinel_total(), 1);
        assert_eq!(h.known_total(), 0);
        assert_eq!(h.touched_known(), 0);
        assert!(h.guard_slot_zero());
    }

    #[test]
    fn hist_touched_counts_distinct_codes() {
        let mut h = CodeHist::new();
        assert_eq!(h.touched_known(), 0);
        h.bump(SEG_BASE + 1);
        h.bump(SEG_BASE + 1);
        assert_eq!(h.touched_known(), 1);
        assert_eq!(h.known_total(), 2);
        let mut i = 2u32;
        while i <= 14 {
            h.bump(SEG_BASE + i);
            i += 1;
        }
        assert_eq!(h.touched_known(), 14);
    }

    #[test]
    fn hist_merge_sums_slots() {
        let mut a = CodeHist::new();
        let mut b = CodeHist::new();
        a.bump(SEG_BASE + 2);
        b.bump(SEG_BASE + 2);
        b.bump(SEG_BASE + 9);
        b.bump(0x73381);
        a.merge(&b);
        assert_eq!(a.slot_count(2), 2);
        assert_eq!(a.slot_count(9), 1);
        assert_eq!(a.sentinel_total(), 1);
        assert_eq!(a.total(), 4);
        assert!(a.guard_slot_zero());
    }

    // ── 参数面 ───────────────────────────────────────────────────
    #[test]
    fn params_standard_is_valid() {
        let p = FuzzParams::standard(0x7501);
        assert!(p.verify());
        assert_eq!(p.ops, 4);
        assert_eq!(p.fault_bits(), FAULT_BITS as u32);
    }

    #[test]
    fn params_reject_zero_seed() {
        assert_eq!(
            FuzzParams::new(0, 4, 256, 200_000, F_ALL).err(),
            Some(Code::SeedBad.val())
        );
    }

    #[test]
    fn params_reject_ops_bounds() {
        assert_eq!(
            FuzzParams::new(1, 0, 256, 200_000, F_ALL).err(),
            Some(Code::ParamBad.val())
        );
        assert_eq!(
            FuzzParams::new(1, OPS_CAP + 1, 256, 200_000, F_ALL).err(),
            Some(Code::ParamBad.val())
        );
        assert!(FuzzParams::new(1, OPS_CAP, 256, 200_000, F_ALL).is_ok());
    }

    #[test]
    fn params_reject_span_bounds() {
        assert_eq!(
            FuzzParams::new(1, 4, 0, 200_000, F_ALL).err(),
            Some(Code::ParamBad.val())
        );
        assert_eq!(
            FuzzParams::new(1, 4, TICKET_CAP + 1, 200_000, F_ALL).err(),
            Some(Code::ParamBad.val())
        );
        assert_eq!(
            FuzzParams::new(1, 4, 256, 0, F_ALL).err(),
            Some(Code::ParamBad.val())
        );
        assert_eq!(
            FuzzParams::new(1, 4, 256, NS_LIMIT + 1, F_ALL).err(),
            Some(Code::ParamBad.val())
        );
    }

    #[test]
    fn params_reject_dirty_fault_mask() {
        assert_eq!(
            FuzzParams::new(1, 4, 256, 200_000, F_ALL | 0x40).err(),
            Some(Code::ParamBad.val())
        );
        assert_eq!(
            FuzzParams::new(1, 4, 256, 200_000, 0).err(),
            None
        );
    }

    #[test]
    fn params_fault_bits_count() {
        let p = FuzzParams::new(1, 4, 256, 200_000, F_TICKET_ZERO | F_COST).expect("p");
        assert_eq!(p.fault_bits(), 2);
        let q = FuzzParams::new(1, 4, 256, 200_000, 0).expect("p");
        assert_eq!(q.fault_bits(), 0);
    }

    // ── 单次观测状态机 ───────────────────────────────────────────
    #[test]
    fn op_rejects_zero_id() {
        assert_eq!(FuzzOp::new(0).err(), Some(Code::OpBad.val()));
        assert!(FuzzOp::new(1).is_ok());
    }

    #[test]
    fn op_full_cycle_closes() {
        let o = op(1, 7, 1_000, 250, SEG_BASE + 2);
        assert!(o.closed());
        assert!(o.four_points());
        assert_eq!(o.points(), FOUR_POINT_MASK);
        assert_eq!(o.cost_ns, 250);
        assert!(o.verify());
        assert_ne!(o.digest, p_core::FNV_OFF);
    }

    #[test]
    fn op_order_guards_every_jump() {
        let mut o = FuzzOp::new(1).expect("op");
        assert_eq!(o.enter(10).err(), Some(Code::OrderBad.val()));
        assert_eq!(o.exit(20).err(), Some(Code::OrderBad.val()));
        assert_eq!(o.cost(5).err(), Some(Code::OrderBad.val()));
        assert_eq!(o.result(SEG_BASE + 1).err(), Some(Code::OrderBad.val()));
        assert!(o.take_ticket(3).is_ok());
        assert_eq!(o.take_ticket(4).err(), Some(Code::OrderBad.val()));
        assert_eq!(o.cost(5).err(), Some(Code::OrderBad.val()));
        assert_eq!(o.result(SEG_BASE + 1).err(), Some(Code::OrderBad.val()));
    }

    #[test]
    fn op_ticket_bounds() {
        let mut o = FuzzOp::new(1).expect("op");
        assert_eq!(o.take_ticket(0).err(), Some(Code::TicketBad.val()));
        assert_eq!(
            o.take_ticket(TICKET_CAP + 1).err(),
            Some(Code::TicketBad.val())
        );
        assert!(o.take_ticket(TICKET_CAP).is_ok());
    }

    #[test]
    fn op_cost_must_equal_span() {
        let mut o = FuzzOp::new(2).expect("op");
        o.take_ticket(1).expect("t");
        o.enter(1_000).expect("e");
        o.exit(1_500).expect("x");
        assert_eq!(o.cost(501).err(), Some(Code::CostBad.val()));
        assert_eq!(o.cost(499).err(), Some(Code::CostBad.val()));
        assert!(o.cost(500).is_ok());
    }

    #[test]
    fn op_result_must_be_in_window() {
        let mut o = FuzzOp::new(3).expect("op");
        o.take_ticket(1).expect("t");
        o.enter(10).expect("e");
        o.exit(20).expect("x");
        o.cost(10).expect("c");
        assert_eq!(o.result(0).err(), Some(Code::ResultBad.val()));
        assert_eq!(o.result(SEG_BASE).err(), Some(Code::ResultBad.val()));
        assert_eq!(
            o.result(SEG_BASE + 0x0F).err(),
            Some(Code::ResultBad.val())
        );
        assert_eq!(o.result(0x73381).err(), Some(Code::ResultBad.val()));
        assert!(o.result(SEG_BASE + 14).is_ok());
    }

    #[test]
    fn op_digest_tracks_every_field() {
        let o = op(1, 2, 10, 5, SEG_BASE + 1);
        let d0 = o.digest;
        let mut t = o;
        t.ticket = 3;
        t.digest = op_digest(t.op, t.ticket, t.enter_ns, t.exit_ns, t.cost_ns, t.code, t.stage);
        assert_ne!(t.digest, d0);
        assert!(t.verify());
    }

    #[test]
    fn stage_names_guard_five_stages() {
        let mut i = 0u8;
        while i < STAGES as u8 {
            assert!(stage_name(i).is_ok());
            assert!(!stage_name(i).expect("n").is_empty());
            i = i.saturating_add(1);
        }
        assert_eq!(stage_name(STAGE_RESULT), Ok("结果"));
        assert_eq!(
            stage_name(STAGES as u8).err(),
            Some(Code::StageBad.val())
        );
        assert_eq!(
            stage_name(STAGE_CLOSED).err(),
            Some(Code::StageBad.val())
        );
        assert_eq!(stage_name(u8::MAX).err(), Some(Code::StageBad.val()));
    }

    // ── 帧账 ─────────────────────────────────────────────────────
    #[test]
    fn frame_accepts_closed_ops() {
        let st = frame(3);
        assert_eq!(st.len(), 3);
        assert_eq!(st.frame(), 0);
        assert_eq!(st.rejected(), 0);
        assert!(st.verify());
        assert_eq!(st.digest(), st.recompute());
        assert_ne!(st.digest(), p_core::FNV_OFF);
        assert_eq!(st.max_cost(), 10);
        assert_eq!(st.mean_cost(), 10);
    }

    #[test]
    fn frame_rejects_unclosed_op() {
        let mut st = FrameState::new(1);
        let o = FuzzOp::new(1).expect("op");
        assert_eq!(st.push(o).err(), Some(Code::OrderBad.val()));
        assert_eq!(st.rejected(), 1);
        assert_eq!(st.hist().known_total(), 1);
        assert_eq!(st.len(), 0);
        assert!(st.verify());
    }

    #[test]
    fn frame_rejects_tampered_digest() {
        let mut st = FrameState::new(2);
        let mut o = op(1, 1, 10, 5, SEG_BASE + 1);
        o.cost_ns = 6;
        assert_eq!(st.push(o).err(), Some(Code::DigestBad.val()));
        assert_eq!(st.rejected(), 1);
        assert_eq!(st.hist().slot_count(10), 1);
    }

    #[test]
    fn frame_rejects_unknown_result() {
        let mut st = FrameState::new(3);
        let mut o = op(1, 1, 10, 5, SEG_BASE + 1);
        o.code = SEG_BASE + 0x0F;
        o.digest = op_digest(o.op, o.ticket, o.enter_ns, o.exit_ns, o.cost_ns, o.code, o.stage);
        assert_eq!(st.push(o).err(), Some(Code::ResultBad.val()));
        assert_eq!(st.rejected(), 1);
        assert_eq!(st.hist().sentinel_total(), 0);
    }

    #[test]
    fn frame_rejects_duplicate_op() {
        let mut st = frame(1);
        let dup = op(1, 2, 200, 10, SEG_BASE + 1);
        assert_eq!(st.push(dup).err(), Some(Code::OpBad.val()));
        assert_eq!(st.len(), 1);
        assert_eq!(st.rejected(), 1);
    }

    #[test]
    fn frame_rejects_duplicate_ticket() {
        let mut st = frame(1);
        let dup = op(2, 1, 200, 10, SEG_BASE + 1);
        assert_eq!(st.push(dup).err(), Some(Code::TicketBad.val()));
        assert_eq!(st.len(), 1);
        assert_eq!(st.rejected(), 1);
    }

    #[test]
    fn frame_capacity_rejects_ninth() {
        let mut st = FrameState::new(9);
        let mut i = 1u32;
        while i <= OPS_CAP {
            st.push(op(i, i, 100, 5, SEG_BASE + 1)).expect("push");
            i = i.saturating_add(1);
        }
        let over = op(OPS_CAP + 1, OPS_CAP + 1, 100, 5, SEG_BASE + 1);
        assert_eq!(st.push(over).err(), Some(Code::ParamBad.val()));
        assert_eq!(st.len(), OPS_CAP as usize);
    }

    #[test]
    fn frame_verify_catches_silent_reject() {
        let mut st = frame(2);
        assert!(st.verify());
        // 手工把拒收数抬一格而不记账：双记账缺口必须被抓到
        st.rejected = st.rejected.saturating_add(1);
        assert!(!st.verify());
    }

    #[test]
    fn frame_verify_catches_external_mismatch() {
        let mut st = frame(1);
        assert!(st.verify());
        st.note_external(0x73381);
        assert_eq!(st.external(), 1);
        assert_eq!(st.hist().sentinel_total(), 1);
        assert!(st.verify());
        st.external = st.external.saturating_add(1);
        assert!(!st.verify());
    }

    #[test]
    fn frame_get_and_len_are_bounded() {
        let st = frame(2);
        assert!(st.get(0).is_some());
        assert!(st.get(1).is_some());
        assert!(st.get(2).is_none());
        assert!(!st.is_empty());
        assert!(FrameState::new(0).is_empty());
        assert_eq!(FrameState::new(0).mean_cost(), 0);
    }

    // ── fuzz 面 ──────────────────────────────────────────────────
    #[test]
    fn fuzz_frame_is_balanced() {
        let p = FuzzParams::standard(0x7501);
        let out = fuzz_frame(&p, 3).expect("frame");
        assert_eq!(out.frame, 3);
        assert!(out.balanced());
        assert_eq!(out.silent(), 0);
        assert_eq!(out.accepted + out.rejected, out.attempts);
        assert!(out.attempts >= p.ops);
        assert!(out.hist.guard_slot_zero());
        assert!(out.hist.guard_slot_zero());
        assert_eq!(out.hist.total(), out.rejected + out.external);
        assert_eq!(out.digest, out.digest);
    }

    #[test]
    fn fuzz_frame_rejects_bad_frame_index() {
        let p = FuzzParams::standard(0x7503);
        assert_eq!(
            fuzz_frame(&p, FUZZ_FRAMES).err(),
            Some(Code::FrameBad.val())
        );
        assert!(fuzz_frame(&p, FUZZ_FRAMES - 1).is_ok());
    }

    #[test]
    fn fuzz_frames_bounds() {
        let p = FuzzParams::standard(0x7505);
        assert_eq!(fuzz_frames(&p, 0).err(), Some(Code::FrameBad.val()));
        assert_eq!(
            fuzz_frames(&p, FRAME_CAP + 1).err(),
            Some(Code::FrameBad.val())
        );
        assert!(fuzz_frames(&p, 1).is_ok());
    }

    #[test]
    fn fuzz_is_deterministic() {
        let p = FuzzParams::standard(0x7507);
        let a = fuzz_frames(&p, 48).expect("a");
        let b = fuzz_frames(&p, 48).expect("b");
        assert_eq!(a, b);
        assert_eq!(a.digest, b.digest);
        assert!(a.balanced());
    }

    #[test]
    fn fuzz_is_seed_sensitive() {
        // 两个种子至少差 2 且低位的 |1 折叠不会把它们并到一起
        let a = FuzzParams::standard(0x7501);
        let b = FuzzParams::standard(0x7503);
        let x = fuzz_frames(&a, 32).expect("x");
        let y = fuzz_frames(&b, 32).expect("y");
        assert_ne!(x.digest, y.digest);
    }

    #[test]
    fn fuzz_report_touches_fault_codes() {
        let p = FuzzParams::standard(0x7509);
        let r = fuzz_frames(&p, 512).expect("r");
        assert!(r.frames == 512);
        assert!(r.codes_covered(), "touched={}", r.hist.touched_known());
        assert!(r.hist.touched_known() >= FAULT_MIN);
        assert_eq!(r.hist.known_total(), r.rejected);
        assert_eq!(r.hist.sentinel_total(), r.external);
        assert!(r.external > 0);
        assert!(r.accepted > 0);
        assert!(r.rejected > 0);
        assert!(r.max_cost > 0);
    }

    #[test]
    fn fuzz_ten_thousand_no_crash() {
        let p = FuzzParams::standard(0x750B);
        let r = fuzz_ten_thousand(&p).expect("10^4");
        assert_eq!(r.frames, FUZZ_FRAMES);
        assert_eq!(r.silent(), 0);
        assert!(r.balanced());
        assert!(r.attempts >= FUZZ_FRAMES * p.ops);
        assert_ne!(r.digest, p_core::FNV_OFF);
    }

    #[test]
    fn fuzz_replay_consistency() {
        let p = FuzzParams::standard(0x750D);
        assert!(fuzz_replay(&p, 3));
        assert_eq!(fuzz_replay_checked(&p, 3).is_ok(), true);
        let d1 = fuzz_replay_checked(&p, 1).expect("1");
        let d3 = fuzz_replay_checked(&p, 3).expect("3");
        assert_eq!(d1, d3);
    }

    #[test]
    fn fuzz_replay_zero_times_is_replay_bad() {
        let p = FuzzParams::standard(0x750F);
        assert!(!fuzz_replay(&p, 0));
        assert_eq!(
            fuzz_replay_checked(&p, 0).err(),
            Some(Code::ReplayBad.val())
        );
        assert_eq!(replay_guard(false).err(), Some(Code::ReplayBad.val()));
        assert!(replay_guard(true).is_ok());
    }

    // ── 账行 ─────────────────────────────────────────────────────
    #[test]
    fn render_row_shape() {
        let d = row_digest(FUZZ_TAG, 1, 2, 3, 4);
        let row = render_row(FUZZ_TAG, 1, 2, 3, 4, d);
        assert!(row.starts_with("FZRS|k1=1|k2=2|k3=3|k4=4|digest="));
        assert_eq!(row.len(), "FZRS|k1=1|k2=2|k3=3|k4=4|digest=".len() + 16);
        let tail = row.split("digest=").last().expect("tail");
        assert_eq!(tail.len(), 16);
        assert!(tail.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    }

    #[test]
    fn parse_row_roundtrip_and_reject() {
        let d = row_digest(CASE_TAG, 9, 3, 1, 7);
        let row = render_row(CASE_TAG, 9, 3, 1, 7, d);
        let (v, g) = parse_row(&row, CASE_TAG).expect("parse");
        assert_eq!(v, [9, 3, 1, 7]);
        assert_eq!(g, d);
        assert_eq!(parse_row(&row, FUZZ_TAG).err(), Some(Code::RenderBad.val()));
        assert_eq!(
            parse_row("CASE|k1=9|k2=3|k3=1|k4=7", CASE_TAG).err(),
            Some(Code::RenderBad.val())
        );
        assert_eq!(
            parse_row("CASE|k2=3|k1=9|k3=1|k4=7|digest=0000000000000000", CASE_TAG).err(),
            Some(Code::RenderBad.val())
        );
    }

    #[test]
    fn parse_u32_is_strict() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("12a"), None);
        assert_eq!(parse_u32("-1"), None);
        assert_eq!(parse_u32("00000000001"), None);
    }

    #[test]
    fn parse_hex16_is_strict() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(parse_hex16("000000000000000"), None);
        assert_eq!(parse_hex16("00000000000000000"), None);
        assert_eq!(parse_hex16("000000000000000g"), None);
    }

    #[test]
    fn frame_row_roundtrip_and_bounds() {
        let p = FuzzParams::standard(0x7511);
        let out = fuzz_frame(&p, 5).expect("frame");
        let row = frame_row(&out);
        assert!(audit_frame_row(&row));
        let mut longer = String::from(row.as_str());
        longer.push('x');
        assert!(!audit_frame_row(&longer));
        let far = row_digest(FUZZ_TAG, FUZZ_FRAMES, 1, 1, 0);
        assert!(!audit_frame_row(&render_row(FUZZ_TAG, FUZZ_FRAMES, 1, 1, 0, far)));
        let wide = row_digest(FUZZ_TAG, 0, OPS_CAP + 1, 0, 0);
        assert!(!audit_frame_row(&render_row(FUZZ_TAG, 0, OPS_CAP + 1, 0, 0, wide)));
    }

    #[test]
    fn audit_render_matches_parse_row() {
        let d = row_digest(ID_TAG, 1, 6, 1, 0);
        let row = render_row(ID_TAG, 1, 6, 1, 0, d);
        assert!(audit_render(&row, ID_TAG));
        assert_eq!(parse_row(&row, ID_TAG).is_ok(), audit_render(&row, ID_TAG));
        assert_eq!(audit_render("", ID_TAG), parse_row("", ID_TAG).is_ok());
    }

    // ── 用例库 ───────────────────────────────────────────────────
    #[test]
    fn case_table_shape() {
        assert_eq!(CASE_TABLE.len(), CASE_COUNT);
        let mut i = 0usize;
        while i < CASE_TABLE.len() {
            let d = CASE_TABLE.get(i).expect("case");
            assert_eq!(d.index as usize, i + 1);
            assert_eq!(d.label.len(), 3);
            assert!(d.label.starts_with('#'));
            assert_ne!(d.seed, 0);
            i += 1;
        }
    }

    #[test]
    fn case_kind_bijection() {
        let mut i = 0u8;
        while i < CASE_COUNT as u8 {
            let k = CaseKind::from_index(i).expect("kind");
            assert_eq!(k.index(), i);
            assert!(!k.name().is_empty());
            i = i.saturating_add(1);
        }
        assert!(CaseKind::from_index(CASE_COUNT as u8).is_none());
    }

    #[test]
    fn case_lookup_bounds() {
        let mut k = 1u8;
        while k <= CASE_COUNT as u8 {
            let d = case_lookup(k).expect("case");
            assert_eq!(d.index, k);
            k = k.saturating_add(1);
        }
        assert_eq!(case_lookup(0).err(), Some(Code::CaseBad.val()));
        assert_eq!(
            case_lookup((CASE_COUNT + 1) as u8).err(),
            Some(Code::CaseBad.val())
        );
        assert_eq!(case_lookup(u8::MAX).err(), Some(Code::CaseBad.val()));
    }

    #[test]
    fn case_seeds_are_pairwise_far_enough() {
        let mut i = 0usize;
        while i < CASE_TABLE.len() {
            let d = CASE_TABLE.get(i).expect("case");
            let mut j = i + 1;
            while j < CASE_TABLE.len() {
                let e = CASE_TABLE.get(j).expect("case");
                let diff = if e.seed > d.seed {
                    e.seed - d.seed
                } else {
                    d.seed - e.seed
                };
                assert!(diff >= 2, "{} vs {} 差 {}", d.label, e.label, diff);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn all_cases_green() {
        let r = run_cases();
        assert!(r.all_green(), "green={} red={}", r.green, r.red);
        assert_eq!(r.green as usize, CASE_COUNT);
        assert_eq!(r.red, 0);
        assert_eq!(r.outcomes.len(), CASE_COUNT);
        assert_eq!(r.rows.len(), CASE_COUNT);
        assert!(r.rows_ok());
    }

    #[test]
    fn case_report_digest_recomputes() {
        let r = run_cases();
        assert_eq!(r.digest, r.recompute());
        assert_ne!(r.digest, p_core::FNV_OFF);
        let mut i = 0usize;
        while i < r.outcomes.len() {
            let o = r.outcomes.get(i).expect("outcome");
            assert!(o.passed, "{} 红", o.label);
            assert!(audit_case_row(&r.rows.get(i).expect("row")));
            assert_eq!(
                o.digest,
                row_digest(
                    CASE_TAG,
                    o.index as u32,
                    o.kind.index() as u32,
                    if o.passed { 1 } else { 0 },
                    o.payload
                )
            );
            i += 1;
        }
    }

    #[test]
    fn audit_case_row_rejects_out_of_range() {
        let good = row_digest(CASE_TAG, 1, 0, 1, 3);
        assert!(audit_case_row(&render_row(CASE_TAG, 1, 0, 1, 3, good)));
        let bad_index = row_digest(CASE_TAG, 0, 0, 1, 3);
        assert!(!audit_case_row(&render_row(CASE_TAG, 0, 0, 1, 3, bad_index)));
        let bad_kind = row_digest(CASE_TAG, 1, CASE_COUNT as u32, 1, 3);
        assert!(!audit_case_row(&render_row(
            CASE_TAG,
            1,
            CASE_COUNT as u32,
            1,
            3,
            bad_kind
        )));
        let bad_flag = row_digest(CASE_TAG, 1, 0, 2, 3);
        assert!(!audit_case_row(&render_row(CASE_TAG, 1, 0, 2, 3, bad_flag)));
    }

    #[test]
    fn case_replay_is_stable() {
        assert!(replay_cases(3));
        assert!(!replay_cases(0));
        assert_eq!(replay_cases_checked(0).err(), Some(Code::ReplayBad.val()));
        let d1 = replay_cases_checked(1).expect("1");
        let d3 = replay_cases_checked(3).expect("3");
        assert_eq!(d1, d3);
    }

    #[test]
    fn run_case_matches_table() {
        let mut i = 0usize;
        while i < CASE_TABLE.len() {
            let d = CASE_TABLE.get(i).expect("case");
            let o = run_case(d);
            assert_eq!(o.index, d.index);
            assert_eq!(o.label, d.label);
            assert_eq!(o.kind, d.kind);
            assert!(o.passed, "{} 红", d.label);
            i += 1;
        }
    }

    // ── 编号登记与收口 ───────────────────────────────────────────
    #[test]
    fn registry_accepts_own_segment() {
        let mut reg = IdRegistry::new();
        assert!(reg.register(SEG_BASE + 1).is_ok());
        assert!(reg.register(case_id(1)).is_ok());
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.collisions(), 0);
        assert!(reg.verify());
        assert_eq!(reg.digest(), reg.recompute());
        assert_eq!(reg.get(0), Some(SEG_BASE + 1));
        assert_eq!(reg.get(2), None);
    }

    #[test]
    fn registry_rejects_foreign_and_duplicates() {
        let mut reg = IdRegistry::new();
        reg.register(SEG_BASE + 1).expect("first");
        assert_eq!(reg.register(SEG_BASE + 1).err(), Some(Code::IdBad.val()));
        assert_eq!(reg.register(0x73381).err(), Some(Code::IdBad.val()));
        assert_eq!(reg.register(SEG_BASE).err(), Some(Code::IdBad.val()));
        assert_eq!(reg.register(0).err(), Some(Code::IdBad.val()));
        assert_eq!(reg.collisions(), 4);
        assert_eq!(reg.len(), 1);
        assert!(reg.verify());
    }

    #[test]
    fn registry_rows_are_auditable() {
        let mut reg = IdRegistry::new();
        reg.register(SEG_BASE + 3).expect("code");
        reg.register(case_id(4)).expect("case");
        let rows = reg.rows();
        assert_eq!(rows.len(), 2);
        let mut i = 0usize;
        while i < rows.len() {
            assert!(audit_id_row(rows.get(i).expect("row")));
            i += 1;
        }
        let mut longer = String::from(rows.get(0).expect("row").as_str());
        longer.push('x');
        assert!(!audit_id_row(&longer));
    }

    #[test]
    fn registry_capacity_is_capped() {
        let mut reg = IdRegistry::new();
        let mut i = 1u32;
        while i <= ID_CAP as u32 {
            assert!(reg.register(SEG_BASE + i).is_ok());
            i = i.saturating_add(1);
        }
        assert_eq!(reg.len(), ID_CAP);
        assert_eq!(
            reg.register(SEG_BASE + ID_CAP as u32 + 1).err(),
            Some(Code::IdBad.val())
        );
        assert_eq!(reg.collisions(), 1);
    }

    #[test]
    fn seal_standard_is_green_and_stable() {
        let (r1, s1) = seal_standard(0x7513).expect("seal");
        let (r2, s2) = seal_standard(0x7513).expect("seal");
        assert!(s1.green);
        assert_eq!(s1, s2);
        assert_eq!(r1.digest(), r2.digest());
        assert_eq!(s1.registered, (KNOWN_CODES + CASE_COUNT) as u32);
        assert_eq!(s1.collisions, 0);
        assert_eq!(s1.rows, s1.registered);
        assert!(r1.verify());
    }

    #[test]
    fn seal_rejects_zero_seed() {
        assert_eq!(seal_standard(0).err(), Some(Code::SeedBad.val()));
        assert!(seal_standard(1).is_ok());
    }

    // ── 码族全扫 ─────────────────────────────────────────────────
    #[test]
    fn sweep_hits_all_fourteen() {
        let sw = exercise_codes();
        assert!(sw.full(), "misses={} touched={}", sw.misses, sw.hist.touched_known());
        assert_eq!(sw.hist.touched_known(), KNOWN_CODES as u32);
        assert_eq!(sw.misses, 0);
        assert!(sw.external > 0);
        assert_eq!(sw.hist.sentinel_total(), sw.external);
        assert!(sw.hist.guard_slot_zero());
    }

    #[test]
    fn sweep_every_slot_positive() {
        let sw = exercise_codes();
        let mut slot = 1u8;
        while slot as usize <= KNOWN_CODES {
            assert!(sw.hist.slot_count(slot) > 0, "槽 {} 空", slot);
            slot = slot.saturating_add(1);
        }
        assert_eq!(sw.hist.slot_count(0), 0);
        assert!(sw.hist.slot_count(SENTINEL_SLOT) >= 2);
    }

    #[test]
    fn sweep_is_repeatable() {
        let a = exercise_codes();
        let b = exercise_codes();
        assert_eq!(a, b);
        assert_eq!(a.hist.total(), b.hist.total());
    }

    #[test]
    fn sweep_note_counts_misses() {
        let mut sw = CodeSweep::new();
        assert!(!sw.note(Some(Code::OpBad.val()), Code::CaseBad.val()));
        assert_eq!(sw.misses, 1);
        assert!(sw.note(Some(Code::CaseBad.val()), Code::CaseBad.val()));
        assert_eq!(sw.misses, 1);
        assert!(!sw.note(None, Code::CaseBad.val()));
        assert_eq!(sw.misses, 2);
        assert!(!sw.full());
    }

    // ── 八项自检 ─────────────────────────────────────────────────
    #[test]
    fn checks_all_green() {
        let set = run_unx49077_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn checks_len_and_truncation() {
        let set = run_unx49077_checks();
        assert_eq!(set.len(), 8);
        assert!(!set.truncated());
        assert_eq!(set.passed_count(), 8);
        assert_eq!(set.failed_count(), 0);
    }

    #[test]
    fn checks_names_distinct() {
        let set = run_unx49077_checks();
        let mut names: Vec<&'static str> = Vec::new();
        let mut i = 0usize;
        for c in set.iter() {
            assert!(!names.contains(&c.name));
            names.push(c.name);
            i += 1;
        }
        assert_eq!(i, 8);
        assert!(names.contains(&"F49077/fuzz零崩"));
        assert!(names.contains(&"F49078/用例集二十"));
    }

    #[test]
    fn checks_detail_nonempty() {
        let rows = extra_checks();
        assert_eq!(rows.len(), 8);
        let mut i = 0usize;
        while i < rows.len() {
            let (name, passed, det) = rows.get(i).expect("row");
            assert!(*passed, "{} red: {}", name, det);
            assert!(!det.is_empty(), "{} detail 为空", name);
            i += 1;
        }
    }

    #[test]
    fn checks_prefix_split() {
        let rows = extra_checks();
        let mut f77 = 0usize;
        let mut f78 = 0usize;
        let mut i = 0usize;
        while i < rows.len() {
            let (name, _, _) = rows.get(i).expect("row");
            if name.starts_with("F49077/") {
                f77 += 1;
            } else if name.starts_with("F49078/") {
                f78 += 1;
            }
            i += 1;
        }
        assert_eq!(f77, 4);
        assert_eq!(f78, 4);
    }

    #[test]
    fn checks_domain_is_f49077() {
        let set = CheckSet::new("UNX-F49077");
        assert_eq!(set.len(), 0);
        assert!(set.is_empty());
    }
}

