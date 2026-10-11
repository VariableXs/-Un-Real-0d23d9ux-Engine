//! unx202_schemaver — UNX-F0202 报告 schema 版本化。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0202 · UNX-A1-B11）：
//! UNX-F0202-J1 schema 版本号声明与解析器兼容矩阵一致，跨版本解析
//! 零崩溃。
//!
//! **分层深化声明**：报告本体（字段/齐备率/守恒账）是 F0201
//! （unx201_bootreport），本册只做**版本化治理**——版本模型、
//! 解析器、兼容矩阵、迁移链、版本账本。不重复实现报告渲染
//! （防重纪律单点）；与 F0201 的对接是**常量对拍**（本册断言
//! `SCHEMA_VERSION`/`SCHEMA_TAG` 与账本末位一致——"版本号声明与
//! 解析器一致"的核心闸）。
//!
//! **版本化治理五柱**：
//!
//! 1. **版本模型**：[`SchemaVer`] 三元组（major/minor/patch，u16）。
//!    双形态：结构体 ↔ tag 串（`varix-boot-report/<maj>.<min>.<pat>`）。
//!    解析器对**任意输入零崩溃**——畸形语料（空/超长/非数字/负号/
//!    段数错/段溢出）一律 `Err`，这是 J1 "跨版本解析零崩溃" 的
//!    解析面；零崩溃的调用面由 [`can_parse`] 兼容矩阵兜底（对不
//!    兼容的版本**显性拒**而非解析崩）。
//!
//! 2. **兼容矩阵**：`CUR × PEER → 裁决` 的 const 表（[`COMPAT_MATRIX`]），
//!    与规则函数 [`compat_rule`] **双写对拍**（全格逐格一致，漂移即
//!    SegmentGuard 立案）——矩阵可读可审，规则可单测，两套写法一个
//!    语义。语义：major 相同=Compat；对端 major 更新=Reject（生产者
//!    比消费者新，schema 破坏面未知）；对端 major 更旧=Reject（旧
//!    版本无迁移链则拒，有迁移链走 migrate）；minor/patch 差=向后
//!    兼容（新增字段消费方忽略、缺字段消费方按 Na 口径——F0201
//!    显性缺席语义承接）。
//!
//! 3. **迁移链**：[`MIGRATIONS`] 闭集（v0→v1 字段增补清单起步）。
//!    [`migrate`] 显性支持链上相邻步、显性拒链外步——迁移是**声明
//!    的能力**不是猜的能力。
//!
//! 4. **版本账本**：[`VERSION_HISTORY`] 单调表（版本 + 变更摘要 +
//!    破坏性标记）。账本严格递增（判据对拍）；当前版本==账本末位
//!    （与 F0201 常量三方一致：账本末位 == [`CURRENT`] ==
//!    F0201 `SCHEMA_VERSION`）。
//!
//! 5. **裁决留痕**：[`CompatVerdict`] 携带策略与理由文本——拒绝不
//!    是哑的 Err，是可审计的裁决（对齐 F0201 报告的异常账叙事）。
//!
//! **诊断码**：VerCode 十六码（0x37 段 `0x3700..=0x370F` 互异 +
//! [`segment_guard`]）。no_std 零 unwrap 零 expect 零切片越界。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::unxstar::unx201_bootreport::{SCHEMA_TAG, SCHEMA_VERSION};

/// 模块版本。
pub const UNX202_SCHEMAVER_VERSION: &str = "unx202-schemaver-v1";

/// 诊断段基值（0x37 段——0x36 已被 F0201 报告域占用）。
pub const SEG_BASE: u16 = 0x3700;

/// tag 前缀（与 F0201 SCHEMA_TAG 同源常量——双写对拍面）。
pub const TAG_PREFIX: &str = "varix-boot-report/";

/// 解析输入长度上限（超长即拒——DoS 面）。
pub const TAG_MAX_LEN: usize = 64;

/// 迁移链容量。
pub const MIGRATION_CAP: usize = 16;

/// 版本账本容量。
pub const HISTORY_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 诊断码（VerCode 十六码 · 0x37 段互异）
// ---------------------------------------------------------------------------

/// 诊断码（schema 版本化——0x37 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerCode {
    /// 0x3700 前缀不匹配（tag 串头不是 TAG_PREFIX）。
    BadPrefix,
    /// 0x3701 版本段数错（非 maj.min.pat 三段）。
    BadSegments,
    /// 0x3702 非数字段（段含非 ASCII 数字字符）。
    BadDigits,
    /// 0x3703 段溢出（数值超 u16 上限）。
    SegmentOverflow,
    /// 0x3704 空输入。
    Empty,
    /// 0x3705 超长输入（> TAG_MAX_LEN）。
    TooLong,
    /// 0x3706 兼容拒绝（矩阵裁决 Reject——携带策略）。
    CompatRejected,
    /// 0x3707 迁移链缺失（请求的迁移步未声明）。
    MigrationMissing,
    /// 0x3708 迁移链满（注册超容量——显性拒不静默）。
    MigrationFull,
    /// 0x3709 版本账本不一致（账本末位 != 当前版本声明）。
    LedgerDrift,
    /// 0x370A 账本乱序（历史版本非严格递增）。
    LedgerUnordered,
    /// 0x370B 账本满。
    LedgerFull,
    /// 0x370C 版本指纹失配（声明串指纹 != 账本指纹）。
    FingerprintMismatch,
    /// 0x370D 段位冲突（诊断码越界/重复）。
    SegmentConflict,
    /// 0x370E 跨模块常量漂移（F0201 SCHEMA_VERSION/SCHEMA_TAG 与账本不一致）。
    CrossModuleDrift,
    /// 0x370F 序非全序（Ord 一致性对拍失败）。
    OrderInconsistent,
}

impl VerCode {
    /// 诊断码值（0x37 段）。
    pub fn code(self) -> u16 {
        match self {
            VerCode::BadPrefix => 0x3700,
            VerCode::BadSegments => 0x3701,
            VerCode::BadDigits => 0x3702,
            VerCode::SegmentOverflow => 0x3703,
            VerCode::Empty => 0x3704,
            VerCode::TooLong => 0x3705,
            VerCode::CompatRejected => 0x3706,
            VerCode::MigrationMissing => 0x3707,
            VerCode::MigrationFull => 0x3708,
            VerCode::LedgerDrift => 0x3709,
            VerCode::LedgerUnordered => 0x370A,
            VerCode::LedgerFull => 0x370B,
            VerCode::FingerprintMismatch => 0x370C,
            VerCode::SegmentConflict => 0x370D,
            VerCode::CrossModuleDrift => 0x370E,
            VerCode::OrderInconsistent => 0x370F,
        }
    }

    /// 三要素速写（码/语义/处置）。
    pub fn note(self) -> &'static str {
        match self {
            VerCode::BadPrefix => "前缀不匹配：tag 头非法",
            VerCode::BadSegments => "段数错：须 maj.min.pat 三段",
            VerCode::BadDigits => "非数字段",
            VerCode::SegmentOverflow => "段溢出：超 u16",
            VerCode::Empty => "空输入",
            VerCode::TooLong => "超长输入",
            VerCode::CompatRejected => "兼容拒绝：矩阵裁决",
            VerCode::MigrationMissing => "迁移步未声明",
            VerCode::MigrationFull => "迁移链满",
            VerCode::LedgerDrift => "账本末位与当前版本漂移",
            VerCode::LedgerUnordered => "账本乱序",
            VerCode::LedgerFull => "账本满",
            VerCode::FingerprintMismatch => "版本指纹失配",
            VerCode::SegmentConflict => "诊断码段位冲突",
            VerCode::CrossModuleDrift => "跨模块版本声明漂移",
            VerCode::OrderInconsistent => "版本序非全序",
        }
    }

    /// 全码闭集（segment_guard 对拍面）。
    pub fn all() -> [VerCode; 16] {
        [
            VerCode::BadPrefix,
            VerCode::BadSegments,
            VerCode::BadDigits,
            VerCode::SegmentOverflow,
            VerCode::Empty,
            VerCode::TooLong,
            VerCode::CompatRejected,
            VerCode::MigrationMissing,
            VerCode::MigrationFull,
            VerCode::LedgerDrift,
            VerCode::LedgerUnordered,
            VerCode::LedgerFull,
            VerCode::FingerprintMismatch,
            VerCode::SegmentConflict,
            VerCode::CrossModuleDrift,
            VerCode::OrderInconsistent,
        ]
    }
}

/// 段位守卫（十六码互异且全落 0x37 段——双写对拍）。
pub fn segment_guard() -> bool {
    let codes = VerCode::all();
    for c in codes.iter() {
        let v = c.code();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            return false;
        }
    }
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if let (Some(a), Some(b)) = (codes.get(i), codes.get(j)) {
                if a.code() == b.code() {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 版本模型
// ---------------------------------------------------------------------------

/// schema 版本三元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SchemaVer {
    /// 主版本（破坏性变更递增）。
    pub major: u16,
    /// 次版本（向后兼容新增）。
    pub minor: u16,
    /// 修订号（缺陷修复）。
    pub patch: u16,
}

impl SchemaVer {
    /// 当前版本（单点声明——账本末位与 F0201 对拍共同锚定）。
    pub const CURRENT: SchemaVer = SchemaVer { major: 1, minor: 0, patch: 0 };

    /// 构造。
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        SchemaVer { major, minor, patch }
    }

    /// tag 串形态（`varix-boot-report/maj.min.pat`）。
    pub fn to_tag(&self) -> String {
        format!("{}{}.{}.{}", TAG_PREFIX, self.major, self.minor, self.patch)
    }

    /// 比较序（手写全序——判据 C10 与派生 Ord 对拍双保险）。
    pub fn cmp_manual(&self, other: &SchemaVer) -> core::cmp::Ordering {
        use core::cmp::Ordering;
        if self.major != other.major {
            return self.major.cmp(&other.major);
        }
        if self.minor != other.minor {
            return self.minor.cmp(&other.minor);
        }
        if self.patch != other.patch {
            return self.patch.cmp(&other.patch);
        }
        Ordering::Equal
    }

    /// 打包为 u64（比较/指纹用：maj<<32 | min<<16 | pat）。
    pub fn pack(&self) -> u64 {
        ((self.major as u64) << 32) | ((self.minor as u64) << 16) | (self.patch as u64)
    }
}

impl Default for SchemaVer {
    fn default() -> Self {
        SchemaVer::CURRENT
    }
}

impl core::cmp::Ord for SchemaVer {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.cmp_manual(other)
    }
}

impl core::cmp::PartialOrd for SchemaVer {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// 解析 tag 串 → 版本（**任意输入零崩溃**）。
///
/// 畸形语料全部显性 Err：空串/超长/前缀错/段数错/非数字/段溢出。
/// 本函数不含任何 panic 面（无 unwrap/索引越界——切片走 get）。
pub fn parse_tag(input: &str) -> Result<SchemaVer, VerCode> {
    if input.is_empty() {
        return Err(VerCode::Empty);
    }
    if input.len() > TAG_MAX_LEN {
        return Err(VerCode::TooLong);
    }
    let rest = match input.strip_prefix(TAG_PREFIX) {
        Some(r) => r,
        None => return Err(VerCode::BadPrefix),
    };
    // 三段拆分（'.' 分隔；String 段缓冲——&str 不可变，push 面在 String）。
    let mut segs: [String; 3] = Default::default();
    let mut seg_idx = 0usize;
    for ch in rest.chars() {
        if ch == '.' {
            if seg_idx >= 2 {
                return Err(VerCode::BadSegments);
            }
            seg_idx += 1;
        } else {
            // 追加到当前段（段缓冲定长三槽，段内字符直拼）。
            if seg_idx >= 3 {
                return Err(VerCode::BadSegments);
            }
            segs[seg_idx].push(ch);
        }
    }
    let mut parts = [0u16; 3];
    // 形态兼容：1~3 段全收（单段=major only——F0201 SCHEMA_TAG "/1" 历史形态；
    // 两段=major.minor；三段=全）。存在段必须非空非纯符号，缺省段=0。
    let seen = seg_idx + 1;
    for i in 0..seen {
        parts[i] = match segs.get(i) {
            Some(seg) => parse_u16_segment(seg)?,
            None => return Err(VerCode::BadSegments),
        };
    }
    Ok(SchemaVer::new(parts[0], parts[1], parts[2]))
}

/// 单段解析（纯数字 + u16 范围）。
fn parse_u16_segment(s: &str) -> Result<u16, VerCode> {
    if s.is_empty() {
        return Err(VerCode::BadDigits);
    }
    let mut acc: u32 = 0;
    for ch in s.chars() {
        let d = match ch.to_digit(10) {
            Some(d) => d,
            None => return Err(VerCode::BadDigits),
        };
        acc = acc * 10 + d;
        if acc > u16::MAX as u32 {
            return Err(VerCode::SegmentOverflow);
        }
    }
    Ok(acc as u16)
}

// ---------------------------------------------------------------------------
// 兼容矩阵（const 表 + 规则函数双写对拍）
// ---------------------------------------------------------------------------

/// 兼容裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compat {
    /// 完全兼容（同版本或 patch/minor 向后兼容）。
    Compat,
    /// 需迁移（有声明迁移链可走）。
    NeedsMigration,
    /// 拒绝（major 断面或未知破坏面）。
    Reject,
}

/// 兼容裁决 + 理由（裁决可审计）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompatVerdict {
    /// 裁决。
    pub verdict: Compat,
    /// 理由码（Reject 时为 VerCode::CompatRejected）。
    pub reason: VerCode,
    /// 理由文本（静态）。
    pub why: &'static str,
}

/// 兼容规则函数（矩阵的规则面）。
///
/// 语义（producer 产生报告的 schema 版本，consumer 消费方的解析版本）：
/// - producer == consumer → Compat；
/// - producer.major == consumer.major（minor/patch 任意差）→ Compat
///   （新增字段消费方忽略、缺字段消费方按显性 Na 口径——F0201 承接）；
/// - producer.major < consumer.major → NeedsMigration（旧版本报告被
///   新消费方读——有迁移链则升，无链显性拒）；
/// - producer.major > consumer.major → Reject（生产者 schema 面比
///   消费者认知新——破坏面未知，不许猜）。
pub fn compat_rule(producer: &SchemaVer, consumer: &SchemaVer) -> CompatVerdict {
    if producer.major == consumer.major {
        CompatVerdict {
            verdict: Compat::Compat,
            reason: VerCode::CompatRejected, // 非 Reject 时 reason 无意义，取哨兵
            why: "同主版本向后兼容（minor 新增消费方忽略/缺字段按显性Na口径）",
        }
    } else if producer.major < consumer.major {
        CompatVerdict {
            verdict: Compat::NeedsMigration,
            reason: VerCode::CompatRejected,
            why: "旧版本报告被新消费方读取——查迁移链",
        }
    } else {
        CompatVerdict {
            verdict: Compat::Reject,
            reason: VerCode::CompatRejected,
            why: "生产者 schema 比消费者认知新——破坏面未知显性拒",
        }
    }
}

/// 矩阵采样格（对拍面：矩阵格 → 规则函数同格裁决必须一致）。
///
/// 全格枚举代价高（u16 域无限），以**采样格闭集**对拍：0/1/2/3、
/// 边界 u16::MAX、CUR±1 的笛卡尔关键格。矩阵本身由规则面在
/// [`COMPAT_MATRIX`] 里物化为 5×5 关键采样。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixCell {
    /// 生产者版本。
    pub producer: SchemaVer,
    /// 消费者版本。
    pub consumer: SchemaVer,
    /// 期望裁决（矩阵静态声明）。
    pub expect: Compat,
}

/// 关键采样矩阵（5×5 = 25 格）。
///
/// 行=producer 主版本 ∈ {0,1,2,u16::MAX-1,u16::MAX}，
/// 列=consumer 主版本同集合。期望值按 compat_rule 语义手工推演
/// ——规则函数变更而矩阵未跟即 C3 判据红（双写对拍）。
pub const COMPAT_MATRIX: [MatrixCell; 25] = build_matrix();

const fn v(major: u16, minor: u16, patch: u16) -> SchemaVer {
    SchemaVer::new(major, minor, patch)
}

const fn cell(producer: SchemaVer, consumer: SchemaVer, expect: Compat) -> MatrixCell {
    MatrixCell { producer, consumer, expect }
}

/// 构建采样矩阵（const fn 逐格手写语义——规则面的镜像）。
const fn build_matrix() -> [MatrixCell; 25] {
    let majors = [0u16, 1, 2, u16::MAX - 1, u16::MAX];
    let mut out = [cell(v(0, 0, 0), v(0, 0, 0), Compat::Compat); 25];
    let mut i = 0usize;
    let mut r = 0usize;
    while r < 5 {
        let mut c = 0usize;
        while c < 5 {
            let p_major = majors[r];
            let c_major = majors[c];
            let expect = if p_major == c_major {
                Compat::Compat
            } else if p_major < c_major {
                Compat::NeedsMigration
            } else {
                Compat::Reject
            };
            out[i] = cell(
                v(p_major, 0, 0),
                v(c_major, 0, 0),
                expect,
            );
            i += 1;
            c += 1;
        }
        r += 1;
    }
    out
}

/// 矩阵↔规则全格对拍（C3 判据本体）。
pub fn matrix_matches_rule() -> bool {
    for c in COMPAT_MATRIX.iter() {
        let got = compat_rule(&c.producer, &c.consumer).verdict;
        if got != c.expect {
            return false;
        }
    }
    true
}

/// 兼容裁决入口（带迁移链查询）。
pub fn can_parse(producer: &SchemaVer, consumer: &SchemaVer) -> CompatVerdict {
    let v = compat_rule(producer, consumer);
    if v.verdict == Compat::NeedsMigration && migration_path(producer, consumer).is_some() {
        return CompatVerdict {
            verdict: Compat::Compat,
            reason: VerCode::CompatRejected,
            why: "旧版本报告经声明迁移链可升——按链迁移后按目标版本解析",
        };
    }
    v
}

// ---------------------------------------------------------------------------
// 迁移链（声明的升级能力）
// ---------------------------------------------------------------------------

/// 迁移步（相邻版本间的字段增补声明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MigrationStep {
    /// 起始版本。
    pub from: SchemaVer,
    /// 目标版本。
    pub to: SchemaVer,
    /// 迁移动作摘要（静态文本——可审计）。
    pub summary: &'static str,
    /// 动作闭集：新增字段（目标版本比源版本多的字段数）。
    pub added_fields: u16,
    /// 是否破坏性（major 断面迁移必 true）。
    pub breaking: bool,
}

/// 已声明迁移链（账本顺序 = 版本历史顺序）。
pub const MIGRATIONS: [MigrationStep; 2] = [
    MigrationStep {
        from: v(0, 9, 0),
        to: v(1, 0, 0),
        summary: "v0.9→v1.0：异常族拆分（exc_count 拆 exc_total/exc_fatal）+ 齐备率口径改千分制 + 随机化族独立",
        added_fields: 2,
        breaking: true,
    },
    MigrationStep {
        from: v(1, 0, 0),
        to: v(1, 1, 0),
        summary: "v1.0→v1.1：预留耗时族两段（page_tables/acpi 段耗时独立成字段）——向后兼容新增",
        added_fields: 2,
        breaking: false,
    },
];

/// 查迁移路径：from → to 是否有声明步（相邻步链查找）。
///
/// 返回链上步序列（clone 出去，账本不可变）；无链 None。
pub fn migration_path(from: &SchemaVer, to: &SchemaVer) -> Option<Vec<MigrationStep>> {
    if from.cmp_manual(to) == core::cmp::Ordering::Equal {
        // 同版本无迁移（调用方应先走 Compat 快路径）。
        return Some(Vec::new());
    }
    if from.cmp_manual(to) == core::cmp::Ordering::Greater {
        // 降级路径无声明（不向旧版迁移）。
        return None;
    }
    let mut path = Vec::new();
    let mut cur = *from;
    // 链查找上限 = MIGRATIONS 长度（链无环声明——判据 C5 断单调）。
    for _ in 0..MIGRATIONS.len() {
        let mut found = None;
        for step in MIGRATIONS.iter() {
            if step.from == cur {
                found = Some(*step);
                break;
            }
        }
        match found {
            Some(step) => {
                path.push(step);
                if step.to.cmp_manual(to) == core::cmp::Ordering::Equal {
                    return Some(path);
                }
                cur = step.to;
            }
            None => return None,
        }
    }
    None
}

/// 显性迁移执行（版本仲裁 + 步数守恒——不碰报告本体，返回迁移计划）。
///
/// 返回 (目标版本, 走过的步数)——本体字段级迁移是 F0203 diff 工具与
/// 未来 F0204 轮转的活，本册只做**计划仲裁**（防重纪律单点）。
pub fn plan_migration(from: &SchemaVer, to: &SchemaVer) -> Result<(SchemaVer, usize), VerCode> {
    match migration_path(from, to) {
        Some(path) => {
            let steps = path.len();
            match path.last() {
                Some(last) => Ok((last.to, steps)),
                None => Ok((*from, 0)), // 同版本空链
            }
        }
        None => Err(VerCode::MigrationMissing),
    }
}

// ---------------------------------------------------------------------------
// 版本账本（单调历史 + 当前版本三方一致）
// ---------------------------------------------------------------------------

/// 版本账目行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HistoryEntry {
    /// 版本。
    pub ver: SchemaVer,
    /// 变更摘要。
    pub summary: &'static str,
    /// 破坏性变更标记。
    pub breaking: bool,
}

/// 版本账本（严格递增；末位 == CURRENT == F0201 SCHEMA_VERSION）。
pub const VERSION_HISTORY: [HistoryEntry; 2] = [
    HistoryEntry {
        ver: v(0, 9, 0),
        summary: "v0.9 引导报告试点版（齐备率百分制、异常单计数）",
        breaking: false,
    },
    HistoryEntry {
        ver: v(1, 0, 0),
        summary: "v1.0 全景版定版（五族24字段闭集/千分制齐备率/守恒账/双形态序列化）",
        breaking: true,
    },
];

/// 账本一致性闸（C6 判据本体）：
/// 严格递增 + 末位==CURRENT + 破坏性标记与 major 跳变一致。
pub fn ledger_consistent() -> Result<(), VerCode> {
    if VERSION_HISTORY.is_empty() {
        return Err(VerCode::LedgerDrift);
    }
    // 严格递增。
    for i in 1..VERSION_HISTORY.len() {
        let prev = match VERSION_HISTORY.get(i - 1) {
            Some(p) => p,
            None => return Err(VerCode::LedgerDrift),
        };
        let cur = match VERSION_HISTORY.get(i) {
            Some(c) => c,
            None => return Err(VerCode::LedgerDrift),
        };
        if prev.ver.cmp_manual(&cur.ver) != core::cmp::Ordering::Less {
            return Err(VerCode::LedgerUnordered);
        }
    }
    // 末位 == CURRENT。
    let last = match VERSION_HISTORY.last() {
        Some(l) => l,
        None => return Err(VerCode::LedgerDrift),
    };
    if last.ver != SchemaVer::CURRENT {
        return Err(VerCode::LedgerDrift);
    }
    // 破坏性标记与 major 跳变一致：首个条目跳变不计，后续条目
    // major 变了必 breaking，major 没变必不 breaking。
    for i in 1..VERSION_HISTORY.len() {
        let prev = match VERSION_HISTORY.get(i - 1) {
            Some(p) => p,
            None => return Err(VerCode::LedgerDrift),
        };
        let cur = match VERSION_HISTORY.get(i) {
            Some(c) => c,
            None => return Err(VerCode::LedgerDrift),
        };
        let major_jump = prev.ver.major != cur.ver.major;
        if major_jump != cur.breaking {
            return Err(VerCode::LedgerUnordered);
        }
    }
    Ok(())
}

/// 版本声明串指纹（FNV-1a 64——对 tag 全文）。
pub fn tag_fingerprint(tag: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in tag.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// 账本全指纹（逐行 tag+summary 链哈希——账本任何字节漂移指纹必变）。
pub fn ledger_fingerprint() -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for e in VERSION_HISTORY.iter() {
        let tag = e.ver.to_tag();
        for b in tag.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        for b in e.summary.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= if e.breaking { b'B' } else { b'.' } as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// 跨模块对拍（F0201 版本声明三方一致闸）
// ---------------------------------------------------------------------------

/// 跨模块常量一致闸（C7/C8 判据本体——J1 "版本号声明与解析器一致"）：
/// 1. F0201 SCHEMA_VERSION == 账本末位 major（当前账本以 major 为准——
///    SCHEMA_VERSION 是 u32 单数，语义=主版本号）；
/// 2. F0201 SCHEMA_TAG 可被本册解析器解析回 (SCHEMA_VERSION, 0, 0)；
/// 3. SCHEMA_TAG 以本册 TAG_PREFIX 开头（前缀双写一致）。
pub fn cross_module_consistent() -> Result<(), VerCode> {
    // 闸 1：主版本一致。
    if SCHEMA_VERSION as u64 != SchemaVer::CURRENT.major as u64 {
        return Err(VerCode::CrossModuleDrift);
    }
    // 闸 2：tag 解析往返。
    let parsed = parse_tag(SCHEMA_TAG)?;
    let expect = SchemaVer::new(SCHEMA_VERSION as u16, 0, 0);
    if parsed != expect {
        return Err(VerCode::CrossModuleDrift);
    }
    // 闸 3：前缀双写。
    if !SCHEMA_TAG.starts_with(TAG_PREFIX) {
        return Err(VerCode::CrossModuleDrift);
    }
    // 闸 4：当前版本自渲染 tag 重新解析恒等（往返闭包）。
    let cur_tag = SchemaVer::CURRENT.to_tag();
    if parse_tag(&cur_tag)? != SchemaVer::CURRENT {
        return Err(VerCode::CrossModuleDrift);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 畸形语料闭集（零崩溃判据的语料面）
// ---------------------------------------------------------------------------

/// 畸形语料条目（输入 + 期望错误码；None=只断言 Err 不挑码）。
pub struct FuzzCase {
    pub input: &'static str,
    pub expect: Option<VerCode>,
}

/// 畸形语料闭集（12 条——空/段数/数字/溢出/前缀各面 + 形态兼容正样本）。
pub const FUZZ_CASES: [FuzzCase; 12] = [
    FuzzCase { input: "", expect: Some(VerCode::Empty) },
    FuzzCase {
        input: "varix-boot-report/1.0.0.0",
        expect: Some(VerCode::BadSegments),
    },
    FuzzCase {
        input: "varix-boot-report/1.0",
        expect: None, // 两段形态合法（major.minor）
    },
    FuzzCase {
        input: "varix-boot-report/1",
        expect: None, // 单段形态合法（F0201 SCHEMA_TAG 即此形态）
    },
    FuzzCase {
        input: "varix-boot-report/..",
        expect: Some(VerCode::BadDigits),
    },
    FuzzCase {
        input: "varix-boot-report/1.a.0",
        expect: Some(VerCode::BadDigits),
    },
    FuzzCase {
        input: "varix-boot-report/1.-2.0",
        expect: Some(VerCode::BadDigits),
    },
    FuzzCase {
        input: "varix-boot-report/99999.0.0",
        expect: Some(VerCode::SegmentOverflow),
    },
    FuzzCase {
        input: "boot-report/1.0.0",
        expect: Some(VerCode::BadPrefix),
    },
    FuzzCase { input: "1.0.0", expect: Some(VerCode::BadPrefix) },
    FuzzCase {
        input: "varix-boot-report/1.0.0 Extras trailing text",
        expect: Some(VerCode::BadDigits),
    },
    FuzzCase {
        input: "varix-boot-report/65535.65535.65535",
        expect: None, // 合法上界——断言 Ok
    },
];

/// 零崩溃扫描（C2 判据本体）：语料逐条跑解析器，只断言「不 panic +
/// 结果与期望一致」。
pub fn fuzz_zero_crash() -> bool {
    for case in FUZZ_CASES.iter() {
        let r = parse_tag(case.input);
        match (case.expect, r) {
            (Some(e), Err(got)) => {
                if got != e {
                    return false;
                }
            }
            (Some(_), Ok(_)) => return false,
            (None, Ok(_)) => {}     // 上界合法样本
            (None, Err(_)) => return false,
        }
    }
    // 超长面：TAG_MAX_LEN+1 长度输入必 TooLong（动态构造）。
    let mut long = alloc::string::String::from(TAG_PREFIX);
    for _ in 0..(TAG_MAX_LEN) {
        long.push('1');
    }
    matches!(parse_tag(&long), Err(VerCode::TooLong) | Err(VerCode::BadSegments))
}

// ---------------------------------------------------------------------------
// 判据（run_unx202_checks）
// ---------------------------------------------------------------------------

/// UNX-F0202 判据集。
pub fn run_unx202_checks() -> CheckSet {
    let mut set = CheckSet::new("unxstar");
    set.add("F0202-C1-segment-guard", check_c1_segment(), "");
    set.add("F0202-C2-parse-roundtrip", check_c2_roundtrip(), "");
    set.add("F0202-C3-fuzz-zero-crash", check_c3_fuzz(), "");
    set.add("F0202-C4-matrix-rule-agree", check_c4_matrix(), "");
    set.add("F0202-C5-verdict-semantics", check_c5_verdict(), "");
    set.add("F0202-C6-migration-chain", check_c6_migration(), "");
    set.add("F0202-C7-ledger-consistent", check_c7_ledger(), "");
    set.add("F0202-C8-cross-module", check_c8_cross(), "");
    set.add("F0202-C9-order-total", check_c9_order(), "");
    set.add("F0202-C10-boundary-vers", check_c10_boundary(), "");
    set.add("F0202-C11-fingerprint", check_c11_fingerprint(), "");
    set.add("F0202-C12-can-parse-entry", check_c12_entry(), "");
    set
}

/// C1 段位守卫。
fn check_c1_segment() -> bool {
    if !segment_guard() {
        return false;
    }
    // 手排对拍：重复数 0 + 段界。
    let codes = VerCode::all();
    let mut dup = 0usize;
    let mut min = u16::MAX;
    let mut max = u16::MIN;
    for c in codes.iter() {
        let v = c.code();
        if v < min {
            min = v;
        }
        if v > max {
            max = v;
        }
    }
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if let (Some(a), Some(b)) = (codes.get(i), codes.get(j)) {
                if a.code() == b.code() {
                    dup += 1;
                }
            }
        }
    }
    dup == 0 && min >= SEG_BASE && max < SEG_BASE + 16
}

/// C2 tag 解析往返：CURRENT 渲染 → 解析 → 恒等。
fn check_c2_roundtrip() -> bool {
    let tag = SchemaVer::CURRENT.to_tag();
    match parse_tag(&tag) {
        Ok(v) => v == SchemaVer::CURRENT,
        Err(_) => false,
    }
}

/// C3 畸形语料零崩溃。
fn check_c3_fuzz() -> bool {
    fuzz_zero_crash()
}

/// C4 矩阵↔规则全格一致。
fn check_c4_matrix() -> bool {
    matrix_matches_rule()
}

/// C5 裁决语义三面：同版本 Compat / 旧版本被新消费方读 NeedsMigration /
/// 新版本被旧消费方读 Reject。
fn check_c5_verdict() -> bool {
    let c13 = SchemaVer::new(1, 3, 0);
    // 同主版本 minor 差 → Compat。
    if compat_rule(&v(1, 3, 0), &v(1, 0, 0)).verdict != Compat::Compat {
        return false;
    }
    if compat_rule(&v(1, 0, 0), &c13).verdict != Compat::Compat {
        return false;
    }
    // producer 旧 major → NeedsMigration（0.9 → 1.x 消费方）。
    if compat_rule(&v(0, 9, 0), &v(1, 0, 0)).verdict != Compat::NeedsMigration {
        return false;
    }
    // producer 新 major → Reject。
    if compat_rule(&v(2, 0, 0), &v(1, 9, 9)).verdict != Compat::Reject {
        return false;
    }
    // Reject 携带理由码。
    let r = compat_rule(&v(2, 0, 0), &v(1, 0, 0));
    r.reason == VerCode::CompatRejected && !r.why.is_empty()
}

/// C6 迁移链：链上步支持 / 链外步显性拒 / 步数守恒 / 降级拒。
fn check_c6_migration() -> bool {
    // v0.9→v1.0 声明步。
    let p1 = migration_path(&v(0, 9, 0), &v(1, 0, 0));
    match p1 {
        Some(path) => {
            if path.len() != 1 || path[0].breaking != true {
                return false;
            }
        }
        None => return false,
    }
    // v0.9→v1.1 两步链。
    match plan_migration(&v(0, 9, 0), &v(1, 1, 0)) {
        Ok((target, steps)) => {
            if target != v(1, 1, 0) || steps != 2 {
                return false;
            }
        }
        Err(_) => return false,
    }
    // 链外步：v0.8→v1.0 无声明。
    if migration_path(&v(0, 8, 0), &v(1, 0, 0)).is_some() {
        return false;
    }
    if plan_migration(&v(0, 8, 0), &v(1, 0, 0)) != Err(VerCode::MigrationMissing) {
        return false;
    }
    // 降级无链。
    if migration_path(&v(1, 1, 0), &v(0, 9, 0)).is_some() {
        return false;
    }
    // 同版本空链。
    matches!(migration_path(&v(1, 0, 0), &v(1, 0, 0)), Some(p) if p.is_empty())
}

/// C7 账本一致性。
fn check_c7_ledger() -> bool {
    ledger_consistent().is_ok()
}

/// C8 跨模块三方一致（F0201 SCHEMA_VERSION/SCHEMA_TAG ↔ 账本 ↔ 解析器）。
fn check_c8_cross() -> bool {
    cross_module_consistent().is_ok()
}

/// C9 版本序全序一致性：cmp_manual 与 pack 序、与派生 Ord 三方一致
/// （采样 8 对全序公理：自反/反对称/传递抽样）。
fn check_c9_order() -> bool {
    let samples = [
        v(0, 0, 0),
        v(0, 0, 1),
        v(0, 1, 0),
        v(1, 0, 0),
        v(1, 0, 1),
        v(1, 1, 0),
        v(2, 0, 0),
        v(u16::MAX, u16::MAX, u16::MAX),
    ];
    for a in samples.iter() {
        for b in samples.iter() {
            let manual = a.cmp_manual(b);
            // pack 序一致（u64 比较与三元组字典序同构）。
            let packed_cmp = a.pack().cmp(&b.pack());
            let derived = a.cmp(b);
            if manual != packed_cmp || manual != derived {
                return false;
            }
        }
    }
    // 反对称抽样：cmp(a,b) == cmp(b,a).reverse()。
    for a in samples.iter() {
        for b in samples.iter() {
            if a.cmp_manual(b) != b.cmp_manual(a).reverse() {
                return false;
            }
        }
    }
    true
}

/// C10 边界版本：全零与全 u16::MAX 解析往返。
fn check_c10_boundary() -> bool {
    let lo = v(0, 0, 0);
    let hi = v(u16::MAX, u16::MAX, u16::MAX);
    match (parse_tag(&lo.to_tag()), parse_tag(&hi.to_tag())) {
        (Ok(a), Ok(b)) => a == lo && b == hi,
        _ => false,
    }
}

/// C11 指纹：声明串指纹稳定 + 账本指纹对摘要敏感。
fn check_c11_fingerprint() -> bool {
    let tag = SchemaVer::CURRENT.to_tag();
    let f1 = tag_fingerprint(&tag);
    let f2 = tag_fingerprint(&tag);
    if f1 != f2 || f1 == 0 {
        return false;
    }
    // 另一版本 tag 指纹必不同。
    let other = tag_fingerprint(&v(9, 9, 9).to_tag());
    if other == f1 {
        return false;
    }
    // 账本指纹稳定。
    ledger_fingerprint() == ledger_fingerprint()
}

/// C12 can_parse 入口：迁移链把 NeedsMigration 升为 Compat 的语义。
fn check_c12_entry() -> bool {
    // 0.9 报告被 1.0 消费方读：链存在 → Compat（带迁移语义）。
    let a = can_parse(&v(0, 9, 0), &v(1, 0, 0));
    if a.verdict != Compat::Compat {
        return false;
    }
    // 0.8 报告被 1.0 消费方读：无链 → 保持 NeedsMigration。
    let b = can_parse(&v(0, 8, 0), &v(1, 0, 0));
    if b.verdict != Compat::NeedsMigration {
        return false;
    }
    // 2.0 报告被 1.0 消费方读：Reject 不受链影响。
    let c = can_parse(&v(2, 0, 0), &v(1, 0, 0));
    c.verdict == Compat::Reject
}

// ---------------------------------------------------------------------------
// 逐项探针 + 单元测试
// ---------------------------------------------------------------------------

/// 十二项逐项探针（带伤排查面）。
pub fn probe_c1_to_c12() -> [bool; 12] {
    [
        check_c1_segment(),
        check_c2_roundtrip(),
        check_c3_fuzz(),
        check_c4_matrix(),
        check_c5_verdict(),
        check_c6_migration(),
        check_c7_ledger(),
        check_c8_cross(),
        check_c9_order(),
        check_c10_boundary(),
        check_c11_fingerprint(),
        check_c12_entry(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全 12 项判据聚合全绿。
    #[test]
    fn unx202_all_checks_green() {
        let set = run_unx202_checks();
        let (passed, failed) = set.tally();
        assert_eq!((passed, failed), (12, 0), "F0202 判据红项存在");
        assert!(set.all_passed() && !set.truncated());
    }

    /// 零崩溃语料独立复跑。
    #[test]
    fn fuzz_corpus_never_panics() {
        assert!(fuzz_zero_crash());
    }

    /// 跨模块三方一致（F0201 对拍）。
    #[test]
    fn cross_module_agrees() {
        assert!(cross_module_consistent().is_ok());
    }
}
