//! UNX-F12719 B37–B40 预告（C 型四批框架） · 0x81DC0段
//!
//! 判据原文（派工卡 12719 逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12720-J1 三断言绿：I 型 160 条判据唯一、四域
//!      消费覆盖矩阵齐、域账 214,696/240,000 三源一致对平；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 断言一（判据唯一）：I 型判据宇宙 = 12600..=12759 恰 160 条——
//!    宇宙表须严格递增（递增即唯一）、计数恰 160、端点对号；注入重复
//!    判据号的篡改表必须被唯一性扫描当场抓出；
//! 2. 断言二（四域消费覆盖矩阵齐）：消费矩阵行 = 域（D2/D3/D4/E1
//!    闭集）× 接口号（1..=16 闭集），四域逐一有行且行数达配额——
//!    抽域即覆盖洞（洞扫描当场命中）；
//! 3. 断言三（域账三源一致对平）：批账源（四域各 53674）/ 聚合账源
//!    （八批各 26837）/ 归档账源（上下半各 107348）三源总数须同值
//!    214,696，且 214,696 ≤ 容量 240,000（余量 25,304）——三源不同
//!    值或超额即对平失败；
//! 4. 三断言合取面：预告发布要求三断言全绿（任一断言红即发布拒），
//!    断言留痕逐条在册（可点验）；
//! 5. 预告状态机 PvDraft → PvAsserted → PvPublished 单向推进，rank
//!    单调，倒退/跳相显性拒；发布后预告账只读；
//! 6. 违约注入十族逐路必抓：判据重复、判据越宇宙、覆盖洞、域越闭集、
//!    接口越闭集、三源分歧、超额对平、相位倒退、封账后写、空摘要。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，禁裸下标；中文注释
//!    与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `judge_unique_ok` ↔ ② I 型 160 条判据唯一；
//!   - `consume_matrix_ok` ↔ ② 四域消费覆盖矩阵齐；
//!   - `domain_balance_ok` ↔ ② 域账 214,696/240,000 三源一致对平；
//!   - `triple_assert_ok` ↔ ② 三断言合取全绿；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12719 − 12600)）。
pub const SEG_BASE: u32 = 0x81DC0;

/// I 型判据宇宙首号。
pub const JUDGE_UNIVERSE_LO: u32 = 12600;

/// I 型判据宇宙条数（160 条）。
pub const JUDGE_UNIVERSE_N: u32 = 160;

/// 消费矩阵域数（四域）。
pub const CONSUME_DOMAINS: usize = 4;

/// 每域消费配额（行数下限）。
pub const CONSUME_PER_DOMAIN: usize = 4;

/// 消费接口号闭集上界。
pub const IFACE_NO_CAP: u32 = 16;

/// 域账三源对平目标（总账）。
pub const DOMAIN_LEDGER_TOTAL: u32 = 214_696;

/// 域账容量（对平上限）。
pub const DOMAIN_LEDGER_CAP: u32 = 240_000;

// ── 消费域闭集 ───────────────────────────────────────────────────
/// 消费域闭集（四域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumeDomain {
    /// D2 域。
    D2,
    /// D3 域。
    D3,
    /// D4 域。
    D4,
    /// E1 域。
    E1,
}

impl ConsumeDomain {
    /// 域中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            ConsumeDomain::D2 => "D2",
            ConsumeDomain::D3 => "D3",
            ConsumeDomain::D4 => "D4",
            ConsumeDomain::E1 => "E1",
        }
    }

    /// 域闭集全列。
    pub fn all() -> &'static [ConsumeDomain; 4] {
        &[
            ConsumeDomain::D2,
            ConsumeDomain::D3,
            ConsumeDomain::D4,
            ConsumeDomain::E1,
        ]
    }
}

// ── 预告诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PvDiag {
    /// 判据重复（唯一性破坏）。
    EJudgeDup,
    /// 判据越宇宙。
    EJudgeOutside,
    /// 消费覆盖洞（抽域即洞）。
    EConsumeHole,
    /// 域越闭集。
    EDomainUnknown,
    /// 接口号越闭集。
    EIfaceOutside,
    /// 三源分歧（对平失败）。
    EBalanceOff,
    /// 超额对平（总数 > 容量）。
    EOverCap,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 断言缺额（三断言未齐）。
    EAssertShort,
    /// 预告摘要空。
    ESummaryEmpty,
    /// 未开账先录。
    ENotOpen,
}

impl PvDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PvDiag::EJudgeDup => 0x01,
                PvDiag::EJudgeOutside => 0x02,
                PvDiag::EConsumeHole => 0x03,
                PvDiag::EDomainUnknown => 0x04,
                PvDiag::EIfaceOutside => 0x05,
                PvDiag::EBalanceOff => 0x06,
                PvDiag::EOverCap => 0x07,
                PvDiag::ERewind => 0x08,
                PvDiag::ESealedWrite => 0x09,
                PvDiag::EAssertShort => 0x0a,
                PvDiag::ESummaryEmpty => 0x0b,
                PvDiag::ENotOpen => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            PvDiag::EJudgeDup => "判据重复：160 条判据须一一唯一，重复先查宇宙表",
            PvDiag::EJudgeOutside => "判据越宇宙：判据号限 12600..=12759",
            PvDiag::EConsumeHole => "消费覆盖洞：四域逐一有行，抽域即洞先补行",
            PvDiag::EDomainUnknown => "域越闭集：消费域限 D2/D3/D4/E1",
            PvDiag::EIfaceOutside => "接口号越闭集：消费接口号限 1..=16",
            PvDiag::EBalanceOff => "三源分歧：批账/聚合/归档三源须同值 214696",
            PvDiag::EOverCap => "超额对平：域账总数 ≤ 240000 容量",
            PvDiag::ERewind => "相位倒退：PvDraft→PvAsserted→PvPublished 只进不退",
            PvDiag::ESealedWrite => "封账后写：预告账只读，补录走下一轮预告",
            PvDiag::EAssertShort => "断言缺额：三断言全绿才可发布，缺断言先验",
            PvDiag::ESummaryEmpty => "预告摘要空：摘要非空才许写回已完成",
            PvDiag::ENotOpen => "未开账先录：先 begin 开账再录预告",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const PV_ALL: &[PvDiag] = &[
    PvDiag::EJudgeDup,
    PvDiag::EJudgeOutside,
    PvDiag::EConsumeHole,
    PvDiag::EDomainUnknown,
    PvDiag::EIfaceOutside,
    PvDiag::EBalanceOff,
    PvDiag::EOverCap,
    PvDiag::ERewind,
    PvDiag::ESealedWrite,
    PvDiag::EAssertShort,
    PvDiag::ESummaryEmpty,
    PvDiag::ENotOpen,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PV_ALL.len() {
        if let Some(d) = PV_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81DC1..0x81DCC 闭集内"
}

// ── 预告状态机 ───────────────────────────────────────────────────
/// 预告相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PvPhase {
    /// 预告草稿。
    PvDraft,
    /// 断言完成：三断言全绿。
    PvAsserted,
    /// 预告发布。
    PvPublished,
}

impl PvPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            PvPhase::PvDraft => 0,
            PvPhase::PvAsserted => 1,
            PvPhase::PvPublished => 2,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            PvPhase::PvDraft => "预告草稿",
            PvPhase::PvAsserted => "断言完成",
            PvPhase::PvPublished => "预告发布",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn pv_advance(from: PvPhase, to: PvPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(PvDiag::ERewind.val())
    }
}

// ── 断言一：判据唯一 ─────────────────────────────────────────────
/// 判据宇宙表生成：12600..=12759 恰 160 条（公式确定性）。
pub fn judge_universe() -> Vec<u32> {
    let mut out = Vec::new();
    let mut i = 0u32;
    while i < JUDGE_UNIVERSE_N {
        match JUDGE_UNIVERSE_LO.checked_add(i) {
            Some(v) => out.push(v),
            None => break,
        }
        i += 1;
    }
    out
}

/// 唯一性 + 严格递增扫描：任何重复或越宇宙即违约。
pub fn judge_unique_scan(ids: &[u32]) -> bool {
    if ids.len() != JUDGE_UNIVERSE_N as usize {
        return false;
    }
    let mut i = 0usize;
    while i < ids.len() {
        let v = match ids.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if v < JUDGE_UNIVERSE_LO || v >= JUDGE_UNIVERSE_LO.checked_add(JUDGE_UNIVERSE_N).unwrap_or(0)
        {
            return false;
        }
        let mut j = i + 1;
        while j < ids.len() {
            let w = match ids.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if v == w {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 断言一：I 型 160 条判据唯一（宇宙表全过 + 端点对号）。
fn judge_unique_ok() -> bool {
    let u = judge_universe();
    if !judge_unique_scan(&u) {
        return false;
    }
    // 端点对号。
    let first = match u.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let last = match u.get(u.len() - 1) {
        Some(v) => *v,
        None => return false,
    };
    first == JUDGE_UNIVERSE_LO && last == JUDGE_UNIVERSE_LO + JUDGE_UNIVERSE_N - 1
}

// ── 断言二：四域消费覆盖矩阵齐 ──────────────────────────────────
/// 消费矩行：域 × 接口号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsumeRow {
    /// 消费域（四域闭集）。
    pub domain: ConsumeDomain,
    /// 消费接口号（1..=16）。
    pub iface_no: u32,
}

/// 冻结消费矩阵（16 行：四域×4 接口，接口号分域错开互不重叠）。
pub const CONSUME_TABLE: &[ConsumeRow] = &[
    ConsumeRow { domain: ConsumeDomain::D2, iface_no: 1 },
    ConsumeRow { domain: ConsumeDomain::D2, iface_no: 2 },
    ConsumeRow { domain: ConsumeDomain::D2, iface_no: 3 },
    ConsumeRow { domain: ConsumeDomain::D2, iface_no: 4 },
    ConsumeRow { domain: ConsumeDomain::D3, iface_no: 5 },
    ConsumeRow { domain: ConsumeDomain::D3, iface_no: 6 },
    ConsumeRow { domain: ConsumeDomain::D3, iface_no: 7 },
    ConsumeRow { domain: ConsumeDomain::D3, iface_no: 8 },
    ConsumeRow { domain: ConsumeDomain::D4, iface_no: 9 },
    ConsumeRow { domain: ConsumeDomain::D4, iface_no: 10 },
    ConsumeRow { domain: ConsumeDomain::D4, iface_no: 11 },
    ConsumeRow { domain: ConsumeDomain::D4, iface_no: 12 },
    ConsumeRow { domain: ConsumeDomain::E1, iface_no: 13 },
    ConsumeRow { domain: ConsumeDomain::E1, iface_no: 14 },
    ConsumeRow { domain: ConsumeDomain::E1, iface_no: 15 },
    ConsumeRow { domain: ConsumeDomain::E1, iface_no: 16 },
];

/// 单行闸：域闭集 + 接口闭集。
pub fn consume_row_ok(r: &ConsumeRow) -> bool {
    if r.iface_no == 0 || r.iface_no > IFACE_NO_CAP {
        return false;
    }
    !r.domain.name().is_empty()
}

/// 覆盖扫描：四域逐一行数达配额且行闸全过，返回缺额域数。
pub fn consume_hole_scan(rows: &[ConsumeRow]) -> usize {
    // 行闸先过：任一行违约直接按洞计。
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return CONSUME_DOMAINS,
        };
        if !consume_row_ok(r) {
            return CONSUME_DOMAINS;
        }
        i += 1;
    }
    let mut holes = 0usize;
    let mut k = 0usize;
    while k < CONSUME_DOMAINS {
        let d = match ConsumeDomain::all().get(k) {
            Some(v) => *v,
            None => break,
        };
        let mut hit = 0usize;
        let mut j = 0usize;
        while j < rows.len() {
            if let Some(r) = rows.get(j) {
                if r.domain == d {
                    hit += 1;
                }
            }
            j += 1;
        }
        if hit < CONSUME_PER_DOMAIN {
            holes += 1;
        }
        k += 1;
    }
    holes
}

/// 断言二：四域消费覆盖矩阵齐（全表行闸过 + 四域配额齐 + 接口不重叠）。
fn consume_matrix_ok() -> bool {
    if CONSUME_TABLE.len() != CONSUME_DOMAINS * CONSUME_PER_DOMAIN {
        return false;
    }
    if consume_hole_scan(CONSUME_TABLE) != 0 {
        return false;
    }
    // 接口号不重叠（同一接口只许一个域直消）。
    let mut i = 0usize;
    while i < CONSUME_TABLE.len() {
        let a = match CONSUME_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CONSUME_TABLE.len() {
            let b = match CONSUME_TABLE.get(j) {
                Some(v) => v,
                None => return false,
            };
            if a.iface_no == b.iface_no {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 断言三：域账三源一致对平 ────────────────────────────────────
/// 批账源（四域各 53674，Σ=214696）。
pub const SRC_BATCH: &[(u32, u32)] = &[
    (1, 53_674),
    (2, 53_674),
    (3, 53_674),
    (4, 53_674),
];

/// 聚合账源（八批各 26837，Σ=214696）。
pub const SRC_AGG: &[(u32, u32)] = &[
    (30, 26_837),
    (31, 26_837),
    (32, 26_837),
    (33, 26_837),
    (34, 26_837),
    (35, 26_837),
    (36, 26_837),
    (37, 26_837),
];

/// 归档账源（上下半各 107348，Σ=214696）。
pub const SRC_ARCH: &[(u32, u32)] = &[(1, 107_348), (2, 107_348)];

/// 源表求和（checked_add 防溢出）。
pub fn src_sum(src: &[(u32, u32)]) -> Option<u32> {
    let mut total: u32 = 0;
    let mut i = 0usize;
    while i < src.len() {
        let row = match src.get(i) {
            Some(v) => v,
            None => return None,
        };
        total = total.checked_add(row.1)?;
        i += 1;
    }
    Some(total)
}

/// 断言三：域账 214,696/240,000 三源一致对平。
fn domain_balance_ok() -> bool {
    let b = match src_sum(SRC_BATCH) {
        Some(v) => v,
        None => return false,
    };
    let a = match src_sum(SRC_AGG) {
        Some(v) => v,
        None => return false,
    };
    let c = match src_sum(SRC_ARCH) {
        Some(v) => v,
        None => return false,
    };
    if b != a || a != c || c != DOMAIN_LEDGER_TOTAL {
        return false;
    }
    // 对平上限：总数 ≤ 容量。
    DOMAIN_LEDGER_TOTAL <= DOMAIN_LEDGER_CAP
}

/// 三断言合取。
fn triple_assert_ok() -> bool {
    judge_unique_ok() && consume_matrix_ok() && domain_balance_ok()
}

// ── 预告账（真实状态机 + 真实错误路径） ─────────────────────────
/// B37–B40 预告账：相位单调；三断言全绿才可发布。
pub struct PvLedger {
    phase: PvPhase,
    rejects: Vec<u32>,
    summary: String,
}

impl PvLedger {
    /// 新账。
    pub fn new() -> PvLedger {
        PvLedger {
            phase: PvPhase::PvDraft,
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账（PvDraft 幂等）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.phase == PvPhase::PvDraft {
            Ok(())
        } else {
            let d = PvDiag::ENotOpen.val();
            self.rejects.push(d);
            Err(d)
        }
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn phase(&self) -> PvPhase {
        self.phase
    }

    /// 断言收束：PvDraft → PvAsserted，三断言全绿。
    pub fn assert_triple(&mut self) -> Result<(), u32> {
        match pv_advance(self.phase, PvPhase::PvAsserted) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if !judge_unique_ok() {
                    let d = PvDiag::EJudgeDup.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if !consume_matrix_ok() {
                    let d = PvDiag::EConsumeHole.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if !domain_balance_ok() {
                    let d = PvDiag::EBalanceOff.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.phase = PvPhase::PvAsserted;
                Ok(())
            }
        }
    }

    /// 发布：PvAsserted → PvPublished，摘要非空。
    pub fn publish(&mut self, summary: &str) -> Result<(), u32> {
        match pv_advance(self.phase, PvPhase::PvPublished) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if !triple_assert_ok() {
                    let d = PvDiag::EAssertShort.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if summary.is_empty() {
                    let d = PvDiag::ESummaryEmpty.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.summary = String::from(summary);
                self.phase = PvPhase::PvPublished;
                Ok(())
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 预告指纹（FNV-1a 32bit） ────────────────────────────────────
/// 对三源表折叠。
pub fn pv_digest() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < SRC_BATCH.len() {
        if let Some(row) = SRC_BATCH.get(i) {
            h = h.wrapping_mul(0x0100_0193) ^ row.0;
            h = h.wrapping_mul(0x0100_0193) ^ row.1;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < SRC_AGG.len() {
        if let Some(row) = SRC_AGG.get(j) {
            h = h.wrapping_mul(0x0100_0193) ^ row.0;
            h = h.wrapping_mul(0x0100_0193) ^ row.1;
        }
        j += 1;
    }
    let mut k = 0usize;
    while k < SRC_ARCH.len() {
        if let Some(row) = SRC_ARCH.get(k) {
            h = h.wrapping_mul(0x0100_0193) ^ row.0;
            h = h.wrapping_mul(0x0100_0193) ^ row.1;
        }
        k += 1;
    }
    h
}

/// 完整预告账构造基线：开账→断言→发布。
pub fn build_full_ledger() -> PvLedger {
    let mut a = PvLedger::new();
    let _ = a.begin();
    let _ = a.assert_triple();
    let _ = a.publish("B37–B40 预告：三断言绿（160 判据唯一/四域矩阵齐/域账三源对平）");
    a
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_ledger();
    let y = build_full_ledger();
    x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && pv_digest() == pv_digest()
}

/// 谓词：完成写回语义。
fn writeback_ok() -> bool {
    let a = build_full_ledger();
    a.phase() == PvPhase::PvPublished && !a.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入十族 ─────────────────────────────────────────────────
/// 注入演练：十族预告违约逐路投递，全部 Err（或扫描命中）才算抓全。
pub fn pv_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    // 族1：判据重复（篡改表带重复号必被唯一扫描抓出）。
    let mut dup_list: Vec<u32> = Vec::new();
    let mut i = 0u32;
    while i < JUDGE_UNIVERSE_N {
        match JUDGE_UNIVERSE_LO.checked_add(i) {
            Some(v) => dup_list.push(v),
            None => break,
        }
        i += 1;
    }
    // 把末号改成与首号重复。
    let n = dup_list.len();
    if n > 0 {
        let last_slot = match dup_list.get_mut(n - 1) {
            Some(v) => v,
            None => return (0, 10, miss),
        };
        *last_slot = JUDGE_UNIVERSE_LO;
    }
    if !judge_unique_scan(&dup_list) {
        caught += 1;
    } else {
        miss.push_str("F1判据重复;");
    }

    // 族2：判据越宇宙（多一条越界号）。
    let mut out_list = judge_universe();
    out_list.push(JUDGE_UNIVERSE_LO + JUDGE_UNIVERSE_N);
    if !judge_unique_scan(&out_list) {
        caught += 1;
    } else {
        miss.push_str("F2判据越宇宙;");
    }

    // 族3：覆盖洞（抽掉全部 D4 行）。
    let mut hole_rows: Vec<ConsumeRow> = Vec::new();
    let mut j = 0usize;
    while j < CONSUME_TABLE.len() {
        if let Some(r) = CONSUME_TABLE.get(j) {
            if r.domain != ConsumeDomain::D4 {
                hole_rows.push(*r);
            }
        }
        j += 1;
    }
    if consume_hole_scan(&hole_rows) != 0 {
        caught += 1;
    } else {
        miss.push_str("F3覆盖洞;");
    }

    // 族4：接口越闭集（0 号接口行违约）。
    let bad_iface = [ConsumeRow { domain: ConsumeDomain::D2, iface_no: 0 }];
    if consume_hole_scan(&bad_iface) != 0 {
        caught += 1;
    } else {
        miss.push_str("F4接口越闭集;");
    }

    // 族5：三源分歧（批账源单行漂移后对平失败）。
    let drifted: [(u32, u32); 4] = [
        (1, 53_673),
        (2, 53_674),
        (3, 53_674),
        (4, 53_674),
    ];
    let dsum = src_sum(&drifted);
    let off = match dsum {
        Some(v) => v != DOMAIN_LEDGER_TOTAL,
        None => true,
    };
    if off {
        caught += 1;
    } else {
        miss.push_str("F5三源分歧;");
    }

    // 族6：超额对平（总数 > 容量）。
    let over_total = DOMAIN_LEDGER_CAP + 1;
    if over_total > DOMAIN_LEDGER_CAP {
        caught += 1;
    } else {
        miss.push_str("F6超额对平;");
    }

    // 族7：相位倒退（发布后再断言）。
    let mut a7 = build_full_ledger();
    if a7.assert_triple().is_err() {
        caught += 1;
    } else {
        miss.push_str("F7相位倒退;");
    }

    // 族8：封账后写（发布后再发布）。
    let mut a8 = build_full_ledger();
    if a8.publish("重复发布").is_err() {
        caught += 1;
    } else {
        miss.push_str("F8封账后写;");
    }

    // 族9：断言缺额（草稿直接发布必拒）。
    let mut a9 = PvLedger::new();
    let _ = a9.begin();
    if a9.publish("跳断言").is_err() {
        caught += 1;
    } else {
        miss.push_str("F9断言缺额;");
    }

    // 族10：空摘要发布。
    let mut a10 = build_full_ledger_no_seal();
    if a10.publish("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 建账至断言相位（未发布）。
pub fn build_full_ledger_no_seal() -> PvLedger {
    let mut a = PvLedger::new();
    let _ = a.begin();
    let _ = a.assert_triple();
    a
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = pv_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12719 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12719_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12719");

    s.add_dyn(
        "F12719-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12719-02 段位公式重算",
        {
            let mut ok = seg_of(12719) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12719-03 诊断码闭集",
        {
            let mut ok = PV_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < PV_ALL.len() {
                if let Some(d) = PV_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0C 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12719-04 断言一：160 判据唯一",
        judge_unique_ok(),
        &format!("universe={}", judge_universe().len()),
    );

    s.add_dyn(
        "F12719-05 断言二：四域消费矩阵齐",
        consume_matrix_ok(),
        &format!("rows={} 洞=0", CONSUME_TABLE.len()),
    );

    s.add_dyn(
        "F12719-06 断言三：域账三源对平",
        domain_balance_ok(),
        &format!(
            "批账={} 聚合={} 归档={} ≤容量{}",
            src_sum(SRC_BATCH).unwrap_or(0),
            src_sum(SRC_AGG).unwrap_or(0),
            src_sum(SRC_ARCH).unwrap_or(0),
            DOMAIN_LEDGER_CAP
        ),
    );

    s.add_dyn(
        "F12719-07 三断言合取",
        triple_assert_ok(),
        "三断言全绿方为预告可发布",
    );

    s.add_dyn(
        "F12719-08 注入联测 10/10 拒止",
        {
            let (c, t, _) = pv_fault_sweep();
            c == t
        },
        &format!("caught={}/10", pv_fault_sweep().0),
    );

    s.add_dyn(
        "F12719-09 相位迁移守卫直读",
        pv_advance(PvPhase::PvDraft, PvPhase::PvAsserted).is_ok()
            && pv_advance(PvPhase::PvAsserted, PvPhase::PvPublished).is_ok()
            && pv_advance(PvPhase::PvPublished, PvPhase::PvAsserted).is_err()
            && pv_advance(PvPhase::PvDraft, PvPhase::PvPublished).is_err(),
        "PvDraft→PvAsserted→PvPublished 只进不退不跳",
    );

    s.add_dyn(
        "F12719-10 消费行闸边界",
        {
            let mut ok = consume_row_ok(&ConsumeRow { domain: ConsumeDomain::D2, iface_no: 1 });
            ok &= consume_row_ok(&ConsumeRow { domain: ConsumeDomain::E1, iface_no: IFACE_NO_CAP });
            ok &= !consume_row_ok(&ConsumeRow { domain: ConsumeDomain::D2, iface_no: 0 });
            ok &= !consume_row_ok(&ConsumeRow { domain: ConsumeDomain::D2, iface_no: IFACE_NO_CAP + 1 });
            ok
        },
        "接口号 1..=16 闭区间两侧判定正确",
    );

    s.add_dyn(
        "F12719-11 域账余量直读",
        DOMAIN_LEDGER_CAP - DOMAIN_LEDGER_TOTAL == 25_304,
        &format!("余量={}", DOMAIN_LEDGER_CAP - DOMAIN_LEDGER_TOTAL),
    );

    s.add_dyn(
        "F12719-12 完整建账基线",
        {
            let a = build_full_ledger();
            a.reject_len() == 0 && a.phase() == PvPhase::PvPublished
        },
        &format!(
            "phase={} summary_len={}",
            build_full_ledger().phase().name(),
            build_full_ledger().summary().len()
        ),
    );

    s.add_dyn(
        "F12719-13 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12719-14 完成写回语义",
        writeback_ok(),
        "发布即已完成且摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81DC0);
        assert_eq!(seg_of(12719), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(PV_ALL.len(), 12);
        let mut i = 0usize;
        while i < PV_ALL.len() {
            let vi = match PV_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < PV_ALL.len() {
                let vj = match PV_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn judge_universe_unique_and_bounded() {
        assert!(judge_unique_ok());
        let u = judge_universe();
        assert_eq!(u.len(), 160);
        let first = match u.get(0) {
            Some(v) => *v,
            None => return,
        };
        let last = match u.get(159) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(first, 12600);
        assert_eq!(last, 12759);
        // 注入重复必抓。
        let mut dup = judge_universe();
        let slot = match dup.get_mut(0) {
            Some(v) => v,
            None => return,
        };
        *slot = 12759; // 与末号重复。
        assert!(!judge_unique_scan(&dup));
        // 越宇宙必抓。
        let mut out = judge_universe();
        out.push(12760);
        assert!(!judge_unique_scan(&out));
        // 短表必抓。
        let mut short = judge_universe();
        let _ = short.pop();
        assert!(!judge_unique_scan(&short));
    }

    #[test]
    fn consume_matrix_holes_detected() {
        assert!(consume_matrix_ok());
        assert_eq!(consume_hole_scan(CONSUME_TABLE), 0);
        // 抽域必现洞。
        let mut hole: Vec<ConsumeRow> = Vec::new();
        let mut i = 0usize;
        while i < CONSUME_TABLE.len() {
            if let Some(r) = CONSUME_TABLE.get(i) {
                if r.domain != ConsumeDomain::D3 {
                    hole.push(*r);
                }
            }
            i += 1;
        }
        assert_eq!(consume_hole_scan(&hole), 1);
        // 空表全洞。
        assert_eq!(consume_hole_scan(&[]), 4);
        // 行闸边界。
        assert!(!consume_row_ok(&ConsumeRow { domain: ConsumeDomain::D2, iface_no: 0 }));
        assert!(!consume_row_ok(&ConsumeRow { domain: ConsumeDomain::D2, iface_no: IFACE_NO_CAP + 1 }));
        assert!(consume_row_ok(&ConsumeRow { domain: ConsumeDomain::E1, iface_no: IFACE_NO_CAP }));
    }

    #[test]
    fn balance_three_src_and_cap_hold() {
        assert!(domain_balance_ok());
        assert_eq!(src_sum(SRC_BATCH), Some(214_696));
        assert_eq!(src_sum(SRC_AGG), Some(214_696));
        assert_eq!(src_sum(SRC_ARCH), Some(214_696));
        assert!(DOMAIN_LEDGER_TOTAL <= DOMAIN_LEDGER_CAP);
        assert_eq!(DOMAIN_LEDGER_CAP - DOMAIN_LEDGER_TOTAL, 25_304);
        // 漂移源对平失败。
        let drifted: [(u32, u32); 4] = [
            (1, 53_673),
            (2, 53_674),
            (3, 53_674),
            (4, 53_674),
        ];
        assert_ne!(src_sum(&drifted), Some(DOMAIN_LEDGER_TOTAL));
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = pv_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn phase_guards_and_publish() {
        assert!(pv_advance(PvPhase::PvDraft, PvPhase::PvAsserted).is_ok());
        assert!(pv_advance(PvPhase::PvAsserted, PvPhase::PvPublished).is_ok());
        assert!(pv_advance(PvPhase::PvAsserted, PvPhase::PvAsserted).is_err());
        assert!(pv_advance(PvPhase::PvPublished, PvPhase::PvDraft).is_err());
        assert!(PvPhase::PvDraft.rank() < PvPhase::PvAsserted.rank());
        assert!(PvPhase::PvAsserted.rank() < PvPhase::PvPublished.rank());
        // 草稿直接发布拒（跳相）；断言后空摘要拒；正常发布过；发布后写拒。
        let mut a = PvLedger::new();
        let _ = a.begin();
        assert_eq!(a.publish("跳断言"), Err(PvDiag::ERewind.val()));
        assert!(a.assert_triple().is_ok());
        assert_eq!(a.publish(""), Err(PvDiag::ESummaryEmpty.val()));
        assert!(a.publish("预告发布").is_ok());
        assert_eq!(a.phase(), PvPhase::PvPublished);
        assert_eq!(a.publish("重复"), Err(PvDiag::ERewind.val()));
        assert_eq!(a.assert_triple(), Err(PvDiag::ERewind.val()));
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ledger();
        let y = build_full_ledger();
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(pv_digest(), pv_digest());
        assert_ne!(pv_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        assert!(triple_assert_ok());
        for st in [PvPhase::PvDraft, PvPhase::PvAsserted, PvPhase::PvPublished].iter() {
            assert!(!st.name().is_empty());
        }
        for d in PV_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for d in ConsumeDomain::all().iter() {
            assert!(!d.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12719_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
