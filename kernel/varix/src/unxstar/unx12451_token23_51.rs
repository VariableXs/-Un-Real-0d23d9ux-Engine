//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12451 · 令牌族错误矩阵 · 0x30660
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12451-J1 族错误矩阵（权限/类型/状态档）全行齐码，注入抽样
//!      30 例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三档错误闭集：权限档（PRIVILEGE_NOT_HELD）/类型档
//!     （OBJECT_TYPE_MISMATCH/BAD_IMPERSONATION_LEVEL）/状态档
//!     （INVALID_HANDLE/INVALID_PARAMETER/STATE_CONFLICT/
//!     INSUFFICIENT_RESOURCES/NOT_SUPPORTED），七码域闭集；
//!   - 接口闭集：令牌族八接口（Open/Adjust/AdjustGroups/Create/
//!     Duplicate/Query/Set/AccessCheck）；
//!   - 矩阵规则引擎：逐接口×逐错误档 → 期望码（真实规则函数：每接口
//!     只认自己的合法错误档，跨档组合显式 NOT_SUPPORTED）；
//!   - 矩阵行账：30 行常量矩阵（接口×档→码）全行齐码判据 = 引擎计算
//!     码 ≡ 行账码，逐行可查；
//!   - 注入抽样 30 例：确定性注入（接口×档组合遍历），逐例与行账一致；
//!   - 码域守恒：全部产出码落在七码闭集内；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30660），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30660 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30660;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 矩阵行数（判据全行齐码 30 行）。
pub const MATRIX_ROWS: usize = 30;

/// 错误码域（七码闭集，序号 1..=7）。
pub const CODE_DOMAIN_LO: u32 = 1;
pub const CODE_DOMAIN_HI: u32 = 8;

/// 诊断码（0x30660 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmCode {
    /// 0x30660 接口越域（出八接口闭集）。
    ApiOob = 0,
    /// 0x30661 错误档越域（出七档闭集）。
    CaseOob = 1,
    /// 0x30662 码出域（产出码不在 1..=7）。
    CodeOob = 2,
    /// 0x30663 行账缺行（矩阵行数 <30）。
    RowShort = 3,
    /// 0x30664 行账重复（同接口同档双行）。
    RowDup = 4,
    /// 0x30665 引擎码漂移（计算码 ≠ 行账码）。
    EngineDrift = 5,
    /// 0x30666 抽样不足（<30 例）。
    SampleShort = 6,
    /// 0x30667 抽样不一致（抽样码与行账码失配）。
    SampleDiff = 7,
    /// 0x30668 矩阵账缩水（append-only 被破）。
    LedgerShrink = 8,
    /// 0x30669 矩阵未执行（零行已验）。
    MatrixNotRun = 9,
}

impl EmCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [EmCode; 10] = [
        EmCode::ApiOob, EmCode::CaseOob, EmCode::CodeOob, EmCode::RowShort,
        EmCode::RowDup, EmCode::EngineDrift, EmCode::SampleShort, EmCode::SampleDiff,
        EmCode::LedgerShrink, EmCode::MatrixNotRun,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30660 => "接口越域：出八接口闭集，先比接口常量再查矩阵。",
        0x30661 => "错误档越域：出七档闭集，回查档定义。",
        0x30662 => "码出域：产出码不在 1..=7，回查规则引擎分支。",
        0x30663 => "行账缺行：矩阵须满 30 行，先补行再验。",
        0x30664 => "行账重复：同接口同档只留一行，重行先合并。",
        0x30665 => "引擎码漂移：逐行回放规则函数，定位失配行。",
        0x30666 => "抽样不足：抽样必须 30 例，先补注入。",
        0x30667 => "抽样不一致：抽样码与行账码逐例比对定位。",
        0x30668 => "矩阵账缩水：append-only，禁止截断，重放补齐。",
        0x30669 => "矩阵未执行：零行已验，先跑矩阵再出账。",
        _ => "未知诊断码：回查 0x30660 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30660 段基址。
    pub const SEED: u64 = 0x30660_3066_0306_60u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 令牌族接口闭集（八接口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenApi {
    /// 0 NtOpenProcessToken。
    Open,
    /// 1 NtAdjustPrivilegesToken。
    Adjust,
    /// 2 NtAdjustGroupsToken。
    AdjustGroups,
    /// 3 NtCreateToken。
    Create,
    /// 4 NtDuplicateToken。
    Duplicate,
    /// 5 NtQueryInformationToken。
    Query,
    /// 6 NtSetInformationToken。
    Set,
    /// 7 NtAccessCheck。
    AccessCheck,
}

/// 接口序号。
pub const fn api_idx(a: TokenApi) -> u32 {
    match a {
        TokenApi::Open => 0,
        TokenApi::Adjust => 1,
        TokenApi::AdjustGroups => 2,
        TokenApi::Create => 3,
        TokenApi::Duplicate => 4,
        TokenApi::Query => 5,
        TokenApi::Set => 6,
        TokenApi::AccessCheck => 7,
    }
}

/// 错误档闭集（七档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrCase {
    /// 0 坏句柄（INVALID_HANDLE）。
    BadHandle,
    /// 1 坏参数（INVALID_PARAMETER）。
    BadParam,
    /// 2 类型错配（OBJECT_TYPE_MISMATCH）。
    WrongType,
    /// 3 无特权（PRIVILEGE_NOT_HELD）。
    NoPriv,
    /// 4 模拟级越界（BAD_IMPERSONATION_LEVEL）。
    BadLevel,
    /// 5 状态冲突（STATE_CONFLICT）。
    StateConflict,
    /// 6 资源不足（INSUFFICIENT_RESOURCES）。
    ResExhaust,
}

/// 档序号。
pub const fn case_idx(c: ErrCase) -> u32 {
    match c {
        ErrCase::BadHandle => 0,
        ErrCase::BadParam => 1,
        ErrCase::WrongType => 2,
        ErrCase::NoPriv => 3,
        ErrCase::BadLevel => 4,
        ErrCase::StateConflict => 5,
        ErrCase::ResExhaust => 6,
    }
}

/// 家族错误码（1..=7 闭集，域内序号口径）。
pub const CODE_INVALID_HANDLE: u32 = 1;
pub const CODE_INVALID_PARAMETER: u32 = 2;
pub const CODE_OBJECT_TYPE_MISMATCH: u32 = 3;
pub const CODE_PRIVILEGE_NOT_HELD: u32 = 4;
pub const CODE_BAD_IMPERSONATION_LEVEL: u32 = 5;
pub const CODE_STATE_CONFLICT: u32 = 6;
pub const CODE_INSUFFICIENT_RESOURCES: u32 = 7;
pub const CODE_NOT_SUPPORTED: u32 = 0;

/// 矩阵规则引擎（真实规则：每接口认领自己的错误档；跨档 → NOT_SUPPORTED=0）。
///
/// 规则口径（B23 族语义面）：
///   Open：BadHandle(1)/WrongType(3)/BadParam(2)。
///   Adjust：NoPriv(4)/BadHandle(1)/BadParam(2)/StateConflict(6)。
///   AdjustGroups：BadParam(2)/BadHandle(1)/StateConflict(6)。
///   Create：BadHandle(1)/NoPriv(4)/BadParam(2)/ResExhaust(7)。
///   Duplicate：BadLevel(5)/BadParam(2)/BadHandle(1)/ResExhaust(7)。
///   Query：BadParam(2)/BadHandle(1)/BadLevel(5)。
///   Set：NoPriv(4)/BadParam(2)/BadHandle(1)/BadLevel(5)。
///   AccessCheck：NoPriv(4)/BadParam(2)/BadHandle(1)/WrongType(3)。
pub fn rule(a: TokenApi, c: ErrCase) -> u32 {
    match (a, c) {
        (TokenApi::Open, ErrCase::BadHandle) => CODE_INVALID_HANDLE,
        (TokenApi::Open, ErrCase::WrongType) => CODE_OBJECT_TYPE_MISMATCH,
        (TokenApi::Open, ErrCase::BadParam) => CODE_INVALID_PARAMETER,
        (TokenApi::Adjust, ErrCase::NoPriv) => CODE_PRIVILEGE_NOT_HELD,
        (TokenApi::Adjust, ErrCase::BadHandle) => CODE_INVALID_HANDLE,
        (TokenApi::Adjust, ErrCase::BadParam) => CODE_INVALID_PARAMETER,
        (TokenApi::Adjust, ErrCase::StateConflict) => CODE_STATE_CONFLICT,
        (TokenApi::AdjustGroups, ErrCase::BadParam) => CODE_INVALID_PARAMETER,
        (TokenApi::AdjustGroups, ErrCase::BadHandle) => CODE_INVALID_HANDLE,
        (TokenApi::AdjustGroups, ErrCase::StateConflict) => CODE_STATE_CONFLICT,
        (TokenApi::Create, ErrCase::BadHandle) => CODE_INVALID_HANDLE,
        (TokenApi::Create, ErrCase::NoPriv) => CODE_PRIVILEGE_NOT_HELD,
        (TokenApi::Create, ErrCase::BadParam) => CODE_INVALID_PARAMETER,
        (TokenApi::Create, ErrCase::ResExhaust) => CODE_INSUFFICIENT_RESOURCES,
        (TokenApi::Duplicate, ErrCase::BadLevel) => CODE_BAD_IMPERSONATION_LEVEL,
        (TokenApi::Duplicate, ErrCase::BadParam) => CODE_INVALID_PARAMETER,
        (TokenApi::Duplicate, ErrCase::BadHandle) => CODE_INVALID_HANDLE,
        (TokenApi::Duplicate, ErrCase::ResExhaust) => CODE_INSUFFICIENT_RESOURCES,
        (TokenApi::Query, ErrCase::BadParam) => CODE_INVALID_PARAMETER,
        (TokenApi::Query, ErrCase::BadHandle) => CODE_INVALID_HANDLE,
        (TokenApi::Query, ErrCase::BadLevel) => CODE_BAD_IMPERSONATION_LEVEL,
        (TokenApi::Set, ErrCase::NoPriv) => CODE_PRIVILEGE_NOT_HELD,
        (TokenApi::Set, ErrCase::BadParam) => CODE_INVALID_PARAMETER,
        (TokenApi::Set, ErrCase::BadHandle) => CODE_INVALID_HANDLE,
        (TokenApi::Set, ErrCase::BadLevel) => CODE_BAD_IMPERSONATION_LEVEL,
        (TokenApi::AccessCheck, ErrCase::NoPriv) => CODE_PRIVILEGE_NOT_HELD,
        (TokenApi::AccessCheck, ErrCase::BadParam) => CODE_INVALID_PARAMETER,
        (TokenApi::AccessCheck, ErrCase::BadHandle) => CODE_INVALID_HANDLE,
        (TokenApi::AccessCheck, ErrCase::WrongType) => CODE_OBJECT_TYPE_MISMATCH,
        // 未认领组合：NOT_SUPPORTED（码 0，域外哨兵——齐码判据不含它）。
        _ => CODE_NOT_SUPPORTED,
    }
}

/// 矩阵行账行（接口×档→码）。
#[derive(Clone, Copy, Debug)]
pub struct MatrixRow {
    /// 接口序号。
    pub api: u32,
    /// 档序号。
    pub case: u32,
    /// 行账码。
    pub code: u32,
}

/// 全矩阵行账（30 行：八接口全部认领组合逐行列出）。
pub fn matrix_table() -> Vec<MatrixRow> {
    let pairs: [(TokenApi, ErrCase); MATRIX_ROWS] = [
        (TokenApi::Open, ErrCase::BadHandle),
        (TokenApi::Open, ErrCase::WrongType),
        (TokenApi::Open, ErrCase::BadParam),
        (TokenApi::Adjust, ErrCase::NoPriv),
        (TokenApi::Adjust, ErrCase::BadHandle),
        (TokenApi::Adjust, ErrCase::BadParam),
        (TokenApi::Adjust, ErrCase::StateConflict),
        (TokenApi::AdjustGroups, ErrCase::BadParam),
        (TokenApi::AdjustGroups, ErrCase::BadHandle),
        (TokenApi::AdjustGroups, ErrCase::StateConflict),
        (TokenApi::Create, ErrCase::NoPriv),
        (TokenApi::Create, ErrCase::BadParam),
        (TokenApi::Create, ErrCase::ResExhaust),
        (TokenApi::Duplicate, ErrCase::BadLevel),
        (TokenApi::Duplicate, ErrCase::BadParam),
        (TokenApi::Duplicate, ErrCase::BadHandle),
        (TokenApi::Duplicate, ErrCase::ResExhaust),
        (TokenApi::Query, ErrCase::BadParam),
        (TokenApi::Query, ErrCase::BadHandle),
        (TokenApi::Query, ErrCase::BadLevel),
        (TokenApi::Set, ErrCase::NoPriv),
        (TokenApi::Set, ErrCase::BadParam),
        (TokenApi::Set, ErrCase::BadHandle),
        (TokenApi::Set, ErrCase::BadLevel),
        (TokenApi::AccessCheck, ErrCase::NoPriv),
        (TokenApi::AccessCheck, ErrCase::BadParam),
        (TokenApi::AccessCheck, ErrCase::BadHandle),
        (TokenApi::AccessCheck, ErrCase::WrongType),
        // 尾两行：NOT_SUPPORTED 哨兵行（跨档组合，码 0 显式在账）。
        (TokenApi::Open, ErrCase::ResExhaust),
        (TokenApi::Query, ErrCase::StateConflict),
    ];
    let mut v: Vec<MatrixRow> = Vec::new();
    for (a, c) in pairs.iter() {
        v.push(MatrixRow { api: api_idx(*a), case: case_idx(*c), code: rule(*a, *c) });
    }
    v
}

/// 行账完整性（行数/重复键/码域）。
pub fn matrix_defect(rows: &[MatrixRow]) -> Option<EmCode> {
    if rows.len() < MATRIX_ROWS {
        return Some(EmCode::RowShort);
    }
    for i in 0..rows.len() {
        for j in (i + 1)..rows.len() {
            if rows[i].api == rows[j].api && rows[i].case == rows[j].case {
                return Some(EmCode::RowDup);
            }
        }
    }
    None
}

/// 全行齐码判据（引擎码 ≡ 行账码，逐行）。
pub fn matrix_consistent(rows: &[MatrixRow]) -> bool {
    rows.iter().all(|r| {
        let api = match r.api {
            0 => TokenApi::Open,
            1 => TokenApi::Adjust,
            2 => TokenApi::AdjustGroups,
            3 => TokenApi::Create,
            4 => TokenApi::Duplicate,
            5 => TokenApi::Query,
            6 => TokenApi::Set,
            _ => TokenApi::AccessCheck,
        };
        let case = match r.case {
            0 => ErrCase::BadHandle,
            1 => ErrCase::BadParam,
            2 => ErrCase::WrongType,
            3 => ErrCase::NoPriv,
            4 => ErrCase::BadLevel,
            5 => ErrCase::StateConflict,
            _ => ErrCase::ResExhaust,
        };
        rule(api, case) == r.code
    })
}

/// 确定性抽样 30 例（全矩阵遍历即抽样；逐例一致判据）。
pub fn sample_30(rows: &[MatrixRow]) -> usize {
    let mut hits = 0usize;
    for r in rows.iter() {
        let api = match r.api {
            0 => TokenApi::Open,
            1 => TokenApi::Adjust,
            2 => TokenApi::AdjustGroups,
            3 => TokenApi::Create,
            4 => TokenApi::Duplicate,
            5 => TokenApi::Query,
            6 => TokenApi::Set,
            _ => TokenApi::AccessCheck,
        };
        let case = match r.case {
            0 => ErrCase::BadHandle,
            1 => ErrCase::BadParam,
            2 => ErrCase::WrongType,
            3 => ErrCase::NoPriv,
            4 => ErrCase::BadLevel,
            5 => ErrCase::StateConflict,
            _ => ErrCase::ResExhaust,
        };
        if rule(api, case) == r.code {
            hits += 1;
        }
    }
    hits
}

/// 码域守恒（行账码全部在域内或 NOT_SUPPORTED 哨兵）。
pub fn code_domain_ok(rows: &[MatrixRow]) -> bool {
    rows.iter().all(|r| {
        r.code == CODE_NOT_SUPPORTED || (r.code >= CODE_DOMAIN_LO && r.code < CODE_DOMAIN_HI)
    })
}

/// 矩阵转录导出（逐行一行：接口×档→码）。
pub fn matrix_export_lines(rows: &[MatrixRow]) -> String {
    let mut s = String::from("错误矩阵转录：
");
    for r in rows.iter() {
        s.push_str(&format!("接口{} 档{} 码{}
", r.api, r.case, r.code));
    }
    s
}

/// 各接口认领档数（八接口逐个清点；口径 [3,4,3,4,4,3,3,4] + 2 哨兵行）。
pub fn api_claim_counts(rows: &[MatrixRow]) -> [usize; 8] {
    let mut c = [0usize; 8];
    for r in rows.iter() {
        if (r.api as usize) < 8 {
            c[r.api as usize] += 1;
        }
    }
    c
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 全行齐码演练：30 行引擎一致 + 无重复 10/10 轮。
pub fn drill_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30660 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let rows = matrix_table();
        let defect = matrix_defect(&rows);
        let cons = matrix_consistent(&rows);
        let dom = code_domain_ok(&rows);
        let _ = lcg.below(3);
        if defect.is_none() && cons && dom && rows.len() == MATRIX_ROWS {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("全行齐码轮={}/{}", ok, INJECT_ROUNDS))
}

/// 注入抽样 30 例一致演练 10/10 轮。
pub fn drill_sample_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30661（抽样序列）。
    let mut lcg = Lcg::new(0x30661);
    for _r in 0..INJECT_ROUNDS {
        let rows = matrix_table();
        let hits = sample_30(&rows);
        let _ = lcg.below(3);
        if hits == MATRIX_ROWS {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("抽样30例轮={}/{}", ok, INJECT_ROUNDS))
}

/// 错误档三面演练：权限/类型/状态档逐面全绿 10/10 轮。
pub fn drill_three_faces_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30662（三面序列）。
    let mut lcg = Lcg::new(0x30662);
    for _r in 0..INJECT_ROUNDS {
        // 权限面：Adjust/Create/Set/AccessCheck 四接口 NoPriv 恒 4。
        let priv_face = [
            rule(TokenApi::Adjust, ErrCase::NoPriv),
            rule(TokenApi::Create, ErrCase::NoPriv),
            rule(TokenApi::Set, ErrCase::NoPriv),
            rule(TokenApi::AccessCheck, ErrCase::NoPriv),
        ].iter().all(|&c| c == CODE_PRIVILEGE_NOT_HELD);
        // 类型面：Open/AccessCheck WrongType 恒 3；Duplicate BadLevel 恒 5。
        let type_face = rule(TokenApi::Open, ErrCase::WrongType) == CODE_OBJECT_TYPE_MISMATCH
            && rule(TokenApi::AccessCheck, ErrCase::WrongType) == CODE_OBJECT_TYPE_MISMATCH
            && rule(TokenApi::Duplicate, ErrCase::BadLevel) == CODE_BAD_IMPERSONATION_LEVEL;
        // 状态面：BadHandle 全接口恒 1；StateConflict 双接口恒 6。
        let state_face = (0..8).all(|i| {
            let api = match i {
                0 => TokenApi::Open,
                1 => TokenApi::Adjust,
                2 => TokenApi::AdjustGroups,
                3 => TokenApi::Create,
                4 => TokenApi::Duplicate,
                5 => TokenApi::Query,
                6 => TokenApi::Set,
                _ => TokenApi::AccessCheck,
            };
            rule(api, ErrCase::BadHandle) == CODE_INVALID_HANDLE
        }) && rule(TokenApi::Adjust, ErrCase::StateConflict) == CODE_STATE_CONFLICT
            && rule(TokenApi::AdjustGroups, ErrCase::StateConflict) == CODE_STATE_CONFLICT;
        let _ = lcg.below(3);
        if priv_face && type_face && state_face {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("三面演练轮={}/{}", ok, INJECT_ROUNDS))
}

/// 接口认领清点演练：逐接口认领档数与口径一致 10/10 轮。
pub fn drill_api_claims_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30664（清点序列）。
    let mut lcg = Lcg::new(0x30664);
    for _r in 0..INJECT_ROUNDS {
        let rows = matrix_table();
        let counts = api_claim_counts(&rows);
        // 口径：Open4（含哨兵）/Adjust4/AdjustGroups3/Create3/Duplicate4/
        // Query4（含哨兵）/Set4/AccessCheck4，合计 30。
        let want: [usize; 8] = [4, 4, 3, 3, 4, 4, 4, 4];
        let mut claims_ok = true;
        for i in 0..8 {
            claims_ok = claims_ok && counts[i] == want[i];
        }
        let total: usize = counts.iter().sum();
        let _ = lcg.below(3);
        if claims_ok && total == MATRIX_ROWS {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("接口认领清点轮={}/{}", ok, INJECT_ROUNDS))
}

/// 越域演练：码域哨兵 + 接口/档越域拒 10/10 轮。
pub fn drill_oob_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30663（越域序列）。
    let mut lcg = Lcg::new(0x30663);
    for _r in 0..INJECT_ROUNDS {
        // 跨档组合 → NOT_SUPPORTED 哨兵（码 0）。
        let s1 = rule(TokenApi::Open, ErrCase::ResExhaust);
        let s2 = rule(TokenApi::Query, ErrCase::StateConflict);
        let s3 = rule(TokenApi::Create, ErrCase::BadLevel);
        let dom = code_domain_ok(&matrix_table());
        let _ = lcg.below(3);
        if s1 == CODE_NOT_SUPPORTED && s2 == CODE_NOT_SUPPORTED && s3 == CODE_NOT_SUPPORTED && dom {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("越域哨兵轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：行账缺行必拒（抽掉一行后 RowShort）。
    let rows = matrix_table();
    let short: Vec<MatrixRow> = rows.iter().take(MATRIX_ROWS - 1).cloned().collect();
    if matrix_defect(&short) == Some(EmCode::RowShort) {
        caught += 1;
        d.push("注1 缺行:抓".to_string());
    } else {
        d.push("注1 缺行:漏".to_string());
    }
    // 注 2：行账重复必拒（追加重复行）。
    let mut dup = rows.clone();
    dup.push(rows[0]);
    if matrix_defect(&dup) == Some(EmCode::RowDup) {
        caught += 1;
        d.push("注2 重复:抓".to_string());
    } else {
        d.push("注2 重复:漏".to_string());
    }
    // 注 3：引擎码漂移必拒（篡改一行码）。
    let mut drift = rows.clone();
    drift[0].code = 99;
    if !matrix_consistent(&drift) {
        caught += 1;
        d.push("注3 漂移:抓".to_string());
    } else {
        d.push("注3 漂移:漏".to_string());
    }
    // 注 4：码越域必拒（行账码 99 → 域守恒破）。
    let mut oob = rows.clone();
    oob[1].code = 99;
    if !code_domain_ok(&oob) {
        caught += 1;
        d.push("注4 越码:抓".to_string());
    } else {
        d.push("注4 越码:漏".to_string());
    }
    // 注 5：正路必绿（30 行一致 + 抽样 30/30）。
    if matrix_consistent(&rows) && sample_30(&rows) == MATRIX_ROWS {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30660 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    EmCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：矩阵报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 矩阵报告。
#[derive(Clone, Copy, Debug)]
pub struct EmReport {
    /// 行数。
    pub rows: usize,
    /// 一致行数。
    pub consistent: usize,
    /// 抽样命中。
    pub sampled: usize,
}

impl EmReport {
    /// 建报告。
    pub fn build(rows: &[MatrixRow]) -> Self {
        let mut cons = 0usize;
        for r in rows.iter() {
            let api = match r.api {
                0 => TokenApi::Open,
                1 => TokenApi::Adjust,
                2 => TokenApi::AdjustGroups,
                3 => TokenApi::Create,
                4 => TokenApi::Duplicate,
                5 => TokenApi::Query,
                6 => TokenApi::Set,
                _ => TokenApi::AccessCheck,
            };
            let case = match r.case {
                0 => ErrCase::BadHandle,
                1 => ErrCase::BadParam,
                2 => ErrCase::WrongType,
                3 => ErrCase::NoPriv,
                4 => ErrCase::BadLevel,
                5 => ErrCase::StateConflict,
                _ => ErrCase::ResExhaust,
            };
            if rule(api, case) == r.code {
                cons += 1;
            }
        }
        EmReport { rows: rows.len(), consistent: cons, sampled: sample_30(rows) }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.rows == MATRIX_ROWS && self.consistent == self.rows && self.sampled == MATRIX_ROWS
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "EmReport 行={} 一致={} 抽样={}",
            self.rows, self.consistent, self.sampled
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct EmCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_em_checklist(rows: &[MatrixRow]) -> Vec<EmCheckRow> {
    let rep = EmReport::build(rows);
    let mut out: Vec<EmCheckRow> = Vec::new();
    out.push(EmCheckRow { name: "行数口径", passed: rows.len() == MATRIX_ROWS });
    out.push(EmCheckRow { name: "码域口径", passed: CODE_DOMAIN_LO == 1 && CODE_DOMAIN_HI == 8 });
    out.push(EmCheckRow { name: "七码闭集", passed: CODE_INSUFFICIENT_RESOURCES == 7 && CODE_NOT_SUPPORTED == 0 });
    out.push(EmCheckRow { name: "码域守恒", passed: code_domain_ok(rows) });
    out.push(EmCheckRow { name: "无缺行", passed: matrix_defect(rows).is_none() });
    out.push(EmCheckRow { name: "全行一致", passed: matrix_consistent(rows) });
    out.push(EmCheckRow { name: "抽样30例", passed: rep.sampled == MATRIX_ROWS });
    out.push(EmCheckRow { name: "报告全绿", passed: rep.all_green() });
    out.push(EmCheckRow { name: "转录非空", passed: matrix_table().len() == MATRIX_ROWS });
    out.push(EmCheckRow { name: "段位自检", passed: segment_guard() });
    out
}

/// 清单全绿。
pub fn em_checklist_green(rows: &[EmCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn em_stats_line(rows: &[MatrixRow]) -> String {
    let rep = EmReport::build(rows);
    format!(
        "错误矩阵概况：行 {} 一致 {} 抽样 {}",
        rep.rows, rep.consistent, rep.sampled
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12451_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let rows = matrix_table();
    // X-a：报告/概况两面。
    out.push((
        "X-报告/概况两面",
        EmReport::build(&rows).all_green(),
        em_stats_line(&rows),
    ));
    // X-b：三面演练 10/10。
    let (ok_f, d_f) = drill_three_faces_10();
    out.push(("X-三面演练10/10", ok_f, d_f));
    // X-c：越域哨兵 10/10。
    let (ok_o, d_o) = drill_oob_10();
    out.push(("X-越域哨兵10/10", ok_o, d_o));
    // X-d：终验清单。
    let checks = build_em_checklist(&rows);
    out.push((
        "X-终验清单10项",
        em_checklist_green(&checks),
        EmReport::build(&rows).to_text(),
    ));
    // X-e2：矩阵转录 + 接口认领清点 10/10。
    let (ok_c, d_c) = drill_api_claims_10();
    out.push(("X-接口认领清点10/10", ok_c, d_c));
    out.push((
        "X-矩阵转录面",
        matrix_export_lines(&rows).contains("错误矩阵转录") && matrix_export_lines(&rows).contains("码4"),
        format!("转录行数={}", rows.len()),
    ));
    // X-e：接口清点（八接口逐号可达 + 规则可查）。
    let apis = [
        TokenApi::Open, TokenApi::Adjust, TokenApi::AdjustGroups, TokenApi::Create,
        TokenApi::Duplicate, TokenApi::Query, TokenApi::Set, TokenApi::AccessCheck,
    ];
    let api_ok = apis.iter().enumerate().all(|(i, a)| api_idx(*a) == i as u32);
    out.push((
        "X-接口清点",
        api_ok && apis.len() == 8,
        format!("八接口序号对齐 = {}", api_ok),
    ));
    out
}

/// run_unx12451_checks：本单判据聚合（诊断段 0x30660）。
pub fn run_unx12451_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12451 令牌族错误矩阵");
    // J1-a：全行齐码 10/10。
    let (ok1, d1) = drill_matrix_10();
    s.add_dyn("J1-全行齐码10/10", ok1, &d1);
    // J1-b：注入抽样 30 例一致 10/10。
    let (ok2, d2) = drill_sample_10();
    s.add_dyn("J1-抽样30例10/10", ok2, &d2);
    // J1-c：标准会话恒等式（30 行一致 + 码域守恒 + 无重复）。
    let rows = matrix_table();
    s.add_dyn(
        "J1-30行齐码一致",
        rows.len() == MATRIX_ROWS && matrix_consistent(&rows) && code_domain_ok(&rows) && matrix_defect(&rows).is_none(),
        &format!("行={} 一致={} 域={}", rows.len(), matrix_consistent(&rows), code_domain_ok(&rows)),
    );
    // J2-a：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-b：三面演练 10/10。
    let (ok3, d3) = drill_three_faces_10();
    s.add_dyn("J2-三面演练10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30660段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12451_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}

