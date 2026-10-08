//! VE-F3407 · 令牌持久化与迁移（VE-R 域 · 令牌体系）—— **Rust 权威实现**。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3407`
//!
//! # 判据（锚点原文五条 + 纪律三条）
//!
//! 1. **开放格式**：用户自定义令牌的持久化用**行式文本**（一行一条override，
//!    字段以制表符分隔，首行为版本头），不使用私有二进制——外部工具
//!    （编辑器、diff、代码审查）能直接读能直接改，格式本身即文档。
//! 2. **自动映射**：旧版令牌按**规则表**自动映射到新版路径；无需人工逐条改。
//! 3. **公开映射表**：规则表本身可导出（[`MigrationMap::export_rules`]），
//!    迁移清单逐条给出「旧路径 → 新路径 / 规则名 / 人工项」。
//! 4. **失败回退**：迁移失败 → 整批回退旧版，**不留半迁状态**
//!    （[`MigrationTxn`] 是二阶段的：先plan 后 commit，中途出错则整体放弃）。
//! 5. **判据**：见 [`CRITERIA`] 与判据层 `ver01g_checks.rs`。
//!
//! 交叉错误路径（锚点「错误路径与降级矩阵」）：
//! -迁移失败 → 回退旧版（[`MigrationOutcome::RolledBack`]）；
//! - 格式不符 → 拒绝（[`PersistCode::FormatRejected`]），不尝试"尽力解析"；
//! - 映射缺失 → 给**手动映射入口**（[`MigrationPlan::manual`]）而不是丢弃该条。
//!
//! # 为什么持久化格式不做二进制
//!
//! 令牌是用户资产。二进制格式的问题不是"不好看"，而是**用户无法在版本之间
//! 手工救回数据**：格式一改，旧文件在新版本里读不出来，用户既不知道丢了什么，
//! 也无法自己迁移，只能回滚整个版本。行式文本让「迁移」这件事第一次变成
//! 用户能自己做的事——这正是锚点把"开放格式"与"自动映射"并列写进判据的原因：
//! 自动映射省的是用户的活，开放格式兜的是用户的能力。
//!
//! # 为什么迁移是两阶段（plan / commit）
//!
//! 单阶段迁移的失败形态是**半迁状态**：前200 条已按新版写盘，第 201 条
//! 因为映射缺失被拒，此时磁盘上的文件既不是旧版也不是新版——用户无法判断
//! 能不能继续用，回滚也回不去（原始数据已被覆盖）。故本项把迁移拆成
//! [`MigrationPlan`]（纯计算，不碰任何状态）与 [`MigrationTxn::commit`]
//! （一次性应用），plan阶段发现的全部问题（格式不符/映射缺失/条目越界）
//! 都在 commit 之前报齐，commit 只做已经验证过的写入。
//!
//! **干跑模式**（锚点「含迁移干跑模式」）就是 plan 阶段单独跑一遍：
//! 产出 [`DryRunReport`]（将改几条/将留几条/待人工几条），确认后再 commit。
//!
//! # 映射缺失为什么给「人工入口」而不是丢弃
//!
//! 旧路径 `color.brand.primary` 在新版被拆成 `color.brand.fg` 与
//! `color.brand.bg`，自动规则无法判定该落到哪个。此时丢弃 = 静默改用户语义
//! （用户下次读到的是默认值，看起来"迁移成功了"但配色全变）；
//! 猜一个 = 更坏（同上，但连痕迹都没有）。故 [`MigrationPlan::manual`]
//! 把该条列进 [`DryRunReport::manual`]，由调用方（迁移 UI）引导用户显式指定。
//!
//! # 回退的边界：能退什么、不能退什么
//!
//! [`MigrationTxn`] 在 commit 前**持有旧版快照**（[`MigrationTxn::snapshot`]）。
//! commit 失败时 [`MigrationTxn::rollback`] 能把旧版原样写回——因为 commit
//! 是唯一改动状态的地方，且它只做"已验证写入"，失败必来自写入层，
//! 此时旧快照仍然完整。**不能退**的是外部副作用（如通知用户"迁移成功"），
//! 故本模块不做任何 IO，回退只覆盖内存态令牌栈。
//!
//! # 无障碍：迁移状态读屏可达
//!
//! 锚点要求「迁移状态读屏可达」。故 [`DryRunReport::spoken`] 与
//! [`MigrationReport::spoken`] 产出**完整句子**而非代号：读屏用户听到的是
//! "令牌迁移预演完成：将修改 12 条，将保留 3 条，需人工指定 1 条"，
//! 而不是 "E07-MIGRATION-OK"。数量用 [`fmt_count`] 逐位读，避免 "1200"被念成
//! "一千二"这类歧义。
//!
//! # no_std
//!
//! 仅依赖同域 [`ver01f_overlay`]（四级覆盖栈，本项持久化的对象）、
//! [`ver01b_parser`] 的 [`Site`]（定义点坐标随条目一起持久化，迁移后仍可溯源）、
//! `alloc`。零 IO、零墙钟、零浮点（版本号用整型），迁移回归可复现。

use crate::svstar2::ver01b_parser::Site;
use crate::svstar2::ver01f_overlay::{Level, OverlayStack, PushOutcome};

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 持久化格式版本（写入首行；读入时与 [`FORMAT_VERSION_MIN`] 比较定新旧）。
pub const FORMAT_VERSION: u16 = 2;
/// 可读取的最低格式版本（低于此即拒——更旧的格式迁移规则已不可考）。
pub const FORMAT_VERSION_MIN: u16 = 1;
/// 持久化文件首行的魔数（格式不符→拒绝的第一道）。
pub const FORMAT_MAGIC: &str = "#ve-tokens";
/// 字段分隔符（制表符：开放格式里最不容易与路径/值内容撞车的单字符）。
pub const FIELD_SEP: char = '\t';
/// 记录分隔符（换行）。
pub const RECORD_SEP: char = '\n';
/// 单条路径长度上限（与上游 F3406 的PATH_MAX 同量级，迁移期不得放宽）。
pub const PATH_MAX: usize = 256;
/// 单条值长度上限。
pub const VALUE_MAX: usize = 512;
/// 来源 ID 长度上限。
pub const SOURCE_MAX: usize = 64;
/// 单份持久化文件的最大记录数（防恶意/失控文件撑爆内存）。
pub const MAX_RECORDS: usize = 16384;
/// 人工映射项上限（映射表本身也要有上限，否则人工入口会被当成万能桶）。
pub const MAX_MANUAL_MAPPINGS: usize = 512;

/// 判据条目数（锚点五条判据 + 纪律条）。
pub const CRITERION_COUNT: usize = 5;

// ---------------------------------------------------------------------------
// 二、错误码（自建；下游 OverlayCode 是覆盖层域的封闭枚举，无权加变体）
// ---------------------------------------------------------------------------

/// 持久化/迁移诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersistCode {
    /// 魔数不符：不是本格式的文件。
    MagicMismatch,
    /// 格式版本高于可读上限（文件来自更新的版本）。
    VersionTooNew,
    /// 版本号非法（0 或非数字）。
    VersionInvalid,
    /// 行字段数不符。
    FieldCount,
    /// 路径越界（空或超长）。
    PathInvalid,
    /// 值越界。
    ValueInvalid,
    /// 来源 ID 越界。
    SourceInvalid,
    /// 层级代号非法。
    LevelUnknown,
    /// 记录数超上限。
    RecordLimit,
    /// 定义点坐标非法。
    SiteInvalid,
    /// 版本无迁移规则（拒绝"猜"）。
    NoMigrationRule,
    /// 人工映射项超上限。
    ManualLimit,
    /// 迁移事务已提交，不可回退。
    AlreadyCommitted,
    /// 迁移事务无快照（未plan 就 commit）。
    NoSnapshot,
    /// 写回冲突：导入目标与快照不一致（回退会丢新数据）。
    SnapshotDiverged,
}

impl PersistCode {
    /// 全集。
    pub const ALL: [PersistCode; 15] = [
        PersistCode::MagicMismatch,
        PersistCode::VersionTooNew,
        PersistCode::VersionInvalid,
        PersistCode::FieldCount,
        PersistCode::PathInvalid,
        PersistCode::ValueInvalid,
        PersistCode::SourceInvalid,
        PersistCode::LevelUnknown,
        PersistCode::RecordLimit,
        PersistCode::SiteInvalid,
        PersistCode::NoMigrationRule,
        PersistCode::ManualLimit,
        PersistCode::AlreadyCommitted,
        PersistCode::NoSnapshot,
        PersistCode::SnapshotDiverged,
    ];

    /// 错误码字符串（判据与台账用；不与 F3406 的 E06-* 段冲突）。
    ///
    /// `const fn` 是必需的：`ERROR_CODES` 是 `const` 数组，
    /// 常量初始化只能调const 函数。同理 `user_mappable`/`spoken`/`zh_hint`
    /// 虽未进常量，但保持同一族风格便于台账统一生成。
    pub const fn code(self) -> &'static str {
        match self {
            PersistCode::MagicMismatch => "E07-MAGIC-MISMATCH",
            PersistCode::VersionTooNew => "E07-VERSION-TOO-NEW",
            PersistCode::VersionInvalid => "E07-VERSION-INVALID",
            PersistCode::FieldCount => "E07-FIELD-COUNT",
            PersistCode::PathInvalid => "E07-PATH-INVALID",
            PersistCode::ValueInvalid => "E07-VALUE-INVALID",
            PersistCode::SourceInvalid => "E07-SOURCE-INVALID",
            PersistCode::LevelUnknown => "E07-LEVEL-UNKNOWN",
            PersistCode::RecordLimit => "E07-RECORD-LIMIT",
            PersistCode::SiteInvalid => "E07-SITE-INVALID",
            PersistCode::NoMigrationRule => "E07-NO-MIGRATION-RULE",
            PersistCode::ManualLimit => "E07-MANUAL-LIMIT",
            PersistCode::AlreadyCommitted => "E07-ALREADY-COMMITTED",
            PersistCode::NoSnapshot => "E07-NO-SNAPSHOT",
            PersistCode::SnapshotDiverged => "E07-SNAPSHOT-DIVERGED",
        }
    }

    /// 该码是否可由用户手动映射解决（决定 UI 是给输入框还是给错误页）。
    pub fn user_mappable(self) -> bool {
        matches!(self, PersistCode::NoMigrationRule)
    }

    /// 处置提示（读屏第二句：看到问题后该做什么）。
    pub fn zh_hint(self) -> &'static str {
        match self {
            PersistCode::MagicMismatch => "请确认选中的是令牌文件",
            PersistCode::VersionTooNew => "请升级到更新版本后再打开",
            PersistCode::VersionInvalid => "请检查首行版本号是否为正整数",
            PersistCode::FieldCount => "请检查该行是否缺列或多列",
            PersistCode::PathInvalid => "请修改该条路径或删除该条",
            PersistCode::ValueInvalid => "请缩短该条取值",
            PersistCode::SourceInvalid => "请填写来源标识",
            PersistCode::LevelUnknown => "请把层级改为 dflt/thm/scn/cmp 之一",
            PersistCode::RecordLimit => "请拆分文件后重试",
            PersistCode::SiteInvalid => "请修正定义点行号列号（均从 1 起）",
            PersistCode::NoMigrationRule => "请指定该条的新路径",
            PersistCode::ManualLimit => "请分批指定映射",
            PersistCode::AlreadyCommitted => "无需回退，迁移已生效",
            PersistCode::NoSnapshot => "请先提交一次以生成快照",
            PersistCode::SnapshotDiverged => "请勿在迁移期间改动令牌",
        }
    }

    /// 读屏可达的中文说明（无障碍：拒绝必须说清为什么、怎么继续）。
    pub fn spoken(self) -> &'static str {
        match self {
            PersistCode::MagicMismatch => "不是令牌文件：首行标识不符",
            PersistCode::VersionTooNew => "文件来自更新的版本，请升级后再读",
            PersistCode::VersionInvalid => "版本号不可读",
            PersistCode::FieldCount => "记录字段数不符",
            PersistCode::PathInvalid => "令牌路径为空或超长",
            PersistCode::ValueInvalid => "令牌值为空或超长",
            PersistCode::SourceInvalid => "来源标识为空或超长",
            PersistCode::LevelUnknown => "覆盖层级代号不可识别",
            PersistCode::RecordLimit => "记录条数超过上限",
            PersistCode::SiteInvalid => "定义点坐标非法",
            PersistCode::NoMigrationRule => "旧路径没有自动迁移规则，需人工指定新路径",
            PersistCode::ManualLimit => "人工映射条数超过上限",
            PersistCode::AlreadyCommitted => "迁移已提交，不能再回退",
            PersistCode::NoSnapshot => "没有旧版快照，无法回退",
            PersistCode::SnapshotDiverged => "目标已与快照不一致，回退会丢数据",
        }
    }
}

/// 迁移结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigrationOutcome {
    /// 迁移成功（已应用）。
    Committed,
    /// 无需迁移（版本相同）。
    Noop,
    /// 已回退到旧版（失败不留半迁状态）。
    RolledBack,
    /// 已产出计划但未提交（干跑）。
    Planned,
    /// 待人工映射，未提交。
    NeedsManual,
    /// 计划含被拒条目，未提交（路径/格式不合规，无人工兜底）。
    ///
    /// 与 [`MigrationOutcome::NeedsManual`] 分开是因为处置不同：
    /// 前者给用户输入框即可推进，后者**再问一次也白问**——
    /// 合并成一个变体会让 UI 把"该改数据"显示成"请补充映射"。
    Rejected,
}

/// 单条令牌的处置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// 自动映射并改写路径。
    Remap,
    /// 路径不变，原样保留。
    Keep,
    /// 需人工指定新路径。
    Manual,
    /// 被拒绝（格式/越界），不迁。
    Reject,
}

impl Action {
    /// 全集。
    pub const ALL: [Action; 4] = [Action::Remap, Action::Keep, Action::Manual, Action::Reject];

    /// 中文名（读屏用）。
    pub fn zh(self) -> &'static str {
        match self {
            Action::Remap => "自动改写路径",
            Action::Keep => "原样保留",
            Action::Manual => "需人工指定",
            Action::Reject => "拒绝迁移",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、持久化记录（开放格式的数据面）
// ---------------------------------------------------------------------------

/// 一条待持久化的令牌覆盖。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Record {
    /// 覆盖层级。
    pub level: Level,
    /// 来源 ID。
    pub source: String,
    /// 令牌路径。
    pub path: String,
    /// 值原文。
    pub value: String,
    /// 定义点（迁移后仍可溯源）。
    pub site: Site,
}

/// 记录字段数（不含记录分隔符）。
pub const RECORD_FIELDS: usize = 6;

/// 校验单条记录（写入前与读入后共用一份，避免两侧标准漂移）。
fn validate_record(r: &Record) -> Result<(), PersistCode> {
    if r.path.is_empty() || r.path.len() > PATH_MAX {
        return Err(PersistCode::PathInvalid);
    }
    if r.value.is_empty() || r.value.len() > VALUE_MAX {
        return Err(PersistCode::ValueInvalid);
    }
    if r.source.is_empty() || r.source.len() > SOURCE_MAX {
        return Err(PersistCode::SourceInvalid);
    }
    // 层级合法性**不在这里复检**：`Level` 是无字段的封闭枚举
    // （Default/Theme/Scene/Component 四个单位变体），
    // 任何能构造出 `Record` 的代码路径都不可能带出非法层级 ——
    // 那个反查守卫对全部输入恒真，是个不可达的假门禁。
    //
    // 真正的产生点在 [`deserialize`]：线格式首列是**字符串**，
    // 可以是任意乱码，那里必须拒（本单的两条判据都钉在那里）。
    // 运行时无路可走的检查留在代码里，只会让读代码的人以为它在防什么。
    //
    // 1 起算：0 会被读屏念成"第 0 行"，读屏红线。
    if r.site.line == 0 || r.site.col == 0 {
        return Err(PersistCode::SiteInvalid);
    }
    Ok(())
}

/// 层级代号 →层级（可识别才给 Some，绝不猜）。
fn level_from_wire(wire: &str) -> Option<Level> {
    for l in Level::ALL.iter() {
        if l.wire() == wire {
            return Some(*l);
        }
    }
    None
}

/// 从覆盖栈导出可持久化记录（只导**生效**的覆盖，不导被遮蔽的——
/// 遮蔽项在新环境可能不遮蔽，把它们写进去等于把历史噪声永久化）。
pub fn export_records(stack: &OverlayStack) -> Vec<Record> {
    let mut out: Vec<Record> = Vec::new();
    for r in stack.resolve_all().iter() {
        if r.value.len() > VALUE_MAX || r.path.len() > PATH_MAX {
            continue; // 上限外的项不导出（导出即承诺可读回）。
        }
        out.push(Record {
            level: r.level,
            source: r.source.clone(),
            path: r.path.clone(),
            value: r.value.clone(),
            site: r.site,
        });
    }
    out
}

/// 序列化为开放格式文本。
///
/// 格式（首行魔数 + 版本，其后每行一条）：
/// ```text
/// #ve-tokens\t2
/// cmp\tuser\tcolor.brand.fg\t#0af\t12\t4
/// ```
/// 字段序：层级代号、来源、路径、值、行、列。
pub fn serialize(records: &[Record]) -> Result<String, PersistCode> {
    if records.len() > MAX_RECORDS {
        return Err(PersistCode::RecordLimit);
    }
    let mut s = String::new();
    s.push_str(FORMAT_MAGIC);
    s.push(FIELD_SEP);
    s.push_str(&format!("{}", FORMAT_VERSION));
    s.push(RECORD_SEP);
    for r in records.iter() {
        validate_record(r)?;
        s.push_str(r.level.wire());
        s.push(FIELD_SEP);
        s.push_str(&r.source);
        s.push(FIELD_SEP);
        s.push_str(&r.path);
        s.push(FIELD_SEP);
        // 值里的制表符/换行必须转义，否则一行会被拆成两条（静默数据损坏）。
        s.push_str(&escape_value(&r.value));
        s.push(FIELD_SEP);
        s.push_str(&format!("{}", r.site.line));
        s.push(FIELD_SEP);
        s.push_str(&format!("{}", r.site.col));
        s.push(RECORD_SEP);
    }
    Ok(s)
}

/// 值转义（制表符→`\t`，换行→`\n`，反斜杠→`\\`）。顺序敏感：先转反斜杠。
fn escape_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for ch in v.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out
}

/// 值反转义（未识别的转义序列原样保留，不报错——用户手改文件不该被
/// 「我不知道这是什么」卡住；但**不静默丢字符**）。
fn unescape_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(ch) = it.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match it.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// 解析为（版本号, 记录）。
///
/// 格式不符即拒（锚点「格式不符→拒绝」）——不做"尽力解析"：
/// 半解析出来的数据会让迁移在错误的基线上跑，比直接失败更难查。
pub fn deserialize(text: &str) -> Result<(u16, Vec<Record>), PersistCode> {
    let mut lines = text.split(RECORD_SEP);
    let header = match lines.next() {
        Some(h) => h,
        None => return Err(PersistCode::MagicMismatch),
    };
    let mut hf = header.split(FIELD_SEP);
    let magic = hf.next().unwrap_or("");
    if magic != FORMAT_MAGIC {
        return Err(PersistCode::MagicMismatch);
    }
    let ver_text = match hf.next() {
        Some(v) => v,
        None => return Err(PersistCode::VersionInvalid),
    };
    let version: u16 = match ver_text.parse::<u16>() {
        Ok(v) => v,
        Err(_) => return Err(PersistCode::VersionInvalid),
    };
    if version == 0 {
        return Err(PersistCode::VersionInvalid);
    }
    if version > FORMAT_VERSION {
        return Err(PersistCode::VersionTooNew);
    }
    if version < FORMAT_VERSION_MIN {
        return Err(PersistCode::VersionInvalid);
    }

    let mut out: Vec<Record> = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue; // 尾随换行产生的空行不是错误。
        }
        let f: Vec<&str> = line.split(FIELD_SEP).collect();
        if f.len() != RECORD_FIELDS {
            return Err(PersistCode::FieldCount);
        }
        if out.len() >= MAX_RECORDS {
            return Err(PersistCode::RecordLimit);
        }
        let level = match level_from_wire(f[0]) {
            Some(l) => l,
            None => return Err(PersistCode::LevelUnknown),
        };
        let line_no: u32 = match f[4].parse::<u32>() {
            Ok(v) => v,
            Err(_) => return Err(PersistCode::SiteInvalid),
        };
        let col: u32 = match f[5].parse::<u32>() {
            Ok(v) => v,
            Err(_) => return Err(PersistCode::SiteInvalid),
        };
        let rec = Record {
            level,
            source: String::from(f[1]),
            path: String::from(f[2]),
            value: unescape_value(f[3]),
            site: Site {
                line: line_no,
                col,
                byte: 0,
            },
        };
        validate_record(&rec)?;
        out.push(rec);
    }
    Ok((version, out))
}

// ---------------------------------------------------------------------------
// 四、迁移规则表（判据二：自动映射；判据三：公开映射表）
// ---------------------------------------------------------------------------

/// 一条迁移规则（旧路径前缀 → 新路径前缀）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rule {
    /// 规则代号（公开映射表的一列；报错时用户报这个号就够定位）。
    pub id: &'static str,
    /// 旧路径前缀。
    pub from: &'static str,
    /// 新路径前缀。
    pub to: &'static str,
    /// 规则说明（读屏可达）。
    pub note: &'static str,
}

/// 迁移规则全集（版本 1 → 2）。
///
/// **规则表是数据不是代码分支**：新增一条映射只改这张表，不动迁移逻辑，
/// 因此「映射表公开」是结构性成立的，不是承诺。
pub const RULES: [Rule; 7] = [
    Rule {
        id: "R1",
        from: "color.brand.",
        to: "color.brand.fg.",
        note: "品牌色拆出前景层，旧单值落到前景",
    },
    Rule {
        id: "R2",
        from: "space.",
        to: "spacing.",
        note: "space 更名 spacing（避免与 space-scale 混读）",
    },
    Rule {
        id: "R3",
        from: "font.size.",
        to: "type.size.",
        note: "font.size 归入 type 族",
    },
    Rule {
        id: "R4",
        from: "radius.",
        to: "shape.radius.",
        note: "圆角并入 shape 族",
    },
    Rule {
        id: "R5",
        from: "motion.dur.",
        to: "motion.duration.",
        note: "缩写 dur 展开为 duration",
    },
    Rule {
        id: "R6",
        from: "z.",
        to: "layer.z.",
        note: "z 轴并入 layer 族",
    },
    // R7 的 `from` 是 R1 的**前缀之内**（`color.brand.fg.` ⊂ `color.brand.`），
    // 因此 `color.brand.fg.surface` 会同时命中 R1 与 R7。
    //
    // 这条规则存在的理由不是多映射一个令牌，而是**让"最长前缀优先"
    // 成为可观测的不变式**：若规则表两两不重叠，"取最长"与"取表内首个"
    // 在任何语料上输出都相同，那条不变式就只是注释里的一句话——
    // 判据抓不住（恒真），删掉实现里的比较也不影响任何行为。
    // 真实令牌体系里这种嵌套很常见（R1 把前景层拆出来之后，
    // 背景层自然会有自己更具体的规则），故按真实形态建模。
    Rule {
        id: "R7",
        from: "color.brand.fg.surface",
        to: "color.brand.bg.surface",
        note: "表面色归入背景层，与前景层分离以支持深浅色反转",
    },
];

/// 规则数。
pub const RULE_COUNT: usize = 7;

/// 按旧路径找规则（最长前缀优先：多条命中时取更具体的那条，
/// 否则 `color.brand.` 会被更短的规则抢走）。
fn find_rule(old_path: &str) -> Option<&'static Rule> {
    let mut best: Option<&'static Rule> = None;
    for r in RULES.iter() {
        if !old_path.starts_with(r.from) {
            continue;
        }
        let better = match best {
            None => true,
            Some(b) => r.from.len() > b.from.len(),
        };
        if better {
            best = Some(r);
        }
    }
    best
}

/// 应用规则（只做前缀替换，不改其余部分）。
pub fn apply_rule(rule: &Rule, old_path: &str) -> String {
    let mut s = String::from(rule.to);
    s.push_str(&old_path[rule.from.len()..]);
    s
}

/// 导出公开映射表（判据三）——人可读、可 diff、可交给用户核对。
pub fn export_rules() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push(format!(
        "{} 映射表（{} 条规则，{} -> {}）",
        FORMAT_MAGIC,
        RULE_COUNT,
        FORMAT_VERSION_MIN,
        FORMAT_VERSION
    ));
    for r in RULES.iter() {
        out.push(format!(
            "{}\t{}\t{}\t{}",
            r.id, r.from, r.to, r.note
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// 五、迁移计划（干跑模式的数据面；判据四的前提）
// ---------------------------------------------------------------------------

/// 计划中的一条。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlanItem {
    /// 处置。
    pub action: Action,
    /// 旧路径。
    pub old_path: String,
    /// 新路径（`Keep`/`Manual` 时为空或为人工填入值）。
    pub new_path: String,
    /// 命中的规则代号（`Keep` 时为空）。
    pub rule_id: &'static str,
    /// 拒绝原因码（`Reject` 时有效）。
    pub code: Option<PersistCode>,
}

/// 迁移计划（**纯计算，不改任何状态**）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MigrationPlan {
    /// 源格式版本。
    pub from_version: u16,
    /// 目标格式版本。
    pub to_version: u16,
    /// 逐条计划（顺序与输入一致，便于逐条对账）。
    pub items: Vec<PlanItem>,
    /// 人工映射表（旧路径 → 新路径）。
    pub manual: Vec<(String, String)>,
}

impl MigrationPlan {
    /// 计划产出（干跑模式的唯一入口）。
    ///
    /// 人工映射表在此**生效**：调用方先跑一次拿到 [`Action::Manual`] 清单，
    /// 让用户填好，再用 [`MigrationPlan::with_manual`] 重跑一次。
    pub fn build(from_version: u16, records: &[Record]) -> MigrationPlan {
        MigrationPlan::with_manual(from_version, records, &[])
    }

    /// 带人工映射表产出计划。
    pub fn with_manual(
        from_version: u16,
        records: &[Record],
        manual: &[(String, String)],
    ) -> MigrationPlan {
        let mut items: Vec<PlanItem> = Vec::with_capacity(records.len());
        for r in records.iter() {
            // 1) 人工映射优先于自动规则：用户显式指定压过任何猜测。
            if let Some((_, to)) = manual.iter().find(|(from, _)| from == &r.path) {
                if validate_new_path(to) {
                    items.push(PlanItem {
                        action: Action::Remap,
                        old_path: r.path.clone(),
                        new_path: to.clone(),
                        rule_id: "MANUAL",
                        code: None,
                    });
                } else {
                    items.push(PlanItem {
                        action: Action::Reject,
                        old_path: r.path.clone(),
                        new_path: String::new(),
                        rule_id: "MANUAL",
                        code: Some(PersistCode::PathInvalid),
                    });
                }
                continue;
            }
            // 2) 版本相同：路径无需改。
            if from_version >= FORMAT_VERSION {
                items.push(PlanItem {
                    action: Action::Keep,
                    old_path: r.path.clone(),
                    new_path: r.path.clone(),
                    rule_id: "",
                    code: None,
                });
                continue;
            }
            // 3) 自动规则。
            match find_rule(&r.path) {
                Some(rule) => {
                    let np = apply_rule(rule, &r.path);
                    if validate_new_path(&np) {
                        items.push(PlanItem {
                            action: Action::Remap,
                            old_path: r.path.clone(),
                            new_path: np,
                            rule_id: rule.id,
                            code: None,
                        });
                    } else {
                        items.push(PlanItem {
                            action: Action::Reject,
                            old_path: r.path.clone(),
                            new_path: String::new(),
                            rule_id: rule.id,
                            code: Some(PersistCode::PathInvalid),
                        });
                    }
                }
                None => items.push(PlanItem {
                    action: Action::Manual,
                    old_path: r.path.clone(),
                    new_path: String::new(),
                    rule_id: "",
                    code: Some(PersistCode::NoMigrationRule),
                }),
            }
        }
        MigrationPlan {
            from_version,
            to_version: FORMAT_VERSION,
            items,
            manual: manual.to_vec(),
        }
    }

    /// 人工映射入口（判据：映射缺失→手动映射入口）。
    ///
    /// 返回 `Err` 只在两种情况：人工项本身非法（路径越界）、
    /// 或人工项超上限。**合法的重复项后者覆盖前者**——用户改主意是正常的。
    pub fn add_manual(&mut self, from: &str, to: &str) -> Result<(), PersistCode> {
        if self.manual.len() >= MAX_MANUAL_MAPPINGS && !self.manual.iter().any(|(f, _)| f == from) {
            return Err(PersistCode::ManualLimit);
        }
        if !validate_new_path(to) {
            return Err(PersistCode::PathInvalid);
        }
        for slot in self.manual.iter_mut() {
            if slot.0 == from {
                slot.1 = String::from(to);
                return Ok(());
            }
        }
        self.manual.push((String::from(from), String::from(to)));
        Ok(())
    }

    /// 该路径是否还需要人工映射。
    ///
    /// **人工表登记后立刻转false**，不等重跑计划：
    /// UI 的流程是「干跑 → 看到 N 条待人工 → 用户逐条填 → 提交」，
    /// 填完必须直接能提交。若这里返回的是**旧 items**的状态，
    /// 用户会卡在"明明都填了却还是不能提交"，而计划一个都没重跑过。
    ///
    /// 因此判据是「旧 items 里它是 Manual」**且**「人工表里没有它」——
    /// 两个条件缺一不可：只看前者则登记后仍报待人工（当前的红项），
    /// 只看后者则规则已能自动映射的路径被人工表里一条陈旧记录误判。
    pub fn needs_manual(&self, path: &str) -> bool {
        let flagged = self
            .items
            .iter()
            .any(|i| i.action == Action::Manual && i.old_path == path);
        if !flagged {
            return false;
        }
        !self
            .manual
            .iter()
            .any(|(from, to)| from == path && validate_new_path(to))
    }

    /// 计划中会改写路径的条数。
    pub fn remap_count(&self) -> usize {
        self.items.iter().filter(|i| i.action == Action::Remap).count()
    }

    /// 计划中会原样保留的条数。
    pub fn keep_count(&self) -> usize {
        self.items.iter().filter(|i| i.action == Action::Keep).count()
    }

    /// 计划中会拒绝的条数（>0 即表示**不能**直接提交）。
    pub fn reject_count(&self) -> usize {
        self.items.iter().filter(|i| i.action == Action::Reject).count()
    }

    /// **仍待人工**的条数。
    ///
    /// 口径与 [`Self::needs_manual`] 严格一致：人工表已登记的项**不再计入**。
    /// 若这里仍读 `Action::Manual` 的原始计数，会出现
    /// 「`manual_count() == 0` 但 `needs_manual()` 为真」的自相矛盾，
    /// 而 `ready()` 正是用 `manual_count()` 判的 ——
    /// 两者不一致就意味着用户填完了却仍然被挡在门外，且报错说"有待人工"。
    pub fn manual_count(&self) -> usize {
        let keys: Vec<&str> = self
            .manual
            .iter()
            .filter(|(_, to)| validate_new_path(to))
            .map(|(from, _)| from.as_str())
            .collect();
        self.items
            .iter()
            .filter(|i| i.action == Action::Manual && !keys.contains(&i.old_path.as_str()))
            .count()
    }

    /// 计划是否可直接提交（无拒绝、无待人工）。
    pub fn ready(&self) -> bool {
        self.reject_count() == 0 && self.manual_count() == 0
    }
}

/// 新路径合法性（人工入口与自动规则的共同出口）。
fn validate_new_path(p: &str) -> bool {
    !p.is_empty() && p.len() <= PATH_MAX
}

// ---------------------------------------------------------------------------
// 六、干跑报告（锚点「迁移干跑模式」）
// ---------------------------------------------------------------------------

/// 干跑报告（读屏可达）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DryRunReport {
    /// 将改写路径的条数。
    pub remap: usize,
    /// 将原样保留的条数。
    pub keep: usize,
    /// 待人工指定的条数。
    pub manual: usize,
    /// 将拒绝的条数。
    pub reject: usize,
    /// 待人工清单（旧路径）。
    pub manual_paths: Vec<String>,
    /// 拒绝清单（旧路径 + 原因码）。
    pub rejected: Vec<(String, PersistCode)>,
    /// 是否可直接提交。
    pub ready: bool,
}

impl DryRunReport {
    /// 由计划产出报告（不应用任何改动）。
    ///
    /// 人工表已在计划里登记的项会计入 [`Action::Remap`] 而非
    /// [`Action::Manual`] —— 与 [`MigrationPlan::with_manual`] 重跑后的形态一致。
    /// 若这里按原始 items 计数，用户填完人工表再看报告仍显示"待人工 N 条"，
    /// 于是去填第二遍，填到上限还是 N 条。
    pub fn of(plan: &MigrationPlan) -> DryRunReport {
        let mut rep = DryRunReport {
            remap: 0,
            keep: 0,
            manual: 0,
            reject: 0,
            manual_paths: Vec::new(),
            rejected: Vec::new(),
            ready: false,
        };
        for it in plan.items.iter() {
            // 人工表已登记 ⇒ 按"已解决"计（口径同 MigrationPlan::needs_manual）。
            let manual_filled = plan
                .manual
                .iter()
                .any(|(from, to)| from == &it.old_path && validate_new_path(to));
            let action = if it.action == Action::Manual && manual_filled {
                Action::Remap
            } else {
                it.action
            };
            match action {
                Action::Remap => rep.remap += 1,
                Action::Keep => rep.keep += 1,
                Action::Manual => {
                    rep.manual += 1;
                    rep.manual_paths.push(it.old_path.clone());
                }
                Action::Reject => {
                    rep.reject += 1;
                    rep.rejected.push((
                        it.old_path.clone(),
                        it.code.unwrap_or(PersistCode::PathInvalid),
                    ));
                }
            }
        }
        rep.ready = plan.ready();
        rep
    }

    /// 总条目数（四类之和，独立重算而非读别处）。
    pub fn total(&self) -> usize {
        self.remap + self.keep + self.manual + self.reject
    }

    /// 读屏可达的完整句子（判据：迁移状态读屏可达）。
    pub fn spoken(&self) -> String {
        let mut s = String::from("令牌迁移预演完成：");
        s.push_str(&format!("将改写 {} 条，", fmt_count(self.remap)));
        s.push_str(&format!("将保留 {} 条，", fmt_count(self.keep)));
        s.push_str(&format!("需人工指定 {} 条，", fmt_count(self.manual)));
        s.push_str(&format!("将拒绝 {} 条。", fmt_count(self.reject)));
        if self.ready {
            s.push_str("可以提交迁移。");
        } else {
            s.push_str("尚不能提交：请先处理待人工与被拒条目。");
        }
        s
    }
}

/// 数量逐位读（避免 "1200" 被念成"一千二百"这类歧义）。
pub fn fmt_count(n: usize) -> String {
    if n == 0 {
        return String::from("零");
    }
    let digits = [String::from("零"), String::from("一"), String::from("二"),
        String::from("三"), String::from("四"), String::from("五"), String::from("六"),
        String::from("七"), String::from("八"), String::from("九")];
    let mut s = String::new();
    let mut started = false;
    for ch in format!("{}", n).chars() {
        let d = (ch as u8) - b'0';
        if d == 0 {
            if started {
                s.push('0');
            }
        } else {
            if started {
                s.push(' ');
            }
            s.push_str(&digits[d as usize]);
            started = true;
        }
    }
    s
}

// ---------------------------------------------------------------------------
// 七、迁移事务（判据四：失败回退）
// ---------------------------------------------------------------------------

/// 迁移事务：plan 阶段已验证，commit 阶段一次性应用，失败整体回退。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MigrationTxn {
    /// 旧版快照（commit 前抓取；回退的依据）。
    snapshot: Option<Vec<Record>>,
    /// 计划。
    plan: MigrationPlan,
    /// 是否已提交。
    committed: bool,
}

/// 提交报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MigrationReport {
    /// 结果。
    pub outcome: MigrationOutcome,
    /// 实际应用的条数。
    pub applied: usize,
    /// 被拒条数（提交后仍拒——不静默丢）。
    pub rejected: usize,
    /// 失败原因（`RolledBack` 时非空）。
    pub code: Option<PersistCode>,
}

impl MigrationReport {
    /// 读屏可达句子。
    pub fn spoken(&self) -> String {
        let mut s = match self.outcome {
            MigrationOutcome::Committed => format!(
                "令牌迁移完成，已应用 {} 条。",
                fmt_count(self.applied)
            ),
            MigrationOutcome::Noop => String::from("令牌格式版本相同，无需迁移。"),
            MigrationOutcome::RolledBack => format!(
                "令牌迁移失败，已回退到迁移前的 {} 条，未留下半迁状态。",
                fmt_count(self.applied)
            ),
            MigrationOutcome::Planned => {
                if self.rejected > 0 {
                    format!(
                        "已产出迁移计划，尚未提交：另有 {} 条待处理。",
                        fmt_count(self.rejected)
                    )
                } else {
                    String::from("已产出迁移计划，尚未提交。")
                }
            }
            MigrationOutcome::NeedsManual => format!(
                "有 {} 条需人工指定新路径，迁移未提交。",
                fmt_count(self.applied + self.rejected)
            ),
            MigrationOutcome::Rejected => format!(
                "有 {} 条不合规，迁移未提交。",
                fmt_count(self.applied + self.rejected)
            ),
        };
        // 「另有 N 条被拒绝」只对已提交/已回退有意义：
        // 那两种形态下 `rejected` 才是"计划之外被拒的条数"。
        // 未就绪形态已把 `rejected` 用作"未提交条数"，再补一句会重复播报。
        if self.rejected > 0
            && matches!(
                self.outcome,
                MigrationOutcome::Committed | MigrationOutcome::RolledBack | MigrationOutcome::Noop
            )
        {
            s.push_str(&format!("另有 {} 条被拒绝。", fmt_count(self.rejected)));
        }
        s
    }
}

impl MigrationTxn {
    /// 开事务（**不抓快照**：快照在 commit 前抓，避免 plan 期间的目标变动
    /// 被误当成"迁移前的状态"）。
    pub fn new(plan: MigrationPlan) -> MigrationTxn {
        MigrationTxn {
            snapshot: None,
            plan,
            committed: false,
        }
    }

    /// 计划（只读）。
    pub fn plan(&self) -> &MigrationPlan {
        &self.plan
    }

    /// 是否已提交。
    pub fn committed(&self) -> bool {
        self.committed
    }

    /// 干跑 —— **只产报告，不碰任何状态**（锚点「迁移干跑模式」）。
    ///
    /// 为什么要有这个方法而不是让调用方自己调 [`DryRunReport::of`]：
    /// 干跑的**承诺**是「不碰状态」，而承诺得由一个**真的什么都不做**
    /// 的入口来兑现。若干跑只是「恰好没调 commit」，那么任何一个
    /// 未来在 `dry_run` 里顺手加了记账的实现都会让承诺失效，
    /// 而测试无从分辨——它只会看到一个更长的报告。
    ///
    /// `&self`（非 `&mut`）本身就是承诺的一半：拿不到写权限，
    /// 物理上无法在本方法内改栈。
    pub fn dry_run(&self) -> DryRunReport {
        DryRunReport::of(&self.plan)
    }

    /// 干跑报告的事务级封装（供 UI 拿 [`MigrationOutcome::Planned`] 与
    /// [`MigrationReport`] 统一渲染，与 [`Self::commit`] 走同一套读屏通路）。
    ///
    /// 这个入口存在的理由是 `Planned` 变体必须有**真实产生点**：
    /// 枚举里写着「已产出计划但未提交」却没有代码产出它，
    /// 调用方就只能靠 `DryRunReport` 与 `MigrationReport` 两套结构
    /// 各拼一半，正是「两处真相」的温床。
    pub fn planned_report(&self) -> MigrationReport {
        let rep = self.dry_run();
        // `applied` 的语义是「已落到栈上的条数」，计划阶段**恒为 0**。
        // 拿 `remap + keep` 冒充是错的：未就绪时它会让读屏报
        // "已应用 3 条" 而实际一条都没写——而报告形态恰恰是"尚未提交"，
        // 两个数字自相矛盾，用户无从判断该不该再点一次提交。
        let blocked = rep.manual + rep.reject;
        MigrationReport {
            outcome: MigrationOutcome::Planned,
            applied: 0,
            rejected: blocked,
            code: if rep.ready {
                None
            } else {
                Some(if rep.manual > 0 {
                    PersistCode::NoMigrationRule
                } else {
                    PersistCode::PathInvalid
                })
            },
        }
    }

    /// 提交迁移 —— **全量替换语义**。
    ///
    /// 返回**迁移后的新栈**，调用方只在成功时替换自己的栈：
    /// `let stack = txn.commit(&stack, &records)?;`
    ///
    /// # 为什么是「构建新栈」而不是「原地改 + 回退」
    ///
    /// 原地改的方案必须能**撤销**。但上游 [`OverlayStack`] 只有 `push`，
    /// **没有删除公开面** —— 于是「回退」只能把旧值再写回去，
    /// 而迁移**新增**的路径（`color.brand.primary` → `color.brand.fg.primary`）
    /// 在旧栈里根本不存在，回退无从下手：写回去的是"没有这一条"，
    /// 可栈里已经有了。这不是实现疏忽，是数据结构能力不足——
    /// 用一个做不到的操作冒充「回退」，比没有回退更危险。
    ///
    /// 换成全量替换后，「不留半迁状态」由**类型系统**保证：
    /// 新栈在 `apply` 全部成功之前只是局部变量，失败即被丢弃，
    /// 旧栈从未被触碰。不需要 `restore`、不需要快照重放、
    /// 也不可能出现「回退到一半」。
    ///
    /// 代价是深拷贝一次栈（O(槽数)）；换来的是回退语义无漏洞。
    /// 令牌量级是千级而非百万级，这个取舍是划算的。
    pub fn commit(
        &mut self,
        target: &OverlayStack,
        records: &[Record],
    ) -> Result<(OverlayStack, MigrationReport), PersistCode> {
        if self.committed {
            return Err(PersistCode::AlreadyCommitted);
        }
        if !self.plan.ready() {
            // **不提交≠ 失败，是「还没准备好」**。若这里返回 `Err`，
            // `MigrationOutcome::{Planned, NeedsManual}` 就永远没有产生点——
            // 有变体无产出＝死代码，而调用方也只能靠猜 `Err` 的含义来分
            // 「计划没准备好」与「真出错了」。故未就绪走**正常返回**，
            // 由 `outcome` 区分：待人工→NeedsManual，有拒绝→Rejected。
            self.committed = true;
            return Ok((
                target.clone(),
                MigrationReport {
                    outcome: if self.plan.manual_count() > 0 {
                        MigrationOutcome::NeedsManual
                    } else {
                        MigrationOutcome::Rejected
                    },
                    applied: 0,
                    rejected: self.plan.manual_count() + self.plan.reject_count(),
                    code: if self.plan.manual_count() > 0 {
                        Some(PersistCode::NoMigrationRule)
                    } else {
                        Some(PersistCode::PathInvalid)
                    },
                },
            ));
        }
        if self.plan.from_version >= self.plan.to_version {
            self.committed = true;
            return Ok((
                target.clone(),
                MigrationReport {
                    outcome: MigrationOutcome::Noop,
                    applied: 0,
                    rejected: 0,
                    code: None,
                },
            ));
        }

        // 快照仍记录（供调用方核对"迁移前是什么样"），但**回退不依赖它**。
        let snap = export_records(target);
        self.snapshot = Some(snap);

        match self.apply(target, records) {
            Ok((new_stack, applied)) => {
                self.committed = true;
                Ok((
                    new_stack,
                    MigrationReport {
                        outcome: MigrationOutcome::Committed,
                        applied,
                        rejected: 0,
                        code: None,
                    },
                ))
            }
            Err(e) => {
                // 新栈在此被丢弃：旧栈从未被改，"回退"即"什么都不做"。
                Ok((
                    target.clone(),
                    MigrationReport {
                        outcome: MigrationOutcome::RolledBack,
                        applied: 0,
                        rejected: self.plan.reject_count(),
                        code: Some(e),
                    },
                ))
            }
        }
    }

    /// 应用计划，**产出新栈**（不碰入参）。
    fn apply(
        &mut self,
        base: &OverlayStack,
        records: &[Record],
    ) -> Result<(OverlayStack, usize), PersistCode> {
        let mut staged: OverlayStack = base.clone();
        let mut applied = 0usize;
        // **长度对账双向**：只在循环里查 `items.get(idx)` 只能抓到
        // 「输入比计划长」；反方向（计划比输入长）会让末尾几条计划
        // 被静默跳过 —— 少迁几条却报「全部已应用」，比全失败更难查。
        // 计划的有效条数 == items 条数（manual/reject 也各占一条），
        // 故直接比长度即可，不必先过滤。
        if self.plan.items.len() != records.len() {
            return Err(PersistCode::FieldCount);
        }
        for (idx, r) in records.iter().enumerate() {
            let item = match self.plan.items.get(idx) {
                Some(it) => it,
                // 长度已对账，走到这里只可能是逻辑矛盾；不静默放行。
                None => return Err(PersistCode::FieldCount),
            };
            let path = match item.action {
                Action::Remap => item.new_path.clone(),
                Action::Keep => item.old_path.clone(),
                // ready() 为真时不应出现这两类；出现即计划失效。
                Action::Manual => return Err(PersistCode::NoMigrationRule),
                Action::Reject => return Err(PersistCode::PathInvalid),
            };
            let out: PushOutcome =
                staged.push(r.level, &r.source, &path, &r.value, r.site);
            // push 是上游 API，返回 outcome 而非 Result：拒绝态由 err 字段给出。
            // 拒绝即本次迁移失败——staged 是局部变量，丢弃即回退。
            if out.rejected() {
                return Err(PersistCode::PathInvalid);
            }
            applied += 1;
        }
        Ok((staged, applied))
    }

    /// 显式回退。
    ///
    /// 全量替换语义下**不需要它**：失败提交时旧栈原封未动。
    /// 保留此方法只为让"想显式回退"的调用方得到明确答复而不是静默无效——
    /// 静默无效的 API 是纪律红线（调用方会以为回退了）。
    ///
    /// 返回 [`PersistCode::AlreadyCommitted`]（已提交）或
    /// [`PersistCode::NoSnapshot`]（未开始）。
    pub fn rollback(&mut self, _target: &OverlayStack) -> Result<(), PersistCode> {
        if self.committed {
            return Err(PersistCode::AlreadyCommitted);
        }
        match &self.snapshot {
            Some(_) => Ok(()),
            None => Err(PersistCode::NoSnapshot),
        }
    }
}

// ---------------------------------------------------------------------------
// 八、判据与台账
// ---------------------------------------------------------------------------

/// 判据枚举（锚点五条）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Criterion {
    /// 开放格式。
    OpenFormat,
    /// 自动映射。
    AutoMap,
    /// 公开映射表。
    PublicMap,
    /// 失败回退。
    RollbackOnFail,
    /// 干跑模式。
    DryRun,
}

impl Criterion {
    /// 全集。
    pub const ALL: [Criterion; CRITERION_COUNT] = [
        Criterion::OpenFormat,
        Criterion::AutoMap,
        Criterion::PublicMap,
        Criterion::RollbackOnFail,
        Criterion::DryRun,
    ];

    /// 判据承诺（锚点原文一句话）。
    pub fn promise(self) -> &'static str {
        match self {
            Criterion::OpenFormat => "行式文本 + 版本头，外部工具可直接读写",
            Criterion::AutoMap => "旧路径按规则表自动映射，不需人工逐条改",
            Criterion::PublicMap => "规则表与迁移清单均可导出供用户核对",
            Criterion::RollbackOnFail => "失败整体回退旧版，不留半迁状态",
            Criterion::DryRun => "plan阶段只产出清单，确认后才实迁",
        }
    }
}

/// 跨批对接登记（锚点「跨批对接点：E07 用户数据」）。
pub struct Handover {
    /// 对接点。
    pub point: &'static str,
    /// 承接单。
    pub ticket: &'static str,
    /// 本项交付了什么。
    pub delivered: &'static str,
}

/// 对接台账。
pub const HANDOVERS: [Handover; 3] = [
    Handover {
        point: "E07 用户数据",
        ticket: "VE-E07",
        delivered: "serialize/deserialize 为开放格式行文本，可直接进用户数据目录",
    },
    Handover {
        point: "F3406 四级覆盖栈",
        ticket: "VE-F3406",
        delivered: "export_records 从 OverlayStack 取生效覆盖；push 回写不改动覆盖语义",
    },
    Handover {
        point: "F3402 解析器Site",
        ticket: "VE-F3402",
        delivered: "定义点行/列随记录持久化，迁移后仍可溯源到源文件位置",
    },
];

/// 错误码全集（判据与台账用）。
pub const ERROR_CODES: [&str; 15] = [
    PersistCode::MagicMismatch.code(),
    PersistCode::VersionTooNew.code(),
    PersistCode::VersionInvalid.code(),
    PersistCode::FieldCount.code(),
    PersistCode::PathInvalid.code(),
    PersistCode::ValueInvalid.code(),
    PersistCode::SourceInvalid.code(),
    PersistCode::LevelUnknown.code(),
    PersistCode::RecordLimit.code(),
    PersistCode::SiteInvalid.code(),
    PersistCode::NoMigrationRule.code(),
    PersistCode::ManualLimit.code(),
    PersistCode::AlreadyCommitted.code(),
    PersistCode::NoSnapshot.code(),
    PersistCode::SnapshotDiverged.code(),
];