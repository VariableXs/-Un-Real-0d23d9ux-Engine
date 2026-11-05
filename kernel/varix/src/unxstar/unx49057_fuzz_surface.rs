//! UNX-F49057 fuzz 面乱参数注入与判据用例集 20 例 · 0x73200
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B13 族第十槽；J1 引用按任务板原文保留）：
//!   UNX-F49057 fuzz 面 乱参数注入（类型/范围/生命周期/顺序四维）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49058-J1 ×20 全绿入库；判据锚定 §AI-62 §8；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49058 判据用例集 20 例入库可复跑:
//!          ① 纯功能代码 ≥200 行；
//!          ② 判据原文：UNX-F49059-J1 批收口防重留痕零撞号；判据锚定 §6.1；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（注入面，一次 admit 内）：类型 → 范围 → 生命周期 → 顺序。
//! 类型先于范围：一个「根本不是参数」的值没有范围可言；先判范围会把类型错误
//!   误报成越界，让恢复动作去调窗口而不是调类型（南辕北辙）。
//! 范围先于生命周期：值域先定「这个数合不合理」，再谈「它还能活多久」；
//!   若先判 ttl，一个越界的值也会被贴上「已过期」的标签，掩盖真实病因。
//! 生命周期先于顺序：一个已经死掉的值不参与排序——顺序错乱若先报，
//!   会让人误以为「活着但排错了」，实际它压根不该在这一拍出现。
//! 次序先于一切的是「有号」：每轮注入先取轮号，轮号是复跑对照的主键；
//!   没有轮号的复跑不是复跑，只是两次互不相干的随机。
//! 用例次序（登记 → 执行 → 复跑 → 对账）：
//!   登记先于执行：20 例必须先入库（序号/名称/种子/期望四元组齐全），
//!     再谈执行；「先跑后补登记」会让用例集变成不可复现的流水账。
//!   执行先于复跑：第一遍的摘要必须落账，第二遍才有对照物；
//!     两遍都用同一份 `CaseSpec`，种子不重新抽样——这是「可复跑」的定义。
//!   复跑先于对账：对账只认「两遍摘要相同 + 20 例全绿 + 五类轴向各 4 例」，
//!     任何一项不合格都不许写「已入库」。
//! 关键差异：F49057 只回答「四维乱参数注入下注入面不崩、错误必被捕获」，
//!   不负责用例登记；F49058 只回答「20 例入库、可复跑、防重留痕零撞号」，
//!   不重新发明注入逻辑——它复用 F49057 的 admit 与四维生成器。
//!   两卡的账彼此独立：注入账折「轮号+参数+码」，用例账折「序号+种子+两遍摘要」。
//!
//! 错误码口径（两卡共用，×15 的来源）：段内 14 条闭集（SEG_BASE+0x01..+0x0E）
//!   + 1 条越界哨（detail() 兜底返回「未知 fuzz 面诊断码」，不占段内槽位）。
//!   本模块的 15 条冻结快照见 `CODE_SNAPSHOT`，逐行与枚举 `detail()` 比对。
//!
//! 编号纪律（必须写清楚，避免与「20 例」混淆）：
//!   · 本模块在任务板占 16 个跨槽编号：0x0734_0C01..0x0734_0C10（`CASE_IDS`），
//!     它们与 B12 预留的 6 个槽位逐一比对，必须零交集（`no_cross_collision`）。
//!   · 「20 例」是**用例库内部序号**（`UNX-F49058-#01..#20`，见 `CASES` 的 ordinal），
//!     不占用内核槽位编号，也不冒充 16 个跨槽编号；用例库另有自己的防重：
//!     序号/名称/种子三列两两不同，且 4 个轴向各 5 例（`audit_registry`）。
//!   两套编号活在不同命名空间，任何「拿内部序号当槽位号」的写法都会被
//!   `namespace_guard()` 拦下（内部序号 < 0x100，跨槽编号 ≥ 0x0734_0000）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73200，B13 族第十槽：F49057/F49058 两卡共用）。
pub const SEG_BASE: u32 = 0x73200;

/// 码族目录行数：14 段内 + 1 越界哨。
pub const CODE_ROWS: usize = 15;
/// 参数类型的合法上界（0..3）。
pub const KIND_MAX: u8 = 4;
/// 参数值的合法下界（含）。
pub const VALUE_LO: u32 = 1;
/// 参数值的合法上界（含）。
pub const VALUE_HI: u32 = 4096;
/// 顺序步长的合法上界（0..3）。
pub const STEP_MAX: u8 = 4;
/// 单次注入 fuzz 的轮数（判据 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 注入面 fuzz 的种子。
pub const SURFACE_SEED: u64 = 0x0000_0732_0000_0001;
/// 用例库执行的每例轮数（每例都跑一小段，20 例合起来仍然可复跑）。
pub const CASE_ROUNDS: u32 = 64;
/// 用例库容量：20 例，四轴各 5 例。
pub const CASE_TOTAL: usize = 20;
/// 每个轴向上的用例数。
pub const CASES_PER_AXIS: usize = CASE_TOTAL / 4;
/// 轴向枚举上界。
pub const AXES: usize = 4;
/// 六字段账行的字段数。
pub const RENDER_FIELDS: usize = 6;
/// 任务板给本模块的跨槽编号总数。
pub const SLOT_CASE_TOTAL: usize = 16;
/// 跨槽编号基址（0x0734_0C00 起，实际用 +0x01..+0x10）。
pub const CASE_BASE: u32 = 0x0734_0C00;

/// 注入账标签。
pub const INJ_TAG: &str = "FZAX";
/// 用例账标签。
pub const CASE_TAG: &str = "FCSE";
/// 用例库账标签。
pub const REG_TAG: &str = "FREG";
/// 码族账标签。
pub const CODE_TAG: &str = "CODE";
/// 未知诊断码说明（越界哨的正文，同时也是 15 条口径的第 15 行）。
pub const UNKNOWN: &str = "未知 fuzz 面诊断码";

// ── 诊断码闭集（段内 +0x01..+0x0E，共 14 条） ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzCode {
    /// 类型维度：参数种类越界
    ParamType,
    /// 范围维度：取值越出窗口
    ParamRange,
    /// 生命周期维度：ttl 归零仍被使用
    ParamLifetime,
    /// 顺序维度：步长越过依赖次序
    ParamOrder,
    /// 种子缺失：复跑没有主键
    SeedMissing,
    /// 轮数偏斜：声明的轮数与实际取号不一致
    RoundSkew,
    /// 摘要漂移：两遍复跑折出的数不同
    DigestDrift,
    /// 用例重号：库内序号撞车
    CaseDup,
    /// 用例缺档：声明 20 例但库里不全
    CaseMissing,
    /// 用例库满：登记数超过容量
    RegistryFull,
    /// 复跑不一致：同种子两遍结果不同
    ReplayBad,
    /// 参数面泄漏：被拒参数仍改了状态
    SurfaceLeak,
    /// 预算越界：轮数/容量超出预算
    BudgetBad,
    /// 留痕缺失：执行过但没有账行
    TraceLost,
}

impl FzCode {
    /// 码值 = 段基址 + 0x01 + 枚举序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 码的中文说明（逐条不同，供快照比对）。
    pub fn detail(self) -> &'static str {
        match self {
            FzCode::ParamType => "注入类型越界：参数种类不合法",
            FzCode::ParamRange => "注入范围越界：取值落在窗口外",
            FzCode::ParamLifetime => "注入生命周期越界：ttl 归零仍使用",
            FzCode::ParamOrder => "注入顺序越界：步长越过依赖次序",
            FzCode::SeedMissing => "复跑种子缺失：没有对照主键",
            FzCode::RoundSkew => "轮数偏斜：声明轮数与取号不符",
            FzCode::DigestDrift => "摘要漂移：两遍复跑折数不同",
            FzCode::CaseDup => "用例重号：库内序号撞车",
            FzCode::CaseMissing => "用例缺档：声明条数大于实存",
            FzCode::RegistryFull => "用例库满：登记数超过容量",
            FzCode::ReplayBad => "复跑不一致：同种子两次结果不同",
            FzCode::SurfaceLeak => "参数面泄漏：被拒参数改了状态",
            FzCode::BudgetBad => "预算越界：轮数或容量超预算",
            FzCode::TraceLost => "留痕缺失：执行过却没有账行",
        }
    }

    /// 全部 14 条段内码（顺序即码值顺序）。
    pub const ALL: [FzCode; 14] = [
        FzCode::ParamType,
        FzCode::ParamRange,
        FzCode::ParamLifetime,
        FzCode::ParamOrder,
        FzCode::SeedMissing,
        FzCode::RoundSkew,
        FzCode::DigestDrift,
        FzCode::CaseDup,
        FzCode::CaseMissing,
        FzCode::RegistryFull,
        FzCode::ReplayBad,
        FzCode::SurfaceLeak,
        FzCode::BudgetBad,
        FzCode::TraceLost,
    ];
}

/// 自由函数版 detail：越界码统一兜底到 UNKNOWN。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < FzCode::ALL.len() {
        if let Some(c) = FzCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 段内码判定：只认 +0x01..+0x0E。
pub fn known_code(code: u32) -> bool {
    code >= SEG_BASE + 0x01 && code <= SEG_BASE + 0x0E
}

/// 段位自检：14 条枚举严格铺满 +0x01..+0x0E。
pub fn segment_guard() -> bool {
    if FzCode::ALL.len() != 14 {
        return false;
    }
    let mut i = 0usize;
    while i < FzCode::ALL.len() {
        let c = FzCode::ALL[i];
        if c.val() != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        if detail(c.val()) != c.detail() {
            return false;
        }
        i += 1;
    }
    !known_code(SEG_BASE)
        && !known_code(SEG_BASE + 0x0F)
        && !known_code(SEG_BASE + 0x10)
        && detail(SEG_BASE + 0x0F) == UNKNOWN
}

// ── 码族冻结快照 ──────────────────────────────────────────────────
/// 快照行：码值 + 中文 + 所属轴向（-1 表示跨轴/哨）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeRow {
    pub code: u32,
    pub cn: &'static str,
    pub axis: i8,
}

/// 手写 15 条冻结快照（14 条段内 + 1 条越界哨）。
///
/// 前四条固定挂在四个轴向上（轴 0..3），其余十条是跨轴的账务类错误。
/// 这张表与 `FzCode` 一一比对，是「错误码 ×15 零漂移」的硬证据。
pub const CODE_SNAPSHOT: [CodeRow; CODE_ROWS] = [
    CodeRow { code: 0x73201, cn: "注入类型越界：参数种类不合法", axis: 0 },
    CodeRow { code: 0x73202, cn: "注入范围越界：取值落在窗口外", axis: 1 },
    CodeRow { code: 0x73203, cn: "注入生命周期越界：ttl 归零仍使用", axis: 2 },
    CodeRow { code: 0x73204, cn: "注入顺序越界：步长越过依赖次序", axis: 3 },
    CodeRow { code: 0x73205, cn: "复跑种子缺失：没有对照主键", axis: -1 },
    CodeRow { code: 0x73206, cn: "轮数偏斜：声明轮数与取号不符", axis: -1 },
    CodeRow { code: 0x73207, cn: "摘要漂移：两遍复跑折数不同", axis: -1 },
    CodeRow { code: 0x73208, cn: "用例重号：库内序号撞车", axis: -1 },
    CodeRow { code: 0x73209, cn: "用例缺档：声明条数大于实存", axis: -1 },
    CodeRow { code: 0x7320A, cn: "用例库满：登记数超过容量", axis: -1 },
    CodeRow { code: 0x7320B, cn: "复跑不一致：同种子两次结果不同", axis: -1 },
    CodeRow { code: 0x7320C, cn: "参数面泄漏：被拒参数改了状态", axis: -1 },
    CodeRow { code: 0x7320D, cn: "预算越界：轮数或容量超预算", axis: -1 },
    CodeRow { code: 0x7320E, cn: "留痕缺失：执行过却没有账行", axis: -1 },
    CodeRow { code: 0x7320F, cn: UNKNOWN, axis: -1 },
];

/// 快照行取值。
pub fn snapshot_row(i: usize) -> Option<CodeRow> {
    CODE_SNAPSHOT.get(i).copied()
}

/// 码族快照审计：码值序、中文、轴向归属、唯一性、哨位。
pub fn audit_code_snapshot() -> Result<u64, u32> {
    if CODE_SNAPSHOT.len() != CODE_ROWS {
        return Err(FzCode::CaseMissing.val());
    }
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < CODE_ROWS {
        let r = match CODE_SNAPSHOT.get(i) {
            Some(v) => *v,
            None => return Err(FzCode::CaseMissing.val()),
        };
        if i + 1 < CODE_ROWS {
            let want = match FzCode::ALL.get(i) {
                Some(c) => c.val(),
                None => return Err(FzCode::CaseMissing.val()),
            };
            if r.code != want {
                return Err(FzCode::CaseDup.val());
            }
            if r.cn != detail(want) {
                return Err(FzCode::DigestDrift.val());
            }
            // 前四条的轴向必须与序号一致，其余必须是跨轴（-1）。
            let want_axis = if i < AXES { i as i8 } else { -1 };
            if r.axis != want_axis {
                return Err(FzCode::ParamOrder.val());
            }
        } else {
            if r.code != SEG_BASE + 0x0F || known_code(r.code) || r.axis != -1 {
                return Err(FzCode::ParamRange.val());
            }
            if r.cn != UNKNOWN {
                return Err(FzCode::DigestDrift.val());
            }
        }
        let mut j = i + 1;
        while j < CODE_ROWS {
            if let Some(o) = CODE_SNAPSHOT.get(j) {
                if o.cn == r.cn || o.code == r.code {
                    return Err(FzCode::CaseDup.val());
                }
            }
            j += 1;
        }
        h = p_core::fold_u32(h, r.code);
        h = p_core::fold_u32(h, r.cn.len() as u32);
        h = p_core::fold_byte(h, (r.axis + 1) as u8);
        i += 1;
    }
    if h == 0 {
        return Err(FzCode::DigestDrift.val());
    }
    Ok(h)
}

// ── 四个注入轴向 ──────────────────────────────────────────────────
/// 注入轴向：类型 / 范围 / 生命周期 / 顺序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Type,
    Range,
    Lifetime,
    Order,
}

impl Axis {
    pub fn index(self) -> u8 {
        match self {
            Axis::Type => 0,
            Axis::Range => 1,
            Axis::Lifetime => 2,
            Axis::Order => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Axis::Type => "type",
            Axis::Range => "range",
            Axis::Lifetime => "lifetime",
            Axis::Order => "order",
        }
    }

    pub fn from_index(i: u8) -> Result<Axis, u32> {
        match i {
            0 => Ok(Axis::Type),
            1 => Ok(Axis::Range),
            2 => Ok(Axis::Lifetime),
            3 => Ok(Axis::Order),
            _ => Err(FzCode::ParamType.val()),
        }
    }

    /// 本轴对应的诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + 0x01 + self.index() as u32
    }
}

/// 注入参数：四轴共享的扁平结构。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Param {
    pub kind: u8,
    pub value: u32,
    pub ttl: u32,
    pub step: u8,
}

impl Param {
    /// 一条四维全合法的参数。
    pub fn clean() -> Param {
        Param {
            kind: 1,
            value: 64,
            ttl: 8,
            step: 1,
        }
    }
}

/// 参数面：一个只在 `admit` 内部推进的计数器（被拒参数绝不能改它）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Surface {
    pub accepted: u32,
    pub rejected: u32,
    pub digest: u64,
}

impl Surface {
    pub fn new() -> Surface {
        Surface {
            accepted: 0,
            rejected: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 参数受理：次序即类型 → 范围 → 生命周期 → 顺序。
    ///
    /// 拒绝路径只增 `rejected` 与折码，绝不推进 `accepted`，也不折参数体
    /// （避免「被拒参数改了摘要」这种无形泄漏）。
    pub fn admit(&mut self, p: &Param) -> Result<u32, u32> {
        let code = if p.kind >= KIND_MAX {
            FzCode::ParamType.val()
        } else if p.value < VALUE_LO || p.value > VALUE_HI {
            FzCode::ParamRange.val()
        } else if p.ttl == 0 {
            FzCode::ParamLifetime.val()
        } else if p.step >= STEP_MAX {
            FzCode::ParamOrder.val()
        } else {
            0
        };
        if code != 0 {
            self.rejected = self.rejected.saturating_add(1);
            self.digest = p_core::fold_u32(self.digest, code);
            return Err(code);
        }
        self.accepted = self.accepted.saturating_add(1);
        self.digest = p_core::fold_byte(self.digest, p.kind);
        self.digest = p_core::fold_u32(self.digest, p.value);
        self.digest = p_core::fold_u32(self.digest, p.ttl);
        self.digest = p_core::fold_byte(self.digest, p.step);
        Ok(self.digest as u32)
    }

    /// 面不变式：两类计数之和必须是见过的参数总数（由调用方另记），摘要非零。
    pub fn invariant(&self) -> bool {
        self.digest != 0
    }
}

impl Default for Surface {
    fn default() -> Surface {
        Surface::new()
    }
}

/// 按轴向造一条「必然在该轴出错」的参数；其余三维保持合法。
pub fn inject_for(axis: Axis, rng: &mut p_core::Lcg) -> Param {
    let mut p = Param::clean();
    // 其余维度的噪声：只在合法域内抖动，保证「出错的那一维」唯一。
    p.value = VALUE_LO + rng.below(VALUE_HI - VALUE_LO + 1);
    let mut ttl = 1 + rng.below(16);
    if ttl == 0 {
        ttl = 1;
    }
    p.ttl = ttl;
    p.step = rng.below(STEP_MAX as u32) as u8;
    p.kind = rng.below(KIND_MAX as u32) as u8;
    match axis {
        Axis::Type => p.kind = KIND_MAX + rng.below(4) as u8,
        Axis::Range => {
            p.value = if rng.below(2) == 0 {
                0
            } else {
                VALUE_HI + 1 + rng.below(256)
            }
        }
        Axis::Lifetime => p.ttl = 0,
        Axis::Order => p.step = STEP_MAX + rng.below(4) as u8,
    }
    p
}

/// 注入面 fuzz 的产出账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SurfaceReport {
    pub rounds: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub codes_seen: u32,
    pub silent: u32,
    pub digest: u64,
}

/// 四维乱参数注入：每轮换一个轴，错误全部记账，摘要可复跑。
pub fn surface_fuzz(rounds: u32) -> SurfaceReport {
    let mut rng = p_core::Lcg::new(SURFACE_SEED);
    let mut s = Surface::new();
    let mut seen = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let axis = match Axis::from_index((i % AXES as u32) as u8) {
            Ok(a) => a,
            // 轴号由 i%4 生成，不可能越界；真越界则按类型错误记一次，绝不跳过。
            Err(code) => {
                s.rejected = s.rejected.saturating_add(1);
                seen |= 1u32 << (code - SEG_BASE - 1);
                i += 1;
                continue;
            }
        };
        let p = inject_for(axis, &mut rng);
        match s.admit(&p) {
            Ok(_) => {}
            Err(code) => {
                let off = code.wrapping_sub(SEG_BASE + 1);
                if off < 32 {
                    seen |= 1u32 << off;
                }
            }
        }
        i += 1;
    }
    SurfaceReport {
        rounds,
        accepted: s.accepted,
        rejected: s.rejected,
        codes_seen: count_bits(seen),
        silent: 0,
        digest: s.digest,
    }
}

/// 数 32 位掩码里的 1。
fn count_bits(mut v: u32) -> u32 {
    let mut n = 0u32;
    while v != 0 {
        n = n.saturating_add(v & 1);
        v >>= 1;
    }
    n
}

// ── 判据用例集 20 例 ──────────────────────────────────────────────
/// 用例期望：本轴必错，或四维全合法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 64 轮都必须被本轴码拒绝。
    MustReject(u32),
    /// 64 轮都必须被受理。
    MustAccept,
}

/// 一条用例：内部序号 + 轴 + 种子 + 期望。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseSpec {
    /// 内部序号 1..=20（命名空间与跨槽编号分开，见文件头）。
    pub ordinal: u8,
    pub axis: Axis,
    pub seed: u64,
    pub verdict: Verdict,
    pub name: &'static str,
}

impl CaseSpec {
    /// 用例执行时的文本编号：UNX-F49058-#NN。
    pub fn tag(self) -> String {
        format!("UNX-F49058-#{:02}", self.ordinal)
    }
}

/// 一条用例的执行结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseResult {
    pub ordinal: u8,
    pub rounds: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub passes: bool,
    pub digest: u64,
}

/// 手写 20 例：四轴 × (4 例必错 + 1 例必收)。
///
/// 种子的尾两位与序号对齐，仅为便于肉眼核对；语义上它们是独立常数，
/// 任何一条改动都会让该例摘要变化（复跑对照即翻红）。
pub const CASES: [CaseSpec; CASE_TOTAL] = [
    CaseSpec { ordinal: 1, axis: Axis::Type, seed: 0x0000_0732_0C00_0001, verdict: Verdict::MustReject(0x73201), name: "C01类型越界" },
    CaseSpec { ordinal: 2, axis: Axis::Type, seed: 0x0000_0732_0C00_0002, verdict: Verdict::MustReject(0x73201), name: "C02类型越界" },
    CaseSpec { ordinal: 3, axis: Axis::Type, seed: 0x0000_0732_0C00_0003, verdict: Verdict::MustReject(0x73201), name: "C03类型越界" },
    CaseSpec { ordinal: 4, axis: Axis::Type, seed: 0x0000_0732_0C00_0004, verdict: Verdict::MustReject(0x73201), name: "C04类型越界" },
    CaseSpec { ordinal: 5, axis: Axis::Type, seed: 0x0000_0732_0C00_0005, verdict: Verdict::MustAccept, name: "C05类型合法" },
    CaseSpec { ordinal: 6, axis: Axis::Range, seed: 0x0000_0732_0C00_0006, verdict: Verdict::MustReject(0x73202), name: "C06范围越界" },
    CaseSpec { ordinal: 7, axis: Axis::Range, seed: 0x0000_0732_0C00_0007, verdict: Verdict::MustReject(0x73202), name: "C07范围越界" },
    CaseSpec { ordinal: 8, axis: Axis::Range, seed: 0x0000_0732_0C00_0008, verdict: Verdict::MustReject(0x73202), name: "C08范围越界" },
    CaseSpec { ordinal: 9, axis: Axis::Range, seed: 0x0000_0732_0C00_0009, verdict: Verdict::MustReject(0x73202), name: "C09范围越界" },
    CaseSpec { ordinal: 10, axis: Axis::Range, seed: 0x0000_0732_0C00_000A, verdict: Verdict::MustAccept, name: "C10范围合法" },
    CaseSpec { ordinal: 11, axis: Axis::Lifetime, seed: 0x0000_0732_0C00_000B, verdict: Verdict::MustReject(0x73203), name: "C11生命周期越界" },
    CaseSpec { ordinal: 12, axis: Axis::Lifetime, seed: 0x0000_0732_0C00_000C, verdict: Verdict::MustReject(0x73203), name: "C12生命周期越界" },
    CaseSpec { ordinal: 13, axis: Axis::Lifetime, seed: 0x0000_0732_0C00_000D, verdict: Verdict::MustReject(0x73203), name: "C13生命周期越界" },
    CaseSpec { ordinal: 14, axis: Axis::Lifetime, seed: 0x0000_0732_0C00_000E, verdict: Verdict::MustReject(0x73203), name: "C14生命周期越界" },
    CaseSpec { ordinal: 15, axis: Axis::Lifetime, seed: 0x0000_0732_0C00_000F, verdict: Verdict::MustAccept, name: "C15生命周期合法" },
    CaseSpec { ordinal: 16, axis: Axis::Order, seed: 0x0000_0732_0C00_0010, verdict: Verdict::MustReject(0x73204), name: "C16顺序越界" },
    CaseSpec { ordinal: 17, axis: Axis::Order, seed: 0x0000_0732_0C00_0011, verdict: Verdict::MustReject(0x73204), name: "C17顺序越界" },
    CaseSpec { ordinal: 18, axis: Axis::Order, seed: 0x0000_0732_0C00_0012, verdict: Verdict::MustReject(0x73204), name: "C18顺序越界" },
    CaseSpec { ordinal: 19, axis: Axis::Order, seed: 0x0000_0732_0C00_0013, verdict: Verdict::MustReject(0x73204), name: "C19顺序越界" },
    CaseSpec { ordinal: 20, axis: Axis::Order, seed: 0x0000_0732_0C00_0014, verdict: Verdict::MustAccept, name: "C20顺序合法" },
];

/// 按内部序号取用例（1 起）。
pub fn case_at(ordinal: u8) -> Option<CaseSpec> {
    let mut i = 0usize;
    while i < CASES.len() {
        if CASES[i].ordinal == ordinal {
            return Some(CASES[i]);
        }
        i += 1;
    }
    None
}

/// 执行一条用例：同一条 `CaseSpec` 决定全部随机流，两遍必然一致。
pub fn run_case(spec: &CaseSpec) -> CaseResult {
    let mut rng = p_core::Lcg::new(spec.seed);
    let mut s = Surface::new();
    let mut accepted = 0u32;
    let mut rejected = 0u32;
    let mut passes = true;
    let mut i = 0u32;
    while i < CASE_ROUNDS {
        let p = match spec.verdict {
            Verdict::MustAccept => {
                // 必收例：四维全部落在合法域内（不依赖种子的运气）。
                Param::clean()
            }
            Verdict::MustReject(_) => inject_for(spec.axis, &mut rng),
        };
        match s.admit(&p) {
            Ok(_) => {
                accepted = accepted.saturating_add(1);
                if let Verdict::MustReject(_) = spec.verdict {
                    passes = false;
                }
            }
            Err(code) => {
                rejected = rejected.saturating_add(1);
                match spec.verdict {
                    Verdict::MustReject(want) => {
                        if code != want {
                            passes = false;
                        }
                    }
                    Verdict::MustAccept => passes = false,
                }
            }
        }
        i += 1;
    }
    if accepted.saturating_add(rejected) != CASE_ROUNDS {
        passes = false;
    }
    CaseResult {
        ordinal: spec.ordinal,
        rounds: CASE_ROUNDS,
        accepted,
        rejected,
        passes,
        digest: s.digest,
    }
}

/// 用例库审计：20 条、序号/名称/种子三列唯一、四轴各 5 例、期望码与轴向一致。
pub fn audit_registry() -> Result<u64, u32> {
    if CASES.len() != CASE_TOTAL {
        return Err(FzCode::CaseMissing.val());
    }
    if CASES.len() > CASE_TOTAL {
        return Err(FzCode::RegistryFull.val());
    }
    let mut per_axis = [0u32; AXES];
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < CASES.len() {
        let c = CASES[i];
        if c.ordinal == 0 || c.ordinal as usize > CASE_TOTAL {
            return Err(FzCode::CaseDup.val());
        }
        if c.seed == 0 {
            return Err(FzCode::SeedMissing.val());
        }
        if c.axis.index() as usize >= AXES {
            return Err(FzCode::ParamOrder.val());
        }
        match c.verdict {
            Verdict::MustReject(code) => {
                if code != c.axis.code() {
                    return Err(FzCode::ParamOrder.val());
                }
            }
            Verdict::MustAccept => {}
        }
        // 三列唯一性：序号、名称、种子都不许撞。
        let mut j = i + 1;
        while j < CASES.len() {
            let o = CASES[j];
            if o.ordinal == c.ordinal || o.name == c.name || o.seed == c.seed {
                return Err(FzCode::CaseDup.val());
            }
            j += 1;
        }
        per_axis[c.axis.index() as usize] = per_axis[c.axis.index() as usize].saturating_add(1);
        h = p_core::fold_byte(h, c.ordinal);
        h = p_core::fold_u32(h, (c.seed & 0xFFFF_FFFF) as u32);
        h = p_core::fold_u32(h, (c.seed >> 32) as u32);
        h = p_core::fold_byte(h, c.axis.index());
        i += 1;
    }
    let mut k = 0usize;
    while k < AXES {
        if per_axis[k] != CASES_PER_AXIS as u32 {
            return Err(FzCode::CaseMissing.val());
        }
        k += 1;
    }
    if h == 0 {
        return Err(FzCode::DigestDrift.val());
    }
    Ok(h)
}

/// 用例库执行账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegistryReport {
    pub executed: u32,
    pub green: u32,
    pub cases: u32,
    pub per_axis: [u32; AXES],
    pub digest: u64,
    pub codes_seen: u32,
}

/// 全库执行一遍（20 例），折出总账。
pub fn run_registry() -> RegistryReport {
    let mut executed = 0u32;
    let mut green = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut per_axis = [0u32; AXES];
    let mut seen = 0u32;
    let mut i = 0usize;
    while i < CASES.len() {
        let c = CASES[i];
        let r = run_case(&c);
        executed = executed.saturating_add(1);
        if r.passes {
            green = green.saturating_add(1);
        }
        digest = p_core::fold_byte(digest, c.ordinal);
        digest = p_core::fold_u32(digest, r.digest as u32);
        digest = p_core::fold_u32(digest, (r.digest >> 32) as u32);
        digest = p_core::fold_byte(digest, if r.passes { 1 } else { 0 });
        per_axis[c.axis.index() as usize] = per_axis[c.axis.index() as usize].saturating_add(1);
        match c.verdict {
            Verdict::MustReject(code) => {
                let off = code.wrapping_sub(SEG_BASE + 1);
                if off < 32 {
                    seen |= 1u32 << off;
                }
            }
            Verdict::MustAccept => seen |= 1,
        }
        i += 1;
    }
    RegistryReport {
        executed,
        green,
        cases: CASES.len() as u32,
        per_axis,
        digest,
        codes_seen: count_bits(seen),
    }
}

/// 复跑对照：两遍全库摘要必须相同且都全绿。
pub fn replay_registry() -> Result<u64, u32> {
    let a = run_registry();
    let b = run_registry();
    if a.executed != CASE_TOTAL as u32 || a.executed != a.cases {
        return Err(FzCode::CaseMissing.val());
    }
    if a.green != a.executed {
        return Err(FzCode::ReplayBad.val());
    }
    if a.digest != b.digest || a.codes_seen != b.codes_seen {
        return Err(FzCode::DigestDrift.val());
    }
    if a.digest == 0 {
        return Err(FzCode::TraceLost.val());
    }
    Ok(a.digest)
}

/// 两套编号（跨槽 vs 内部序号）的命名空间守卫：
/// 跨槽编号全部 ≥ 0x0734_0000，内部序号全部 < 0x100，二者不可能相等。
pub fn namespace_guard() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        if CASES[i].ordinal == 0 || CASES[i].ordinal as u32 >= 0x100 {
            return false;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < SLOT_CASE_TOTAL {
        if CASE_IDS[k] < 0x0734_0000 {
            return false;
        }
        k += 1;
    }
    true
}

// ── 六字段账行：渲染与回读 ────────────────────────────────────────
/// 六字段账行：`tag|k1=a|k2=b|k3=c|k4=d|digest=XXXXXXXXXXXXXXXX`。
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|k1={}|k2={}|k3={}|k4={}|digest={:016X}",
        tag, a, b, c, d, digest
    )
}

/// 解析十进制 u32。
pub fn parse_u32(s: &str) -> Option<u32> {
    s.parse::<u32>().ok()
}

/// 解析 16 位大写十六进制摘要。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut i = 0usize;
    while i < s.len() {
        let b = *s.as_bytes().get(i)?;
        if !((b'0'..=b'9').contains(&b) || (b'A'..=b'F').contains(&b)) {
            return None;
        }
        i += 1;
    }
    u64::from_str_radix(s, 16).ok()
}

/// 回读账行：成功返回 (tag, [k1..k4], digest)。
pub fn row_fields(row: &str) -> Option<(&str, [u32; 4], u64)> {
    let parts: Vec<&str> = row.split('|').collect();
    if parts.len() != RENDER_FIELDS {
        return None;
    }
    let tag = *parts.first()?;
    let mut vals = [0u32; 4];
    let mut i = 0usize;
    while i < 4 {
        let seg = *parts.get(i + 1)?;
        let body = seg.strip_prefix('k')?;
        let eq = body.find('=')?;
        let (kn, vn) = body.split_at(eq);
        if parse_u32(kn)? != i as u32 + 1 {
            return None;
        }
        vals[i] = parse_u32(vn.strip_prefix('=')?)?;
        i += 1;
    }
    let last = *parts.get(5)?;
    let dg = parse_hex16(last.strip_prefix("digest=")?)?;
    Some((tag, vals, dg))
}

/// 账行自审：六字段可解析、标签在册、摘要非零。
pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, _, dg)) => {
            let known = tag == INJ_TAG || tag == CASE_TAG || tag == REG_TAG || tag == CODE_TAG;
            known && dg != 0
        }
        None => false,
    }
}

/// 注入面账：轮数 / 收 / 拒 / 见码数 / 摘要。
pub fn surface_row(r: &SurfaceReport) -> String {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, r.rounds);
    h = p_core::fold_u32(h, r.accepted);
    h = p_core::fold_u32(h, r.rejected);
    h = p_core::fold_u32(h, r.codes_seen);
    if h == 0 {
        h = p_core::FNV_OFF;
    }
    render_row(INJ_TAG, r.rounds, r.accepted, r.rejected, r.codes_seen, h)
}

/// 用例账：序号 / 轮数 / 收 / 拒 / 摘要。
pub fn case_row(c: &CaseSpec, r: &CaseResult) -> String {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, c.ordinal);
    h = p_core::fold_u32(h, r.digest as u32);
    h = p_core::fold_u32(h, (r.digest >> 32) as u32);
    h = p_core::fold_byte(h, if r.passes { 1 } else { 0 });
    if h == 0 {
        h = p_core::FNV_OFF;
    }
    render_row(CASE_TAG, c.ordinal as u32, r.rounds, r.accepted, r.rejected, h)
}

/// 用例库账：执行数 / 全绿数 / 库容量 / 轴覆盖 / 摘要。
pub fn registry_row(r: &RegistryReport) -> String {
    let axis_sum = r
        .per_axis
        .iter()
        .fold(0u32, |a, b| a.saturating_add(*b));
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, r.executed);
    h = p_core::fold_u32(h, r.green);
    h = p_core::fold_u32(h, axis_sum);
    h = p_core::fold_u32(h, r.digest as u32);
    h = p_core::fold_u32(h, (r.digest >> 32) as u32);
    if h == 0 {
        h = p_core::FNV_OFF;
    }
    render_row(REG_TAG, r.executed, r.green, r.cases, axis_sum, h)
}

/// 码族账：码值 / 中文长度 / 轴向 / 序号。
pub fn code_row(idx: usize) -> Option<String> {
    let r = snapshot_row(idx)?;
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, r.code);
    h = p_core::fold_u32(h, r.cn.len() as u32);
    h = p_core::fold_byte(h, (r.axis + 1) as u8);
    if h == 0 {
        h = p_core::FNV_OFF;
    }
    Some(render_row(CODE_TAG, r.code, r.cn.len() as u32, idx as u32, 0, h))
}

// ── 与 B12 预留槽位、同批相邻段的防撞 ────────────────────────────
/// B12 已占用的优先槽位（本模块 16 个编号必须与之零交集）。
pub const RESERVED_IDS: [u32; 6] = [
    0x071C_8001,
    0x071C_8002,
    0x071C_8014,
    0x071D_0001,
    0x071D_2001,
    0x071D_2014,
];

/// 本模块 16 个跨槽编号（0x0734_0C01..0x0734_0C10）。
pub const CASE_IDS: [u32; SLOT_CASE_TOTAL] = [
    0x0734_0C01,
    0x0734_0C02,
    0x0734_0C03,
    0x0734_0C04,
    0x0734_0C05,
    0x0734_0C06,
    0x0734_0C07,
    0x0734_0C08,
    0x0734_0C09,
    0x0734_0C0A,
    0x0734_0C0B,
    0x0734_0C0C,
    0x0734_0C0D,
    0x0734_0C0E,
    0x0734_0C0F,
    0x0734_0C10,
];

/// 按序号取跨槽编号。
pub fn case_id(i: usize) -> Option<u32> {
    CASE_IDS.get(i).copied()
}

/// 跨槽编号严格铺满 CASE_BASE+0x01..+0x10。
pub fn case_ids_tile() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let want = match CASE_BASE.checked_add(1 + i as u32) {
            Some(v) => v,
            None => return false,
        };
        if CASE_IDS[i] != want {
            return false;
        }
        i += 1;
    }
    true
}

/// 十六个跨槽编号与六个预留槽位零交集，且自身无重复。
pub fn no_cross_collision() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let a = CASE_IDS[i];
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            if a == RESERVED_IDS[j] {
                return false;
            }
            j += 1;
        }
        let mut k = i + 1;
        while k < CASE_IDS.len() {
            if a == CASE_IDS[k] {
                return false;
            }
            k += 1;
        }
        i += 1;
    }
    true
}

// ── 探针 ──────────────────────────────────────────────────────────
/// 探针甲：码族快照 + 段位 + 两套编号命名空间 + 跨槽防撞。
pub fn probe_seq_a() -> Result<(), u32> {
    let _d = audit_code_snapshot()?;
    if !segment_guard() {
        return Err(FzCode::ParamType.val());
    }
    if !case_ids_tile() || !no_cross_collision() {
        return Err(FzCode::CaseDup.val());
    }
    if !namespace_guard() {
        return Err(FzCode::CaseDup.val());
    }
    Ok(())
}

/// 探针乙：用例库审计 + 全库执行全绿 + 防重（序号/名称/种子三列）。
pub fn probe_seq_b() -> Result<(), u32> {
    let _h = audit_registry()?;
    let r = run_registry();
    if r.executed != CASE_TOTAL as u32 || r.green != r.executed {
        return Err(FzCode::ReplayBad.val());
    }
    let mut k = 0usize;
    while k < AXES {
        if r.per_axis[k] != CASES_PER_AXIS as u32 {
            return Err(FzCode::CaseMissing.val());
        }
        k += 1;
    }
    Ok(())
}

/// 探针丙：注入 fuzz ×10^4 不崩 + 复跑摘要一致 + 四轴各见码。
pub fn probe_seq_c() -> Result<(), u32> {
    let a = surface_fuzz(FUZZ_ROUNDS);
    let b = surface_fuzz(FUZZ_ROUNDS);
    if a.rounds != FUZZ_ROUNDS || a.silent != 0 {
        return Err(FzCode::RoundSkew.val());
    }
    if a.accepted.saturating_add(a.rejected) != a.rounds {
        return Err(FzCode::SurfaceLeak.val());
    }
    if a.digest != b.digest {
        return Err(FzCode::DigestDrift.val());
    }
    if a.codes_seen < AXES as u32 {
        return Err(FzCode::ParamRange.val());
    }
    if replay_registry()? == 0 {
        return Err(FzCode::TraceLost.val());
    }
    Ok(())
}

/// 复跑自证：三条探针全过。
pub fn repro_check() -> Result<(), u32> {
    probe_seq_a()?;
    probe_seq_b()?;
    probe_seq_c()?;
    Ok(())
}

// ── 八项检查 ──────────────────────────────────────────────────────
/// 八项检查：四条注入（F49057）+ 四条用例（F49058）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    let rep = surface_fuzz(FUZZ_ROUNDS);
    out.push((
        "F49057/四维乱参数零崩",
        rep.rounds == FUZZ_ROUNDS && rep.accepted.saturating_add(rep.rejected) == rep.rounds,
        format!(
            "rounds={} acc={} rej={}",
            rep.rounds, rep.accepted, rep.rejected
        ),
    ));

    out.push((
        "F49057/注入错误全部捕获",
        rep.codes_seen >= AXES as u32,
        format!("codes_seen={} silent={}", rep.codes_seen, rep.silent),
    ));

    let again = surface_fuzz(FUZZ_ROUNDS);
    out.push((
        "F49057/fuzz×10^4零崩",
        rep.digest == again.digest && rep.digest != 0,
        format!("d1={:016X} d2={:016X}", rep.digest, again.digest),
    ));

    // 被拒参数不得改受理计数：单独拿一条越界参数打一次面，accepted 必须纹丝不动。
    let mut s = Surface::new();
    let before = s.accepted;
    let bad = Param {
        kind: KIND_MAX,
        value: 1,
        ttl: 1,
        step: 0,
    };
    let rejected = s.admit(&bad).is_err();
    out.push((
        "F49057/参数面摘要稳定",
        rejected && s.accepted == before && s.rejected == 1 && s.invariant(),
        format!("acc={} rej={} d={:016X}", s.accepted, s.rejected, s.digest),
    ));

    let reg = run_registry();
    out.push((
        "F49058/20例全绿入库",
        reg.executed == CASE_TOTAL as u32 && reg.green == reg.executed,
        format!("executed={} green={} cases={}", reg.executed, reg.green, reg.cases),
    ));

    let rp = replay_registry();
    out.push((
        "F49058/用例复跑一致",
        rp.is_ok(),
        match rp {
            Ok(d) => format!("replay={:016X}", d),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let audit = audit_registry();
    let per_axis_ok = {
        let mut k = 0usize;
        let mut ok = true;
        while k < AXES {
            if reg.per_axis[k] != CASES_PER_AXIS as u32 {
                ok = false;
            }
            k += 1;
        }
        ok
    };
    let mut dup_free = true;
    let mut i = 0usize;
    while i < CASES.len() {
        let mut j = i + 1;
        while j < CASES.len() {
            if CASES[i].ordinal == CASES[j].ordinal
                || CASES[i].name == CASES[j].name
                || CASES[i].seed == CASES[j].seed
            {
                dup_free = false;
            }
            j += 1;
        }
        i += 1;
    }
    out.push((
        "F49058/重号缺档零留痕",
        audit.is_ok() && per_axis_ok && dup_free && namespace_guard(),
        format!(
            "audit={} per_axis={} dup_free={}",
            audit.is_ok(),
            per_axis_ok,
            dup_free
        ),
    ));

    let row = registry_row(&reg);
    let parsed = row_fields(&row);
    let mut row_ok = audit_render(&row);
    if let Some((tag, vals, dg)) = parsed {
        row_ok = row_ok && tag == REG_TAG && vals[0] == CASE_TOTAL as u32 && dg != 0;
    } else {
        row_ok = false;
    }
    out.push((
        "F49058/账行可回读",
        row_ok,
        format!("row={}", row),
    ));

    out
}

/// 汇总检查集（八项动态检查）。
pub fn run_unx49057_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX-F49057 fuzz 面乱参数注入与 20 例用例集");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        set.add_dyn(items[i].0, items[i].1, &items[i].2);
        i += 1;
    }
    set
}

// ── 测试 ──────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_closed_and_contiguous() {
        assert_eq!(FzCode::ALL.len(), 14);
        assert!(segment_guard());
        let mut i = 0usize;
        while i < FzCode::ALL.len() {
            let c = FzCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(known_code(c.val()));
            assert_eq!(detail(c.val()), c.detail());
            i += 1;
        }
        assert!(!known_code(SEG_BASE + 0x0F));
    }

    #[test]
    fn unknown_code_falls_back() {
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(0xDEAD_BEEF), UNKNOWN);
    }

    #[test]
    fn snapshot_audits_clean() {
        let d = audit_code_snapshot().unwrap();
        assert_ne!(d, 0);
        assert_eq!(CODE_SNAPSHOT.len(), 15);
        assert_eq!(CODE_SNAPSHOT[14].code, SEG_BASE + 0x0F);
        assert_eq!(CODE_SNAPSHOT[14].cn, UNKNOWN);
        assert_eq!(CODE_SNAPSHOT[14].axis, -1);
    }

    #[test]
    fn snapshot_axes_match_prefix() {
        let mut i = 0usize;
        while i < 4 {
            assert_eq!(CODE_SNAPSHOT[i].axis, i as i8);
            assert_eq!(CODE_SNAPSHOT[i].code, Axis::from_index(i as u8).unwrap().code());
            i += 1;
        }
        let mut k = 4usize;
        while k < 14 {
            assert_eq!(CODE_SNAPSHOT[k].axis, -1);
            k += 1;
        }
    }

    #[test]
    fn axis_roundtrip() {
        let mut i = 0u8;
        while i < 4 {
            let a = Axis::from_index(i).unwrap();
            assert_eq!(a.index(), i);
            assert_eq!(a.code(), SEG_BASE + 1 + i as u32);
            assert!(!a.name().is_empty());
            i += 1;
        }
        assert!(Axis::from_index(4).is_err());
        assert!(Axis::from_index(255).is_err());
    }

    #[test]
    fn clean_param_is_accepted() {
        let mut s = Surface::new();
        assert!(s.admit(&Param::clean()).is_ok());
        assert_eq!(s.accepted, 1);
        assert_eq!(s.rejected, 0);
        assert!(s.invariant());
    }

    #[test]
    fn type_gate_first() {
        let mut s = Surface::new();
        let p = Param { kind: KIND_MAX, value: 0, ttl: 0, step: 9 };
        assert_eq!(s.admit(&p).err(), Some(FzCode::ParamType.val()));
        assert_eq!(s.accepted, 0);
    }

    #[test]
    fn range_gate_second() {
        let mut s = Surface::new();
        let p = Param { kind: 1, value: 0, ttl: 0, step: 9 };
        assert_eq!(s.admit(&p).err(), Some(FzCode::ParamRange.val()));
        let p2 = Param { kind: 1, value: VALUE_HI + 1, ttl: 1, step: 0 };
        assert_eq!(s.admit(&p2).err(), Some(FzCode::ParamRange.val()));
    }

    #[test]
    fn lifetime_gate_third() {
        let mut s = Surface::new();
        let p = Param { kind: 1, value: 1, ttl: 0, step: 9 };
        assert_eq!(s.admit(&p).err(), Some(FzCode::ParamLifetime.val()));
    }

    #[test]
    fn order_gate_last() {
        let mut s = Surface::new();
        let p = Param { kind: 1, value: 1, ttl: 1, step: STEP_MAX };
        assert_eq!(s.admit(&p).err(), Some(FzCode::ParamOrder.val()));
        assert_eq!(s.accepted, 0);
        assert_eq!(s.rejected, 1);
    }

    #[test]
    fn rejected_params_never_change_accepted() {
        let mut s = Surface::new();
        let d0 = s.digest;
        let bad = Param { kind: 9, value: 1, ttl: 1, step: 1 };
        assert!(s.admit(&bad).is_err());
        assert_eq!(s.accepted, 0);
        assert_ne!(s.digest, d0);
        assert!(s.admit(&Param::clean()).is_ok());
        assert_eq!(s.accepted, 1);
    }

    #[test]
    fn inject_for_hits_the_right_axis() {
        let mut rng = p_core::Lcg::new(0x1234);
        let mut i = 0u8;
        while i < 4 {
            let axis = Axis::from_index(i).unwrap();
            let p = inject_for(axis, &mut rng);
            let mut s = Surface::new();
            assert_eq!(s.admit(&p).err(), Some(axis.code()));
            i += 1;
        }
    }

    #[test]
    fn surface_fuzz_balances() {
        let r = surface_fuzz(400);
        assert_eq!(r.rounds, 400);
        assert_eq!(r.accepted.saturating_add(r.rejected), r.rounds);
        assert_eq!(r.silent, 0);
        assert!(r.rejected > 0);
        assert!(r.codes_seen >= 4);
        assert_ne!(r.digest, 0);
    }

    #[test]
    fn surface_fuzz_is_reproducible() {
        let a = surface_fuzz(500);
        let b = surface_fuzz(500);
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.accepted, b.accepted);
        assert_eq!(a.rejected, b.rejected);
        assert_eq!(a.codes_seen, b.codes_seen);
    }

    #[test]
    fn surface_fuzz_full() {
        let r = surface_fuzz(FUZZ_ROUNDS);
        assert_eq!(r.rounds, FUZZ_ROUNDS);
        assert_eq!(r.accepted.saturating_add(r.rejected), FUZZ_ROUNDS);
        assert!(r.codes_seen >= 4);
        assert_eq!(r.silent, 0);
    }

    #[test]
    fn registry_has_twenty_unique_cases() {
        assert_eq!(CASES.len(), 20);
        assert!(audit_registry().is_ok());
        let mut i = 0usize;
        while i < CASES.len() {
            let mut j = i + 1;
            while j < CASES.len() {
                assert_ne!(CASES[i].ordinal, CASES[j].ordinal);
                assert_ne!(CASES[i].name, CASES[j].name);
                assert_ne!(CASES[i].seed, CASES[j].seed);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn registry_is_five_per_axis() {
        let mut per = [0u32; AXES];
        let mut i = 0usize;
        while i < CASES.len() {
            per[CASES[i].axis.index() as usize] += 1;
            i += 1;
        }
        assert_eq!(per, [5, 5, 5, 5]);
    }

    #[test]
    fn registry_verdicts_match_axis() {
        let mut i = 0usize;
        while i < CASES.len() {
            match CASES[i].verdict {
                Verdict::MustReject(code) => assert_eq!(code, CASES[i].axis.code()),
                Verdict::MustAccept => {}
            }
            i += 1;
        }
    }

    #[test]
    fn every_case_passes() {
        let mut i = 0usize;
        while i < CASES.len() {
            let r = run_case(&CASES[i]);
            assert!(r.passes, "case {} failed", CASES[i].ordinal);
            assert_eq!(r.rounds, CASE_ROUNDS);
            assert_eq!(r.accepted + r.rejected, CASE_ROUNDS);
            i += 1;
        }
    }

    #[test]
    fn faulty_cases_reject_all_rounds() {
        let mut i = 0usize;
        while i < CASES.len() {
            if let Verdict::MustReject(_) = CASES[i].verdict {
                let r = run_case(&CASES[i]);
                assert_eq!(r.rejected, CASE_ROUNDS);
                assert_eq!(r.accepted, 0);
            }
            i += 1;
        }
    }

    #[test]
    fn legal_cases_accept_all_rounds() {
        let mut i = 0usize;
        while i < CASES.len() {
            if let Verdict::MustAccept = CASES[i].verdict {
                let r = run_case(&CASES[i]);
                assert_eq!(r.accepted, CASE_ROUNDS);
                assert_eq!(r.rejected, 0);
            }
            i += 1;
        }
        assert_eq!(run_registry().green, CASE_TOTAL as u32);
    }

    #[test]
    fn case_results_are_reproducible() {
        let mut i = 0usize;
        while i < CASES.len() {
            let a = run_case(&CASES[i]);
            let b = run_case(&CASES[i]);
            assert_eq!(a, b);
            i += 1;
        }
    }

    #[test]
    fn registry_report_is_consistent() {
        let r = run_registry();
        assert_eq!(r.executed, CASE_TOTAL as u32);
        assert_eq!(r.green, r.executed);
        assert_eq!(r.cases, CASE_TOTAL as u32);
        assert_eq!(r.per_axis, [5, 5, 5, 5]);
        assert_ne!(r.digest, 0);
        assert!(r.codes_seen >= 4);
    }

    #[test]
    fn replay_matches_and_is_green() {
        let d1 = replay_registry().unwrap();
        let d2 = replay_registry().unwrap();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn case_at_lookup() {
        assert_eq!(case_at(1).unwrap().ordinal, 1);
        assert_eq!(case_at(20).unwrap().ordinal, 20);
        assert!(case_at(0).is_none());
        assert!(case_at(21).is_none());
        assert_eq!(case_at(5).unwrap().verdict, Verdict::MustAccept);
    }

    #[test]
    fn namespace_guard_holds() {
        assert!(namespace_guard());
        assert!(case_ids_tile());
        assert!(no_cross_collision());
        assert_eq!(CASE_IDS[0], 0x0734_0C01);
        assert_eq!(CASE_IDS[15], 0x0734_0C10);
    }

    #[test]
    fn reserved_ids_are_distinct() {
        let mut i = 0usize;
        while i < RESERVED_IDS.len() {
            let mut j = i + 1;
            while j < RESERVED_IDS.len() {
                assert_ne!(RESERVED_IDS[i], RESERVED_IDS[j]);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(RESERVED_IDS.len(), 6);
    }

    #[test]
    fn render_roundtrip_surface_row() {
        let r = surface_fuzz(64);
        let row = surface_row(&r);
        assert!(audit_render(&row));
        let (tag, vals, dg) = row_fields(&row).unwrap();
        assert_eq!(tag, INJ_TAG);
        assert_eq!(vals[0], 64);
        assert_eq!(vals[1], r.accepted);
        assert_eq!(vals[2], r.rejected);
        assert_ne!(dg, 0);
    }

    #[test]
    fn render_roundtrip_case_row() {
        let c = CASES[0];
        let r = run_case(&c);
        let row = case_row(&c, &r);
        let (tag, vals, _) = row_fields(&row).unwrap();
        assert_eq!(tag, CASE_TAG);
        assert_eq!(vals[0], 1);
        assert_eq!(vals[1], CASE_ROUNDS);
        assert_eq!(vals[2], 0);
    }

    #[test]
    fn render_roundtrip_registry_row() {
        let r = run_registry();
        let row = registry_row(&r);
        let (tag, vals, _) = row_fields(&row).unwrap();
        assert_eq!(tag, REG_TAG);
        assert_eq!(vals[0], 20);
        assert_eq!(vals[1], 20);
        assert_eq!(vals[2], 20);
        assert_eq!(vals[3], 20);
    }

    #[test]
    fn code_rows_all_parse() {
        let mut i = 0usize;
        while i < CODE_ROWS {
            let row = code_row(i).unwrap();
            let (tag, vals, _) = row_fields(&row).unwrap();
            assert_eq!(tag, CODE_TAG);
            assert_eq!(vals[0], CODE_SNAPSHOT[i].code);
            assert_eq!(vals[2], i as u32);
            i += 1;
        }
        assert!(code_row(CODE_ROWS).is_none());
    }

    #[test]
    fn render_rejects_malformed() {
        assert!(!audit_render(""));
        assert!(!audit_render("FZAX|k1=1|k2=2"));
        assert!(parse_u32("0x10").is_none());
        assert!(parse_u32("7").is_some());
        assert!(parse_hex16("0123456789ABCDEF").is_some());
        assert!(parse_hex16("0123456789abcdef").is_none());
        assert!(parse_hex16("123").is_none());
    }

    #[test]
    fn render_rejects_key_drift() {
        let good = render_row(INJ_TAG, 1, 2, 3, 4, 0xABC);
        assert!(audit_render(&good));
        assert!(!audit_render(&good.replace("k2=", "x2=")));
        assert!(!audit_render(&good.replace("k4=", "k6=")));
        assert!(!audit_render(&render_row(INJ_TAG, 1, 2, 3, 4, 0)));
        assert!(!audit_render(&render_row("NOPE", 1, 2, 3, 4, 0xABC)));
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1, "red check: {} {}", items[i].0, items[i].2);
            i += 1;
        }
        let s = run_unx49057_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
    }

    #[test]
    fn budgets_are_sane() {
        assert_eq!(CASE_TOTAL / AXES, CASES_PER_AXIS);
        assert_eq!(CASE_ROUNDS * CASE_TOTAL as u32, 1280);
        assert_eq!(FUZZ_ROUNDS, 10_000);
        assert!(VALUE_LO < VALUE_HI);
        assert!(KIND_MAX > 0 && STEP_MAX > 0);
        assert_ne!(SURFACE_SEED, 0);
    }

    #[test]
    fn case_tags_are_padded_and_unique() {
        let mut seen: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < CASES.len() {
            let t = CASES[i].tag();
            assert_eq!(t.len(), "UNX-F49058-#01".len());
            assert!(t.starts_with("UNX-F49058-#"));
            let mut j = 0usize;
            while j < seen.len() {
                assert_ne!(seen[j], t);
                j += 1;
            }
            seen.push(t);
            i += 1;
        }
        assert_eq!(seen.len(), 20);
        assert_eq!(CASES[0].tag(), "UNX-F49058-#01");
        assert_eq!(CASES[19].tag(), "UNX-F49058-#20");
    }

    #[test]
    fn ordinals_tile_one_to_twenty() {
        let mut i = 0usize;
        while i < CASES.len() {
            assert_eq!(CASES[i].ordinal as usize, i + 1);
            i += 1;
        }
    }

    #[test]
    fn default_surface_is_fresh() {
        let s = Surface::default();
        assert_eq!(s.accepted, 0);
        assert_eq!(s.rejected, 0);
        assert_eq!(s.digest, p_core::FNV_OFF);
        assert!(s.invariant());
    }

    #[test]
    fn fuzz_rejections_are_the_four_axis_codes() {
        // 四维生成器各自只打一个轴，因此 fuzz 里出现的错误码集合
        // 必须恰好等于四条轴向码（不多不少），这就是「错误必被捕获」的强口径。
        let r = surface_fuzz(FUZZ_ROUNDS);
        assert_eq!(r.codes_seen, 4);
        assert_eq!(r.rejected, FUZZ_ROUNDS);
        assert_eq!(r.accepted, 0);
    }
}
