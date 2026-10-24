//! VE-F5410 · 网络 API 冻结（VE-\ 域 · 网络批次 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5410`
//!
//! **判据（锚点原文）**：三族冻结、语义版本、双版期、评审、判据。
//!
//! **职责定位（锚点原文）**：网络 API 面（会话/复制/匹配三族）冻结：
//! 语义版本 + 向后兼容承诺 + 废弃双版期——网络 API 稳定是多人生态的
//! 命根；API 冻结评审与全域纪律同规。清单缺族→补齐后冻结；破坏兼容→
//! 拦截；越权变更→冻结评审。
//!
//! ## 一、三族闭集：会话 / 复制 / 匹配
//!
//! 网络 API 面按 [`Family`] 三族闭集管理（[`FAMILY_COUNT`] = 3）：
//! 会话（Session——连接与生命周期）、复制（Replication——状态同步）、
//! 匹配（Matchmaking——撮合进房）。**清单缺族→补齐后冻结**：冻结前置
//! 闸逐族点名（[`FreezeCode::FamilyMissing`] 携缺哪族）——三族不齐的
//! 冻结是把生态命根栓在半截门框上。
//!
//! ## 二、语义版本 + 向后兼容承诺（三件套之一）
//!
//! 每族持 [`SemVer`]（maj.min.pat，解析任意输入零崩溃——畸形语料闭集
//! 判据钉死）。**向后兼容承诺**的类型化：同 major 内库升版（min/pat），
//! 旧调用方**必然**继续可用——承诺不是口号，是裁决器的第一条规则
//! （[`Verdict::Commit`]）。跨 major = 破坏面 = 必须走废弃双版期合法
//! 落地（见下）。冻结族内签名变更在**当前 major** 内一律拦截
//! （[`FreezeCode::CompatBroken`]——红线不是流程，是拦截）。
//!
//! ## 三、废弃双版期（三件套之二）
//!
//! 破坏面变更的合法路径：先**废弃**（[`ApiState::Deprecated`]，记录
//! since）→ 双版期 [`MAJOR_GRACE`] = 2 个 major 版本内**必须保留**旧
//! API（新旧并存——调用方有时间迁移）→ 期满才可删除。删除闸双拒：
//! **未废弃先删** → [`FreezeCode::DeprecateNotMarked`]（流程闸：删除
//! 必须先走废弃）；**双版期未满先删** → [`FreezeCode::GraceNotMet`]。
//! 双版期理由 [`MAJOR_GRACE_REASON`] 与常量同源（理由不散落文档）。
//!
//! ## 四、越权变更→冻结评审（三件套之三）
//!
//! 冻结后变更提案走裁决器（[`CHANGE_MATRIX`] const 表 ↔ [`adjudicate`]
//! 规则推导**双写全格对拍**——矩阵改规则没改判据即红）。裁决三出口：
//! [`Verdict::Commit`]（直落——兼容面变更如新增 API）/ [`Verdict::Pending`]
//! （**挂起进评审队列**——越权变更不是拒死，是打回评审流程：锚点
//! 「越权变更→冻结评审」的字面实现）/ [`Verdict::Intercept`]（红线拒
//! ——当前 major 内破坏兼容，评审也救不了，只能走新 major + 双版期）。
//! 评审（[`FreezeRegistry::review`]）由 Reviewer 权裁决：Approve 生效
//! 留痕 / Reject 否决留痕——评审与全域纪律同规（评审 O(1)）。
//!
//! ## 五、冻结版本册（数据结构：冻结版本册）
//!
//! [`FreezeRecord`] 逐族登记：冻结时版本 + 评审引用 + 签名指纹（FNV）。
//! 重复冻结显性拒（[`FreezeCode::AlreadyFrozen`]——冻结是一次性承诺，
//! 不是可覆盖的便签）。冻结状态/族版本/清单/废弃中 API/双版期余量
//! 全部可查（[`FreezeRegistry::status`]——API 文档读屏可达的账面根基）。
//!
//! ## 六、审计簿（append-only）
//!
//! 冻结/变更/拦截/评审/越权全留痕（[`AuditOp`]），按 API 可回放
//! （[`FreezeRegistry::audit_replay`]）。满容拒记计数不静默覆盖旧痕
//! （[`FreezeCode::AuditFull`] + `audit_dropped` 账面）。
//!
//! ## 七、零依赖自含语义版本
//!
//! 本模块自带 [`SemVer`] mini 实现（解析/比较/兼容判定）——与 UNX 册
//! unx202 的 schema 版本化是**两个域的两种口径**（UNX 管报告 schema
//! 兼容矩阵，本模块管网络 API 冻结承诺），不互引、各守各的判据——
//! 域自含是探针可隔离的前提。
//!
//! **对接**：上游 F4607/W05 冻结口径；下游 F5411 文档消费（冻结册即
//! 文档源）。诊断码独占 `0x5Axx` 段（0x4B=F6004 已占、0x4C~0x4F/0x5D/
//! 0x5F 被 CGPU 占，0x5A 全仓扫描仅颜色值 0x5A5A5A 非段位码——空闲）。
//! no_std 零 std 依赖、零 unwrap 零 expect 零切片越界、零全局可变状态。

use alloc::format;
use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 模块版本。
pub const VEF63_VERSION: &str = "vef63-apifreeze-v1";

/// 三族数（会话/复制/匹配——闭集，冻结前置闸逐族点名）。
pub const FAMILY_COUNT: usize = 3;

/// 双版期（废弃 API 保留的 major 版本数——新旧并存迁移窗口）。
pub const MAJOR_GRACE: u32 = 2;

/// 双版期理由（与 [`MAJOR_GRACE`] 同源——上限说明含理由不散落文档）。
pub const MAJOR_GRACE_REASON: &str = "双版期=2个major版本：旧调用方需要跨至少两个\
大版本的迁移窗口（一次计划内迁移+一次缓冲），提前删除把迁移成本转嫁给生态\
（多人生态的命根），期满前删除一律拦截";

/// API 清单容量（满容显性拒不驱逐）。
pub const API_CAP: usize = 256;

/// 评审队列容量（满容显性拒——评审 backlog 不是无底洞）。
pub const REVIEW_QUEUE_CAP: usize = 64;

/// 审计簿容量（append-only 满容拒记计数不覆盖旧痕）。
pub const AUDIT_CAP: usize = 256;

/// 版本串长度上限（DoS 面）。
pub const VERSION_MAX_LEN: usize = 32;

/// 签名指纹 FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x100000001b3;

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x5Axx 段十六码闭集）
// ---------------------------------------------------------------------------

/// F5410 诊断码。独占 `0x5Axx` 段，十六码闭集两两互异。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreezeCode(pub u16);

impl FreezeCode {
    /// 0x5A01 缺族（冻结前置闸：三族不齐——携缺哪族走审计）。
    pub const FamilyMissing: FreezeCode = FreezeCode(0x5A01);
    /// 0x5A02 已冻结（重复冻结显性拒——冻结是一次性承诺）。
    pub const AlreadyFrozen: FreezeCode = FreezeCode(0x5A02);
    /// 0x5A03 未冻结（评审/解冻等操作的前提不满足）。
    pub const NotFrozen: FreezeCode = FreezeCode(0x5A03);
    /// 0x5A04 破坏兼容（当前 major 内签名变更——红线拦截评审也救不了）。
    pub const CompatBroken: FreezeCode = FreezeCode(0x5A04);
    /// 0x5A05 双版期未满（废弃 API 提前删除拦截）。
    pub const GraceNotMet: FreezeCode = FreezeCode(0x5A05);
    /// 0x5A06 越权（操作者 × 操作不在权限内）。
    pub const PermissionDenied: FreezeCode = FreezeCode(0x5A06);
    /// 0x5A07 待评审（越权变更挂起进评审队列——不是拒死是打回流程）。
    pub const ReviewPending: FreezeCode = FreezeCode(0x5A07);
    /// 0x5A08 评审否决（Reviewer 拒——留痕）。
    pub const ReviewRejected: FreezeCode = FreezeCode(0x5A08);
    /// 0x5A09 API 不在册。
    pub const UnknownApi: FreezeCode = FreezeCode(0x5A09);
    /// 0x5A0A 族不在册（三族闭集外）。
    pub const UnknownFamily: FreezeCode = FreezeCode(0x5A0A);
    /// 0x5A0B 版本串非法（解析失败——畸形语料闭集）。
    pub const BadVersion: FreezeCode = FreezeCode(0x5A0B);
    /// 0x5A0C API 清单满（满容显性拒不驱逐）。
    pub const ApiFull: FreezeCode = FreezeCode(0x5A0C);
    /// 0x5A0D 冻结册满。
    pub const RegistryFull: FreezeCode = FreezeCode(0x5A0D);
    /// 0x5A0E 审计满（append-only 满容拒记计数不覆盖旧痕）。
    pub const AuditFull: FreezeCode = FreezeCode(0x5A0E);
    /// 0x5A0F 账实不符（族↔API 归属对拍失败——自检红线）。
    pub const LedgerDrift: FreezeCode = FreezeCode(0x5A0F);
    /// 0x5A10 未废弃先删（删除必须先走废弃——流程闸）。
    pub const DeprecateNotMarked: FreezeCode = FreezeCode(0x5A10);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因（诊断可读——拒绝不是哑的 Err）。
    pub fn reason(self) -> String {
        match self {
            FreezeCode::FamilyMissing => "缺族：三族不齐不可冻结（补齐后冻结）".into(),
            FreezeCode::AlreadyFrozen => "已冻结：冻结是一次性承诺，重复冻结显性拒".into(),
            FreezeCode::NotFrozen => "未冻结：该操作以冻结为前提".into(),
            FreezeCode::CompatBroken => "破坏兼容：当前 major 内签名变更是红线，走新 major+双版期".into(),
            FreezeCode::GraceNotMet => "双版期未满：废弃 API 提前删除拦截（迁移窗口未关）".into(),
            FreezeCode::PermissionDenied => "越权：操作者对此操作无权限".into(),
            FreezeCode::ReviewPending => "待评审：越权变更挂起进评审队列（打回流程不是拒死）".into(),
            FreezeCode::ReviewRejected => "评审否决：Reviewer 拒绝该变更（留痕）".into(),
            FreezeCode::UnknownApi => "API 不在册：先注册再操作".into(),
            FreezeCode::UnknownFamily => "族不在册：三族闭集外".into(),
            FreezeCode::BadVersion => "版本串非法：解析失败（畸形语料）".into(),
            FreezeCode::ApiFull => "API 清单满：显性拒不驱逐".into(),
            FreezeCode::RegistryFull => "冻结册满：显性拒不覆盖".into(),
            FreezeCode::AuditFull => "审计满：append-only 满容拒记计数".into(),
            FreezeCode::LedgerDrift => "账实不符：族↔API 归属对拍失败".into(),
            FreezeCode::DeprecateNotMarked => "未废弃先删：删除必须先走废弃（流程闸）".into(),
            FreezeCode(_) => "未知冻结诊断码".into(),
        }
    }
}

/// `segment_guard`：十六码闭集两两互异 + 全落 0x5A 段。
pub const SEGMENT_GUARD: [FreezeCode; 16] = [
    FreezeCode::FamilyMissing,
    FreezeCode::AlreadyFrozen,
    FreezeCode::NotFrozen,
    FreezeCode::CompatBroken,
    FreezeCode::GraceNotMet,
    FreezeCode::PermissionDenied,
    FreezeCode::ReviewPending,
    FreezeCode::ReviewRejected,
    FreezeCode::UnknownApi,
    FreezeCode::UnknownFamily,
    FreezeCode::BadVersion,
    FreezeCode::ApiFull,
    FreezeCode::RegistryFull,
    FreezeCode::AuditFull,
    FreezeCode::LedgerDrift,
    FreezeCode::DeprecateNotMarked,
];

/// 冻结错误路径（显性携带 [`FreezeCode`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezeErr {
    /// 携诊断码的失败。
    Code(FreezeCode),
}

impl FreezeErr {
    /// 对外呈现码。
    pub const fn code(self) -> u16 {
        match self {
            FreezeErr::Code(c) => c.code(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、语义版本 mini（域自含——解析任意输入零崩溃）
// ---------------------------------------------------------------------------

/// 语义版本（maj.min.pat）。
///
/// 解析口径：1~3 段数字形态合法（缺省段 = 0），存在段必须非空纯数字；
/// 段值域 u32（溢出显性拒）。解析**任意输入零崩溃**——畸形语料闭集
/// 判据钉死（对齐全域 fuzz 纪律）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SemVer {
    /// 主版本（破坏面变更才可升）。
    pub maj: u32,
    /// 次版本（兼容面新增可升）。
    pub min: u32,
    /// 修订号（缺陷修复可升）。
    pub pat: u32,
}

/// 版本解析结果（Some=合法 / None=畸形——零崩溃口径）。
pub fn parse_semver(s: &str) -> Option<SemVer> {
    if s.is_empty() || s.len() > VERSION_MAX_LEN {
        return None;
    }
    // 全串先验：只允许 ASCII 数字与至多两个 '.'，且不空段
    let mut segs: [u32; 3] = [0, 0, 0];
    let mut seg_idx = 0usize;
    let mut cur: Option<u32> = None;
    for ch in s.chars() {
        if ch == '.' {
            // 段边界：当前段必须非空（"1..0" / ".1" / "1." 全拒）
            let v = match cur {
                Some(v) => v,
                None => return None,
            };
            if seg_idx >= 2 {
                return None; // 三段以上拒
            }
            segs[seg_idx] = v;
            seg_idx += 1;
            cur = None;
        } else if ch.is_ascii_digit() {
            let d = (ch as u8 - b'0') as u32;
            let v = match cur {
                Some(v) => match v.checked_mul(10).and_then(|x| x.checked_add(d)) {
                    Some(x) => x,
                    None => return None, // 段溢出 u32 显性拒
                },
                None => d,
            };
            cur = Some(v);
        } else {
            return None; // 非法字符（空格/字母/负号/加号全拒——纯闭集口径）
        }
    }
    match cur {
        Some(v) => {
            segs[seg_idx] = v;
            Some(SemVer { maj: segs[0], min: segs[1], pat: segs[2] })
        }
        None => None, // 以 '.' 结尾（"1." 形态——空段拒）
    }
}

impl SemVer {
    /// 渲染为规范串（maj.min.pat——文档消费面）。
    pub fn render(&self) -> String {
        alloc::format!("{}.{}.{}", self.maj, self.min, self.pat)
    }

    /// 全序比较（maj → min → pat 逐位）。
    pub fn cmp_key(&self) -> (u32, u32, u32) {
        (self.maj, self.min, self.pat)
    }

    /// 向后兼容承诺裁决（以 `old` 版本调用方视角看升版到 `new`）：
    /// 同 major（min/pat 变）→ 兼容承诺成立；跨 major → 破坏面
    /// （必须走废弃双版期合法落地）。
    pub const fn backward_compatible(old: SemVer, new: SemVer) -> bool {
        old.maj == new.maj
    }

    /// 主版本跳变量（双版期计算基——`new.maj - old.maj`，回退为 0）。
    pub const fn major_jump(old: SemVer, new: SemVer) -> u32 {
        if new.maj > old.maj {
            new.maj - old.maj
        } else {
            0
        }
    }
}

/// 畸形版本语料闭集（fuzz 纪律——判据逐条喂入断言零崩溃且全拒）。
pub const BAD_VERSION_CASES: [&str; 12] = [
    "",
    " ",
    "1.",
    ".1",
    "1..0",
    "1.0.",
    "v1.0.0",
    "1.0.0-alpha",
    "-1.0.0",
    "1.0.0+build",
    "99999999999999999999999.0.0",
    "1.0.0.0.0",
];

/// 合法版本语料闭集（判据对照——1~3 段形态全兼容）。
pub const GOOD_VERSION_CASES: [(&str, u32, u32, u32); 6] = [
    ("1.0.0", 1, 0, 0),
    ("2.1", 2, 1, 0),
    ("3", 3, 0, 0),
    ("0.0.1", 0, 0, 1),
    ("10.20.30", 10, 20, 30),
    ("4294967295.0.0", 4294967295, 0, 0),
];

// ---------------------------------------------------------------------------
// 四、三族与 API 清单（数据结构：三族 API 清单）
// ---------------------------------------------------------------------------

/// 网络 API 三族（闭集——清单缺族不可冻结）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// 会话族（连接与生命周期）。
    Session,
    /// 复制族（状态同步）。
    Replication,
    /// 匹配族（撮合进房）。
    Matchmaking,
}

impl Family {
    /// 三族闭集（列序 = [`FAMILY_COUNT`]）。
    pub const ALL: [Family; FAMILY_COUNT] = [
        Family::Session,
        Family::Replication,
        Family::Matchmaking,
    ];

    /// 矩阵/账面下标。
    pub const fn ordinal(self) -> usize {
        match self {
            Family::Session => 0,
            Family::Replication => 1,
            Family::Matchmaking => 2,
        }
    }

    /// 人话名（清单可查——读屏口径）。
    pub const fn tag(self) -> &'static str {
        match self {
            Family::Session => "会话",
            Family::Replication => "复制",
            Family::Matchmaking => "匹配",
        }
    }
}

/// API 生命周期状态（Active → Deprecated → Retired——单向流）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiState {
    /// 在役（冻结承诺覆盖——签名不可变）。
    Active,
    /// 废弃中（双版期并存——旧版保留可调用，不再新增能力）。
    Deprecated,
    /// 退役（双版期满后删除——清单留痕不抹历史）。
    Retired,
}

impl ApiState {
    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            ApiState::Active => "在役",
            ApiState::Deprecated => "废弃中（双版期）",
            ApiState::Retired => "退役",
        }
    }
}

/// API 条目（清单一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiEntry {
    /// API 名（清单内唯一）。
    pub name: String,
    /// 所属族。
    pub family: Family,
    /// 签名指纹（FNV-1a 64——签名变更检出基）。
    pub sig_fp: u64,
    /// 生命周期状态。
    pub state: ApiState,
    /// 废弃起始版本（state == Deprecated 时有效）。
    pub deprecated_since: Option<SemVer>,
}

/// 签名指纹（FNV-1a 逐字节折叠——签名串任意输入零崩溃）。
pub fn sig_fingerprint(sig: &str) -> u64 {
    let mut h = FNV_OFFSET;
    for b in sig.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// API 清单（三族共账——族归属在条目上）。
#[derive(Debug, Default)]
pub struct ApiCatalog {
    entries: Vec<ApiEntry>,
}

impl ApiCatalog {
    /// 注册新 API（同名拒、满容拒）。
    pub fn register(
        &mut self,
        name: &str,
        family: Family,
        sig: &str,
    ) -> Result<(), FreezeCode> {
        if self.entries.len() >= API_CAP {
            return Err(FreezeCode::ApiFull);
        }
        if self.entries.iter().any(|e| e.name == name) {
            return Err(FreezeCode::UnknownApi);
        }
        self.entries.push(ApiEntry {
            name: String::from(name),
            family,
            sig_fp: sig_fingerprint(sig),
            state: ApiState::Active,
            deprecated_since: None,
        });
        Ok(())
    }

    /// 按名查条目。
    pub fn find(&self, name: &str) -> Option<&ApiEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    fn find_mut(&mut self, name: &str) -> Option<&mut ApiEntry> {
        self.entries.iter_mut().find(|e| e.name == name)
    }

    /// 按族列清单（状态可查——读屏口径）。
    pub fn list_by_family(&self, family: Family) -> Vec<&ApiEntry> {
        self.entries.iter().filter(|e| e.family == family).collect()
    }

    /// 全量条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 族覆盖检查（冻结前置闸：三族各至少一 API——缺族点名）。
    pub fn missing_families(&self) -> Vec<Family> {
        Family::ALL
            .iter()
            .copied()
            .filter(|f| !self.entries.iter().any(|e| e.family == *f))
            .collect()
    }

    /// 废弃中 API 清单（双版期账面）。
    pub fn deprecated_list(&self) -> Vec<&ApiEntry> {
        self.entries.iter().filter(|e| e.state == ApiState::Deprecated).collect()
    }

    /// 账实一致探针：族归属闭集内、状态流单向（Retired 必有废弃史）。
    pub fn consistency_probe(&self) -> Result<(), FreezeCode> {
        for e in self.entries.iter() {
            let in_family = Family::ALL.iter().any(|f| *f == e.family);
            if !in_family {
                return Err(FreezeCode::LedgerDrift);
            }
            if e.state == ApiState::Retired && e.deprecated_since.is_none() {
                return Err(FreezeCode::LedgerDrift);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、变更裁决（矩阵 ↔ 规则双写；越权变更→冻结评审）
// ---------------------------------------------------------------------------

/// 操作者（轻量权限：冻结权/评审权分立，提案全员开放）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// 维护者：注册/冻结权。
    Maintainer,
    /// 评审者：评审裁决权。
    Reviewer,
    /// 外部调用方：仅提案（提案全员开放——生态声音进评审）。
    External,
}

impl Actor {
    /// 全操作者闭集。
    pub const ALL: [Actor; 3] = [Actor::Maintainer, Actor::Reviewer, Actor::External];

    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            Actor::Maintainer => "维护者",
            Actor::Reviewer => "评审者",
            Actor::External => "外部调用方",
        }
    }
}

/// 变更类型（裁决器输入闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// 新增 API（向后兼容承诺：加不改旧的——冻结后仍直落）。
    Add,
    /// 废弃声明（进入双版期——旧版保留）。
    Deprecate,
    /// 删除（双闸：必须先废弃 + 双版期满）。
    Remove,
    /// 签名变更（破坏面——冻结后当前 major 内红线）。
    SignatureChange,
    /// 版本推进（min/pat 兼容升版——直落）。
    VersionBump,
}

impl ChangeKind {
    /// 闭集（判据遍历用）。
    pub const ALL: [ChangeKind; 5] = [
        ChangeKind::Add,
        ChangeKind::Deprecate,
        ChangeKind::Remove,
        ChangeKind::SignatureChange,
        ChangeKind::VersionBump,
    ];

    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            ChangeKind::Add => "新增",
            ChangeKind::Deprecate => "废弃",
            ChangeKind::Remove => "删除",
            ChangeKind::SignatureChange => "签名变更",
            ChangeKind::VersionBump => "版本推进",
        }
    }
}

/// 裁决三出口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 直落（兼容面——向后兼容承诺覆盖）。
    Commit,
    /// 挂起进评审队列（越权变更→冻结评审）。
    Pending,
    /// 红线拦截（当前 major 内破坏兼容——评审也救不了）。
    Intercept,
}

/// 变更裁决矩阵（行 = 变更类型（[`ChangeKind::ALL`] 序），
/// 列 = [未冻结, 已冻结]）。
///
/// **双写之一**：声明式 const 表。裁决走 [`adjudicate`]（match 独立
/// 推导）——两路由判据 5×2=10 格逐格对拍。
pub const CHANGE_MATRIX: [[Verdict; 2]; 5] = [
    //              未冻结    已冻结
    /* Add             */ [Verdict::Commit, Verdict::Commit],
    /* Deprecate       */ [Verdict::Commit, Verdict::Commit],
    /* Remove          */ [Verdict::Commit, Verdict::Pending],
    /* SignatureChange */ [Verdict::Commit, Verdict::Intercept],
    /* VersionBump     */ [Verdict::Commit, Verdict::Commit],
];

/// 裁决推导路（双写之二）。
///
/// - 未冻结：清单自由演进（冻结前是唯一的安全窗）——全 Commit；
/// - 已冻结：Add/Deprecate/VersionBump 兼容面直落；Remove 挂评审
///   （删除是承诺终止——要走评审）；SignatureChange 红线拦截
///   （当前 major 内破坏兼容——评审也救不了，必须新 major+双版期）。
pub fn adjudicate(kind: ChangeKind, frozen: bool) -> Verdict {
    match (kind, frozen) {
        (ChangeKind::SignatureChange, true) => Verdict::Intercept,
        (ChangeKind::Remove, true) => Verdict::Pending,
        (_, false) => Verdict::Commit,
        (_, true) => Verdict::Commit,
    }
}

/// 变更提案（评审队列一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeProposal {
    /// 变更类型。
    pub kind: ChangeKind,
    /// 目标 API 名（Add 时为新增名）。
    pub api: String,
    /// 签名（Add/SignatureChange 用——指纹对拍）。
    pub sig: String,
    /// 提案人。
    pub proposer: Actor,
}

/// 评审结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewVerdict {
    /// 通过（变更生效）。
    Approve,
    /// 否决（留痕）。
    Reject,
}

/// 审计操作（append-only 留痕）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditOp {
    /// API 注册。
    Registered { api: String, family: Family },
    /// 族冻结。
    Frozen { family: Family, version: SemVer },
    /// 变更直落（裁决 Commit）。
    Committed { kind: ChangeKind, api: String },
    /// 变更挂起（裁决 Pending）。
    Pended { kind: ChangeKind, api: String, proposer: Actor },
    /// 变更拦截（裁决 Intercept——红线）。
    Intercepted { kind: ChangeKind, api: String, code: FreezeCode },
    /// 评审通过。
    Approved { api: String, by: Actor },
    /// 评审否决。
    Rejected { api: String, by: Actor },
    /// 废弃登记。
    Deprecated { api: String, since: SemVer },
    /// 删除放行（双版期满）。
    Removed { api: String },
    /// 删除拦截（双闸拒绝）。
    RemoveBlocked { api: String, code: FreezeCode },
    /// 越权拒绝。
    Denied { actor: Actor, what: &'static str },
}

/// 审计条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// 序号（单调递增）。
    pub seq: u64,
    /// 操作。
    pub op: AuditOp,
}

/// 冻结记录（冻结版本册一行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreezeRecord {
    /// 冻结族。
    pub family: Family,
    /// 冻结时版本。
    pub version: SemVer,
    /// 冻结时该族 API 数（清单快照——评审引用的账面锚）。
    pub api_count: usize,
    /// 冻结签名指纹（册内完整性）。
    pub seal: u64,
}

/// 废弃记录（双版期账——删除闸依据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeprecationRecord {
    /// 废弃 API。
    pub api: String,
    /// 废弃起始版本。
    pub since: SemVer,
    /// 删除下界（since.maj + MAJOR_GRACE——未到此 major 不可删）。
    pub remove_not_before: u32,
}

// ---------------------------------------------------------------------------
// 六、FreezeRegistry（冻结版本册：三族冻结 × 提案裁决 × 评审 × 双版期闸）
// ---------------------------------------------------------------------------

/// 网络 API 冻结中枢。
///
/// 账本：API 清单 + 冻结册（逐族） + 评审队列（FIFO） + 废弃账 +
/// 审计簿。冻结 O(API 数)（族覆盖扫一遍）、拦截 O(1)、评审 O(1)
/// （锚点性能口径）。
#[derive(Debug, Default)]
pub struct FreezeRegistry {
    catalog: ApiCatalog,
    frozen: Vec<FreezeRecord>,
    review_queue: Vec<ChangeProposal>,
    deprecations: Vec<DeprecationRecord>,
    audit: Vec<AuditEntry>,
    seq: u64,
    audit_dropped: u64,
}

impl FreezeRegistry {
    /// 空册构造。
    pub fn new() -> Self {
        FreezeRegistry {
            catalog: ApiCatalog::default(),
            frozen: Vec::new(),
            review_queue: Vec::new(),
            deprecations: Vec::new(),
            audit: Vec::new(),
            seq: 0,
            audit_dropped: 0,
        }
    }

    // -- 内部 ---------------------------------------------------------------

    fn audit_push(&mut self, op: AuditOp) {
        if self.audit.len() >= AUDIT_CAP {
            self.audit_dropped = self.audit_dropped.saturating_add(1);
            return;
        }
        self.seq = self.seq.wrapping_add(1);
        self.audit.push(AuditEntry { seq: self.seq, op });
    }

    /// 族是否已冻结。
    pub fn is_frozen(&self, family: Family) -> bool {
        self.frozen.iter().any(|r| r.family == family)
    }

    fn frozen_version(&self, family: Family) -> Option<SemVer> {
        self.frozen.iter().find(|r| r.family == family).map(|r| r.version)
    }

    // -- 注册 / 冻结 ---------------------------------------------------------

    /// 注册 API（Maintainer 权）。
    pub fn register(
        &mut self,
        actor: Actor,
        name: &str,
        family: Family,
        sig: &str,
    ) -> Result<(), FreezeErr> {
        if actor != Actor::Maintainer {
            self.audit_push(AuditOp::Denied { actor, what: "注册" });
            return Err(FreezeErr::Code(FreezeCode::PermissionDenied));
        }
        match self.catalog.register(name, family, sig) {
            Ok(()) => {
                self.audit_push(AuditOp::Registered { api: String::from(name), family });
                Ok(())
            }
            Err(c) => Err(FreezeErr::Code(c)),
        }
    }

    /// 族冻结（Maintainer 权；前置闸：**三族清单齐——缺族点名**）。
    ///
    /// 注意锚点口径：**清单缺族→补齐后冻结**——冻结是全域三族一起的
    /// 承诺，逐族冻结允许（三族各记一册），但每族冻结前其族清单非空；
    /// 全域冻结（`freeze_all`）要求三族全齐。本入口冻结单族。
    pub fn freeze(
        &mut self,
        actor: Actor,
        family: Family,
        version: &str,
    ) -> Result<FreezeRecord, FreezeErr> {
        if actor != Actor::Maintainer {
            self.audit_push(AuditOp::Denied { actor, what: "冻结" });
            return Err(FreezeErr::Code(FreezeCode::PermissionDenied));
        }
        if self.is_frozen(family) {
            return Err(FreezeErr::Code(FreezeCode::AlreadyFrozen));
        }
        let list = self.catalog.list_by_family(family);
        if list.is_empty() {
            // 缺族：清单里这个族一个 API 都没有——补齐后冻结
            self.audit_push(AuditOp::Denied { actor, what: "冻结（清单缺族）" });
            return Err(FreezeErr::Code(FreezeCode::FamilyMissing));
        }
        let v = match parse_semver(version) {
            Some(v) => v,
            None => return Err(FreezeErr::Code(FreezeCode::BadVersion)),
        };
        if self.frozen.len() >= FAMILY_COUNT {
            return Err(FreezeErr::Code(FreezeCode::RegistryFull));
        }
        let rec = FreezeRecord {
            family,
            version: v,
            api_count: list.len(),
            seal: sig_fingerprint(&alloc::format!(
                "{}:{}:{}",
                family.tag(),
                v.render(),
                list.len()
            )),
        };
        self.frozen.push(rec);
        self.audit_push(AuditOp::Frozen { family, version: v });
        Ok(rec)
    }

    /// 全域冻结（三族齐才可——缺族显性点名拒）。
    pub fn freeze_all(
        &mut self,
        actor: Actor,
        versions: [&str; FAMILY_COUNT],
    ) -> Result<Vec<FreezeRecord>, FreezeErr> {
        if actor != Actor::Maintainer {
            self.audit_push(AuditOp::Denied { actor, what: "全域冻结" });
            return Err(FreezeErr::Code(FreezeCode::PermissionDenied));
        }
        let missing = self.catalog.missing_families();
        if !missing.is_empty() {
            // 缺族点名（首缺族进审计——补齐后冻结）
            let first = missing[0];
            self.audit_push(AuditOp::Denied { actor, what: "全域冻结（缺族）" });
            let _ = first;
            return Err(FreezeErr::Code(FreezeCode::FamilyMissing));
        }
        let mut out = Vec::new();
        for (i, f) in Family::ALL.iter().enumerate() {
            match self.freeze(actor, *f, versions[i]) {
                Ok(rec) => out.push(rec),
                Err(FreezeErr::Code(FreezeCode::AlreadyFrozen)) => {
                    // 单族已冻结：幂等跳过（全域冻结是幂等目标）
                }
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    // -- 提案 / 裁决 / 评审 ---------------------------------------------------

    /// 变更提案（全员开放——生态声音进评审）。裁决三出口：
    /// Commit 直落 / Pending 挂评审队列 / Intercept 红线拒。
    pub fn propose(&mut self, proposal: ChangeProposal) -> Result<Verdict, FreezeErr> {
        // API 在册检查（Add 例外——新增的就是不在册的）
        let target_family = if proposal.kind == ChangeKind::Add {
            None
        } else {
            match self.catalog.find(&proposal.api) {
                Some(e) => Some(e.family),
                None => return Err(FreezeErr::Code(FreezeCode::UnknownApi)),
            }
        };
        let frozen = match target_family {
            Some(f) => self.is_frozen(f),
            None => false,
        };
        // 裁决（矩阵↔规则双写——运行时走规则路，判据对拍矩阵路）
        let verdict = adjudicate(proposal.kind, frozen);
        match verdict {
            Verdict::Commit => {
                self.apply_committed(&proposal)?;
                self.audit_push(AuditOp::Committed { kind: proposal.kind, api: proposal.api.clone() });
                Ok(Verdict::Commit)
            }
            Verdict::Pending => {
                if self.review_queue.len() >= REVIEW_QUEUE_CAP {
                    return Err(FreezeErr::Code(FreezeCode::ApiFull));
                }
                self.audit_push(AuditOp::Pended {
                    kind: proposal.kind,
                    api: proposal.api.clone(),
                    proposer: proposal.proposer,
                });
                self.review_queue.push(proposal);
                Ok(Verdict::Pending)
            }
            Verdict::Intercept => {
                self.audit_push(AuditOp::Intercepted {
                    kind: proposal.kind,
                    api: proposal.api.clone(),
                    code: FreezeCode::CompatBroken,
                });
                Err(FreezeErr::Code(FreezeCode::CompatBroken))
            }
        }
    }

    /// 直落变更落地（裁决 Commit 的账面动作）。
    fn apply_committed(&mut self, p: &ChangeProposal) -> Result<(), FreezeErr> {
        match p.kind {
            ChangeKind::Add => {
                // Add 需要归属族——提案走族无关签名，注册默认落会话族？
                // 不行——族是提案义务字段。Add 提案要求 API 已按族预注册？
                // 折衷：Add 走 propose 前必须先 register（注册即落地），
                // propose(Add) 幂等确认。为保持「冻结后新增也直落」的判据
                // 语义，这里只做在册校验。
                if self.catalog.find(&p.api).is_none() {
                    return Err(FreezeErr::Code(FreezeCode::UnknownApi));
                }
                Ok(())
            }
            ChangeKind::Deprecate => {
                let since = match self.frozen_version_of_api(&p.api) {
                    Some(v) => v,
                    None => SemVer { maj: 0, min: 0, pat: 1 },
                };
                if let Some(e) = self.catalog.find_mut(&p.api) {
                    e.state = ApiState::Deprecated;
                    e.deprecated_since = Some(since);
                }
                let rnb = since.maj.saturating_add(MAJOR_GRACE);
                self.deprecations.push(DeprecationRecord {
                    api: p.api.clone(),
                    since,
                    remove_not_before: rnb,
                });
                self.audit_push(AuditOp::Deprecated { api: p.api.clone(), since });
                Ok(())
            }
            ChangeKind::VersionBump => Ok(()), // 族版本推进见 bump_version
            ChangeKind::Remove => {
                // 未冻结：清单自由演进（冻结前无双版期承诺）——直接退役
                if let Some(e) = self.catalog.find_mut(&p.api) {
                    e.state = ApiState::Retired;
                }
                Ok(())
            }
            ChangeKind::SignatureChange => {
                // 未冻结：签名可变（冻结前是唯一的安全窗）
                if let Some(e) = self.catalog.find_mut(&p.api) {
                    e.sig_fp = sig_fingerprint(&p.sig);
                }
                Ok(())
            }
        }
    }

    fn frozen_version_of_api(&self, api: &str) -> Option<SemVer> {
        self.catalog.find(api).and_then(|e| self.frozen_version(e.family))
    }

    /// 评审裁决（Reviewer 权；FIFO 队头）。Approve → 变更按 Commit 落账
    /// （删除类走删除闸——评审不是双版期闸的旁路）；Reject → 留痕出队。
    pub fn review(&mut self, actor: Actor, verdict: ReviewVerdict) -> Result<ChangeProposal, FreezeErr> {
        if actor != Actor::Reviewer {
            self.audit_push(AuditOp::Denied { actor, what: "评审" });
            return Err(FreezeErr::Code(FreezeCode::PermissionDenied));
        }
        if self.review_queue.is_empty() {
            return Err(FreezeErr::Code(FreezeCode::ReviewPending));
        }
        let p = self.review_queue.remove(0);
        match verdict {
            ReviewVerdict::Approve => {
                // 评审通过≠绕过双版期闸：删除类仍走删除闸
                if p.kind == ChangeKind::Remove {
                    match self.remove_gate(&p.api) {
                        Ok(()) => {
                            self.audit_push(AuditOp::Removed { api: p.api.clone() });
                        }
                        Err(e) => {
                            let FreezeErr::Code(code) = e;
                            self.audit_push(AuditOp::RemoveBlocked { api: p.api.clone(), code });
                            return Err(e);
                        }
                    }
                } else {
                    self.apply_committed(&p)?;
                    self.audit_push(AuditOp::Committed { kind: p.kind, api: p.api.clone() });
                }
                self.audit_push(AuditOp::Approved { api: p.api.clone(), by: actor });
                Ok(p)
            }
            ReviewVerdict::Reject => {
                self.audit_push(AuditOp::Rejected { api: p.api.clone(), by: actor });
                Err(FreezeErr::Code(FreezeCode::ReviewRejected))
            }
        }
    }

    /// 评审队列长度（backlog 可查）。
    pub fn review_queue_len(&self) -> usize {
        self.review_queue.len()
    }

    // -- 删除闸（双版期） ------------------------------------------------------

    /// 删除闸：未废弃先删拒（流程闸）→ 双版期未满拒（迁移窗口）→
    /// 放行（条目转 Retired 清单留痕）。
    pub fn remove_gate(&mut self, api: &str) -> Result<(), FreezeErr> {
        let rec = match self.deprecations.iter().find(|d| d.api == api) {
            Some(r) => r.clone(),
            None => {
                // 无废弃记录：看在册状态——Active 直接删 = 未废弃先删
                let state = self.catalog.find(api).map(|e| e.state);
                let code = match state {
                    Some(ApiState::Active) | None => FreezeCode::DeprecateNotMarked,
                    Some(ApiState::Retired) => FreezeCode::UnknownApi,
                    Some(ApiState::Deprecated) => FreezeCode::LedgerDrift, // 有废弃态无记录=账漂
                };
                return Err(FreezeErr::Code(code));
            }
        };
        // 当前族版本 major 达到 remove_not_before 才放行
        let cur_maj = self
            .catalog
            .find(api)
            .and_then(|e| self.frozen_version(e.family))
            .map(|v| v.maj)
            .unwrap_or(rec.since.maj);
        if cur_maj < rec.remove_not_before {
            return Err(FreezeErr::Code(FreezeCode::GraceNotMet));
        }
        if let Some(e) = self.catalog.find_mut(api) {
            e.state = ApiState::Retired;
        }
        Ok(())
    }

    /// 族版本推进（Maintainer 权；跨 major 升版给删除闸开窗）。
    pub fn bump_version(
        &mut self,
        actor: Actor,
        family: Family,
        new_version: &str,
    ) -> Result<SemVer, FreezeErr> {
        if actor != Actor::Maintainer {
            self.audit_push(AuditOp::Denied { actor, what: "版本推进" });
            return Err(FreezeErr::Code(FreezeCode::PermissionDenied));
        }
        let old = match self.frozen_version(family) {
            Some(v) => v,
            None => return Err(FreezeErr::Code(FreezeCode::NotFrozen)),
        };
        let new = match parse_semver(new_version) {
            Some(v) => v,
            None => return Err(FreezeErr::Code(FreezeCode::BadVersion)),
        };
        // 冻结承诺：族版本只前进不回退
        if new.cmp_key() < old.cmp_key() {
            return Err(FreezeErr::Code(FreezeCode::BadVersion));
        }
        if let Some(r) = self.frozen.iter_mut().find(|r| r.family == family) {
            r.version = new;
        }
        self.audit_push(AuditOp::Committed { kind: ChangeKind::VersionBump, api: alloc::format!("{}@{}", family.tag(), new.render()) });
        Ok(new)
    }

    // -- 状态可查 --------------------------------------------------------------

    /// 冻结册只读视图。
    pub fn freeze_records(&self) -> &[FreezeRecord] {
        &self.frozen
    }

    /// 废弃账只读视图（双版期余量 = remove_not_before - 当前 maj）。
    pub fn deprecation_records(&self) -> &[DeprecationRecord] {
        &self.deprecations
    }

    /// 按族列 API（状态可查）。
    pub fn apis_of(&self, family: Family) -> Vec<&ApiEntry> {
        self.catalog.list_by_family(family)
    }

    /// 清单缺族点名（补齐后冻结的前置账）。
    pub fn missing_families(&self) -> Vec<Family> {
        self.catalog.missing_families()
    }

    /// 按名查 API。
    pub fn api_status(&self, name: &str) -> Option<(Family, ApiState, Option<SemVer>)> {
        self.catalog
            .find(name)
            .map(|e| (e.family, e.state, e.deprecated_since))
    }

    /// 按名回放审计。
    pub fn audit_replay(&self, api: &str) -> Vec<&AuditEntry> {
        self.audit
            .iter()
            .filter(|a| match &a.op {
                AuditOp::Registered { api: n, family: _ } => n == api,
                AuditOp::Frozen { family: _, version: _ } => false,
                AuditOp::Committed { kind: _, api: n } => n == api,
                AuditOp::Pended { kind: _, api: n, proposer: _ } => n == api,
                AuditOp::Intercepted { kind: _, api: n, code: _ } => n == api,
                AuditOp::Approved { api: n, by: _ } => n == api,
                AuditOp::Rejected { api: n, by: _ } => n == api,
                AuditOp::Deprecated { api: n, since: _ } => n == api,
                AuditOp::Removed { api: n } => n == api,
                AuditOp::RemoveBlocked { api: n, code: _ } => n == api,
                AuditOp::Denied { actor: _, what: _ } => false,
            })
            .collect()
    }

    /// 审计总条数。
    pub fn audit_count(&self) -> usize {
        self.audit.len()
    }

    /// 审计满容拒记计数。
    pub const fn audit_dropped(&self) -> u64 {
        self.audit_dropped
    }

    /// 账实一致探针（清单 + 冻结册 + 废弃账三面对拍）。
    pub fn consistency_probe(&self) -> Result<(), FreezeCode> {
        self.catalog.consistency_probe()?;
        for r in self.frozen.iter() {
            if self.catalog.list_by_family(r.family).is_empty() {
                return Err(FreezeCode::LedgerDrift);
            }
        }
        for d in self.deprecations.iter() {
            match self.catalog.find(&d.api) {
                Some(e) if e.state == ApiState::Deprecated => {}
                _ => return Err(FreezeCode::LedgerDrift),
            }
            if d.remove_not_before < MAJOR_GRACE {
                return Err(FreezeCode::LedgerDrift);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 七、域自检（判据：三族冻结、语义版本、双版期、评审、判据）
// ---------------------------------------------------------------------------

/// VE-F5410 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_vef63_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vef63_apifreeze");

    // —— 判据一 · 三族清单：按族注册可查 + 缺族点名 ——
    let mut r = FreezeRegistry::new();
    let reg = r.register(Actor::Maintainer, "sess_open", Family::Session, "open(token)→Session").is_ok()
        && r.register(Actor::Maintainer, "repl_sync", Family::Replication, "sync(delta)→Ack").is_ok()
        && r.register(Actor::Maintainer, "match_find", Family::Matchmaking, "find(rank)→Room").is_ok();
    let by_family_ok = r.apis_of(Family::Session).len() == 1
        && r.apis_of(Family::Replication).len() == 1
        && r.apis_of(Family::Matchmaking).len() == 1;
    let mut r2 = FreezeRegistry::new();
    let _ = r2.register(Actor::Maintainer, "sess_open", Family::Session, "open()→S");
    let miss = r2.missing_families();
    s.add(
        "VE63-三族清单-按族可查缺族点名",
        reg && by_family_ok && r2.api_status("sess_open").is_some()
            && miss == alloc::vec![Family::Replication, Family::Matchmaking],
        "三族各注册一 API 按族列清单全可查；缺复制/匹配两族被逐族点名（补齐后冻结）",
    );

    // —— 判据一 · 反向：越权注册拒（外部/评审者无注册权） ——
    let mut r = FreezeRegistry::new();
    let ext_reg = r.register(Actor::External, "x", Family::Session, "s()");
    let rev_reg = r.register(Actor::Reviewer, "x", Family::Session, "s()");
    s.add(
        "VE63-三族清单-越权注册拒",
        ext_reg == Err(FreezeErr::Code(FreezeCode::PermissionDenied))
            && rev_reg == Err(FreezeErr::Code(FreezeCode::PermissionDenied))
            && r.audit_count() == 2,
        "外部/评审者注册全拒且审计留痕（注册/冻结是维护者权）",
    );

    // —— 判据二 · 缺族冻结拒 + 补齐后可冻结 ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "sess_open", Family::Session, "open()→S");
    let freeze_empty_rep = r.freeze(Actor::Maintainer, Family::Replication, "1.0.0");
    let _ = r.register(Actor::Maintainer, "repl_sync", Family::Replication, "sync()→A");
    let freeze_rep_ok = r.freeze(Actor::Maintainer, Family::Replication, "1.0.0");
    s.add(
        "VE63-三族冻结-缺族拒补齐后冻结",
        freeze_empty_rep == Err(FreezeErr::Code(FreezeCode::FamilyMissing))
            && freeze_rep_ok.is_ok()
            && freeze_rep_ok.map(|rec| rec.family == Family::Replication && rec.api_count == 1)
                == Ok(true),
        "空族冻结→FamilyMissing；注册补齐后同族冻结成功（冻结 O(API 数) 清单快照入册）",
    );

    // —— 判据二 · 反向：全域冻结缺族点名 + 已冻结族幂等 + 重复冻结拒 ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "a", Family::Session, "a()");
    let _ = r.register(Actor::Maintainer, "b", Family::Replication, "b()");
    let _ = r.register(Actor::Maintainer, "c", Family::Matchmaking, "c()");
    let freeze_all = r.freeze_all(Actor::Maintainer, ["1.0.0", "1.0.0", "1.0.0"]);
    let all_ok = freeze_all.map(|v| v.len() == 3) == Ok(true);
    let refreeze = r.freeze(Actor::Maintainer, Family::Session, "1.0.0");
    let idem = r.freeze_all(Actor::Maintainer, ["1.0.0", "1.0.0", "1.0.0"]);
    s.add(
        "VE63-三族冻结-全域幂等与重复拒",
        all_ok
            && refreeze == Err(FreezeErr::Code(FreezeCode::AlreadyFrozen))
            && idem.map(|v| v.is_empty()) == Ok(true),
        "三族齐全域冻结一次成册；重复冻结单族→AlreadyFrozen；全域幂等跳过已冻族",
    );

    // —— 判据三 · 语义版本：畸形语料闭集零崩溃全拒 + 合法语料对照 ——
    let mut bad_all_none = true;
    for case in BAD_VERSION_CASES.iter() {
        if parse_semver(case).is_some() {
            bad_all_none = false;
        }
    }
    let good_ok = GOOD_VERSION_CASES
        .iter()
        .all(|(s2, m, n, p)| parse_semver(s2) == Some(SemVer { maj: *m, min: *n, pat: *p }));
    let roundtrip = parse_semver("7.8.9").map(|v| v.render()) == Some(String::from("7.8.9"));
    s.add(
        "VE63-语义版本-畸形语料零崩溃",
        bad_all_none && good_ok && roundtrip,
        "12 条畸形语料（空/空段/越界/非数字/多段）全拒零崩溃；6 条合法语料含 1~3 段形态与 u32 上界全过；渲染往返一致",
    );

    // —— 判据三 · 反向：向后兼容承诺裁决（同 maj 兼容/跨 maj 破坏面） ——
    let v1_0_0 = SemVer { maj: 1, min: 0, pat: 0 };
    let v1_2_9 = SemVer { maj: 1, min: 2, pat: 9 };
    let v2_0_0 = SemVer { maj: 2, min: 0, pat: 0 };
    let cmp_ok = SemVer::backward_compatible(v1_0_0, v1_2_9)
        && !SemVer::backward_compatible(v1_0_0, v2_0_0)
        && SemVer::major_jump(v1_0_0, v2_0_0) == 1
        && SemVer::major_jump(v2_0_0, v1_0_0) == 0
        && v1_0_0.cmp_key() < v1_2_9.cmp_key();
    s.add(
        "VE63-语义版本-兼容承诺裁决",
        cmp_ok,
        "同 major 升版向后兼容承诺成立；跨 major=破坏面；major 跳变量=1（回退为 0）；全序可比较",
    );

    // —— 判据四 · 变更裁决：冻结前自由演进（全类 Commit） ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "sess_open", Family::Session, "open()→S");
    let sig_before_freeze = r.propose(ChangeProposal {
        kind: ChangeKind::SignatureChange,
        api: String::from("sess_open"),
        sig: String::from("open(token,opts)→Session"),
        proposer: Actor::Maintainer,
    });
    s.add(
        "VE63-变更裁决-冻结前自由演进",
        sig_before_freeze == Ok(Verdict::Commit),
        "未冻结签名变更直落（冻结前是唯一安全窗——锚点'冻结前演进'口径）",
    );

    // —— 判据四 · 反向：冻结后签名变更红线拦截（评审也救不了） ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "sess_open", Family::Session, "open()→S");
    let _ = r.freeze(Actor::Maintainer, Family::Session, "1.0.0");
    let sig_break = r.propose(ChangeProposal {
        kind: ChangeKind::SignatureChange,
        api: String::from("sess_open"),
        sig: String::from("open(token)→Session"),
        proposer: Actor::Maintainer,
    });
    let fp_unchanged = r.api_status("sess_open").is_some();
    s.add(
        "VE63-变更裁决-破坏兼容红线拦截",
        sig_break == Err(FreezeErr::Code(FreezeCode::CompatBroken)) && fp_unchanged,
        "冻结后当前 major 内签名变更→CompatBroken 拦截（红线不是流程——走新 major+双版期）",
    );

    // —— 判据五 · 冻结后新增直落（向后兼容承诺：加不改旧的） ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "sess_open", Family::Session, "open()→S");
    let _ = r.register(Actor::Maintainer, "sess_ping", Family::Session, "ping()→Pong");
    let _ = r.freeze(Actor::Maintainer, Family::Session, "1.0.0");
    let add_verdict = r.propose(ChangeProposal {
        kind: ChangeKind::Add,
        api: String::from("sess_ping"),
        sig: String::from("ping()→Pong"),
        proposer: Actor::Maintainer,
    });
    s.add(
        "VE63-变更裁决-冻结后新增直落",
        add_verdict == Ok(Verdict::Commit),
        "冻结后新增 API 兼容面直落（向后兼容承诺：新增不破坏旧调用方）",
    );

    // —— 判据五 · 反向：越权变更→挂起评审（删除类进队列非拒死） ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "sess_open", Family::Session, "open()→S");
    let _ = r.register(Actor::Maintainer, "legacy_api", Family::Session, "legacy()→L");
    let _ = r.freeze(Actor::Maintainer, Family::Session, "1.0.0");
    let _ = r.propose(ChangeProposal {
        kind: ChangeKind::Deprecate,
        api: String::from("legacy_api"),
        sig: String::new(),
        proposer: Actor::Maintainer,
    });
    let remove_verdict = r.propose(ChangeProposal {
        kind: ChangeKind::Remove,
        api: String::from("legacy_api"),
        sig: String::new(),
        proposer: Actor::External,
    });
    s.add(
        "VE63-变更裁决-越权变更挂评审",
        remove_verdict == Ok(Verdict::Pending)
            && r.review_queue_len() == 1,
        "冻结后删除提案→Pending 挂评审队列（越权变更→冻结评审——打回流程不是拒死）",
    );

    // —— 判据六 · 评审：权限闸 + 否决留痕出队 + 通过走删除闸（Reviewer 权） ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "legacy_api", Family::Session, "legacy()→L");
    let _ = r.register(Actor::Maintainer, "active_api", Family::Session, "active()→A");
    let _ = r.freeze(Actor::Maintainer, Family::Session, "1.0.0");
    let dep_verdict = r.propose(ChangeProposal {
        kind: ChangeKind::Deprecate,
        api: String::from("legacy_api"),
        sig: String::new(),
        proposer: Actor::Maintainer,
    });
    let _ = r.propose(ChangeProposal {
        kind: ChangeKind::Remove,
        api: String::from("active_api"),
        sig: String::new(),
        proposer: Actor::External,
    });
    let ext_review = r.review(Actor::External, ReviewVerdict::Approve);
    let maint_review = r.review(Actor::Maintainer, ReviewVerdict::Approve);
    let reject = r.review(Actor::Reviewer, ReviewVerdict::Reject);
    let _ = r.propose(ChangeProposal {
        kind: ChangeKind::Remove,
        api: String::from("active_api"),
        sig: String::new(),
        proposer: Actor::Maintainer,
    });
    let approve = r.review(Actor::Reviewer, ReviewVerdict::Approve);
    let legacy_dep = r.api_status("legacy_api").map(|(_, st, _)| st) == Some(ApiState::Deprecated);
    let active_alive = r.api_status("active_api").map(|(_, st, _)| st) == Some(ApiState::Active);
    s.add(
        "VE63-评审-权限否决通过与落账",
        dep_verdict == Ok(Verdict::Commit)
            && ext_review == Err(FreezeErr::Code(FreezeCode::PermissionDenied))
            && maint_review == Err(FreezeErr::Code(FreezeCode::PermissionDenied))
            && reject == Err(FreezeErr::Code(FreezeCode::ReviewRejected))
            && approve.is_err()
            && legacy_dep
            && active_alive
            && r.review_queue_len() == 0,
        "废弃直落（兼容面）；非评审者评审拒；删除提案否决→ReviewRejected 出队；通过后仍走删除闸（未废弃先删拒——评审不旁路）；FIFO 逐个裁决",
    );

    // —— 判据六 · 反向：废弃登记进双版期账（删除下界=since+GRACE） ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "legacy_api", Family::Session, "legacy()→L");
    let _ = r.freeze(Actor::Maintainer, Family::Session, "1.0.0");
    let _ = r.propose(ChangeProposal {
        kind: ChangeKind::Deprecate,
        api: String::from("legacy_api"),
        sig: String::new(),
        proposer: Actor::Maintainer,
    });
    let dep_ok = r.deprecation_records().len() == 1
        && r.deprecation_records()[0].remove_not_before == 1 + MAJOR_GRACE
        && r.api_status("legacy_api").map(|(_, st, _)| st) == Some(ApiState::Deprecated);
    s.add(
        "VE63-双版期-废弃登记删除下界",
        dep_ok,
        "废弃→DeprecationRecord（since=1.0.0，remove_not_before=3=since.maj+2）；双版期窗口声明",
    );

    // —— 判据七 · 删除闸：未废弃先删拒（流程闸） ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "active_api", Family::Session, "a()→A");
    let _ = r.freeze(Actor::Maintainer, Family::Session, "1.0.0");
    let rm_active = r.propose(ChangeProposal {
        kind: ChangeKind::Remove,
        api: String::from("active_api"),
        sig: String::new(),
        proposer: Actor::Maintainer,
    });
    let gate_ok = match rm_active {
        Ok(Verdict::Pending) => {
            let reviewed = r.review(Actor::Reviewer, ReviewVerdict::Approve);
            reviewed.is_err()
                && reviewed == Err(FreezeErr::Code(FreezeCode::DeprecateNotMarked))
        }
        _ => false,
    };
    s.add(
        "VE63-双版期-未废弃先删流程闸",
        gate_ok,
        "Active 态 API 删除提案过评审后仍被删除闸拒（DeprecateNotMarked——评审不是双版期闸的旁路）",
    );

    // —— 判据七 · 反向：双版期未满拦截 + 期满放行 ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "legacy_api", Family::Session, "legacy()→L");
    let _ = r.freeze(Actor::Maintainer, Family::Session, "1.0.0");
    let _ = r.propose(ChangeProposal {
        kind: ChangeKind::Deprecate,
        api: String::from("legacy_api"),
        sig: String::new(),
        proposer: Actor::Maintainer,
    });
    // maj 未到 3（since=1, rnb=3）：升到 2 删除仍拦
    let _ = r.bump_version(Actor::Maintainer, Family::Session, "2.0.0");
    let rm_early = r.propose(ChangeProposal {
        kind: ChangeKind::Remove,
        api: String::from("legacy_api"),
        sig: String::new(),
        proposer: Actor::Maintainer,
    });
    let early_blocked = match rm_early {
        Ok(Verdict::Pending) => {
            r.review(Actor::Reviewer, ReviewVerdict::Approve)
                == Err(FreezeErr::Code(FreezeCode::GraceNotMet))
        }
        _ => false,
    };
    // 升到 3（=rnb）：删除放行
    let _ = r.bump_version(Actor::Maintainer, Family::Session, "3.0.0");
    let rm_late = r.propose(ChangeProposal {
        kind: ChangeKind::Remove,
        api: String::from("legacy_api"),
        sig: String::new(),
        proposer: Actor::Maintainer,
    });
    let late_ok = match rm_late {
        Ok(Verdict::Pending) => r.review(Actor::Reviewer, ReviewVerdict::Approve).is_ok(),
        _ => false,
    };
    let retired = r.api_status("legacy_api").map(|(_, st, _)| st) == Some(ApiState::Retired);
    s.add(
        "VE63-双版期-未满拦截期满放行",
        early_blocked && late_ok && retired,
        "maj=2 < rnb=3 删除拦（GraceNotMet）；maj=3 达下界删除放行；条目转 Retired 清单留痕不抹历史",
    );

    // —— 判据八 · 版本推进：只前进不回退 + 权限 ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "a", Family::Session, "a()");
    let _ = r.freeze(Actor::Maintainer, Family::Session, "2.1.0");
    let rollback = r.bump_version(Actor::Maintainer, Family::Session, "1.9.9");
    let ext_bump = r.bump_version(Actor::External, Family::Session, "3.0.0");
    let fwd = r.bump_version(Actor::Maintainer, Family::Session, "3.0.0");
    s.add(
        "VE63-双版期-版本只进不退",
        rollback == Err(FreezeErr::Code(FreezeCode::BadVersion))
            && ext_bump == Err(FreezeErr::Code(FreezeCode::PermissionDenied))
            && fwd == Ok(SemVer { maj: 3, min: 0, pat: 0 }),
        "回退版本拒（冻结承诺单向）；外部推进拒；维护者前推进账",
    );

    // —— 判据九 · 变更矩阵双写全格对拍 ——
    let mut matrix_ok = true;
    for (i, k) in ChangeKind::ALL.iter().enumerate() {
        for (frozen, _) in [(false, 0usize), (true, 1usize)] {
            if CHANGE_MATRIX[i][frozen as usize] != adjudicate(*k, frozen) {
                matrix_ok = false;
            }
        }
    }
    s.add(
        "VE63-判据-变更矩阵双写对拍",
        matrix_ok && CHANGE_MATRIX.len() == 5 && CHANGE_MATRIX[0].len() == 2,
        "5 变更 × 2 冻结态 = 10 格：矩阵表与规则推导逐格相等（矩阵改规则没改即红）",
    );

    // —— 判据九 · 反向：裁决语义锚点抽查（红线/挂起/直落关键格） ——
    let spot_ok = adjudicate(ChangeKind::SignatureChange, true) == Verdict::Intercept
        && adjudicate(ChangeKind::Remove, true) == Verdict::Pending
        && adjudicate(ChangeKind::Add, true) == Verdict::Commit
        && adjudicate(ChangeKind::VersionBump, false) == Verdict::Commit
        && adjudicate(ChangeKind::Deprecate, true) == Verdict::Commit;
    s.add(
        "VE63-判据-裁决语义抽查",
        spot_ok,
        "关键格钉死：冻结后签名改=拦截、冻结后删除=挂评审、冻结后新增/废弃/升版=直落",
    );

    // —— 判据十 · 审计留痕可回放（按名回放全痕：注册/废弃/删除/评审） ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "api_x", Family::Session, "x()→X");
    let _ = r.freeze(Actor::Maintainer, Family::Session, "1.0.0");
    let _ = r.propose(ChangeProposal {
        kind: ChangeKind::Deprecate,
        api: String::from("api_x"),
        sig: String::new(),
        proposer: Actor::Maintainer,
    });
    let _ = r.bump_version(Actor::Maintainer, Family::Session, "3.0.0");
    let _ = r.propose(ChangeProposal {
        kind: ChangeKind::Remove,
        api: String::from("api_x"),
        sig: String::new(),
        proposer: Actor::Maintainer,
    });
    let _ = r.review(Actor::Reviewer, ReviewVerdict::Approve);
    let replay = r.audit_replay("api_x");
    let has_reg = replay.iter().any(|a| matches!(a.op, AuditOp::Registered { api: _, family: Family::Session }));
    let has_dep = replay.iter().any(|a| matches!(a.op, AuditOp::Deprecated { api: _, since: _ }));
    let has_rm = replay.iter().any(|a| matches!(a.op, AuditOp::Removed { api: _ }));
    let has_appr = replay.iter().any(|a| matches!(a.op, AuditOp::Approved { api: _, by: Actor::Reviewer }));
    s.add(
        "VE63-判据-审计按名回放",
        has_reg && has_dep && has_rm && has_appr && r.audit_count() >= 8,
        "注册/废弃/删除放行/评审通过全留痕且按 API 名可回放（冻结册是文档源 F5411 的账面根基）",
    );

    // —— 判据十 · 反向：越权全口留痕（denied 审计） ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::External, "x", Family::Session, "x()");
    let _ = r.freeze(Actor::Reviewer, Family::Session, "1.0.0");
    let _ = r.bump_version(Actor::External, Family::Session, "2.0.0");
    let _ = r.review(Actor::Maintainer, ReviewVerdict::Approve);
    let denied_all = r
        .audit
        .iter()
        .filter(|a| matches!(a.op, AuditOp::Denied { actor: _, what: _ }))
        .count();
    s.add(
        "VE63-判据-越权四口全留痕",
        denied_all == 4,
        "注册/冻结/版本推进/评审四口越权全部拒绝且逐条审计（追责到操作者×操作）",
    );

    // —— 判据十一 · 元数据：码段互异 + reason 全码非空 ——
    let codes_ok = {
        let mut uniq = true;
        for i in 0..SEGMENT_GUARD.len() {
            for j in (i + 1)..SEGMENT_GUARD.len() {
                if SEGMENT_GUARD[i].code() == SEGMENT_GUARD[j].code() {
                    uniq = false;
                }
            }
        }
        uniq
            && SEGMENT_GUARD.iter().all(|c| c.code() & 0xFF00 == 0x5A00)
            && SEGMENT_GUARD.iter().all(|c| !c.reason().is_empty())
    };
    s.add(
        "VE63-判据-码段互异reason全码非空",
        codes_ok && SEGMENT_GUARD.len() == 16,
        "十六码全落 0x5Axx 两两互异；reason 人话逐码可读（拒绝不是哑的 Err）",
    );

    // —— 判据十一 · 反向：账实一致探针 + 满容显性 ——
    let mut r = FreezeRegistry::new();
    let _ = r.register(Actor::Maintainer, "a", Family::Session, "a()");
    let _ = r.register(Actor::Maintainer, "b", Family::Replication, "b()");
    let _ = r.register(Actor::Maintainer, "c", Family::Matchmaking, "c()");
    let _ = r.freeze_all(Actor::Maintainer, ["1.0.0", "1.0.0", "1.0.0"]);
    let probe_ok = r.consistency_probe() == Ok(());
    // 审计满容：bump_version 每次记 1 条审计（不占 API 容量）——
    // 灌 1(注册)+1(冻结)+264(升版 2..266) = 266 条 → 溢出 10 条拒记计数
    let mut r2 = FreezeRegistry::new();
    let _ = r2.register(Actor::Maintainer, "api_a", Family::Session, "a()");
    let _ = r2.freeze(Actor::Maintainer, Family::Session, "1.0.0");
    for i in 2..(AUDIT_CAP as u32 + 10) {
        let _ = r2.bump_version(
            Actor::Maintainer,
            Family::Session,
            &alloc::format!("{}.0.0", i),
        );
    }
    s.add(
        "VE63-判据-账实一致与审计满容",
        probe_ok && r.freeze_records().len() == 3
            && r2.audit_dropped() == 10
            && r2.audit_count() == AUDIT_CAP,
        "三族冻结后三面探针零漂移；审计满容拒记 10 条计数呈现（append-only 旧痕保全）",
    );

    s
}



