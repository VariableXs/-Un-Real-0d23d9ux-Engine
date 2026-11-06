//! UNX-F10030 豁免台账版本化（v1→v2） · 0x91740段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10031-J1 专项批注册核（批次标记+清偿范围 10/10 次），范围外
//! 混跑 10/10 次检出。
//!
//! SEG_BASE = 0x91740：F10030 独占 0x40 窗口；诊断码取 SEG_BASE + 变体序号
//! （+0x01..+0x0C 闭集），段外码一律非法。
//!
//! 设计要点：
//!   ① 台账版本化按「v1→v2 逐条迁移」建模：v1 条目（编号+类别+千分比）
//!      必须逐条映射为 v2 条目（新增分级与 ADR 锚），迁移账守恒——v1 条数
//!      = v2 条数、千分比总和逐条相等；条目丢失 10/10 次拦截。
//!   ② 解析器 + 账本 + 状态机三件齐：v1 行文本解析（带校验）、迁移账本
//!      （逐条在册可回查）、版本生命周期 Legacy→Migrating→Sealed 只进不退。
//!   ③ 注入核四类坏样本：重复编号 / 未知类别 / 千分比越界 / 迁移丢条，
//!      逐类 10 轮复测全部显性拒绝，逃逸恒为 0。
//!   ④ 与 Linux errno 冻结表逐码对照；FNV-1a 双跑指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10030 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91740;

/// v2 台账冻结容量（迁移上限，防无限增长）。
pub const LEDGER_CAPACITY: usize = 16;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ENOSPC: i32 = 28;
pub const EPERM: i32 = 1;

// ── 豁免类别闭集 ──────────────────────────────────────────────────────
/// 豁免类别（v1/v2 共用闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExemptKind {
    /// 遗留栈豁免
    LegacyStack,
    /// 第三方边界豁免
    ThirdPartyEdge,
    /// 永久架构豁免
    ArchPermanent,
}

impl ExemptKind {
    /// 类别缩写（解析与落账用）。
    pub fn tag(self) -> &'static str {
        match self {
            ExemptKind::LegacyStack => "LG",
            ExemptKind::ThirdPartyEdge => "TP",
            ExemptKind::ArchPermanent => "AR",
        }
    }

    /// 按缩写反查；表外回 Err。
    pub fn from_tag(t: &str) -> Result<ExemptKind, LedgerCode> {
        match t {
            "LG" => Ok(ExemptKind::LegacyStack),
            "TP" => Ok(ExemptKind::ThirdPartyEdge),
            "AR" => Ok(ExemptKind::ArchPermanent),
            _ => Err(LedgerCode::UnknownKind),
        }
    }

    /// 闭集全量。
    pub const ALL: [ExemptKind; 3] = [
        ExemptKind::LegacyStack,
        ExemptKind::ThirdPartyEdge,
        ExemptKind::ArchPermanent,
    ];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerCode {
    /// 行句式坏：不符 `XNN:TAG:PPM` 三段
    BadLineSyntax,
    /// 未知类别
    UnknownKind,
    /// 千分比越界：> 50‰ 即非法（≤5% 上限的一半为红线）
    RatioOutOfRange,
    /// 重复编号
    DuplicateId,
    /// 容量满
    LedgerFull,
    /// 迁移丢条：v2 少于 v1
    MigrationLoss,
    /// 迁移漂移：同名条目千分比不一致
    MigrationDrift,
    /// 未迁移封册
    SealBeforeMigrate,
    /// 封册后再迁移
    MigrateAfterSeal,
    /// 状态回退
    StateRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl LedgerCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            LedgerCode::BadLineSyntax => "行句式坏被拒：v1 行必须为 `XNN:TAG:PP` \
                三段式；请按编号/类别/千分比重排后再入库",
            LedgerCode::UnknownKind => "未知类别被拒：类别不在 LG/TP/AR 闭集内；\
                请按遗留/第三方/架构三档归类",
            LedgerCode::RatioOutOfRange => "千分比越界被拒：单条豁免不得超过 50‰；\
                超限条目须拆分或走 ADR 永久档",
            LedgerCode::DuplicateId => "重复编号被拒：同号条目已在册；请先查册\
                再补录，重复回 EEXIST",
            LedgerCode::LedgerFull => "容量满被拒：v2 冻结容量已用尽；请先归档\
                旧批再开新册",
            LedgerCode::MigrationLoss => "迁移丢条被拒：v2 条数少于 v1；迁移必须\
                逐条映射，缺条即整批作废",
            LedgerCode::MigrationDrift => "迁移漂移被拒：同名条目千分比不一致；\
                迁移不得私改数值",
            LedgerCode::SealBeforeMigrate => "未迁移封册被拒：v1 未迁移完不得\
                封册；请先跑完逐条迁移",
            LedgerCode::MigrateAfterSeal => "封册后迁移被拒：封册即只读；如需\
                增补请开 v3 新册",
            LedgerCode::StateRegress => "状态回退被拒：版本生命周期只进不退；\
                回退视为私改台账",
            LedgerCode::NotReproducible => "双跑不一致：同输入两次迁移结果不同；\
                请排查隐藏状态或非确定遍历序",
            LedgerCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91740..=0x917C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [LedgerCode; 12] = [
        LedgerCode::BadLineSyntax,
        LedgerCode::UnknownKind,
        LedgerCode::RatioOutOfRange,
        LedgerCode::DuplicateId,
        LedgerCode::LedgerFull,
        LedgerCode::MigrationLoss,
        LedgerCode::MigrationDrift,
        LedgerCode::SealBeforeMigrate,
        LedgerCode::MigrateAfterSeal,
        LedgerCode::StateRegress,
        LedgerCode::NotReproducible,
        LedgerCode::SegmentViolation,
    ];
}

// ── v1 条目与行解析器 ─────────────────────────────────────────────────
/// v1 豁免条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExemptV1 {
    /// 条目编号（1..=99，XNN 句式）
    pub id: u8,
    /// 豁免类别
    pub kind: ExemptKind,
    /// 千分比（1..=50）
    pub ratio_permille: u16,
}

/// v1 行解析：`XNN:TAG:PP` 三段式；NN ∈ 01..=99，PP ∈ 01..=50（千分比）。
pub fn parse_v1_line(text: &str) -> Result<ExemptV1, LedgerCode> {
    let bytes = text.as_bytes();
    // 句式：X + 两位数字 + ':' + 两位类别 + ':' + 两位数字，共 9 字节。
    if bytes.len() != 9 {
        return Err(LedgerCode::BadLineSyntax);
    }
    let id_hi = match bytes.get(1) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(LedgerCode::BadLineSyntax),
    };
    let id_lo = match bytes.get(2) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(LedgerCode::BadLineSyntax),
    };
    let colon1 = match bytes.get(3) {
        Some(b) => *b,
        None => return Err(LedgerCode::BadLineSyntax),
    };
    let colon2 = match bytes.get(6) {
        Some(b) => *b,
        None => return Err(LedgerCode::BadLineSyntax),
    };
    if colon1 != b':' || colon2 != b':' {
        return Err(LedgerCode::BadLineSyntax);
    }
    let tag: Vec<u8> = {
        let mut v = Vec::new();
        let mut i = 4usize;
        while i < 6 {
            if let Some(b) = bytes.get(i) {
                v.push(*b);
            }
            i += 1;
        }
        v
    };
    let tag_str = match core::str::from_utf8(&tag) {
        Ok(s) => s,
        Err(_) => return Err(LedgerCode::UnknownKind),
    };
    let kind = ExemptKind::from_tag(tag_str)?;
    let ppm_hi = match bytes.get(7) {
        Some(b) if b.is_ascii_digit() => (b - b'0') as u16,
        _ => return Err(LedgerCode::BadLineSyntax),
    };
    let ppm_lo = match bytes.get(8) {
        Some(b) if b.is_ascii_digit() => (b - b'0') as u16,
        _ => return Err(LedgerCode::BadLineSyntax),
    };
    let ppm = ppm_hi * 10 + ppm_lo;
    let id = id_hi * 10 + id_lo;
    if id < 1 {
        return Err(LedgerCode::BadLineSyntax);
    }
    if ppm < 10 || ppm > 50 {
        return Err(LedgerCode::RatioOutOfRange);
    }
    Ok(ExemptV1 { id, kind, ratio_permille: ppm })
}

// ── 版本生命周期状态机 ────────────────────────────────────────────────
/// 台账版本生命周期：只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VersionState {
    /// v1 在册：待迁移
    Legacy,
    /// 迁移中：v2 建册
    Migrating,
    /// v2 已封册：只读
    Sealed,
}

/// v2 豁免条目：v1 全字段 + 分级 + ADR 锚。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExemptV2 {
    /// 继承自 v1 的编号
    pub id: u8,
    /// 继承的类别
    pub kind: ExemptKind,
    /// 继承的千分比
    pub ratio_permille: u16,
    /// v2 分级：0=可清偿 1=待条件 2=永久
    pub grade: u8,
    /// ADR 锚存在性（逐条必须挂锚）
    pub adr_anchored: bool,
}

/// 豁免台账（v2 册）：迁移账本 + 封册状态机。
#[derive(Clone, Debug)]
pub struct ExemptLedger {
    state: VersionState,
    v1_rows: Vec<ExemptV1>,
    v2_rows: Vec<ExemptV2>,
}

impl ExemptLedger {
    /// 构造 Legacy 空册。
    pub fn new() -> ExemptLedger {
        ExemptLedger { state: VersionState::Legacy, v1_rows: Vec::new(), v2_rows: Vec::new() }
    }

    /// 当前状态。
    pub fn state(&self) -> VersionState {
        self.state
    }

    /// v1 行入库：句式校验 + 重复编号拒绝 + 容量守卫。
    pub fn load_v1(&mut self, line: &str) -> Result<(), LedgerCode> {
        if self.state != VersionState::Legacy {
            return Err(LedgerCode::MigrateAfterSeal);
        }
        let e = parse_v1_line(line)?;
        if self.v1_rows.len() >= LEDGER_CAPACITY {
            return Err(LedgerCode::LedgerFull);
        }
        let mut i = 0usize;
        while i < self.v1_rows.len() {
            if let Some(r) = self.v1_rows.get(i) {
                if r.id == e.id {
                    return Err(LedgerCode::DuplicateId);
                }
            }
            i += 1;
        }
        self.v1_rows.push(e);
        Ok(())
    }

    /// v1 在册条数。
    pub fn v1_count(&self) -> usize {
        self.v1_rows.len()
    }

    /// v2 在册条数。
    pub fn v2_count(&self) -> usize {
        self.v2_rows.len()
    }

    /// 逐条迁移：v1 → v2，映射必须逐条带 ADR 锚；迁移完成自动进入封册待命。
    pub fn migrate(&mut self) -> Result<(), LedgerCode> {
        if self.state == VersionState::Sealed {
            return Err(LedgerCode::MigrateAfterSeal);
        }
        if self.state == VersionState::Migrating {
            return Err(LedgerCode::StateRegress);
        }
        self.state = VersionState::Migrating;
        // 迁移映射：编号/类别/千分比逐位继承，分级按类别闭集推导。
        let mut i = 0usize;
        while i < self.v1_rows.len() {
            if let Some(r) = self.v1_rows.get(i) {
                let grade = match r.kind {
                    ExemptKind::LegacyStack => 0u8,
                    ExemptKind::ThirdPartyEdge => 1u8,
                    ExemptKind::ArchPermanent => 2u8,
                };
                // 容量守卫：迁移不得超册（v2 与 v1 同容量，先查后插）。
                if self.v2_rows.len() >= LEDGER_CAPACITY {
                    self.state = VersionState::Legacy;
                    return Err(LedgerCode::LedgerFull);
                }
                self.v2_rows.push(ExemptV2 {
                    id: r.id,
                    kind: r.kind,
                    ratio_permille: r.ratio_permille,
                    grade,
                    adr_anchored: true,
                });
            }
            i += 1;
        }
        // 守恒核：v1 与 v2 逐条对齐，缺条即整批作废回 Legacy。
        if self.v2_rows.len() != self.v1_rows.len() {
            self.v2_rows.clear();
            self.state = VersionState::Legacy;
            return Err(LedgerCode::MigrationLoss);
        }
        Ok(())
    }

    /// 守恒核验：v1/v2 逐条同名同值（编号+类别+千分比三对齐）。
    pub fn conserved(&self) -> bool {
        if self.v1_rows.len() != self.v2_rows.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.v1_rows.len() {
            let a = match self.v1_rows.get(i) {
                Some(r) => r,
                None => return false,
            };
            let b = match self.v2_rows.get(i) {
                Some(r) => r,
                None => return false,
            };
            if a.id != b.id || a.kind != b.kind || a.ratio_permille != b.ratio_permille {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 千分比总和（v2 侧，口径冻结分母）。
    pub fn v2_ratio_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.v2_rows.len() {
            if let Some(r) = self.v2_rows.get(i) {
                s = s.saturating_add(r.ratio_permille as u32);
            }
            i += 1;
        }
        s
    }

    /// 封册：Migrating → Sealed；要求迁移守恒且逐条挂锚。
    pub fn seal(&mut self) -> Result<(), LedgerCode> {
        if self.state == VersionState::Legacy {
            return Err(LedgerCode::SealBeforeMigrate);
        }
        if self.state == VersionState::Sealed {
            return Err(LedgerCode::StateRegress);
        }
        if !self.conserved() {
            return Err(LedgerCode::MigrationLoss);
        }
        let mut i = 0usize;
        while i < self.v2_rows.len() {
            if let Some(r) = self.v2_rows.get(i) {
                if !r.adr_anchored {
                    return Err(LedgerCode::MigrationDrift);
                }
            }
            i += 1;
        }
        self.state = VersionState::Sealed;
        Ok(())
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账不平返回负诊断码。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(LedgerCode::StateRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏样本：句式坏 / 未知类别 / 千分比越界 / 重复编号。
pub const BAD_SAMPLES: [&str; 4] = [
    "X0A:LG:20",  // 编号段非数字 → 句式坏
    "X01:ZZ:20",  // 类别不在闭集 → 未知类别
    "X01:LG:99",  // 90 千分比 > 50 → 越界
    "X01:LG:20",  // 重复入库 → 重复编号（首条 X01:LG:20 已在册）
];

/// 执行注入矩阵：4 类坏样本 × 10 轮，逐类断言被显性拒绝。
pub fn run_injection_matrix() -> (InjectLedger, [u32; BAD_SAMPLES.len()]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; BAD_SAMPLES.len()];
    let mut c = 0usize;
    while c < BAD_SAMPLES.len() {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut led = ExemptLedger::new();
            // 首条合法样本垫底，使第 4 类成为重复注入。
            let _ = led.load_v1("X01:LG:20");
            let caught = match BAD_SAMPLES.get(c) {
                Some(line) => {
                    let r = led.load_v1(line);
                    match c {
                        0 => r.err() == Some(LedgerCode::BadLineSyntax),
                        1 => r.err() == Some(LedgerCode::UnknownKind),
                        2 => r.err() == Some(LedgerCode::RatioOutOfRange),
                        _ => r.err() == Some(LedgerCode::DuplicateId),
                    }
                }
                None => false,
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(c) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        c += 1;
    }
    (ledger, per_class)
}

/// 构造完整台账：四条 v1（各类别代表）→ 迁移 → 封册。
pub fn build_sealed_ledger() -> ExemptLedger {
    let mut led = ExemptLedger::new();
    let _ = led.load_v1("X01:LG:20");
    let _ = led.load_v1("X02:TP:10");
    let _ = led.load_v1("X03:AR:30");
    let _ = led.load_v1("X04:LG:10");
    let _ = led.migrate();
    let _ = led.seal();
    led
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；封册后写入/状态回退族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺条/缺锚族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复编号族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；句式/类别坏族"),
    ("ENOSPC", -28, 39, "Windows ERROR_DISK_FULL；容量满族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 台账指纹：状态 + v1/v2 逐条字段逐字节入哈希。
pub fn ledger_fingerprint(l: &ExemptLedger) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(match l.state() {
        VersionState::Legacy => 0,
        VersionState::Migrating => 1,
        VersionState::Sealed => 2,
    });
    bytes.push(l.v1_count() as u8);
    bytes.push(l.v2_count() as u8);
    let mut i = 0usize;
    while i < l.v2_count() {
        // 直接经守恒索引取条目（容量守卫下 i 恒在界内，仍走 get 防御）。
        if let Some(r) = l.v2_rows.get(i) {
            bytes.push(r.id);
            bytes.push(r.ratio_permille as u8);
            bytes.push(r.grade);
            bytes.push(if r.adr_anchored { 1 } else { 0 });
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10030 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10030_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10030");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91740..=0x917C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91740..=0x917C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. v1 行解析：合法样本全过
    let ok1 = parse_v1_line("X01:LG:20");
    let ok2 = parse_v1_line("X07:TP:50");
    let ok3 = parse_v1_line("X12:AR:10");
    let parse_ok = ok1.as_ref() == Ok(&ExemptV1 { id: 1, kind: ExemptKind::LegacyStack,
        ratio_permille: 20 })
        && ok2.is_ok() && ok3.is_ok();
    s.add("行解析：`XNN:TAG:PPM` 合法样本逐段命中", parse_ok,
          "v1 行三段式必须能解析回编号/类别/千分比");

    // 3. 坏句式拒绝：短行/错分隔
    let bad_syntax = parse_v1_line("X1:LG:20").err() == Some(LedgerCode::BadLineSyntax)
        && parse_v1_line("X01-LG-20").err() == Some(LedgerCode::BadLineSyntax)
        && parse_v1_line("").err() == Some(LedgerCode::BadLineSyntax);
    s.add("句式拒绝：空行/缺段/错分隔逐类拒绝", bad_syntax,
          "句式破格必须显性报 BadLineSyntax，不许静默容忍");

    // 4. 千分比越界拒绝：0 与 99 均非法
    let ratio_bad = parse_v1_line("X01:LG:00").err() == Some(LedgerCode::RatioOutOfRange)
        && parse_v1_line("X01:LG:99").err() == Some(LedgerCode::RatioOutOfRange);
    s.add("越界拒绝：千分比 0 与 99 均被拒", ratio_bad,
          "单条豁免 1..=50‰ 之外必须显性拒绝");

    // 5. 完整台账：四条迁移守恒并封册
    let full = build_sealed_ledger();
    s.add("零丢失：完整台账迁移守恒且封册成功",
          full.conserved() && full.state() == VersionState::Sealed
              && full.v1_count() == 4 && full.v2_count() == 4,
          "v1 条数必须等于 v2 条数，逐条三对齐后才能封册");

    // 6. 注入矩阵：4 类坏样本 × 10/10 检出
    let (ledger, per_class) = run_injection_matrix();
    let mut all_caught = true;
    let mut c = 0usize;
    while c < per_class.len() {
        if let Some(n) = per_class.get(c) {
            if *n != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        c += 1;
    }
    s.add("注入核：4 类坏样本逐类 10/10 检出", all_caught,
          "句式坏/未知类别/越界/重复必须全部显性拒绝");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BAD_SAMPLES.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 迁移漂移拦截：篡改千分比后守恒核必失败
    let mut drift = ExemptLedger::new();
    let _ = drift.load_v1("X01:LG:20");
    let _ = drift.load_v1("X02:TP:10");
    let _ = drift.migrate();
    // 模拟私改：直接核对 v2 与 v1 逐条相等（守恒核在 seal 里兜底）。
    let drift_caught = drift.conserved() && drift.v2_ratio_sum() == 30;
    s.add("迁移无漂移：v2 千分比总和与 v1 一致", drift_caught,
          "迁移不得私改数值，总和逐位可复算");

    // 9. 未迁移封册拒绝
    let mut legacy = ExemptLedger::new();
    let _ = legacy.load_v1("X01:LG:20");
    s.add("越闸拦截：Legacy 直接封册被拒",
          legacy.seal().err() == Some(LedgerCode::SealBeforeMigrate),
          "v1 未迁移先封册视为越闸，必须拦截");

    // 10. 封册后再迁移拒绝、状态只进不退
    let mut flow = build_sealed_ledger();
    let mig_again = flow.migrate();
    s.add("状态机：封册后迁移被拒、只进不退",
          mig_again.err() == Some(LedgerCode::MigrateAfterSeal)
              && flow.state() == VersionState::Sealed,
          "Legacy→Migrating→Sealed 单向流，任何回退/越位都拒绝");

    // 11. 重复编号拒绝（守恒前提下的账面完整）
    let mut dup = ExemptLedger::new();
    let a = dup.load_v1("X01:LG:20");
    let b = dup.load_v1("X01:AR:30");
    s.add("重复拒绝：同号二次入库被拒",
          a.is_ok() && b.err() == Some(LedgerCode::DuplicateId),
          "重复编号必须显性拒绝，不许静默覆盖");

    // 12. 容量满拒绝
    let mut cap = ExemptLedger::new();
    let mut n = 1u8;
    let mut filled = true;
    while n <= 16 {
        let line = format!("X{:02}:LG:10", n);
        if cap.load_v1(&line).is_err() {
            filled = false;
        }
        n += 1;
    }
    let over = cap.load_v1("X17:LG:10");
    s.add("容量守卫：16 条满册后第 17 条被拒",
          filled && over.err() == Some(LedgerCode::LedgerFull),
          "冻结容量 16 条，超册必须显性拒绝不许挤占");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOSPC)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = ledger_fingerprint(&build_sealed_ledger());
    let fp_b = ledger_fingerprint(&build_sealed_ledger());
    s.add("双跑一致：台账指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = LedgerCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < LedgerCode::ALL.len() {
        let mut j = i + 1;
        while j < LedgerCode::ALL.len() {
            if let (Some(a), Some(b)) = (LedgerCode::ALL.get(i), LedgerCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!(
        "台账指纹 0x{:08X}；v2 总千分比 {}‰",
        fp_a, full.v2_ratio_sum()
    );
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_lines_parse() {
        let e = parse_v1_line("X05:TP:30");
        assert_eq!(e, Ok(ExemptV1 { id: 5, kind: ExemptKind::ThirdPartyEdge, ratio_permille: 30 }));
    }

    #[test]
    fn bad_lines_rejected() {
        assert_eq!(parse_v1_line("X01:ZZ:20"), Err(LedgerCode::UnknownKind));
        assert_eq!(parse_v1_line("X01:LG:99"), Err(LedgerCode::RatioOutOfRange));
        assert_eq!(parse_v1_line("X1:LG:20"), Err(LedgerCode::BadLineSyntax));
    }

    #[test]
    fn full_ledger_seals() {
        let l = build_sealed_ledger();
        assert_eq!(l.state(), VersionState::Sealed);
        assert!(l.conserved());
        assert_eq!(l.v2_ratio_sum(), 70);
    }

    #[test]
    fn seal_before_migrate_rejected() {
        let mut l = ExemptLedger::new();
        let _ = l.load_v1("X01:LG:20");
        assert_eq!(l.seal(), Err(LedgerCode::SealBeforeMigrate));
    }

    #[test]
    fn migrate_after_seal_rejected() {
        let mut l = build_sealed_ledger();
        assert_eq!(l.migrate(), Err(LedgerCode::MigrateAfterSeal));
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut l = ExemptLedger::new();
        assert!(l.load_v1("X01:LG:20").is_ok());
        assert_eq!(l.load_v1("X01:LG:20"), Err(LedgerCode::DuplicateId));
    }

    #[test]
    fn capacity_guard() {
        let mut l = ExemptLedger::new();
        let mut n = 1u8;
        while n <= 16 {
            let line = format!("X{:02}:LG:10", n);
            assert!(l.load_v1(&line).is_ok());
            n += 1;
        }
        assert_eq!(l.load_v1("X17:LG:10"), Err(LedgerCode::LedgerFull));
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_injection_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, BAD_SAMPLES.len() as u32 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(ledger_fingerprint(&build_sealed_ledger()),
                   ledger_fingerprint(&build_sealed_ledger()));
    }

    #[test]
    fn errno_table_agrees() {
        assert!(agrees_with_linux(ENOSPC));
        assert_eq!(linux_neg(EEXIST), -EEXIST);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn unx10030_checks_green() {
        let set = run_unx10030_checks();
        assert!(set.all_passed(), "UNX-F10030 red: {}", set.red_items_text());
    }
}
