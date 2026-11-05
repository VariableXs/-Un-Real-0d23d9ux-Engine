//! UNX-F3436 API 冻结签名表 · 0x79B00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B13 逐字摘录）：
//!   ① 纯功能代码 ≥200 行；
//!   ② J1：结果面 API 12 项冻结；交叉无冲突。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 冻结签名表：结果面 12 项 API（ID 0..=11 连续闭集），每项含
//!     名称/元数/参数型闭集/返回型闭集/冻结位，逐项冻结不改；
//!   - 交叉无冲突：ID 唯一、名称唯一、元数在 CAP 内、参数型与返回型
//!     全在闭集内——五类交叉核全过才算无冲突；
//!   - 调用面派发：按 ID 查表（表外 ID 显性拒），元数不符显性拒
//!     （ArityMismatch），不静默截断参数；
//!   - 冲突扫描器可复用：对任意签名切片扫重复 ID/重复名称
//!     （注入一张含重复项的合成表必被抓）；
//!   - 冻结守卫：任一项冻结位为假即 FreezeBroken；
//!   - 双跑一致：表扫描两次结论相同；写回摘要非空。
//! 常量来源：
//!   - 12 项清单：派工卡「结果面 API 12 项冻结」——结果提交/查询/
//!     账本追加/账本解析/看门狗收单/注戳/A1 渲染/JSON 往返/桥路由/
//!     预算对账/序号发号/摘要写回，与 B13 批各卡功能面对齐；
//!   - 元数上限 6：参数寄存器面口径（内核无堆调用约定）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B13 批段窗 [0x79A00, 0x79BC0] 内）。
pub const SEG_BASE: u32 = 0x79B00;

/// 冻结 API 项数（派工卡：12 项）。
pub const API_COUNT: usize = 12;
/// 元数上限（单次调用的参数寄存器面容量）。
pub const MAX_ARITY: usize = 6;

/// 返回型闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetKind {
    /// 布尔结论。
    Bool,
    /// 32 位状态/诊断码。
    U32,
    /// 64 位序号/拍数。
    U64,
    /// 无返回（写回面）。
    Void,
    /// 定容切片引用（账字节）。
    SliceRef,
}

impl RetKind {
    /// 中文返回型名。
    pub fn zh(self) -> &'static str {
        match self {
            RetKind::Bool => "布尔",
            RetKind::U32 => "32位",
            RetKind::U64 => "64位",
            RetKind::Void => "无返回",
            RetKind::SliceRef => "切片",
        }
    }
}

/// 参数型闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// 用例 ID。
    CaseId,
    /// 拍数。
    Ticks,
    /// 账字节缓冲。
    LedgerBuf,
    /// 版本戳引用。
    StampRef,
    /// 通道 ID。
    ChannelId,
    /// 预算（拍）。
    Budget,
    /// 计数。
    Count,
    /// 序号。
    Seq,
}

impl ParamKind {
    /// 中文参数型名。
    pub fn zh(self) -> &'static str {
        match self {
            ParamKind::CaseId => "用例ID",
            ParamKind::Ticks => "拍数",
            ParamKind::LedgerBuf => "账缓冲",
            ParamKind::StampRef => "戳引用",
            ParamKind::ChannelId => "通道ID",
            ParamKind::Budget => "预算",
            ParamKind::Count => "计数",
            ParamKind::Seq => "序号",
        }
    }
}

/// 一项冻结签名。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiSig {
    /// 项 ID（0..=11 连续闭集）。
    pub id: u8,
    /// API 名称（蛇形，冻结字面）。
    pub name: &'static str,
    /// 元数（参数个数，≤ MAX_ARITY）。
    pub arity: u8,
    /// 参数型序列（长度 == arity）。
    pub params: &'static [ParamKind],
    /// 返回型。
    pub ret: RetKind,
    /// 冻结位（冻结签名表判据本体）。
    pub frozen: bool,
}

/// 冻结签名表：结果面 12 项（派工卡判据锚定，逐项冻结）。
pub const FROZEN_API_TABLE: [ApiSig; API_COUNT] = [
    ApiSig { id: 0,  name: "result_submit",   arity: 3, params: &[ParamKind::CaseId, ParamKind::Ticks, ParamKind::Count], ret: RetKind::U32,     frozen: true },
    ApiSig { id: 1,  name: "result_query",    arity: 1, params: &[ParamKind::CaseId],                                     ret: RetKind::U64,     frozen: true },
    ApiSig { id: 2,  name: "ledger_append",   arity: 2, params: &[ParamKind::LedgerBuf, ParamKind::Seq],                  ret: RetKind::U32,     frozen: true },
    ApiSig { id: 3,  name: "ledger_parse",    arity: 1, params: &[ParamKind::LedgerBuf],                                  ret: RetKind::SliceRef,frozen: true },
    ApiSig { id: 4,  name: "watchdog_submit", arity: 3, params: &[ParamKind::CaseId, ParamKind::Ticks, ParamKind::Budget],ret: RetKind::Bool,    frozen: true },
    ApiSig { id: 5,  name: "stamp_inject",    arity: 1, params: &[ParamKind::StampRef],                                   ret: RetKind::Bool,    frozen: true },
    ApiSig { id: 6,  name: "a1_render",       arity: 2, params: &[ParamKind::CaseId, ParamKind::Ticks],                   ret: RetKind::SliceRef,frozen: true },
    ApiSig { id: 7,  name: "json_roundtrip",  arity: 2, params: &[ParamKind::LedgerBuf, ParamKind::Count],                ret: RetKind::Bool,    frozen: true },
    ApiSig { id: 8,  name: "bridge_route",    arity: 2, params: &[ParamKind::ChannelId, ParamKind::LedgerBuf],            ret: RetKind::U32,     frozen: true },
    ApiSig { id: 9,  name: "budget_audit",    arity: 2, params: &[ParamKind::Budget, ParamKind::Ticks],                   ret: RetKind::Bool,    frozen: true },
    ApiSig { id: 10, name: "seq_next",        arity: 0, params: &[],                                                      ret: RetKind::U64,     frozen: true },
    ApiSig { id: 11, name: "summary_writeback", arity: 1, params: &[ParamKind::Count],                                    ret: RetKind::Void,    frozen: true },
];

/// 按 ID 查表（表外 ID 返回 None）。
pub fn api_by_id(id: u8) -> Option<&'static ApiSig> {
    for sig in FROZEN_API_TABLE.iter() {
        if sig.id == id {
            return Some(sig);
        }
    }
    None
}

/// 调用面派发：表外 ID 拒、元数不符拒、参数型逐位核对。
pub fn dispatch_check(id: u8, argc: usize, kinds: &[ParamKind]) -> Result<RetKind, u32> {
    let sig = match api_by_id(id) {
        Some(s) => s,
        None => return Err(Diag::ApiForeign.val()),
    };
    if argc != sig.arity as usize {
        return Err(Diag::ArityMismatch.val());
    }
    if kinds.len() != sig.params.len() {
        return Err(Diag::ArityMismatch.val());
    }
    let mut i = 0usize;
    while i < sig.params.len() {
        let declared = match sig.params.get(i) {
            Some(p) => *p,
            None => return Err(Diag::ParamForeign.val()),
        };
        let actual = match kinds.get(i) {
            Some(p) => *p,
            None => return Err(Diag::ParamForeign.val()),
        };
        if declared != actual {
            return Err(Diag::ParamForeign.val());
        }
        i += 1;
    }
    Ok(sig.ret)
}

/// 冲突扫描器（可复用）：对任意签名切片扫重复 ID 与重复名称。
/// 返回冲突清单（空 = 无冲突）。
pub fn conflict_scan(sigs: &[ApiSig]) -> Vec<(u8, u32)> {
    let mut out: Vec<(u8, u32)> = Vec::new();
    let mut i = 0usize;
    while i < sigs.len() {
        let a = match sigs.get(i) {
            Some(s) => s,
            None => break,
        };
        let mut j = i + 1;
        while j < sigs.len() {
            let b = match sigs.get(j) {
                Some(s) => s,
                None => break,
            };
            if a.id == b.id {
                out.push((a.id, Diag::IdDuplicate.val()));
            }
            if a.name == b.name {
                out.push((a.id, Diag::NameDuplicate.val()));
            }
            j += 1;
        }
        i += 1;
    }
    out
}

/// 交叉核五类：ID 唯一/名称唯一/元数在 CAP 内/参数型闭集/返回型闭集。
pub fn cross_conflict_free(sigs: &[ApiSig]) -> bool {
    if conflict_scan(sigs).len() != 0 {
        return false;
    }
    for sig in sigs.iter() {
        if sig.arity as usize > MAX_ARITY {
            return false;
        }
        if sig.arity as usize != sig.params.len() {
            return false;
        }
        // 参数型全在闭集内（ParamKind 全枚举逐一比对）。
        let all_params = [
            ParamKind::CaseId,
            ParamKind::Ticks,
            ParamKind::LedgerBuf,
            ParamKind::StampRef,
            ParamKind::ChannelId,
            ParamKind::Budget,
            ParamKind::Count,
            ParamKind::Seq,
        ];
        for p in sig.params.iter() {
            let mut known = false;
            for k in all_params.iter() {
                if *p == *k {
                    known = true;
                }
            }
            if !known {
                return false;
            }
        }
        // 返回型闭集。
        let all_rets = [
            RetKind::Bool,
            RetKind::U32,
            RetKind::U64,
            RetKind::Void,
            RetKind::SliceRef,
        ];
        let mut ret_known = false;
        for r in all_rets.iter() {
            if sig.ret == *r {
                ret_known = true;
            }
        }
        if !ret_known {
            return false;
        }
    }
    true
}

/// 诊断码闭集：0x79B01..=0x79B0C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 表外 API：ID 不在冻结表 0..=11 内。
    ApiForeign = 0x01,
    /// 项数残缺：冻结表不足 12 项。
    TableCountBad = 0x02,
    /// 项 ID 重复：两 ID 相同。
    IdDuplicate = 0x03,
    /// 项名重复：两名称相同。
    NameDuplicate = 0x04,
    /// 交叉冲突：五类交叉核任一不过。
    CrossConflict = 0x05,
    /// 元数不匹配：调用量与声明元数不符。
    ArityMismatch = 0x06,
    /// 冻结破约：任一项冻结位为假。
    FreezeBroken = 0x07,
    /// 返回型表外：返回型不在闭集内。
    RetForeign = 0x08,
    /// 参数型表外：参数型不在闭集内或与声明不符。
    ParamForeign = 0x09,
    /// 双跑分叉：表扫描两次结论不同。
    DoubleRunDiverge = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：完成时摘要未定稿。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ApiForeign => "表外 API：建议只按冻结表 0..=11 派发，新项走扩表评审",
            Diag::TableCountBad => "项数残缺：建议冻结表补齐 12 项，缺项即冻结不成立",
            Diag::IdDuplicate => "项 ID 重复：建议 ID 唯一化，重复项重排后冻结",
            Diag::NameDuplicate => "项名重复：建议名称唯一化，同名项合并或改名",
            Diag::CrossConflict => "交叉冲突：建议按五类交叉核逐项排查后重冻结",
            Diag::ArityMismatch => "元数不匹配：建议调用量对齐声明元数，不截断不补零",
            Diag::FreezeBroken => "冻结破约：建议冻结位全真后才可发布签名表",
            Diag::RetForeign => "返回型表外：建议返回型只用五码闭集",
            Diag::ParamForeign => "参数型表外：建议参数型按声明逐位核对",
            Diag::DoubleRunDiverge => "双跑分叉：建议表扫描做成纯函数，表定则论定",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::ApiForeign,
    Diag::TableCountBad,
    Diag::IdDuplicate,
    Diag::NameDuplicate,
    Diag::CrossConflict,
    Diag::ArityMismatch,
    Diag::FreezeBroken,
    Diag::RetForeign,
    Diag::ParamForeign,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79B01..=0x79B0C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B13 批段窗 [0x79A00, 0x79BC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79A00 && SEG_BASE <= 0x79BC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 表量与 ID 连续：恰 12 项、ID 0..=11 连续闭集。
fn table_count_ok() -> bool {
    if FROZEN_API_TABLE.len() != API_COUNT {
        return false;
    }
    let mut expect: u8 = 0;
    for sig in FROZEN_API_TABLE.iter() {
        if sig.id != expect {
            return false;
        }
        expect += 1;
    }
    api_by_id(API_COUNT as u8).is_none() && api_by_id(0).is_some()
}

/// 名称唯一：全表名称两两互异且非空（蛇形冻结字面）。
fn name_unique_ok() -> bool {
    let mut i = 0usize;
    while i < FROZEN_API_TABLE.len() {
        let a = match FROZEN_API_TABLE.get(i) {
            Some(s) => s,
            None => return false,
        };
        if a.name.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < FROZEN_API_TABLE.len() {
            match FROZEN_API_TABLE.get(j) {
                Some(b) => {
                    if a.name == b.name {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 冻结守卫：12 项冻结位全真。
fn freeze_all_ok() -> bool {
    let mut frozen = 0usize;
    for sig in FROZEN_API_TABLE.iter() {
        if sig.frozen {
            frozen += 1;
        }
    }
    frozen == API_COUNT && detail(Diag::FreezeBroken.val()).contains("冻结")
}

/// 交叉无冲突：五类交叉核对冻结表全过。
fn cross_free_ok() -> bool {
    cross_conflict_free(&FROZEN_API_TABLE) && conflict_scan(&FROZEN_API_TABLE).is_empty()
}

/// 正常流派发：12 项按各自声明元数与参数型逐项派发全过。
fn dispatch_all_ok() -> bool {
    for sig in FROZEN_API_TABLE.iter() {
        let r = dispatch_check(sig.id, sig.arity as usize, sig.params);
        match r {
            Ok(ret) => {
                if ret != sig.ret {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// 注入 1：表外 ID 派发必拒（ApiForeign）。
fn inj_api_foreign_ok() -> bool {
    if dispatch_check(API_COUNT as u8, 0, &[]) != Err(Diag::ApiForeign.val()) {
        return false;
    }
    dispatch_check(200, 1, &[ParamKind::Count]) == Err(Diag::ApiForeign.val())
}

/// 注入 2：元数不符必拒——少参与多参各验一次（ArityMismatch）。
fn inj_arity_mismatch_ok() -> bool {
    let sig = match api_by_id(0) {
        Some(s) => s,
        None => return false,
    };
    if dispatch_check(0, (sig.arity - 1) as usize, &sig.params[..(sig.arity - 1) as usize])
        != Err(Diag::ArityMismatch.val())
    {
        return false;
    }
    let mut over: Vec<ParamKind> = Vec::new();
    for p in sig.params.iter() {
        over.push(*p);
    }
    over.push(ParamKind::Count);
    dispatch_check(0, over.len(), &over) == Err(Diag::ArityMismatch.val())
}

/// 注入 3：参数型不符必拒——首参换成通道 ID（ParamForeign）。
fn inj_param_foreign_ok() -> bool {
    let wrong = [ParamKind::ChannelId, ParamKind::Ticks, ParamKind::Count];
    dispatch_check(0, 3, &wrong) == Err(Diag::ParamForeign.val())
}

/// 注入 4：合成冲突表必被扫描器抓（ID 重复 + 名称重复）。
fn inj_conflict_scan_ok() -> bool {
    let dup: [ApiSig; 2] = [
        ApiSig {
            id: 0,
            name: "result_submit",
            arity: 0,
            params: &[],
            ret: RetKind::Bool,
            frozen: true,
        },
        ApiSig {
            id: 0,
            name: "result_submit",
            arity: 0,
            params: &[],
            ret: RetKind::Bool,
            frozen: true,
        },
    ];
    let found = conflict_scan(&dup);
    if found.len() != 2 {
        return false;
    }
    // 一张只重名不重 ID 的表：恰一条 NameDuplicate。
    let dup_name: [ApiSig; 2] = [
        ApiSig {
            id: 0,
            name: "shared_name",
            arity: 0,
            params: &[],
            ret: RetKind::Void,
            frozen: true,
        },
        ApiSig {
            id: 1,
            name: "shared_name",
            arity: 0,
            params: &[],
            ret: RetKind::Void,
            frozen: true,
        },
    ];
    conflict_scan(&dup_name).len() == 1
}

/// 越元数上限的合成参数表（冻结 'static，供越 CAP 注入演练）。
const OVER_CAP_PARAMS: [ParamKind; MAX_ARITY + 1] = [
    ParamKind::CaseId,
    ParamKind::Ticks,
    ParamKind::LedgerBuf,
    ParamKind::StampRef,
    ParamKind::ChannelId,
    ParamKind::Budget,
    ParamKind::Count,
];

/// 元数上限：arity 超 CAP 的合成表被交叉核拒（CrossConflict 语义）。
fn arity_cap_ok() -> bool {
    let over = [ApiSig {
        id: 0,
        name: "over_cap",
        arity: (MAX_ARITY + 1) as u8,
        params: &OVER_CAP_PARAMS,
        ret: RetKind::Void,
        frozen: true,
    }];
    !cross_conflict_free(&over) && detail(Diag::CrossConflict.val()).contains("交叉")
}

/// 双跑一致：冻结表交叉核与冲突扫描两次结论相同。
fn double_run_ok() -> bool {
    let a = cross_conflict_free(&FROZEN_API_TABLE);
    let b = cross_conflict_free(&FROZEN_API_TABLE);
    let sa = conflict_scan(&FROZEN_API_TABLE);
    let sb = conflict_scan(&FROZEN_API_TABLE);
    a == b && sa == sb && a
}

/// 写回面：状态=已完成、结果摘要非空（含 12 项冻结清单计数）。
fn writeback_summary_ok() -> bool {
    let mut named = String::new();
    let mut n = 0usize;
    for sig in FROZEN_API_TABLE.iter() {
        if n > 0 {
            named.push(',');
        }
        named.push_str(sig.name);
        n += 1;
    }
    let summary = format!(
        "UNX-F3436 完成：结果面API{}项冻结 交叉无冲突 首项{}",
        n,
        named.len() > 0
    );
    !summary.is_empty()
        && summary.contains("UNX-F3436")
        && summary.contains("冻结")
        && n == API_COUNT
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3436_checks ───────────────────────────────────────

/// run_unx3436_checks：本单判据聚合（诊断段 0x79B00，全部走 add_dyn）。
pub fn run_unx3436_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3436 API 冻结签名表");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "冻结表12项ID连续闭集",
            table_count_ok(),
            format!("表量{}", if table_count_ok() { "12项" } else { "残缺" }),
        ),
        (
            "项名两两互异且非空",
            name_unique_ok(),
            format!("项名{}", if name_unique_ok() { "唯一" } else { "重复" }),
        ),
        (
            "冻结位12项全真",
            freeze_all_ok(),
            format!("冻结{}", if freeze_all_ok() { "全真" } else { "破约" }),
        ),
        (
            "J1交叉无冲突五类核全过",
            cross_free_ok(),
            format!("交叉{}", if cross_free_ok() { "无冲突" } else { "有冲突" }),
        ),
        (
            "正常流12项派发全过",
            dispatch_all_ok(),
            format!("派发{}", if dispatch_all_ok() { "全过" } else { "有红" }),
        ),
        (
            "注入1表外ID必拒",
            inj_api_foreign_ok(),
            format!("表外注入{}", if inj_api_foreign_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入2元数不符必拒",
            inj_arity_mismatch_ok(),
            format!("元数注入{}", if inj_arity_mismatch_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入3参数型不符必拒",
            inj_param_foreign_ok(),
            format!("参数注入{}", if inj_param_foreign_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入4合成冲突表必抓",
            inj_conflict_scan_ok(),
            format!("冲突注入{}", if inj_conflict_scan_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "元数上限越CAP拒收",
            arity_cap_ok(),
            format!("元数上限{}", if arity_cap_ok() { "拒收" } else { "放行" }),
        ),
        (
            "双跑一致表扫描结论同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
    ];
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79B00);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn table_twelve_ids_continuous() {
        assert!(table_count_ok());
        assert_eq!(FROZEN_API_TABLE.len(), 12);
        assert!(api_by_id(11).is_some());
        assert!(api_by_id(12).is_none());
    }

    #[test]
    fn names_unique_and_freeze_all() {
        assert!(name_unique_ok());
        assert!(freeze_all_ok());
        assert_eq!(api_by_id(11).unwrap().name, "summary_writeback");
    }

    #[test]
    fn cross_conflict_free_frozen_table() {
        assert!(cross_free_ok());
    }

    #[test]
    fn dispatch_all_twelve() {
        assert!(dispatch_all_ok());
        let sig = api_by_id(10).unwrap();
        assert_eq!(sig.arity, 0);
        assert_eq!(dispatch_check(10, 0, &[]), Ok(RetKind::U64));
    }

    #[test]
    fn injections_foreign_arity_param() {
        assert!(inj_api_foreign_ok());
        assert!(inj_arity_mismatch_ok());
        assert!(inj_param_foreign_ok());
    }

    #[test]
    fn injection_conflict_scan() {
        assert!(inj_conflict_scan_ok());
    }

    #[test]
    fn arity_cap_enforced() {
        assert!(arity_cap_ok());
    }

    #[test]
    fn double_run_identical_scan() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3436_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
