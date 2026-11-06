//! UNX-F12778 文档资产移交清单 · 0x82C80段
//!
//! 判据原文（派工卡 12778 逐字锚定）：
//!   UNX-F12779-J1 待实测移交清单齐（项/判据/闸门条件三列），零虚构。
//!
//! 设计要点：
//! 1. 四类资产登记表：资产类别闭集 Ledger|Glossary|Wisdom|Snapshot，每行持
//!    资产号/版本号/移交旗标/指纹 四元组；冻结闭集 16 行，同类别资产号不重，
//!    重复登记、越容量登记一律显性拒；
//! 2. 移交判定器 judge_handoff(asset_kind, asset_no)：四道闸按固定次序——
//!    未登记资产→ENOENT；版本号未冻结→EROFS；指纹不符→ETAMPER；
//!    重复移交→EALREADY；判定次序本身是验收面，越序判定即违约；
//! 3. 交接演练 10/10：rehearse_handoff() 全量移交一遍并逐行复算指纹
//!    （FNV-1a 重算，与登记指纹逐位相等）；连跑 10 轮，轮轮全绿且指纹
//!    摘要逐轮一致，任何一轮一笔不符即演练失败；
//! 4. 注入器三类：指纹篡改 / 未冻结移交 / 重复移交，各 10 轮全抓，
//!    逃脱即判据失效；注入面显性建模（tamper_fingerprint / thaw_row），
//!    注入后守恒必须仍然平账；
//! 5. 移交账守恒：登记数 = 移交数 + 挂起数 + 拒绝数，显式计数器与逐行
//!    扫描双面复算，任何一面不平即守恒破坏（ECons）；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标；错误路径一律 Result<T, u32>。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12778 − 12600) = 0x82C80）。
pub const SEG_BASE: u32 = 0x82C80;

// ── 移交诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffDiag {
    /// 未登记资产：资产类别+资产号不在冻结闭集登记表内。
    ENoent,
    /// 版本号未冻结：未走冻结口就发起移交。
    ERofs,
    /// 指纹不符：登记指纹与按（类别/资产号/版本号）重算指纹不一致。
    ETamper,
    /// 重复移交：该资产已移交过，二次移交被拒。
    EAlready,
    /// 资产类别越闭集：类别编号不在 Ledger|Glossary|Wisdom|Snapshot 四类内。
    EBadKind,
    /// 登记重复：同类别同资产号已有登记行。
    EDupReg,
    /// 登记容量耗尽：登记表行数越 REG_CAP 上限。
    EFullReg,
    /// 移交日志容量耗尽：日志条数越 LOG_CAP 上限，该笔移交挂起。
    ECapLog,
    /// 演练复算不符：逐行复算指纹与登记指纹不逐位相等。
    ERehearse,
    /// 注入逃脱：注入器构造的违约未被判定器抓住。
    EEscape,
    /// 移交账守恒破坏：登记数 ≠ 移交数 + 挂起数 + 拒绝数。
    ECons,
    /// 双跑不一致：两次全流程指纹摘要或注入结果不一致。
    EDual,
}

impl HandoffDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                HandoffDiag::ENoent => 0x01,
                HandoffDiag::ERofs => 0x02,
                HandoffDiag::ETamper => 0x03,
                HandoffDiag::EAlready => 0x04,
                HandoffDiag::EBadKind => 0x05,
                HandoffDiag::EDupReg => 0x06,
                HandoffDiag::EFullReg => 0x07,
                HandoffDiag::ECapLog => 0x08,
                HandoffDiag::ERehearse => 0x09,
                HandoffDiag::EEscape => 0x0a,
                HandoffDiag::ECons => 0x0b,
                HandoffDiag::EDual => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            HandoffDiag::ENoent => "未登记资产：资产类别+资产号不在冻结闭集内，先按登记表补登记再移交，勿移交表外资产",
            HandoffDiag::ERofs => "版本号未冻结：移交前必须先走版本冻结口，未冻结版本视为在变，冻结后再发起移交",
            HandoffDiag::ETamper => "指纹不符：登记指纹与重算指纹不一致，资产内容疑似被篡改，先比对登记原件再决定重登或拒收",
            HandoffDiag::EAlready => "重复移交：该资产已有一笔移交在账，二次移交拒绝，查移交日志序号即可定位首笔",
            HandoffDiag::EBadKind => "资产类别越闭集：只认 Ledger|Glossary|Wisdom|Snapshot 四类，先纠类别编号再登记",
            HandoffDiag::EDupReg => "登记重复：同类别同资产号已有行，先查登记表再补行，勿重复占号",
            HandoffDiag::EFullReg => "登记容量耗尽：登记表已满 REG_CAP 行，先归档旧行或扩容再登记新资产",
            HandoffDiag::ECapLog => "移交日志容量耗尽：日志已满 LOG_CAP 条，该笔移交转挂起，先出账腾槽再续移交",
            HandoffDiag::ERehearse => "演练复算不符：逐行重算指纹与登记指纹不逐位相等，按行号回查指纹源与版本冻结链",
            HandoffDiag::EEscape => "注入逃脱：构造的违约未被判定器抓住，判定面失效须整表复核",
            HandoffDiag::ECons => "移交账守恒破坏：登记数≠移交数+挂起数+拒绝数，按三桶扫描逐行对账找回差账",
            HandoffDiag::EDual => "双跑不一致：两次全流程摘要不同，存在非确定路径，先查种子与冻结序",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const HANDOFF_ALL: &[HandoffDiag] = &[
    HandoffDiag::ENoent,
    HandoffDiag::ERofs,
    HandoffDiag::ETamper,
    HandoffDiag::EAlready,
    HandoffDiag::EBadKind,
    HandoffDiag::EDupReg,
    HandoffDiag::EFullReg,
    HandoffDiag::ECapLog,
    HandoffDiag::ERehearse,
    HandoffDiag::EEscape,
    HandoffDiag::ECons,
    HandoffDiag::EDual,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in HANDOFF_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82C81..0x82C8C 闭集内"
}

// ── 资产类别闭集 ─────────────────────────────────────────────────
/// 资产类别闭集：Ledger|Glossary|Wisdom|Snapshot 四类，类别编号冻结。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// 第 0 类：台账资产（登记流水/账目类文档）。
    Ledger,
    /// 第 1 类：术语表资产（名词定义类文档）。
    Glossary,
    /// 第 2 类：经验库资产（Wisdom 条目类文档）。
    Wisdom,
    /// 第 3 类：快照资产（状态定格类文档）。
    Snapshot,
}

impl AssetKind {
    /// 类别编号（闭集序）。
    pub fn index(self) -> u8 {
        match self {
            AssetKind::Ledger => 0,
            AssetKind::Glossary => 1,
            AssetKind::Wisdom => 2,
            AssetKind::Snapshot => 3,
        }
    }

    /// 类别中文名。
    pub fn name(self) -> &'static str {
        match self {
            AssetKind::Ledger => "台账",
            AssetKind::Glossary => "术语表",
            AssetKind::Wisdom => "经验库",
            AssetKind::Snapshot => "快照",
        }
    }

    /// 由类别编号反查闭集类别（越界返回 None）。
    pub fn from_u8(k: u8) -> Option<AssetKind> {
        match k {
            0 => Some(AssetKind::Ledger),
            1 => Some(AssetKind::Glossary),
            2 => Some(AssetKind::Wisdom),
            3 => Some(AssetKind::Snapshot),
            _ => None,
        }
    }
}

/// 四类闭集数组（类别序即登记优先序）。
pub const KIND_ALL: &[AssetKind] = &[
    AssetKind::Ledger,
    AssetKind::Glossary,
    AssetKind::Wisdom,
    AssetKind::Snapshot,
];

// ── 指纹函数（FNV-1a 32 位，确定性，逐位可复算） ─────────────────
/// FNV-1a 单字节推进。
fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

/// 资产指纹：对（类别编号/资产号/版本号）做 FNV-1a 32 位摘要。
/// 登记时算一次，移交时重算一次，两次逐位相等才许过指纹闸。
pub fn fingerprint(kind: AssetKind, asset_no: u32, version: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    h = fnv_byte(h, kind.index());
    h = fnv_byte(h, (asset_no & 0xFF) as u8);
    h = fnv_byte(h, ((asset_no >> 8) & 0xFF) as u8);
    h = fnv_byte(h, ((asset_no >> 16) & 0xFF) as u8);
    h = fnv_byte(h, ((asset_no >> 24) & 0xFF) as u8);
    h = fnv_byte(h, (version & 0xFF) as u8);
    h = fnv_byte(h, ((version >> 8) & 0xFF) as u8);
    h = fnv_byte(h, ((version >> 16) & 0xFF) as u8);
    h = fnv_byte(h, ((version >> 24) & 0xFF) as u8);
    h
}

// ── 冻结闭集登记表（16 行，判据下限 ≥16 行） ─────────────────────
/// 冻结闭集登记表容量下限（判据原文：冻结闭集 ≥16 行）。
pub const CORE_ROWS: usize = 16;
/// 登记表容量上限（容量闸）。
pub const REG_CAP: usize = 24;
/// 移交日志容量上限（日志闸，满后移交转挂起）。
pub const LOG_CAP: usize = 16;

/// 冻结闭集核心表：(类别编号, 资产号, 版本号) 三元组，行序冻结。
/// 版本号取 0x0101..0x0404 语义段（类别号作高字节，行内小版本作低字节）。
const CORE_TABLE: &[(u8, u32, u32)] = &[
    (0, 101, 0x0101), // 台账-101 v1.1
    (0, 102, 0x0101), // 台账-102 v1.1
    (0, 103, 0x0102), // 台账-103 v1.2
    (0, 104, 0x0103), // 台账-104 v1.3
    (1, 201, 0x0201), // 术语表-201 v2.1
    (1, 202, 0x0202), // 术语表-202 v2.2
    (2, 301, 0x0301), // 经验库-301 v3.1
    (2, 302, 0x0301), // 经验库-302 v3.1
    (2, 303, 0x0302), // 经验库-303 v3.2
    (3, 401, 0x0401), // 快照-401 v4.1
    (3, 402, 0x0401), // 快照-402 v4.1
    (3, 403, 0x0402), // 快照-403 v4.2
    (3, 404, 0x0402), // 快照-404 v4.2
    (3, 405, 0x0403), // 快照-405 v4.3
    (3, 406, 0x0403), // 快照-406 v4.3
    (3, 407, 0x0404), // 快照-407 v4.4
];

// ── 行与日志数据模型 ─────────────────────────────────────────────
/// 登记表单行：资产号/版本号/移交旗标/指纹 + 冻结旗标 + 拒绝旗标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetRow {
    /// 资产类别。
    pub kind: AssetKind,
    /// 资产号（同类别内唯一）。
    pub asset_no: u32,
    /// 版本号（冻结后不许变）。
    pub version: u32,
    /// 版本冻结旗标：false 时移交被 EROFS 拒。
    pub frozen: bool,
    /// 登记指纹（登记时按三元组算定）。
    pub fingerprint: u32,
    /// 移交旗标：true 表示已有一笔移交在账。
    pub handed: bool,
    /// 拒绝旗标：true 表示曾因指纹不符被拒（进拒绝桶）。
    pub rejected: bool,
}

/// 移交日志条目：一笔成功移交的定格记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffRow {
    /// 移交序号（日志内从 1 起单调递增）。
    pub seq: u32,
    /// 资产类别。
    pub kind: AssetKind,
    /// 资产号。
    pub asset_no: u32,
    /// 移交时的版本号。
    pub version: u32,
    /// 移交时的指纹（与登记指纹逐位相等才成账）。
    pub fingerprint: u32,
}

/// 演练报告：一轮全量移交的定格摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RehearsalReport {
    /// 本轮移交笔数（成功过四道闸的行数）。
    pub rounds: u32,
    /// 移交桶计数（应等于登记行数）。
    pub handed: u32,
    /// 本轮登记表指纹摘要（轮间一致才算双跑一致）。
    pub digest: u32,
}

// ── 登记表账本 ───────────────────────────────────────────────────
/// 文档资产移交登记表账本：登记→冻结→移交/挂起/拒绝 三桶守恒。
#[derive(Clone, Debug)]
pub struct AssetRegistry {
    /// 登记行（按登记序）。
    rows: Vec<AssetRow>,
    /// 移交日志（成功移交逐笔落账）。
    log: Vec<HandoffRow>,
    /// 移交桶显式计数。
    handed_cnt: u32,
    /// 拒绝桶显式计数。
    rejected_cnt: u32,
}

impl AssetRegistry {
    /// 开一本空登记表。
    pub fn new() -> AssetRegistry {
        AssetRegistry {
            rows: Vec::new(),
            log: Vec::new(),
            handed_cnt: 0,
            rejected_cnt: 0,
        }
    }

    /// 已登记行数（登记数）。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取第 idx 行副本（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<AssetRow> {
        self.rows.get(idx).copied()
    }

    /// 按类别+资产号查行副本。
    pub fn find_row(&self, kind: AssetKind, asset_no: u32) -> Option<AssetRow> {
        for r in self.rows.iter() {
            if r.kind == kind && r.asset_no == asset_no {
                return Some(*r);
            }
        }
        None
    }

    /// 登记一行（冻结旗标初始为 false，重复/越容量显性拒）。
    pub fn register(&mut self, kind: AssetKind, asset_no: u32, version: u32) -> Result<(), u32> {
        if self.rows.len() >= REG_CAP {
            return Err(HandoffDiag::EFullReg.val());
        }
        if self.find_row(kind, asset_no).is_some() {
            return Err(HandoffDiag::EDupReg.val());
        }
        self.rows.push(AssetRow {
            kind,
            asset_no,
            version,
            frozen: false,
            fingerprint: fingerprint(kind, asset_no, version),
            handed: false,
            rejected: false,
        });
        Ok(())
    }

    /// 冻结单行版本（返回是否找到行）。
    pub fn freeze_row(&mut self, kind: AssetKind, asset_no: u32) -> bool {
        match self
            .rows
            .iter_mut()
            .find(|r| r.kind == kind && r.asset_no == asset_no)
        {
            Some(r) => {
                r.frozen = true;
                true
            }
            None => false,
        }
    }

    /// 解冻单行（注入面：制造"未冻结移交"违约样本）。
    pub fn thaw_row(&mut self, kind: AssetKind, asset_no: u32) -> bool {
        match self
            .rows
            .iter_mut()
            .find(|r| r.kind == kind && r.asset_no == asset_no)
        {
            Some(r) => {
                r.frozen = false;
                true
            }
            None => false,
        }
    }

    /// 全表冻结（冻结闭集登记表的标准收口）。
    pub fn freeze_all(&mut self) {
        for r in self.rows.iter_mut() {
            r.frozen = true;
        }
    }

    /// 篡改单行登记指纹（注入面：制造"指纹不符"违约样本，
    /// 加非零常数模 2^32，必与原指纹相异）。
    pub fn tamper_fingerprint(&mut self, kind: AssetKind, asset_no: u32) -> bool {
        match self
            .rows
            .iter_mut()
            .find(|r| r.kind == kind && r.asset_no == asset_no)
        {
            Some(r) => {
                r.fingerprint = r.fingerprint.wrapping_add(0x9E37_79B9);
                true
            }
            None => false,
        }
    }

    /// 移交桶计数。
    pub fn handed_count(&self) -> u32 {
        self.handed_cnt
    }

    /// 拒绝桶计数。
    pub fn rejected_count(&self) -> u32 {
        self.rejected_cnt
    }

    /// 移交日志条数。
    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    /// 取第 i 笔移交日志（越界 None）。
    pub fn log_at(&self, i: usize) -> Option<HandoffRow> {
        self.log.get(i).copied()
    }

    /// 三桶扫描：拒绝桶优先（拒/移/挂 三桶互斥落桶）。
    pub fn buckets(&self) -> (usize, usize, usize) {
        let mut handed = 0usize;
        let mut pending = 0usize;
        let mut rejected = 0usize;
        for r in self.rows.iter() {
            if r.rejected {
                rejected += 1;
            } else if r.handed {
                handed += 1;
            } else {
                pending += 1;
            }
        }
        (handed, pending, rejected)
    }

    /// 移交账守恒：登记数 = 移交数 + 挂起数 + 拒绝数，
    /// 显式计数器与逐行扫描双面复算。
    pub fn conservation(&self) -> bool {
        let (h, p, r) = self.buckets();
        if self.handed_cnt as usize != h {
            return false;
        }
        if self.rejected_cnt as usize != r {
            return false;
        }
        self.rows.len() == h + p + r
    }

    /// 登记表指纹摘要：全行指纹按 FNV 链折叠（双跑一致面）。
    pub fn digest(&self) -> u32 {
        let mut acc: u32 = 0x811C_9DC5;
        for r in self.rows.iter() {
            acc = acc.wrapping_mul(0x0100_0193) ^ r.fingerprint;
        }
        acc
    }

    /// 移交判定器（判据原文四道闸，次序冻结）：
    /// 未登记→ENOENT；版本号未冻结→EROFS；指纹不符→ETAMPER；
    /// 重复移交→EALREADY；其后日志容量闸（满→ECapLog，该笔挂起）。
    pub fn judge_handoff(
        &mut self,
        asset_kind: AssetKind,
        asset_no: u32,
    ) -> Result<HandoffRow, u32> {
        // 闸 1（ENOENT）：未登记资产。
        let idx = match self
            .rows
            .iter()
            .position(|r| r.kind == asset_kind && r.asset_no == asset_no)
        {
            Some(i) => i,
            None => return Err(HandoffDiag::ENoent.val()),
        };
        let row = match self.rows.get_mut(idx) {
            Some(r) => r,
            None => return Err(HandoffDiag::ENoent.val()),
        };
        // 闸 2（EROFS）：版本号未冻结。
        if !row.frozen {
            return Err(HandoffDiag::ERofs.val());
        }
        // 闸 3（ETAMPER）：指纹不符（重算与登记逐位比对）。
        if fingerprint(row.kind, row.asset_no, row.version) != row.fingerprint {
            if !row.rejected {
                row.rejected = true;
                self.rejected_cnt = self.rejected_cnt.saturating_add(1);
            }
            return Err(HandoffDiag::ETamper.val());
        }
        // 闸 4（EALREADY）：重复移交。
        if row.handed {
            return Err(HandoffDiag::EAlready.val());
        }
        // 闸 5（ECapLog）：移交日志容量耗尽，该笔转挂起。
        if self.log.len() >= LOG_CAP {
            return Err(HandoffDiag::ECapLog.val());
        }
        // 全闸通过：落移交桶 + 落日志。
        row.handed = true;
        self.handed_cnt = self.handed_cnt.saturating_add(1);
        let h = HandoffRow {
            seq: (self.log.len() as u32).saturating_add(1),
            kind: row.kind,
            asset_no: row.asset_no,
            version: row.version,
            fingerprint: row.fingerprint,
        };
        self.log.push(h);
        Ok(h)
    }
}

// ── 登记表构建器 ─────────────────────────────────────────────────
/// 构建冻结闭集登记表（可选拒冻结，制造未冻结样本用）。
pub fn build_registry_custom(frozen: bool) -> AssetRegistry {
    let mut reg = AssetRegistry::new();
    for e in CORE_TABLE.iter() {
        let kind = match AssetKind::from_u8(e.0) {
            Some(k) => k,
            None => continue,
        };
        let _ = reg.register(kind, e.1, e.2);
    }
    if frozen {
        reg.freeze_all();
    }
    reg
}

/// 构建标准冻结闭集登记表（16 行全冻结）。
pub fn build_registry() -> AssetRegistry {
    build_registry_custom(true)
}

/// 构建压测登记表：冻结闭集核心 16 行 + extra 行合成资产（快照段 901..），
/// 用于移交日志容量耗尽路径（extra 饱和到 4 行封顶）。
pub fn build_registry_with_extra(extra: usize) -> AssetRegistry {
    let mut reg = build_registry();
    let mut i = 0usize;
    // extra 饱和到 4 行封顶（REG_CAP 24 − 核心 16 = 8，留余量）。
    let cap = if extra > 4 { 4 } else { extra };
    while i < cap {
        let no = 901u32.saturating_add(i as u32);
        let ver = 0x9001u32.saturating_add(i as u32);
        let _ = reg.register(AssetKind::Snapshot, no, ver);
        i += 1;
    }
    reg.freeze_all();
    reg
}

// ── 交接演练 ─────────────────────────────────────────────────────
/// 交接演练：全量移交一遍并逐行复算——重算指纹、登记指纹、日志指纹
/// 三方逐位相等，任何一行不符即演练失败（ERehearse）。
pub fn rehearse_handoff(reg: &mut AssetRegistry) -> Result<RehearsalReport, u32> {
    let n = reg.len();
    let mut i = 0usize;
    while i < n {
        let expect = match reg.row_at(i) {
            Some(r) => r,
            None => return Err(HandoffDiag::ERehearse.val()),
        };
        let h = reg.judge_handoff(expect.kind, expect.asset_no)?;
        let recomputed = fingerprint(expect.kind, expect.asset_no, expect.version);
        if recomputed != expect.fingerprint
            || h.fingerprint != expect.fingerprint
            || h.kind != expect.kind
            || h.asset_no != expect.asset_no
            || h.version != expect.version
        {
            return Err(HandoffDiag::ERehearse.val());
        }
        i += 1;
    }
    Ok(RehearsalReport {
        rounds: n as u32,
        handed: reg.handed_count(),
        digest: reg.digest(),
    })
}

/// 交接演练 10/10：连跑 10 轮（每轮新册全量移交+逐行复算），
/// 轮轮全绿且指纹摘要逐轮一致，返回摘要；任一轮不符即 Err。
pub fn rehearse_rounds(rounds: usize) -> Result<u32, u32> {
    let mut digest: Option<u32> = None;
    let mut k = 0usize;
    while k < rounds {
        let mut reg = build_registry();
        let rep = rehearse_handoff(&mut reg)?;
        if rep.handed != CORE_ROWS as u32 {
            return Err(HandoffDiag::ERehearse.val());
        }
        match digest {
            None => digest = Some(rep.digest),
            Some(d) => {
                if d != rep.digest {
                    return Err(HandoffDiag::EDual.val());
                }
            }
        }
        k += 1;
    }
    match digest {
        Some(d) => Ok(d),
        None => Err(HandoffDiag::ERehearse.val()),
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 派生，零随机红线合规） ───────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 注入面：三类违约各 10 轮全抓 ─────────────────────────────────
/// 注入器一（指纹篡改）：每轮新册取一行加常数改指纹，判定器必须回
/// ETAMPER 且守恒仍平；返回 (抓到, 总轮, 首个逃脱描述)。
pub fn inject_tamper_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x11) as u64);
    let mut k = 0usize;
    while k < 10 {
        let mut reg = build_registry();
        let pick = rng.below(reg.len() as u32) as usize;
        let target = match reg.row_at(pick) {
            Some(r) => (r.kind, r.asset_no),
            None => {
                total += 1;
                if escape.is_empty() {
                    escape = format!("轮{}取行失败", k);
                }
                k += 1;
                continue;
            }
        };
        let _ = reg.tamper_fingerprint(target.0, target.1);
        let verdict = reg.judge_handoff(target.0, target.1);
        total += 1;
        match verdict {
            Err(e) if e == HandoffDiag::ETamper.val() && reg.conservation() => caught += 1,
            _ => {
                if escape.is_empty() {
                    escape = format!("轮{}篡改注入逃脱", k);
                }
            }
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入器二（未冻结移交）：每轮新册解冻一行后发起移交，判定器必须回
/// EROFS；返回 (抓到, 总轮, 首个逃脱描述)。
pub fn inject_thaw_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x22) as u64);
    let mut k = 0usize;
    while k < 10 {
        let mut reg = build_registry();
        let pick = rng.below(reg.len() as u32) as usize;
        let target = match reg.row_at(pick) {
            Some(r) => (r.kind, r.asset_no),
            None => {
                total += 1;
                if escape.is_empty() {
                    escape = format!("轮{}取行失败", k);
                }
                k += 1;
                continue;
            }
        };
        let _ = reg.thaw_row(target.0, target.1);
        let verdict = reg.judge_handoff(target.0, target.1);
        total += 1;
        match verdict {
            Err(e) if e == HandoffDiag::ERofs.val() => caught += 1,
            _ => {
                if escape.is_empty() {
                    escape = format!("轮{}未冻结移交注入逃脱", k);
                }
            }
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入器三（重复移交）：每轮新册对同一行连发两笔，首笔必须成功、
/// 次笔必须回 EALREADY 且账面不再变；返回 (抓到, 总轮, 首个逃脱描述)。
pub fn inject_dup_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x33) as u64);
    let mut k = 0usize;
    while k < 10 {
        let mut reg = build_registry();
        let pick = rng.below(reg.len() as u32) as usize;
        let target = match reg.row_at(pick) {
            Some(r) => (r.kind, r.asset_no),
            None => {
                total += 1;
                if escape.is_empty() {
                    escape = format!("轮{}取行失败", k);
                }
                k += 1;
                continue;
            }
        };
        total += 1;
        let first = reg.judge_handoff(target.0, target.1);
        let handed_before = reg.handed_count();
        let log_before = reg.log_len();
        let second = reg.judge_handoff(target.0, target.1);
        match (first, second) {
            (Ok(_), Err(e))
                if e == HandoffDiag::EAlready.val()
                    && reg.handed_count() == handed_before
                    && reg.log_len() == log_before =>
            {
                caught += 1
            }
            _ => {
                if escape.is_empty() {
                    escape = format!("轮{}重复移交注入逃脱", k);
                }
            }
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：12 码互异、全部落窗口、查表命中、未知码回落。
fn diag_closed_ok() -> bool {
    if HANDOFF_ALL.len() != 12 {
        return false;
    }
    for d in HANDOFF_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
        if detail(v) != d.detail() {
            return false;
        }
    }
    // 两两互异。
    let mut i = 0usize;
    while i < HANDOFF_ALL.len() {
        let vi = match HANDOFF_ALL.get(i) {
            Some(d) => d.val(),
            None => return false,
        };
        let mut j = i + 1;
        while j < HANDOFF_ALL.len() {
            let vj = match HANDOFF_ALL.get(j) {
                Some(d) => d.val(),
                None => return false,
            };
            if vi == vj {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 未知码必须回落"未知"文案。
    if !detail(SEG_BASE + 0x3F).contains("未知") {
        return false;
    }
    true
}

/// 资产类别闭集守卫：四类互异、编号回查闭合、中文名非空。
fn kind_closed_ok() -> bool {
    if KIND_ALL.len() != 4 {
        return false;
    }
    let mut i = 0u8;
    while i < 4 {
        let k = match AssetKind::from_u8(i) {
            Some(k) => k,
            None => return false,
        };
        if k.index() != i || k.name().is_empty() {
            return false;
        }
        i += 1;
    }
    // 越界类别编号必须 None。
    AssetKind::from_u8(4).is_none() && AssetKind::from_u8(0xFF).is_none()
}

/// 冻结闭集登记表守卫：恰 16 行、全冻结、指纹与重算逐位相等、
/// 同类别资产号不重。
fn registry_closed_ok() -> bool {
    let reg = build_registry();
    if reg.len() != CORE_ROWS {
        return false;
    }
    let mut i = 0usize;
    while i < reg.len() {
        let r = match reg.row_at(i) {
            Some(r) => r,
            None => return false,
        };
        if !r.frozen || r.handed || r.rejected {
            return false;
        }
        if fingerprint(r.kind, r.asset_no, r.version) != r.fingerprint {
            return false;
        }
        let mut j = i + 1;
        while j < reg.len() {
            let q = match reg.row_at(j) {
                Some(q) => q,
                None => return false,
            };
            if q.kind == r.kind && q.asset_no == r.asset_no {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    reg.conservation()
}

/// 正常流：新册→全量移交演练→16 行全移交→日志 16 笔→守恒平账。
fn happy_path_ok() -> bool {
    let mut reg = build_registry();
    let rep = match rehearse_handoff(&mut reg) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if rep.rounds != CORE_ROWS as u32 || rep.handed != CORE_ROWS as u32 {
        return false;
    }
    if reg.handed_count() != CORE_ROWS as u32 || reg.log_len() != CORE_ROWS {
        return false;
    }
    if reg.rejected_count() != 0 {
        return false;
    }
    reg.conservation()
}

/// 指纹逐位复算：移交日志逐笔与重算指纹、登记行指纹三方逐位相等。
fn fp_recheck_ok() -> bool {
    let mut reg = build_registry();
    if rehearse_handoff(&mut reg).is_err() {
        return false;
    }
    let mut i = 0usize;
    while i < reg.log_len() {
        let h = match reg.log_at(i) {
            Some(h) => h,
            None => return false,
        };
        if h.seq != (i as u32).saturating_add(1) {
            return false;
        }
        if fingerprint(h.kind, h.asset_no, h.version) != h.fingerprint {
            return false;
        }
        match reg.find_row(h.kind, h.asset_no) {
            Some(r) => {
                if r.fingerprint != h.fingerprint || !r.handed {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 判定次序守卫：ENOENT > EROFS > ETAMPER > EALREADY 四道闸按冻结序，
/// 越序判定即违约（重笔+篡改样本必须回 ETAMPER 而非 EALREADY）。
fn judge_order_ok() -> bool {
    // 闸 1：未登记资产（含类别+号不匹配的组合）。
    let mut reg = build_registry();
    if reg.judge_handoff(AssetKind::Ledger, 999) != Err(HandoffDiag::ENoent.val()) {
        return false;
    }
    if reg.judge_handoff(AssetKind::Glossary, 101) != Err(HandoffDiag::ENoent.val()) {
        return false;
    }
    // 闸 2：未冻结行（新册不冻结，直发移交）。
    let mut raw = build_registry_custom(false);
    if raw.judge_handoff(AssetKind::Ledger, 101) != Err(HandoffDiag::ERofs.val()) {
        return false;
    }
    // 闸 3 先于闸 4：已移交行再篡改指纹，必须回 ETAMPER。
    let mut dup = build_registry();
    if dup.judge_handoff(AssetKind::Ledger, 101).is_err() {
        return false;
    }
    if !dup.tamper_fingerprint(AssetKind::Ledger, 101) {
        return false;
    }
    if dup.judge_handoff(AssetKind::Ledger, 101) != Err(HandoffDiag::ETamper.val()) {
        return false;
    }
    // 闸 4：干净重复移交，必须回 EALREADY 且账面不再变。
    let mut d2 = build_registry();
    if d2.judge_handoff(AssetKind::Ledger, 101).is_err() {
        return false;
    }
    let handed_before = d2.handed_count();
    if d2.judge_handoff(AssetKind::Ledger, 101) != Err(HandoffDiag::EAlready.val()) {
        return false;
    }
    if d2.handed_count() != handed_before {
        return false;
    }
    true
}

/// 四类边界：四类各取首行单独移交，全部过四道闸且类别回环一致。
fn four_kind_ok() -> bool {
    if KIND_ALL.len() != 4 {
        return false;
    }
    for k in KIND_ALL.iter() {
        let mut reg = build_registry();
        // 用该类的真实首行资产号（遍历登记表找该类第一行）。
        let mut target: Option<u32> = None;
        let mut i = 0usize;
        while i < reg.len() {
            match reg.row_at(i) {
                Some(r) if r.kind == *k => {
                    target = Some(r.asset_no);
                    i = reg.len();
                }
                _ => i += 1,
            }
        }
        let tno = match target {
            Some(t) => t,
            None => return false,
        };
        let h = match reg.judge_handoff(*k, tno) {
            Ok(h) => h,
            Err(_) => return false,
        };
        if h.kind != *k || !reg.conservation() {
            return false;
        }
        // 二次移交必被闸 4 拒。
        if reg.judge_handoff(*k, tno) != Err(HandoffDiag::EAlready.val()) {
            return false;
        }
    }
    true
}

/// 注入必抓：三类注入器各 10 轮全数命中、无逃脱。
fn inject_all_caught_ok() -> bool {
    let (c1, t1, e1) = inject_tamper_sweep();
    if t1 != 10 || c1 != 10 || !e1.is_empty() {
        return false;
    }
    let (c2, t2, e2) = inject_thaw_sweep();
    if t2 != 10 || c2 != 10 || !e2.is_empty() {
        return false;
    }
    let (c3, t3, e3) = inject_dup_sweep();
    if t3 != 10 || c3 != 10 || !e3.is_empty() {
        return false;
    }
    true
}

/// 移交账守恒三面复算：新册 / 全量移交后 / 篡改拒收后 三态都平账。
fn conservation_ok() -> bool {
    // 面 1：新册（16 行全挂起）。
    let fresh = build_registry();
    if !fresh.conservation() {
        return false;
    }
    let (h0, p0, r0) = fresh.buckets();
    if h0 != 0 || p0 != CORE_ROWS || r0 != 0 {
        return false;
    }
    // 面 2：全量移交（16 行全移交）。
    let mut full = build_registry();
    if rehearse_handoff(&mut full).is_err() {
        return false;
    }
    if !full.conservation() {
        return false;
    }
    let (h1, p1, r1) = full.buckets();
    if h1 != CORE_ROWS || p1 != 0 || r1 != 0 {
        return false;
    }
    // 面 3：篡改拒收（1 行进拒绝桶，其余挂起）。
    let mut rej = build_registry();
    let _ = rej.tamper_fingerprint(AssetKind::Ledger, 101);
    if rej.judge_handoff(AssetKind::Ledger, 101) != Err(HandoffDiag::ETamper.val()) {
        return false;
    }
    if !rej.conservation() {
        return false;
    }
    let (h2, p2, r2) = rej.buckets();
    if h2 != 0 || p2 != CORE_ROWS - 1 || r2 != 1 {
        return false;
    }
    // 守恒恒等式直读：登记数 = 移交数 + 挂起数 + 拒绝数。
    let reg_total = rej.len();
    let (hh, pp, rr) = rej.buckets();
    reg_total == hh + pp + rr
}

/// 交接演练 10/10：10 轮全绿且指纹摘要逐轮一致。
fn rehearse_10_ok() -> bool {
    match rehearse_rounds(10) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// 容量耗尽：18 行压测册全量移交，日志 16 笔满后第 17 笔回 ECapLog，
/// 尾 2 行转挂起、守恒仍平。
fn capacity_ok() -> bool {
    let mut reg = build_registry_with_extra(2);
    if reg.len() != CORE_ROWS + 2 {
        return false;
    }
    match rehearse_handoff(&mut reg) {
        Ok(_) => return false,
        Err(e) => {
            if e != HandoffDiag::ECapLog.val() {
                return false;
            }
        }
    }
    if reg.log_len() != LOG_CAP || reg.handed_count() != LOG_CAP as u32 {
        return false;
    }
    if !reg.conservation() {
        return false;
    }
    let (h, p, r) = reg.buckets();
    h == LOG_CAP && p == 2 && r == 0
}

/// 边界与回冻：首行/尾行移交语义 + 解冻→拒→回冻→过 四段链。
fn boundary_ok() -> bool {
    // 尾行先交：移交不设行序闸，尾行可先过四道闸。
    let mut reg = build_registry();
    let last = match reg.row_at(CORE_ROWS - 1) {
        Some(r) => (r.kind, r.asset_no),
        None => return false,
    };
    if reg.judge_handoff(last.0, last.1).is_err() {
        return false;
    }
    // 首行：解冻→EROFS→回冻→过闸。
    let first = match reg.row_at(0) {
        Some(r) => (r.kind, r.asset_no),
        None => return false,
    };
    if !reg.thaw_row(first.0, first.1) {
        return false;
    }
    if reg.judge_handoff(first.0, first.1) != Err(HandoffDiag::ERofs.val()) {
        return false;
    }
    if !reg.freeze_row(first.0, first.1) {
        return false;
    }
    if reg.judge_handoff(first.0, first.1).is_err() {
        return false;
    }
    // 类别越闭集：from_u8(4) 为 None，越界类别无行可查（ENOENT 面）。
    if AssetKind::from_u8(4).is_some() {
        return false;
    }
    reg.conservation()
}

/// 双跑一致：两次构建逐行相等、摘要相等；注入与演练结果逐项相等。
fn dual_run_ok() -> bool {
    let a = build_registry();
    let b = build_registry();
    if a.len() != b.len() || a.digest() != b.digest() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let s1 = inject_tamper_sweep();
    let s2 = inject_tamper_sweep();
    if s1 != s2 {
        return false;
    }
    let r1 = rehearse_rounds(3);
    let r2 = rehearse_rounds(3);
    r1 == r2
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// 本卡判据条数（UNX-F12779-J1 一条）。
pub const CRITERIA_COUNT: u32 = 1;
/// 本检查节条目数（01..15 含收口）。
pub const CHECK_ITEMS: usize = 15;

/// UNX-F12778 自检（判据 12778 卡：待实测移交清单齐，零虚构）。
pub fn run_unx12778_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12778");
    let reg = build_registry();

    s.add_dyn(
        "F12778-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={} window=[0x80000,0x831C0]", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12778-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x} last=0x{:05x}",
            HANDOFF_ALL.len(),
            HANDOFF_ALL.first().map(|d| d.val()).unwrap_or(0),
            HANDOFF_ALL.last().map(|d| d.val()).unwrap_or(0),
        ),
    );

    s.add_dyn(
        "F12778-03 资产类别闭集",
        kind_closed_ok(),
        &format!(
            "kinds={}首={}尾={}",
            KIND_ALL.len(),
            KIND_ALL.first().map(|k| k.name()).unwrap_or("?"),
            KIND_ALL.last().map(|k| k.name()).unwrap_or("?"),
        ),
    );

    s.add_dyn(
        "F12778-04 冻结闭集登记表 16 行",
        registry_closed_ok(),
        &format!("rows={} cap={} digest=0x{:08x}", reg.len(), REG_CAP, reg.digest()),
    );

    // 正常流：新册全量移交演练，16 行全过四道闸。
    let mut hp = build_registry();
    let hp_ok = match rehearse_handoff(&mut hp) {
        Ok(rep) => rep.handed == CORE_ROWS as u32,
        Err(_) => false,
    };
    s.add_dyn(
        "F12778-05 正常流全量移交",
        hp_ok && happy_path_ok(),
        &format!("handed={} log={} cons={}", hp.handed_count(), hp.log_len(), hp.conservation()),
    );

    s.add_dyn(
        "F12778-06 指纹逐位复算",
        fp_recheck_ok(),
        "日志/重算/登记 三方逐位相等 16 笔",
    );

    s.add_dyn(
        "F12778-07 判定次序四闸",
        judge_order_ok(),
        "ENOENT>EROFS>ETAMPER>EALREADY 次序冻结",
    );

    s.add_dyn(
        "F12778-08 四类边界",
        four_kind_ok(),
        &format!(
            "四类各移交一笔：{}",
            KIND_ALL.iter().map(|k| k.name().to_string()).collect::<Vec<_>>().join("·")
        ),
    );

    // 注入必抓：三类注入器各 10 轮，人为构造违约必被拒。
    let (c1, t1, e1) = inject_tamper_sweep();
    s.add_dyn(
        "F12778-09 注入-指纹篡改 10/10",
        c1 == 10 && t1 == 10 && e1.is_empty(),
        &format!("caught={}/{} {}", c1, t1, e1),
    );

    let (c2, t2, e2) = inject_thaw_sweep();
    s.add_dyn(
        "F12778-10 注入-未冻结移交 10/10",
        c2 == 10 && t2 == 10 && e2.is_empty(),
        &format!("caught={}/{} {}", c2, t2, e2),
    );

    let (c3, t3, e3) = inject_dup_sweep();
    s.add_dyn(
        "F12778-11 注入-重复移交 10/10",
        c3 == 10 && t3 == 10 && e3.is_empty(),
        &format!("caught={}/{} {}", c3, t3, e3),
    );

    s.add_dyn(
        "F12778-12 移交账守恒三面",
        conservation_ok(),
        &format!(
            "登记=移交+挂起+拒绝 fresh={} full={} rej={}",
            build_registry().conservation(),
            hp.conservation(),
            {
                let mut r = build_registry();
                let _ = r.tamper_fingerprint(AssetKind::Ledger, 101);
                let _ = r.judge_handoff(AssetKind::Ledger, 101);
                r.conservation()
            }
        ),
    );

    s.add_dyn(
        "F12778-13 交接演练 10/10",
        rehearse_10_ok(),
        "10 轮全量移交+逐行复算，摘要逐轮一致",
    );

    s.add_dyn(
        "F12778-14 容量耗尽挂起",
        capacity_ok(),
        &format!("log_cap={} 满后第17笔=ECapLog 尾2行挂起", LOG_CAP),
    );

    s.add_dyn(
        "F12778-15 收口：判据条数声明",
        CRITERIA_COUNT == 1 && CHECK_ITEMS == 15 && diag_closed_ok() && dual_run_ok() && boundary_ok(),
        &format!(
            "判据 {} 条（UNX-F12779-J1）检查 {} 条；双跑=注入/演练逐项一致，边界=首尾行+回冻链",
            CRITERIA_COUNT, CHECK_ITEMS
        ),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82C80);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表命中、未知码回落。
        assert_eq!(HANDOFF_ALL.len(), 12);
        let mut i = 0usize;
        while i < HANDOFF_ALL.len() {
            let vi = HANDOFF_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            assert!(!detail(vi).is_empty());
            let mut j = i + 1;
            while j < HANDOFF_ALL.len() {
                let vj = HANDOFF_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
        assert_eq!(detail(HandoffDiag::ENoent.val()), HandoffDiag::ENoent.detail());
    }

    #[test]
    fn frozen_closed_set_is_sixteen_rows() {
        // 冻结闭集：16 行、全冻结、指纹自洽、同类资产号不重、守恒平账。
        let reg = build_registry();
        assert_eq!(reg.len(), CORE_ROWS);
        assert!(registry_closed_ok());
        assert!(reg.conservation());
        let (h, p, r) = reg.buckets();
        assert_eq!(h, 0);
        assert_eq!(p, CORE_ROWS);
        assert_eq!(r, 0);
        // 登记闸：重复登记与越容量显性拒。
        let mut dup = build_registry_custom(false);
        assert_eq!(
            dup.register(AssetKind::Ledger, 101, 0x0101),
            Err(HandoffDiag::EDupReg.val())
        );
        let mut full = build_registry_custom(false);
        let mut n = 0usize;
        while full.len() < REG_CAP {
            let no = 1000u32.saturating_add(n as u32);
            assert!(full.register(AssetKind::Snapshot, no, 0x0001).is_ok());
            n += 1;
        }
        assert_eq!(
            full.register(AssetKind::Snapshot, 2000, 0x0001),
            Err(HandoffDiag::EFullReg.val())
        );
    }

    #[test]
    fn happy_path_hands_off_all_sixteen() {
        // 正常流：全量移交一遍，16 行全移交，日志 16 笔，守恒平账。
        let mut reg = build_registry();
        let rep = match rehearse_handoff(&mut reg) {
            Ok(r) => r,
            Err(e) => {
                assert!(false, "演练必须全绿，Err=0x{:05x}", e);
                return;
            }
        };
        assert_eq!(rep.rounds, 16);
        assert_eq!(rep.handed, 16);
        assert_eq!(reg.log_len(), 16);
        assert_eq!(reg.handed_count(), 16);
        assert!(reg.conservation());
        // 逐行复算：日志指纹与重算指纹逐位相等。
        let mut i = 0usize;
        while i < reg.log_len() {
            let h = reg.log_at(i).unwrap_or(HandoffRow {
                seq: 0,
                kind: AssetKind::Ledger,
                asset_no: 0,
                version: 0,
                fingerprint: 0,
            });
            assert_eq!(h.seq, (i + 1) as u32);
            assert_eq!(fingerprint(h.kind, h.asset_no, h.version), h.fingerprint);
            i += 1;
        }
    }

    #[test]
    fn four_kinds_boundary_each_hands_off() {
        // 四类边界：四类各移交一笔全过四道闸，类别闭集回查闭合。
        assert_eq!(KIND_ALL.len(), 4);
        assert!(AssetKind::from_u8(4).is_none());
        for k in KIND_ALL.iter() {
            let mut reg = build_registry();
            let mut target: Option<(AssetKind, u32)> = None;
            let mut i = 0usize;
            while i < reg.len() {
                match reg.row_at(i) {
                    Some(r) if r.kind == *k => {
                        target = Some((r.kind, r.asset_no));
                        i = reg.len();
                    }
                    _ => i += 1,
                }
            }
            let (kk, no) = match target {
                Some(t) => t,
                None => {
                    assert!(false, "每类至少一行");
                    return;
                }
            };
            let h = match reg.judge_handoff(kk, no) {
                Ok(h) => h,
                Err(e) => {
                    assert!(false, "首笔必须过闸，Err=0x{:05x}", e);
                    return;
                }
            };
            assert_eq!(h.kind, kk);
            assert_eq!(reg.judge_handoff(kk, no), Err(HandoffDiag::EAlready.val()));
            assert!(reg.conservation());
        }
    }

    #[test]
    fn judge_order_enoent_rofs_tamper_already() {
        // 判定次序：四道闸按冻结序，重笔+篡改样本必须回 ETAMPER。
        let mut reg = build_registry();
        assert_eq!(
            reg.judge_handoff(AssetKind::Ledger, 999),
            Err(HandoffDiag::ENoent.val())
        );
        let mut raw = build_registry_custom(false);
        assert_eq!(
            raw.judge_handoff(AssetKind::Ledger, 101),
            Err(HandoffDiag::ERofs.val())
        );
        let mut tam = build_registry();
        let _ = tam.tamper_fingerprint(AssetKind::Wisdom, 301);
        assert_eq!(
            tam.judge_handoff(AssetKind::Wisdom, 301),
            Err(HandoffDiag::ETamper.val())
        );
        let mut dup = build_registry();
        assert!(dup.judge_handoff(AssetKind::Snapshot, 407).is_ok());
        let _ = dup.tamper_fingerprint(AssetKind::Snapshot, 407);
        // 已移交+篡改：ETAMPER 先于 EALREADY。
        assert_eq!(
            dup.judge_handoff(AssetKind::Snapshot, 407),
            Err(HandoffDiag::ETamper.val())
        );
        let mut d2 = build_registry();
        assert!(d2.judge_handoff(AssetKind::Glossary, 201).is_ok());
        assert_eq!(
            d2.judge_handoff(AssetKind::Glossary, 201),
            Err(HandoffDiag::EAlready.val())
        );
    }

    #[test]
    fn inject_tamper_ten_rounds_all_caught() {
        // 注入一（指纹篡改）：10 轮全抓，守恒仍平，无逃脱。
        let (caught, total, escape) = inject_tamper_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn inject_thaw_ten_rounds_all_caught() {
        // 注入二（未冻结移交）：10 轮全抓，无逃脱。
        let (caught, total, escape) = inject_thaw_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn inject_dup_ten_rounds_all_caught() {
        // 注入三（重复移交）：10 轮全抓，账面不再变，无逃脱。
        let (caught, total, escape) = inject_dup_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性种子下两次全流程逐项相同。
        let a = build_registry();
        let b = build_registry();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.digest(), b.digest());
        let mut i = 0usize;
        while i < a.len() {
            assert_eq!(a.row_at(i), b.row_at(i));
            i += 1;
        }
        assert_eq!(inject_tamper_sweep(), inject_tamper_sweep());
        assert_eq!(inject_thaw_sweep(), inject_thaw_sweep());
        assert_eq!(inject_dup_sweep(), inject_dup_sweep());
        assert_eq!(rehearse_rounds(3), rehearse_rounds(3));
    }

    #[test]
    fn capacity_exhaustion_pends_remainder() {
        // 容量耗尽：18 行压测册，日志 16 笔满后第 17 笔回 ECapLog，
        // 尾 2 行挂起，守恒仍平。
        let mut reg = build_registry_with_extra(2);
        assert_eq!(reg.len(), CORE_ROWS + 2);
        assert_eq!(
            rehearse_handoff(&mut reg),
            Err(HandoffDiag::ECapLog.val())
        );
        assert_eq!(reg.log_len(), LOG_CAP);
        assert_eq!(reg.handed_count(), LOG_CAP as u32);
        let (h, p, r) = reg.buckets();
        assert_eq!(h, LOG_CAP);
        assert_eq!(p, 2);
        assert_eq!(r, 0);
        assert!(reg.conservation());
    }

    #[test]
    fn rehearse_ten_rounds_and_conservation_faces() {
        // 交接演练 10/10 + 守恒三面（新册/全移交/篡改拒收）。
        assert!(rehearse_rounds(10).is_ok());
        let fresh = build_registry();
        assert!(fresh.conservation());
        let mut full = build_registry();
        assert!(rehearse_handoff(&mut full).is_ok());
        assert!(full.conservation());
        let mut rej = build_registry();
        let _ = rej.tamper_fingerprint(AssetKind::Ledger, 101);
        assert_eq!(
            rej.judge_handoff(AssetKind::Ledger, 101),
            Err(HandoffDiag::ETamper.val())
        );
        assert!(rej.conservation());
        let (h, p, r) = rej.buckets();
        assert_eq!((h, p, r), (0, CORE_ROWS - 1, 1));
    }

    #[test]
    fn boundary_first_last_and_refreeze_chain() {
        // 边界：尾行先交、首行解冻→拒→回冻→过、越界类别 None。
        let mut reg = build_registry();
        let last = match reg.row_at(CORE_ROWS - 1) {
            Some(r) => r,
            None => {
                assert!(false, "尾行存在");
                return;
            }
        };
        assert!(reg.judge_handoff(last.kind, last.asset_no).is_ok());
        let first = match reg.row_at(0) {
            Some(r) => r,
            None => {
                assert!(false, "首行存在");
                return;
            }
        };
        assert!(reg.thaw_row(first.kind, first.asset_no));
        assert_eq!(
            reg.judge_handoff(first.kind, first.asset_no),
            Err(HandoffDiag::ERofs.val())
        );
        assert!(reg.freeze_row(first.kind, first.asset_no));
        assert!(reg.judge_handoff(first.kind, first.asset_no).is_ok());
        assert!(reg.conservation());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12778_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
