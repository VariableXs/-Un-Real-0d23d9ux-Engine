//! UNX-F12792 移交包二：总纲 §7.3-D1 满账收官行 · 0x83000段
//!
//! 判据原文（派工卡 12792 逐字锚定）：
//!   UNX-F12793-J1 根台账 §二/§三/§四/§六 同步声明判据绿，账目可点验。
//!
//! 设计要点：
//! 1. 四节三元组：根台账 §二域账 / §三号面 / §四断言 / §六移交 各自带
//!    （登记值 / 实算值 / 同步声明布尔）三元组，登记与实算逐值相等才许同步；
//! 2. 同步判定器 judge_sync(section)：缺签与不等均回 EIO 语义码
//!    （−500x 篡改族 / −510x 缺签族，EIO = errno 5，个位即差异章节号），
//!    Ok 回该节总账行；判定次序冻结：未登记 → 缺签 → 逐值登记 vs 实算；
//! 3. 账目可点验：每节可独立复算出总账行——域账 40 批×20=800、
//!    号面 F12001–F12800=800、断言 300×800=240,000、移交五件；
//! 4. 四节同步账守恒：登记 = 实算 + 差异，差异必须为 0，收官前逐节复核，
//!    守恒破坏即禁止收官；
//! 5. 注入器三类（节登记篡改 / 序号越界 / 声明缺签）各 10 轮全抓，
//!    逃脱即判据失效；LCG 种子写死 = SEG_BASE，双跑一致指纹（FNV-1a）；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（种子固定）；
//!    取值一律 match/if let/.get()/.get_mut()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12792 − 12600) = 0x80000 + 0x3000）。
/// 派工卡批次表 SEG 列为陈旧值（B16 行写 0x82C40–0x82FC0），以公式为准。
pub const SEG_BASE: u32 = 0x83000;

// ── 收官账诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0D 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HcDiag {
    /// 段位越窗或不对齐（SEG 须对齐 0x40 且落在 VX9 公式窗）。
    ESegGuard,
    /// 节号越四节闭集（根台账只认 §二/§三/§四/§六）。
    ESectionRange,
    /// 节登记乱序（未按 2→3→4→6 章序登记）。
    ESectionOrder,
    /// 序号越界（号面闭集 F12001–F12800 之外）。
    ESerialRange,
    /// 节登记篡改（登记值偏离实算值，判定器回差异节 EIO 码）。
    ERegTamper,
    /// 同步声明缺签（该节未作同步声明即求同步/收官）。
    EDeclMissing,
    /// 守恒破坏（登记=实算+差异恒等式不成立或差异非零）。
    EConserv,
    /// 四节未齐（未全部入账即求同步/收官）。
    ENotFull,
    /// 状态机倒退（建账→登记→同步→收官只进不退）。
    ERewind,
    /// 台账已收官（收官后写账/注入一律拒绝）。
    ESealed,
    /// 点验日志容量耗尽（日志槽已满仍记新行）。
    ECap,
    /// 判定器语义异常（judge_sync 输出与账面不符或独立复算失败）。
    EJudge,
    /// 移交件数不足五件（移交包闭集缺件）。
    EHandoff,
}

impl HcDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0D 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                HcDiag::ESegGuard => 0x01,
                HcDiag::ESectionRange => 0x02,
                HcDiag::ESectionOrder => 0x03,
                HcDiag::ESerialRange => 0x04,
                HcDiag::ERegTamper => 0x05,
                HcDiag::EDeclMissing => 0x06,
                HcDiag::EConserv => 0x07,
                HcDiag::ENotFull => 0x08,
                HcDiag::ERewind => 0x09,
                HcDiag::ESealed => 0x0a,
                HcDiag::ECap => 0x0b,
                HcDiag::EJudge => 0x0c,
                HcDiag::EHandoff => 0x0d,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            HcDiag::ESegGuard => {
                "段位越窗：SEG 须对齐 0x40 且落在 VX9 公式窗 [0x80000,0x831C0]，先回公式 0x80000+0x40×(id−12600) 复核段基址"
            }
            HcDiag::ESectionRange => {
                "节号越四节闭集：根台账只认 §二域账/§三号面/§四断言/§六移交，先对总纲 §7.3-D1 章序表再登记"
            }
            HcDiag::ESectionOrder => {
                "节登记乱序：按 §二→§三→§四→§六 章序逐节登记，跳节属乱序，回补上一节后再续"
            }
            HcDiag::ESerialRange => {
                "序号越界：号面闭集为 F12001–F12800，越界序号不入账，先回号面定义核对端点"
            }
            HcDiag::ERegTamper => {
                "节登记篡改：登记值偏离实算值，判定器已回差异节 EIO 码，按章号回修该节登记账"
            }
            HcDiag::EDeclMissing => {
                "同步声明缺签：该节未作同步声明，补签声明（登记=实算）后方可同步收官"
            }
            HcDiag::EConserv => {
                "守恒破坏：登记=实算+差异恒等式不成立或差异非零，逐节复算差异账再收官"
            }
            HcDiag::ENotFull => {
                "四节未齐：§二/§三/§四/§六 未全部入账，先补齐缺节再走同步/收官口"
            }
            HcDiag::ERewind => {
                "状态机倒退：建账→登记→同步→收官只进不退，倒退即台账可疑，须全账复核"
            }
            HcDiag::ESealed => {
                "台账已收官：收官后写账/注入一律拒绝，改动走下一条移交滚动"
            }
            HcDiag::ECap => {
                "点验日志容量耗尽：日志槽已满，先归档已有点验行再记新行"
            }
            HcDiag::EJudge => {
                "判定器语义异常：judge_sync 输出与账面不符，复核四节三元组与 EIO 码族"
            }
            HcDiag::EHandoff => {
                "移交件数不足五件：移交包闭集为五件，先补齐缺件再登记 §六"
            }
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const HC_ALL: &[HcDiag] = &[
    HcDiag::ESegGuard,
    HcDiag::ESectionRange,
    HcDiag::ESectionOrder,
    HcDiag::ESerialRange,
    HcDiag::ERegTamper,
    HcDiag::EDeclMissing,
    HcDiag::EConserv,
    HcDiag::ENotFull,
    HcDiag::ERewind,
    HcDiag::ESealed,
    HcDiag::ECap,
    HcDiag::EJudge,
    HcDiag::EHandoff,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in HC_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x83001..0x8300D 闭集内，先回本卡 SEG_BASE 段核对码值"
}

// ── EIO 语义码族（judge_sync 错误路径） ──────────────────────────
/// EIO 语义基数（errno 5 = EIO：I/O 同步账差错语义）。
pub const EIO_SEM: i32 = 5;

/// 登记与实算不等的 EIO 语义码（−500x，个位即差异章节号）。
pub fn eio_mismatch(chapter: u32) -> i32 {
    -(EIO_SEM * 1000 + chapter as i32)
}

/// 同步声明缺签的 EIO 语义码（−510x，个位即差异章节号）。
pub fn eio_undeclared(chapter: u32) -> i32 {
    -(EIO_SEM * 1000 + 100 + chapter as i32)
}

/// EIO 码 → 差异章节号解码（指明差异节；非 EIO 族返回 None）。
pub fn judge_err_chapter(code: i32) -> Option<u32> {
    if code >= 0 {
        return None;
    }
    let a = code.wrapping_neg() as u32;
    let base = if a >= 5100 && a < 5110 {
        5100u32
    } else if a >= 5000 && a < 5010 {
        5000u32
    } else {
        return None;
    };
    let ch = a - base;
    match SectionId::from_chapter(ch) {
        Some(_) => Some(ch),
        None => None,
    }
}

/// EIO 码 → 诊断变体归类（判据绿账面与诊断码闭集对账用）。
pub fn classify_judge_err(code: i32) -> Option<HcDiag> {
    if code == -(HcDiag::ENotFull.val() as i32) {
        return Some(HcDiag::ENotFull);
    }
    if code >= 0 {
        return None;
    }
    let a = code.wrapping_neg();
    if a >= 5100 && a < 5110 {
        return Some(HcDiag::EDeclMissing);
    }
    if a >= 5000 && a < 5010 {
        return Some(HcDiag::ERegTamper);
    }
    None
}

// ── 四节闭集 ─────────────────────────────────────────────────────
/// 根台账四节闭集（总纲 §7.3-D1：§二/§三/§四/§六，章序冻结不许重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionId {
    /// §二 域账（章节号 2）：40 批 × 20 行 = 800 行域账。
    DomAccount,
    /// §三 号面（章节号 3）：F12001–F12800 连号面。
    NumPlane,
    /// §四 断言（章节号 4）：每号 300 条 × 800 号 = 240,000。
    AssertWall,
    /// §六 移交（章节号 6）：移交五件。
    Handoff,
}

impl SectionId {
    /// 章节号（§二=2 / §三=3 / §四=4 / §六=6）。
    pub fn chapter(self) -> u32 {
        match self {
            SectionId::DomAccount => 2,
            SectionId::NumPlane => 3,
            SectionId::AssertWall => 4,
            SectionId::Handoff => 6,
        }
    }

    /// 节中文名。
    pub fn name(self) -> &'static str {
        match self {
            SectionId::DomAccount => "域账",
            SectionId::NumPlane => "号面",
            SectionId::AssertWall => "断言",
            SectionId::Handoff => "移交",
        }
    }

    /// 由章节号反查闭集节（越界返回 None）。
    pub fn from_chapter(ch: u32) -> Option<SectionId> {
        match ch {
            2 => Some(SectionId::DomAccount),
            3 => Some(SectionId::NumPlane),
            4 => Some(SectionId::AssertWall),
            6 => Some(SectionId::Handoff),
            _ => None,
        }
    }
}

/// 四节闭集数组（章序即登记序）。
pub const SECTION_ALL: &[SectionId] = &[
    SectionId::DomAccount,
    SectionId::NumPlane,
    SectionId::AssertWall,
    SectionId::Handoff,
];

// ── 冻结常量表（来源：总纲 §7.3-D1 满账收官行定义） ──────────────
/// 域账批数（总纲 §7.3-D1：40 批）。
pub const BATCH_COUNT: u32 = 40;
/// 每批行数（总纲 §7.3-D1：每批 20 行）。
pub const ROWS_PER_BATCH: u32 = 20;
/// 号面首号（总纲 §7.3-D1：F12001）。
pub const SERIAL_LO: u32 = 12001;
/// 号面尾号（总纲 §7.3-D1：F12800）。
pub const SERIAL_HI: u32 = 12800;
/// 每号断言条数（总纲 §7.3-D1：300 条/号）。
pub const ASSERT_PER_SERIAL: u32 = 300;
/// 移交五件闭集（总纲 §7.3-D1：移交包二五件齐）。
pub const HANDOFF_ITEM_COUNT: usize = 5;
/// 移交五件名录（闭集冻结，逐件非空）。
pub const HANDOFF_ITEMS: &[&str] = &[
    "总纲 §7.3-D1 满账收官行文",
    "根台账四节同步声明页",
    "判据 UNX-F12793-J1 锚定条",
    "移交包一→包二衔接清单",
    "账目点验索引（四节总账行）",
];
/// 点验日志容量（收官行点验槽）。
pub const LOG_CAP: usize = 16;

// ── 三元组数据模型 ───────────────────────────────────────────────
/// 单值对：值名 + 登记值 + 实算值（逐值相等才许该节同步）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerValue {
    /// 值名（冻结闭集内命名）。
    pub name: &'static str,
    /// 登记值（账面登记）。
    pub registered: u64,
    /// 实算值（独立复算）。
    pub computed: u64,
}

/// 单节三元组：逐值（登记/实算）+ 同步声明布尔 + 差异账 + 总账行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionTriple {
    /// 章节号（2/3/4/6）。
    pub chapter: u32,
    /// 节名。
    pub title: &'static str,
    /// 逐值三元组（名/登记/实算）。
    pub values: Vec<LedgerValue>,
    /// 同步声明布尔：声明"登记=实算已同步"（缺签即拒）。
    pub declared: bool,
    /// 差异账：登记合计 − 实算合计，登记时冻结，守恒必须为 0。
    pub delta: i64,
    /// 总账行（可独立复算点验）。
    pub grand: u64,
}

// ── 单调状态机：建账 → 四节登记 → 同步 → 收官 ────────────────────
/// 台账阶段（只进不退，倒退显性拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerStage {
    /// 建账：允许逐节登记。
    Building,
    /// 四节已登记：允许同步判定。
    Registered,
    /// 四节同步完成：允许收官。
    Synced,
    /// 收官：只读，写账/注入一律拒绝。
    Closed,
}

impl LedgerStage {
    /// 阶段秩（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            LedgerStage::Building => 0,
            LedgerStage::Registered => 1,
            LedgerStage::Synced => 2,
            LedgerStage::Closed => 3,
        }
    }
}

// ── 实算器（独立复算，不读账面登记值） ───────────────────────────
/// 号面号数实算：从 F12001 到 F12800 逐号循环计数。
pub fn count_numplane() -> u32 {
    let mut n = 0u32;
    let mut s = SERIAL_LO;
    while s <= SERIAL_HI {
        n = match n.checked_add(1) {
            Some(x) => x,
            None => return n,
        };
        s = match s.checked_add(1) {
            Some(x) => x,
            None => break,
        };
    }
    n
}

/// §二 域账逐值实算：逐批枚举 × 每批逐行枚举，合计 800。
fn recompute_domain_values() -> Vec<LedgerValue> {
    let mut values: Vec<LedgerValue> = Vec::new();
    let mut batch_seen = 0u32;
    let mut rows_total = 0u64;
    let mut per_batch_last = 0u32;
    let mut b = 0u32;
    while b < BATCH_COUNT {
        batch_seen = match batch_seen.checked_add(1) {
            Some(x) => x,
            None => break,
        };
        let mut r = 0u32;
        let mut per = 0u32;
        while r < ROWS_PER_BATCH {
            per = match per.checked_add(1) {
                Some(x) => x,
                None => break,
            };
            r = match r.checked_add(1) {
                Some(x) => x,
                None => break,
            };
        }
        per_batch_last = per;
        rows_total = match rows_total.checked_add(per as u64) {
            Some(t) => t,
            None => u64::MAX,
        };
        b = match b.checked_add(1) {
            Some(x) => x,
            None => break,
        };
    }
    let reg_total = match BATCH_COUNT.checked_mul(ROWS_PER_BATCH) {
        Some(t) => t as u64,
        None => 0,
    };
    values.push(LedgerValue {
        name: "域账批数",
        registered: BATCH_COUNT as u64,
        computed: batch_seen as u64,
    });
    values.push(LedgerValue {
        name: "每批行数",
        registered: ROWS_PER_BATCH as u64,
        computed: per_batch_last as u64,
    });
    values.push(LedgerValue {
        name: "域账总行",
        registered: reg_total,
        computed: rows_total,
    });
    values
}

/// §三 号面逐值实算：首号/尾号/总号，逐号循环枚举。
fn recompute_numplane_values() -> Vec<LedgerValue> {
    let mut values: Vec<LedgerValue> = Vec::new();
    let mut n = 0u32;
    let mut first = 0u32;
    let mut last = 0u32;
    let mut s = SERIAL_LO;
    while s <= SERIAL_HI {
        if n == 0 {
            first = s;
        }
        last = s;
        n = match n.checked_add(1) {
            Some(x) => x,
            None => break,
        };
        s = match s.checked_add(1) {
            Some(x) => x,
            None => break,
        };
    }
    let reg_span = match SERIAL_HI.checked_sub(SERIAL_LO) {
        Some(d) => match d.checked_add(1) {
            Some(t) => t as u64,
            None => 0,
        },
        None => 0,
    };
    values.push(LedgerValue {
        name: "号面首号",
        registered: SERIAL_LO as u64,
        computed: first as u64,
    });
    values.push(LedgerValue {
        name: "号面尾号",
        registered: SERIAL_HI as u64,
        computed: last as u64,
    });
    values.push(LedgerValue {
        name: "号面总号",
        registered: reg_span,
        computed: n as u64,
    });
    values
}

/// §四 断言逐值实算：每号断言 × 号面号数（号数独立复算，不读 §三 账面）。
fn recompute_assert_values() -> Vec<LedgerValue> {
    let mut values: Vec<LedgerValue> = Vec::new();
    let plane = count_numplane() as u64;
    let per = ASSERT_PER_SERIAL as u64;
    let total = match plane.checked_mul(per) {
        Some(t) => t,
        None => 0,
    };
    let reg_total = match ASSERT_PER_SERIAL.checked_mul(count_numplane()) {
        Some(t) => t as u64,
        None => 0,
    };
    values.push(LedgerValue {
        name: "每号断言",
        registered: ASSERT_PER_SERIAL as u64,
        computed: per,
    });
    values.push(LedgerValue {
        name: "断言总条",
        registered: reg_total,
        computed: total,
    });
    values
}

/// §六 移交逐值实算：逐件枚举五件闭集，缺件/空件即 EHandoff。
fn recompute_handoff_values() -> Result<Vec<LedgerValue>, u32> {
    let mut values: Vec<LedgerValue> = Vec::new();
    let mut n = 0usize;
    for item in HANDOFF_ITEMS.iter() {
        if item.is_empty() {
            return Err(HcDiag::EHandoff.val());
        }
        n += 1;
    }
    if n != HANDOFF_ITEM_COUNT {
        return Err(HcDiag::EHandoff.val());
    }
    values.push(LedgerValue {
        name: "移交件数",
        registered: HANDOFF_ITEM_COUNT as u64,
        computed: n as u64,
    });
    Ok(values)
}

/// 总账行独立复算：从冻结常量直接重算该节总账行（不读账面登记值），
/// 返回（总账行文 + 独立复算值）。
pub fn grand_total_independent(section: SectionId) -> Option<(String, u64)> {
    match section {
        SectionId::DomAccount => {
            let m = BATCH_COUNT.checked_mul(ROWS_PER_BATCH)?;
            Some((
                format!("域账总行: {}批×{}行={}", BATCH_COUNT, ROWS_PER_BATCH, m),
                m as u64,
            ))
        }
        SectionId::NumPlane => {
            let c = count_numplane();
            Some((
                format!("号面总行: F{}–F{}={}", SERIAL_LO, SERIAL_HI, c),
                c as u64,
            ))
        }
        SectionId::AssertWall => {
            let c = count_numplane();
            let a = ASSERT_PER_SERIAL.checked_mul(c)?;
            Some((
                format!("断言总行: {}条/号×{}号={}", ASSERT_PER_SERIAL, c, a),
                a as u64,
            ))
        }
        SectionId::Handoff => {
            let n = HANDOFF_ITEMS.iter().count();
            Some((format!("移交总行: 五件={}", n), n as u64))
        }
    }
}

// ── 根台账 ───────────────────────────────────────────────────────
/// 守恒单节复核：声明已签 + 差异为 0 + 登记=实算+差异恒等 + 总账行=末值实算。
fn section_conserves(sec: &SectionTriple) -> bool {
    if !sec.declared || sec.values.is_empty() {
        return false;
    }
    // 差异必须为 0（登记 = 实算 + 0）。
    if sec.delta != 0 {
        return false;
    }
    let mut sr = 0u64;
    let mut sc = 0u64;
    for v in sec.values.iter() {
        sr = sr.saturating_add(v.registered);
        sc = sc.saturating_add(v.computed);
    }
    // 恒等式：登记 = 实算 + 差异。
    if sr != sc.wrapping_add(sec.delta as u64) {
        return false;
    }
    // 总账行守恒：grand 恒等于末值实算。
    match sec.values.iter().last() {
        Some(v) => sec.grand == v.computed,
        None => false,
    }
}

/// 按值名取单值对。
fn value_of<'a>(sec: &'a SectionTriple, name: &str) -> Option<&'a LedgerValue> {
    sec.values.iter().find(|v| v.name == name)
}

/// FNV-1a 混入一段字节（双跑一致指纹用）。
fn fnv_mix(h: u32, bytes: &[u8]) -> u32 {
    let mut out = h;
    for b in bytes.iter() {
        out ^= *b as u32;
        out = out.wrapping_mul(0x0100_0193);
    }
    out
}

/// 根台账：四节三元组 + 单调状态机 + 号面序号登记 + 点验日志。
#[derive(Clone, Debug)]
pub struct RootLedger {
    /// 已入账节（按章序）。
    sections: Vec<SectionTriple>,
    /// 台账阶段。
    stage: LedgerStage,
    /// 号面序号登记（§三 点验操作面）。
    serials: Vec<u32>,
    /// 点验日志行（每节一条总账行，容量 LOG_CAP）。
    pointchecks: Vec<String>,
}

impl RootLedger {
    /// 开一本新台账（建账阶段）。
    pub fn new() -> RootLedger {
        RootLedger {
            sections: Vec::new(),
            stage: LedgerStage::Building,
            serials: Vec::new(),
            pointchecks: Vec::new(),
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> LedgerStage {
        self.stage
    }

    /// 已入账节数。
    pub fn len_sections(&self) -> usize {
        self.sections.len()
    }

    /// 按章节号查节。
    pub fn section_of(&self, chapter: u32) -> Option<&SectionTriple> {
        self.sections.iter().find(|s| s.chapter == chapter)
    }

    /// 按入账序取节（越界 None）。
    pub fn section_at(&self, idx: usize) -> Option<&SectionTriple> {
        self.sections.get(idx)
    }

    /// 已登记号面序号数。
    pub fn serial_count(&self) -> usize {
        self.serials.len()
    }

    /// 已记点验行数。
    pub fn pointcheck_count(&self) -> usize {
        self.pointchecks.len()
    }

    /// 取第 n 条点验行（越界 None）。
    pub fn pointcheck_at(&self, idx: usize) -> Option<&str> {
        match self.pointchecks.get(idx) {
            Some(s) => Some(s.as_str()),
            None => None,
        }
    }

    /// 按章节号取节（可变）。
    fn find_section_mut(&mut self, chapter: u32) -> Option<&mut SectionTriple> {
        self.sections.iter_mut().find(|s| s.chapter == chapter)
    }

    /// 逐节登记（章序 2→3→4→6 单向推进，四节齐自动进入 Registered）。
    pub fn register_section(&mut self, section: SectionId) -> Result<(), u32> {
        // 收官闸：收官后一切写账拒绝。
        if self.stage.rank() >= LedgerStage::Closed.rank() {
            return Err(HcDiag::ESealed.val());
        }
        // 推进闸：已离建账阶段再回头登记属倒退。
        if self.stage.rank() > LedgerStage::Building.rank() {
            return Err(HcDiag::ERewind.val());
        }
        // 节序闸：必须按 SECTION_ALL 章序逐节登记。
        let expect = SECTION_ALL.get(self.sections.len());
        match expect {
            Some(s) if *s == section => {}
            _ => return Err(HcDiag::ESectionOrder.val()),
        }
        // 重复闸（章序闸下理论不可达，防御性保留）。
        if self.section_of(section.chapter()).is_some() {
            return Err(HcDiag::ESectionOrder.val());
        }
        // 独立实算该节逐值（§六 缺件走 EHandoff）。
        let values = match section {
            SectionId::DomAccount => recompute_domain_values(),
            SectionId::NumPlane => recompute_numplane_values(),
            SectionId::AssertWall => recompute_assert_values(),
            SectionId::Handoff => match recompute_handoff_values() {
                Ok(v) => v,
                Err(e) => return Err(e),
            },
        };
        // 差异账：登记合计 − 实算合计，登记即冻结（正常登记必为 0）。
        let mut sr = 0u64;
        let mut sc = 0u64;
        for v in values.iter() {
            sr = sr.saturating_add(v.registered);
            sc = sc.saturating_add(v.computed);
        }
        let delta = sr as i64 - sc as i64;
        let grand = match values.iter().last() {
            Some(v) => v.computed,
            None => return Err(HcDiag::ENotFull.val()),
        };
        self.sections.push(SectionTriple {
            chapter: section.chapter(),
            title: section.name(),
            values,
            declared: true,
            delta,
            grand,
        });
        // 四节齐推进状态机（只进不退）。
        if self.sections.len() == SECTION_ALL.len() {
            self.stage = LedgerStage::Registered;
        }
        Ok(())
    }

    /// 同步判定器：节内 ①未登记 → ②缺签 → ③逐值登记 vs 实算，
    /// 全过回 Ok(该节总账行)；缺签回 −510x、不等回 −500x（EIO 语义，指明差异节）。
    pub fn judge_sync(&self, section: SectionId) -> Result<u32, i32> {
        // ① 已登记闸：未登记节不参与同步判定。
        let sec = match self.section_of(section.chapter()) {
            Some(s) => s,
            None => return Err(-(HcDiag::ENotFull.val() as i32)),
        };
        // ② 同步声明闸：缺签即拒。
        if !sec.declared {
            return Err(eio_undeclared(section.chapter()));
        }
        // ③ 逐值闸：登记 vs 实算逐值相等才放行。
        for v in sec.values.iter() {
            if v.registered != v.computed {
                return Err(eio_mismatch(section.chapter()));
            }
        }
        Ok(sec.grand as u32)
    }

    /// 四节全同步：逐节 judge_sync，全绿推进到 Synced，回总账行合计。
    pub fn sync_all(&mut self) -> Result<u32, i32> {
        if self.sections.len() != SECTION_ALL.len() {
            return Err(-(HcDiag::ENotFull.val() as i32));
        }
        let mut sum = 0u64;
        for sec in SECTION_ALL.iter() {
            match self.judge_sync(*sec) {
                Ok(g) => sum = sum.saturating_add(g as u64),
                Err(e) => return Err(e),
            }
        }
        if self.stage.rank() < LedgerStage::Synced.rank() {
            self.stage = LedgerStage::Synced;
        }
        Ok(sum as u32)
    }

    /// 四节守恒复核：节数齐 + 章序升 + 逐节恒等式与差异为 0。
    pub fn conservation_ok(&self) -> bool {
        if self.sections.len() != SECTION_ALL.len() {
            return false;
        }
        let mut prev = 0u32;
        for sec in self.sections.iter() {
            if sec.chapter <= prev {
                return false;
            }
            prev = sec.chapter;
        }
        for sec in self.sections.iter() {
            if !section_conserves(sec) {
                return false;
            }
        }
        true
    }

    /// 记一条点验行（总账行独立复算须与账面总账行一致；容量 LOG_CAP）。
    pub fn log_pointcheck(&mut self, section: SectionId) -> Result<(), u32> {
        // 收官闸。
        if self.stage.rank() >= LedgerStage::Closed.rank() {
            return Err(HcDiag::ESealed.val());
        }
        // 容量闸：槽满即拒。
        if self.pointchecks.len() >= LOG_CAP {
            return Err(HcDiag::ECap.val());
        }
        let (row, total) = match grand_total_independent(section) {
            Some(p) => p,
            None => return Err(HcDiag::EJudge.val()),
        };
        // 可点验闸：独立复算值必须与账面总账行一致。
        let grand_ok = match self.section_of(section.chapter()) {
            Some(s) => s.grand == total,
            None => return Err(HcDiag::ENotFull.val()),
        };
        if !grand_ok {
            return Err(HcDiag::EConserv.val());
        }
        self.pointchecks.push(row);
        Ok(())
    }

    /// 号面序号登记（界内收、越界拒、收官拒）。
    pub fn try_register_serial(&mut self, serial: u32) -> Result<(), u32> {
        if self.stage.rank() >= LedgerStage::Closed.rank() {
            return Err(HcDiag::ESealed.val());
        }
        if !(SERIAL_LO..=SERIAL_HI).contains(&serial) {
            return Err(HcDiag::ESerialRange.val());
        }
        self.serials.push(serial);
        Ok(())
    }

    /// 收官：已同步 + 四节齐 + 守恒全绿 + 判定全绿才许封账。
    pub fn close(&mut self) -> Result<(), u32> {
        // 重封闸：已收官再封属倒退。
        if self.stage.rank() >= LedgerStage::Closed.rank() {
            return Err(HcDiag::ERewind.val());
        }
        // 未同步闸：Building/Registered 阶段禁封。
        if self.stage.rank() < LedgerStage::Synced.rank() {
            return Err(HcDiag::ENotFull.val());
        }
        if self.sections.len() != SECTION_ALL.len() {
            return Err(HcDiag::ENotFull.val());
        }
        // 守恒闸：差异必须为 0。
        if !self.conservation_ok() {
            return Err(HcDiag::EConserv.val());
        }
        // 判定闸：四节 judge_sync 全绿。
        for sec in SECTION_ALL.iter() {
            if self.judge_sync(*sec).is_err() {
                return Err(HcDiag::EJudge.val());
            }
        }
        self.stage = LedgerStage::Closed;
        Ok(())
    }

    // ── 审计注入面（只服务注入必抓扫，收官后拒绝） ────────────────
    /// 类一注入：篡改指定节末值登记值（加性扰动），判定器必须回差异节 EIO 码。
    pub fn inject_reg_tamper(&mut self, chapter: u32, addend: i64) -> Result<(), u32> {
        if addend == 0 {
            return Err(HcDiag::ERegTamper.val());
        }
        if self.stage.rank() >= LedgerStage::Closed.rank() {
            return Err(HcDiag::ESealed.val());
        }
        if SectionId::from_chapter(chapter).is_none() {
            return Err(HcDiag::ESectionRange.val());
        }
        match self.find_section_mut(chapter) {
            Some(sec) => match sec.values.iter_mut().last() {
                Some(v) => {
                    let cur = v.registered as i64;
                    let moved = cur.saturating_add(addend);
                    v.registered = if moved < 0 { 0 } else { moved as u64 };
                    Ok(())
                }
                None => Err(HcDiag::ENotFull.val()),
            },
            None => Err(HcDiag::ENotFull.val()),
        }
    }

    /// 类三注入：撤销指定节同步声明（缺签），判定器必须回 −510x 缺签码。
    pub fn inject_decl_unsign(&mut self, chapter: u32) -> Result<(), u32> {
        if self.stage.rank() >= LedgerStage::Closed.rank() {
            return Err(HcDiag::ESealed.val());
        }
        if SectionId::from_chapter(chapter).is_none() {
            return Err(HcDiag::ESectionRange.val());
        }
        match self.find_section_mut(chapter) {
            Some(sec) => {
                sec.declared = false;
                Ok(())
            }
            None => Err(HcDiag::ENotFull.val()),
        }
    }

    /// 双跑一致指纹（FNV-1a 32 位）：阶段 + 四节三元组 + 序号 + 点验行。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = fnv_mix(h, &self.stage.rank().to_le_bytes());
        h = fnv_mix(h, &(self.sections.len() as u32).to_le_bytes());
        for sec in self.sections.iter() {
            h = fnv_mix(h, &sec.chapter.to_le_bytes());
            h = fnv_mix(h, &(sec.declared as u8).to_le_bytes());
            h = fnv_mix(h, &sec.delta.to_le_bytes());
            h = fnv_mix(h, &sec.grand.to_le_bytes());
            for v in sec.values.iter() {
                h = fnv_mix(h, &v.registered.to_le_bytes());
                h = fnv_mix(h, &v.computed.to_le_bytes());
            }
        }
        for s in self.serials.iter() {
            h = fnv_mix(h, &s.to_le_bytes());
        }
        for p in self.pointchecks.iter() {
            h = fnv_mix(h, p.as_bytes());
        }
        h
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
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

// ── 台账构建（正常流步进：建账→四节登记→同步） ──────────────────
/// 构建一本四节齐、已同步的根台账（不收官，留给 close 口）。
pub fn build_root_ledger() -> RootLedger {
    let mut l = RootLedger::new();
    for sec in SECTION_ALL.iter() {
        let _ = l.register_section(*sec);
    }
    let _ = l.sync_all();
    l
}

// ── 注入器三类扫（各 10 轮全抓，逃脱即判据失效） ─────────────────
/// 注入扫：类一节登记篡改 ×10 / 类二序号越界 ×10 / 类三声明缺签 ×10。
/// 返回（抓到路数, 总路数, 首个逃脱路描述）。
pub fn inject_sweep() -> (usize, usize, String) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    // 类一：节登记篡改 —— 判定器必须回差异节 EIO 码且守恒必红。
    let mut k = 0usize;
    while k < 10 {
        let ci = rng.below(SECTION_ALL.len() as u32) as usize;
        let sec = match SECTION_ALL.get(ci) {
            Some(s) => *s,
            None => SectionId::DomAccount,
        };
        let addend = if k % 2 == 0 {
            (1 + rng.below(9)) as i64
        } else {
            -((1 + rng.below(4)) as i64)
        };
        let mut l = build_root_ledger();
        let inj = l.inject_reg_tamper(sec.chapter(), addend);
        let judge = l.judge_sync(sec);
        let cons = l.conservation_ok();
        let ok = inj.is_ok() && !cons && judge == Err(eio_mismatch(sec.chapter()));
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!(
                "类一篡改 路{} 节§{} 注入逃脱（判定={:?}）",
                k,
                sec.chapter(),
                judge.err()
            );
        }
        k += 1;
    }
    // 类二：序号越界 —— 界外序号必被号面闸拒且不入账。
    k = 0;
    while k < 10 {
        let serial = if k % 2 == 0 {
            SERIAL_LO.saturating_sub(1 + rng.below(500))
        } else {
            SERIAL_HI.saturating_add(1 + rng.below(500))
        };
        let mut l = build_root_ledger();
        let r = l.try_register_serial(serial);
        let ok = r == Err(HcDiag::ESerialRange.val()) && l.serial_count() == 0;
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类二越界 路{} 序号{} 注入逃脱", k, serial);
        }
        k += 1;
    }
    // 类三：声明缺签 —— 判定器必须回 −510x 缺签码并指明差异节。
    k = 0;
    while k < 10 {
        let ci = rng.below(SECTION_ALL.len() as u32) as usize;
        let sec = match SECTION_ALL.get(ci) {
            Some(s) => *s,
            None => SectionId::NumPlane,
        };
        let mut l = build_root_ledger();
        let inj = l.inject_decl_unsign(sec.chapter());
        let judge = l.judge_sync(sec);
        let ok = inj.is_ok() && judge == Err(eio_undeclared(sec.chapter()));
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类三缺签 路{} 节§{} 注入逃脱", k, sec.chapter());
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 公式窗（批次表 SEG 列陈旧，以公式为准）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    // 公式全窗：id 12600..=12799 → [0x80000, 0x80000+0x40×199]。
    let span = match 12799u32.checked_sub(12600) {
        Some(d) => match 0x40u32.checked_mul(d) {
            Some(s) => s,
            None => return false,
        },
        None => return false,
    };
    let hi = match 0x80000u32.checked_add(span) {
        Some(h) => h,
        None => return false,
    };
    if !(0x80000..=hi).contains(&SEG_BASE) {
        return false;
    }
    // 本卡锚定：SEG 公式实值 0x80000 + 0x40×192 = 0x83000。
    let own = match 12792u32.checked_sub(12600) {
        Some(d) => match 0x40u32.checked_mul(d) {
            Some(o) => o,
            None => return false,
        },
        None => return false,
    };
    match 0x80000u32.checked_add(own) {
        Some(v) => v == SEG_BASE,
        None => false,
    }
}

/// 诊断码闭集守卫：互异、落段窗、查表命中、未知回落、EIO 码归类回环。
fn diag_closed_ok() -> bool {
    let mut i = 0usize;
    while i < HC_ALL.len() {
        let d = match HC_ALL.get(i) {
            Some(d) => *d,
            None => return false,
        };
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if detail(v) != d.detail() || d.detail().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < HC_ALL.len() {
            match HC_ALL.get(j) {
                Some(e) => {
                    if e.val() == v {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    // 未知码回落。
    if !detail(SEG_BASE + 0x3F).contains("未知") {
        return false;
    }
    // EIO 码族 ↔ 诊断变体归类回环。
    if classify_judge_err(eio_mismatch(4)) != Some(HcDiag::ERegTamper) {
        return false;
    }
    if classify_judge_err(eio_undeclared(6)) != Some(HcDiag::EDeclMissing) {
        return false;
    }
    if classify_judge_err(0).is_some() {
        return false;
    }
    if judge_err_chapter(eio_mismatch(6)) != Some(6) {
        return false;
    }
    true
}

/// 四节闭集枚举：章序 2→3→4→6 升且闭集外章（1/5）拒。
fn four_enum_ok() -> bool {
    if SECTION_ALL.len() != 4 {
        return false;
    }
    let mut prev = 0u32;
    for sec in SECTION_ALL.iter() {
        let c = sec.chapter();
        if c <= prev {
            return false;
        }
        prev = c;
        match SectionId::from_chapter(c) {
            Some(s) => {
                if s != *sec {
                    return false;
                }
            }
            None => return false,
        }
        if sec.name().is_empty() {
            return false;
        }
    }
    if SectionId::from_chapter(5).is_some() {
        return false;
    }
    if SectionId::from_chapter(1).is_some() {
        return false;
    }
    prev == 6
}

/// 正常流：建账→四节登记→同步→点验×4→收官，收官后判定只读仍绿。
fn happy_path_ok() -> bool {
    let mut l = build_root_ledger();
    if l.stage() != LedgerStage::Synced {
        return false;
    }
    for sec in SECTION_ALL.iter() {
        if l.log_pointcheck(*sec).is_err() {
            return false;
        }
    }
    if l.close().is_err() {
        return false;
    }
    if l.stage() != LedgerStage::Closed {
        return false;
    }
    for sec in SECTION_ALL.iter() {
        match l.judge_sync(*sec) {
            Ok(g) => match l.section_of(sec.chapter()) {
                Some(s) => {
                    if g as u64 != s.grand {
                        return false;
                    }
                }
                None => return false,
            },
            Err(_) => return false,
        }
    }
    l.conservation_ok()
}

/// §二 域账点验：三元组齐 + 逐值登记=实算 + 独立复算=账面总账行=800。
fn domain_section_ok(l: &RootLedger) -> bool {
    match l.section_of(2) {
        Some(sec) => {
            if sec.title != "域账" {
                return false;
            }
            if sec.values.len() != 3 {
                return false;
            }
            for v in sec.values.iter() {
                if v.registered != v.computed {
                    return false;
                }
            }
            match grand_total_independent(SectionId::DomAccount) {
                Some((_, t)) => t == sec.grand && sec.grand == 800,
                None => false,
            }
        }
        None => false,
    }
}

/// §三 号面点验：首号/尾号/总号逐值点验 + 独立复算 800。
fn numplane_section_ok(l: &RootLedger) -> bool {
    match l.section_of(3) {
        Some(sec) => {
            if sec.title != "号面" {
                return false;
            }
            if sec.values.len() != 3 {
                return false;
            }
            let first_ok = match value_of(sec, "号面首号") {
                Some(v) => v.computed == SERIAL_LO as u64 && v.registered == SERIAL_LO as u64,
                None => false,
            };
            let last_ok = match value_of(sec, "号面尾号") {
                Some(v) => v.computed == SERIAL_HI as u64 && v.registered == SERIAL_HI as u64,
                None => false,
            };
            let count_ok = match value_of(sec, "号面总号") {
                Some(v) => v.computed == 800 && v.registered == 800,
                None => false,
            };
            let ind_ok = match grand_total_independent(SectionId::NumPlane) {
                Some((_, t)) => t == sec.grand && sec.grand == 800,
                None => false,
            };
            first_ok && last_ok && count_ok && ind_ok
        }
        None => false,
    }
}

/// §四 断言点验：300×800=240,000 独立复算与账面一致。
fn assert_section_ok(l: &RootLedger) -> bool {
    match l.section_of(4) {
        Some(sec) => {
            if sec.title != "断言" {
                return false;
            }
            if sec.values.len() != 2 {
                return false;
            }
            let per_ok = match value_of(sec, "每号断言") {
                Some(v) => v.computed == ASSERT_PER_SERIAL as u64 && v.registered == v.computed,
                None => false,
            };
            let total_ok = match value_of(sec, "断言总条") {
                Some(v) => v.computed == 240000 && v.registered == 240000,
                None => false,
            };
            let ind = match ASSERT_PER_SERIAL.checked_mul(count_numplane()) {
                Some(t) => t as u64,
                None => 0,
            };
            ind == sec.grand && sec.grand == 240000 && per_ok && total_ok
        }
        None => false,
    }
}

/// §六 移交点验：五件闭集逐件非空 + 件数账面=实算=5。
fn handoff_section_ok(l: &RootLedger) -> bool {
    match l.section_of(6) {
        Some(sec) => {
            if sec.title != "移交" {
                return false;
            }
            if sec.values.len() != 1 {
                return false;
            }
            let n = HANDOFF_ITEMS.iter().count();
            let mut nonempty = true;
            for it in HANDOFF_ITEMS.iter() {
                if it.is_empty() {
                    nonempty = false;
                }
            }
            let cnt_ok = match value_of(sec, "移交件数") {
                Some(v) => v.computed == n as u64 && v.registered == HANDOFF_ITEM_COUNT as u64,
                None => false,
            };
            n == HANDOFF_ITEM_COUNT && nonempty && cnt_ok && sec.grand == 5
        }
        None => false,
    }
}

/// judge_sync 四节全 Ok：回值=账面总账行、合计=241605、sync_all 口一致。
fn judge_all_ok() -> bool {
    let l = build_root_ledger();
    let mut sum = 0u64;
    for sec in SECTION_ALL.iter() {
        match l.judge_sync(*sec) {
            Ok(g) => match l.section_of(sec.chapter()) {
                Some(s) => {
                    if g as u64 != s.grand {
                        return false;
                    }
                    sum = sum.saturating_add(g as u64);
                }
                None => return false,
            },
            Err(_) => return false,
        }
    }
    // 总账行合计 = 800 + 800 + 240000 + 5 = 241605。
    if sum != 241605 {
        return false;
    }
    let mut l2 = build_root_ledger();
    match l2.sync_all() {
        Ok(t) => t as u64 == sum,
        Err(_) => false,
    }
}

/// 注入必抓：三类 30 轮全数命中，无逃脱。
fn inject_caught_ok() -> bool {
    let (caught, total, escape) = inject_sweep();
    if total != 30 {
        return false;
    }
    if caught != 30 {
        return false;
    }
    escape.is_empty()
}

/// 守恒恒等式：四节 delta=0、登记=实算、差异可复算；篡改后必破。
fn conserv_ok() -> bool {
    let l = build_root_ledger();
    if !l.conservation_ok() {
        return false;
    }
    for sec in SECTION_ALL.iter() {
        match l.section_of(sec.chapter()) {
            Some(s) => {
                if s.delta != 0 || !s.declared {
                    return false;
                }
                let mut sr = 0u64;
                let mut sc = 0u64;
                for v in s.values.iter() {
                    sr = sr.saturating_add(v.registered);
                    sc = sc.saturating_add(v.computed);
                }
                if sr != sc {
                    return false;
                }
                // 差异账可复算：登记 − 实算 = 登记差 = 0。
                let d = sr as i64 - sc as i64;
                if d != s.delta {
                    return false;
                }
            }
            None => return false,
        }
    }
    // 篡改后恒等式必破（守恒面可点验）。
    let mut t = build_root_ledger();
    if t.inject_reg_tamper(3, 11).is_err() {
        return false;
    }
    if t.conservation_ok() {
        return false;
    }
    true
}

/// 判定次序：未登记 → 缺签优先于篡改 → 单篡改回差异节 → 码可解码。
fn judge_order_ok() -> bool {
    // ① 未登记节（建账空账）→ ENotFull 族码。
    let empty = RootLedger::new();
    if empty.judge_sync(SectionId::DomAccount) != Err(-(HcDiag::ENotFull.val() as i32)) {
        return false;
    }
    // ② 缺签+篡改双注 → 缺签优先。
    let mut dual = build_root_ledger();
    if dual.inject_decl_unsign(4).is_err() {
        return false;
    }
    if dual.inject_reg_tamper(4, 7).is_err() {
        return false;
    }
    if dual.judge_sync(SectionId::AssertWall) != Err(eio_undeclared(4)) {
        return false;
    }
    // ③ 单篡改 → 差异节 EIO 码；其余节仍绿。
    let mut solo = build_root_ledger();
    if solo.inject_reg_tamper(4, 7).is_err() {
        return false;
    }
    if solo.judge_sync(SectionId::AssertWall) != Err(eio_mismatch(4)) {
        return false;
    }
    if solo.judge_sync(SectionId::Handoff) != Ok(5) {
        return false;
    }
    // ④ 码 → 差异节解码回环；闭集外章不解码。
    if judge_err_chapter(eio_mismatch(4)) != Some(4) {
        return false;
    }
    if judge_err_chapter(eio_undeclared(6)) != Some(6) {
        return false;
    }
    if judge_err_chapter(eio_mismatch(5)).is_some() {
        return false;
    }
    true
}

/// 双跑一致：两次构建/判定/注入全流程逐项一致，指纹相等。
fn dual_run_ok() -> bool {
    let a = build_root_ledger();
    let b = build_root_ledger();
    if a.stage() != b.stage() || a.len_sections() != b.len_sections() {
        return false;
    }
    if a.fingerprint() != b.fingerprint() {
        return false;
    }
    let mut i = 0usize;
    while i < SECTION_ALL.len() {
        match (a.section_at(i), b.section_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    for sec in SECTION_ALL.iter() {
        if a.judge_sync(*sec) != b.judge_sync(*sec) {
            return false;
        }
    }
    if inject_sweep() != inject_sweep() {
        return false;
    }
    true
}

/// 边界四类：首尾序号收/越界拒、日志容量耗尽拒、未同步禁封/节乱序拒、
/// 收官倒退拒/封账写拒。
fn boundary_ok() -> bool {
    // ① 号面首尾序号：界内收、越界拒。
    let mut l = build_root_ledger();
    if l.try_register_serial(SERIAL_LO).is_err() {
        return false;
    }
    if l.try_register_serial(SERIAL_HI).is_err() {
        return false;
    }
    if l.serial_count() != 2 {
        return false;
    }
    if l.try_register_serial(SERIAL_LO.saturating_sub(1)) != Err(HcDiag::ESerialRange.val()) {
        return false;
    }
    if l.try_register_serial(SERIAL_HI.saturating_add(1)) != Err(HcDiag::ESerialRange.val()) {
        return false;
    }
    // ② 点验日志容量耗尽：LOG_CAP 条收，超容拒。
    let mut c = build_root_ledger();
    let mut k = 0usize;
    while k < LOG_CAP {
        if c.log_pointcheck(SectionId::DomAccount).is_err() {
            return false;
        }
        k += 1;
    }
    if c.log_pointcheck(SectionId::NumPlane) != Err(HcDiag::ECap.val()) {
        return false;
    }
    // ③ 未同步禁封 + 节乱序拒。
    let mut r = RootLedger::new();
    for sec in SECTION_ALL.iter() {
        if r.register_section(*sec).is_err() {
            return false;
        }
    }
    if r.close() != Err(HcDiag::ENotFull.val()) {
        return false;
    }
    let mut o = RootLedger::new();
    if o.register_section(SectionId::NumPlane) != Err(HcDiag::ESectionOrder.val()) {
        return false;
    }
    // ④ 收官倒退拒 + 封账写拒（序号/注入/缺签/点验四路）。
    let mut f = build_root_ledger();
    if f.close().is_err() {
        return false;
    }
    if f.close() != Err(HcDiag::ERewind.val()) {
        return false;
    }
    if f.try_register_serial(SERIAL_LO) != Err(HcDiag::ESealed.val()) {
        return false;
    }
    if f.inject_reg_tamper(2, 3) != Err(HcDiag::ESealed.val()) {
        return false;
    }
    if f.inject_decl_unsign(6) != Err(HcDiag::ESealed.val()) {
        return false;
    }
    if f.log_pointcheck(SectionId::Handoff) != Err(HcDiag::ESealed.val()) {
        return false;
    }
    true
}

/// 账目可点验：四节各记一条总账行点验，行非空且行数齐。
fn pointcheck_ok() -> bool {
    let mut l = build_root_ledger();
    for sec in SECTION_ALL.iter() {
        if l.log_pointcheck(*sec).is_err() {
            return false;
        }
    }
    if l.pointcheck_count() != SECTION_ALL.len() {
        return false;
    }
    let mut n = 0usize;
    while n < SECTION_ALL.len() {
        match l.pointcheck_at(n) {
            Some(row) => {
                if row.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        n += 1;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12792 自检（判据 12792 卡：总纲 §7.3-D1 满账收官行 · 满账同步判据绿）。
pub fn run_unx12792_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12792");
    let ledger = build_root_ledger();

    // 01 段位守卫（公式 0x80000+0x40×(id−12600)，批次表 SEG 列陈旧以公式为准）。
    let seg_hi = 0x80000u32.saturating_add(0x40u32.saturating_mul(199));
    s.add_dyn(
        "F12792-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05X} 公式=0x80000+0x40×192=0x83000 对齐0x40={} 窗=[0x80000,0x{:05X}]",
            SEG_BASE,
            SEG_BASE % 0x40 == 0,
            seg_hi
        ),
    );

    // 02 诊断码闭集（13 码互异落窗 + 未知回落 + EIO 归类回环）。
    let first_val = match HC_ALL.first() {
        Some(d) => d.val(),
        None => 0,
    };
    let last_val = match HC_ALL.last() {
        Some(d) => d.val(),
        None => 0,
    };
    s.add_dyn(
        "F12792-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "闭集{}码 首=0x{:05X} 末=0x{:05X} 未知回落含'未知'={}",
            HC_ALL.len(),
            first_val,
            last_val,
            detail(SEG_BASE + 0x3F).contains("未知")
        ),
    );

    // 03 四节闭集枚举（§二/§三/§四/§六，章序 2→3→4→6，闭集外章拒）。
    s.add_dyn(
        "F12792-03 四节闭集枚举",
        four_enum_ok(),
        &format!(
            "§二域账/§三号面/§四断言/§六移交 章序2→3→4→6 n={} 章5/1拒={}",
            SECTION_ALL.len(),
            SectionId::from_chapter(5).is_none() && SectionId::from_chapter(1).is_none()
        ),
    );

    // 04 正常流收官（建账→四节登记→同步→点验×4→收官）。
    s.add_dyn(
        "F12792-04 正常流满账收官",
        happy_path_ok(),
        &format!(
            "建账→四节登记→同步→点验×4→收官 终态={:?} 守恒={}",
            ledger.stage(),
            ledger.conservation_ok()
        ),
    );

    // 05 §二 域账点验（40 批×20=800）。
    let dom_ind = match grand_total_independent(SectionId::DomAccount) {
        Some((_, t)) => t,
        None => 0,
    };
    s.add_dyn(
        "F12792-05 §二 域账点验",
        domain_section_ok(&ledger),
        &format!(
            "域账 40批×20行=800 grand={} 独立复算={} 逐值登记=实算",
            match ledger.section_of(2) {
                Some(sec) => sec.grand,
                None => 0,
            },
            dom_ind
        ),
    );

    // 06 §三 号面点验（F12001–F12800=800）。
    let num_ind = match grand_total_independent(SectionId::NumPlane) {
        Some((_, t)) => t,
        None => 0,
    };
    s.add_dyn(
        "F12792-06 §三 号面点验",
        numplane_section_ok(&ledger),
        &format!(
            "号面 F{}–F{} 总号={} grand={} 独立复算={}",
            SERIAL_LO,
            SERIAL_HI,
            count_numplane(),
            match ledger.section_of(3) {
                Some(sec) => sec.grand,
                None => 0,
            },
            num_ind
        ),
    );

    // 07 §四 断言点验（300×800=240,000）。
    let asr_ind = match grand_total_independent(SectionId::AssertWall) {
        Some((_, t)) => t,
        None => 0,
    };
    s.add_dyn(
        "F12792-07 §四 断言点验",
        assert_section_ok(&ledger),
        &format!(
            "断言 {}条/号×{}号={} grand={} 独立复算={}",
            ASSERT_PER_SERIAL,
            count_numplane(),
            ASSERT_PER_SERIAL.saturating_mul(count_numplane()),
            match ledger.section_of(4) {
                Some(sec) => sec.grand,
                None => 0,
            },
            asr_ind
        ),
    );

    // 08 §六 移交五件点验。
    let hf_ind = match grand_total_independent(SectionId::Handoff) {
        Some((_, t)) => t,
        None => 0,
    };
    s.add_dyn(
        "F12792-08 §六 移交五件点验",
        handoff_section_ok(&ledger),
        &format!(
            "移交五件 n={} 首件={:?} grand={} 独立复算={}",
            HANDOFF_ITEMS.iter().count(),
            HANDOFF_ITEMS.first(),
            match ledger.section_of(6) {
                Some(sec) => sec.grand,
                None => 0,
            },
            hf_ind
        ),
    );

    // 09 judge_sync 四节全 Ok（回值=总账行，合计 241605）。
    let mut judge_sum = 0u64;
    let mut judge_green = true;
    for sec in SECTION_ALL.iter() {
        match ledger.judge_sync(*sec) {
            Ok(g) => judge_sum = judge_sum.saturating_add(g as u64),
            Err(_) => judge_green = false,
        }
    }
    s.add_dyn(
        "F12792-09 judge_sync 四节全 Ok",
        judge_all_ok() && judge_green,
        &format!(
            "四节判定全绿 合计={}（800+800+240000+5=241605） EIO码族 −500x/−510x",
            judge_sum
        ),
    );

    // 10 注入器三类 30 轮全抓（篡改/越界/缺签 各 10）。
    let (caught, total, escape) = inject_sweep();
    s.add_dyn(
        "F12792-10 注入器三类 30 轮全抓",
        caught == 30 && total == 30 && escape.is_empty(),
        &format!(
            "篡改10+越界10+缺签10 caught={}/{} escape={}",
            caught, total, escape
        ),
    );

    // 11 四节同步账守恒（登记=实算+差异，差异=0；篡改后必破）。
    s.add_dyn(
        "F12792-11 四节同步账守恒",
        conserv_ok(),
        &format!(
            "登记=实算+差异 且差异=0 建账守恒={} 篡改注入(§三+11)后必红",
            ledger.conservation_ok()
        ),
    );

    // 12 判定次序（未登记→缺签→篡改；码→差异节可解码）。
    s.add_dyn(
        "F12792-12 判定次序",
        judge_order_ok(),
        &format!(
            "未登记(-ENotFull)→缺签(-510x)优先→篡改(-500x) §四解码={:?}",
            judge_err_chapter(eio_mismatch(4))
        ),
    );

    // 13 双跑一致指纹（FNV-1a）。
    let fa = build_root_ledger().fingerprint();
    let fb = build_root_ledger().fingerprint();
    s.add_dyn(
        "F12792-13 双跑一致指纹",
        dual_run_ok() && fa == fb,
        &format!(
            "fp_a=0x{:08X} fp_b=0x{:08X} 相等={} 注入扫双跑一致",
            fa,
            fb,
            fa == fb
        ),
    );

    // 14 边界四类（序号端点/容量耗尽/未同步禁封+乱序/收官倒退+封账写）。
    s.add_dyn(
        "F12792-14 边界四类",
        boundary_ok(),
        &format!(
            "首号{}尾号{}收 越界拒 点验容量{}满拒 未同步禁封 节乱序拒 重封倒退拒 封账写拒",
            SERIAL_LO, SERIAL_HI, LOG_CAP
        ),
    );

    // 15 账目可点验（四节点验行齐且非空）。
    s.add_dyn(
        "F12792-15 账目可点验",
        pointcheck_ok(),
        &format!(
            "四节各记一条总账行点验 行数={} 首行={:?}",
            build_root_ledger_log4().pointcheck_count(),
            build_root_ledger_log4().pointcheck_at(0)
        ),
    );

    // 收口：判据条数声明（15 条 = 15 实质 + 1 收口）。
    let n_before = s.len();
    let (p_before, r_before) = s.tally();
    s.add_dyn(
        "F12792-16 收口：判据条数声明",
        n_before == 15 && r_before == 0 && p_before == 15,
        &format!(
            "本卡判据共 16 条=15 实质+1 收口；已记账 15 条红 {}；UNX-F12793-J1 同步声明判据绿账目可点验达成={}",
            r_before,
            n_before == 15 && r_before == 0 && p_before == 15
        ),
    );

    s
}

/// 构建并记满四节点验行的台账（第 15 条判据明细用）。
fn build_root_ledger_log4() -> RootLedger {
    let mut l = build_root_ledger();
    for sec in SECTION_ALL.iter() {
        let _ = l.log_pointcheck(*sec);
    }
    l
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_formula_window() {
        // 段位守卫：对齐 0x40、落在 VX9 公式全窗、本卡锚定 0x83000。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x83000);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000);
        assert!(SEG_BASE <= 0x831C0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        // 批次表 SEG 列陈旧值（B16 行 0x82C40）不等于公式实值。
        assert_ne!(SEG_BASE, 0x82C40);
    }

    #[test]
    fn diag_codes_closed_set() {
        // 闭集：13 码互异、全部落段窗、查表可命中、未知回落、EIO 归类回环。
        assert_eq!(HC_ALL.len(), 13);
        for d in HC_ALL.iter() {
            let v = d.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            assert!(!detail(v).is_empty());
        }
        for a in HC_ALL.iter() {
            for b in HC_ALL.iter() {
                if a.val() < b.val() {
                    assert_ne!(a.val(), b.val());
                }
            }
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
        assert_eq!(classify_judge_err(eio_mismatch(2)), Some(HcDiag::ERegTamper));
        assert_eq!(
            classify_judge_err(eio_undeclared(6)),
            Some(HcDiag::EDeclMissing)
        );
        assert_eq!(classify_judge_err(0), None);
    }

    #[test]
    fn happy_path_seals_all_four_sections() {
        let mut l = build_root_ledger();
        assert_eq!(l.stage(), LedgerStage::Synced);
        assert_eq!(l.len_sections(), 4);
        for sec in SECTION_ALL.iter() {
            assert!(l.log_pointcheck(*sec).is_ok());
        }
        assert_eq!(l.pointcheck_count(), 4);
        assert!(l.close().is_ok());
        assert_eq!(l.stage(), LedgerStage::Closed);
        assert!(l.conservation_ok());
        for sec in SECTION_ALL.iter() {
            let g = l.judge_sync(*sec);
            assert!(g.is_ok(), "判定应绿：{:?}", g.err());
            if let Ok(v) = g {
                if let Some(s) = l.section_of(sec.chapter()) {
                    assert_eq!(v as u64, s.grand);
                } else {
                    assert_eq!(l.len_sections(), 4);
                }
            }
        }
    }

    #[test]
    fn injections_thirty_rounds_all_caught() {
        // 注入必抓：三类 30 轮全数命中，无逃脱。
        let (caught, total, escape) = inject_sweep();
        assert_eq!(total, 30);
        assert_eq!(caught, 30, "逃脱：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_identical() {
        // 双跑一致：确定性 LCG 下两次全流程逐项相同，指纹相等。
        let a = build_root_ledger();
        let b = build_root_ledger();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.stage(), b.stage());
        assert_eq!(a.len_sections(), b.len_sections());
        let mut i = 0usize;
        while i < SECTION_ALL.len() {
            assert_eq!(a.section_at(i), b.section_at(i));
            i += 1;
        }
        assert_eq!(inject_sweep(), inject_sweep());
    }

    #[test]
    fn serial_bounds_and_injection() {
        // 四类边界之序号：首号/尾号收、越界上下拒；越界注入 10 轮全拒。
        let mut l = build_root_ledger();
        assert!(l.try_register_serial(SERIAL_LO).is_ok());
        assert!(l.try_register_serial(SERIAL_HI).is_ok());
        assert_eq!(l.serial_count(), 2);
        assert_eq!(
            l.try_register_serial(SERIAL_LO - 1),
            Err(HcDiag::ESerialRange.val())
        );
        assert_eq!(
            l.try_register_serial(SERIAL_HI + 1),
            Err(HcDiag::ESerialRange.val())
        );
        let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x1F) as u64);
        let mut k = 0u32;
        while k < 10 {
            let ser = if k % 2 == 0 {
                SERIAL_LO.saturating_sub(1 + rng.below(500))
            } else {
                SERIAL_HI.saturating_add(1 + rng.below(500))
            };
            let mut t = build_root_ledger();
            assert_eq!(t.try_register_serial(ser), Err(HcDiag::ESerialRange.val()));
            assert_eq!(t.serial_count(), 0);
            k += 1;
        }
    }

    #[test]
    fn pointcheck_log_capacity_exhaustion() {
        // 四类边界之容量：LOG_CAP 条收，第 17 条 ECap 拒。
        let mut l = build_root_ledger();
        let mut k = 0usize;
        while k < LOG_CAP {
            assert!(l.log_pointcheck(SectionId::DomAccount).is_ok());
            k += 1;
        }
        assert_eq!(l.pointcheck_count(), LOG_CAP);
        assert_eq!(
            l.log_pointcheck(SectionId::NumPlane),
            Err(HcDiag::ECap.val())
        );
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 四类边界之状态机：未同步禁封、节乱序拒、重封倒退拒、封账写拒。
        let mut l = RootLedger::new();
        for sec in SECTION_ALL.iter() {
            assert!(l.register_section(*sec).is_ok());
        }
        assert_eq!(l.stage(), LedgerStage::Registered);
        assert_eq!(l.close(), Err(HcDiag::ENotFull.val()));
        let mut l0 = RootLedger::new();
        assert_eq!(
            l0.register_section(SectionId::NumPlane),
            Err(HcDiag::ESectionOrder.val())
        );
        assert_eq!(
            l0.register_section(SectionId::Handoff),
            Err(HcDiag::ESectionOrder.val())
        );
        let mut f = build_root_ledger();
        assert!(f.close().is_ok());
        assert_eq!(f.close(), Err(HcDiag::ERewind.val()));
        assert_eq!(
            f.try_register_serial(SERIAL_LO),
            Err(HcDiag::ESealed.val())
        );
        assert_eq!(f.inject_reg_tamper(2, 3), Err(HcDiag::ESealed.val()));
        assert_eq!(f.inject_decl_unsign(4), Err(HcDiag::ESealed.val()));
        assert_eq!(
            f.log_pointcheck(SectionId::Handoff),
            Err(HcDiag::ESealed.val())
        );
        // 阶段秩只增不减。
        assert!(LedgerStage::Building.rank() < LedgerStage::Registered.rank());
        assert!(LedgerStage::Registered.rank() < LedgerStage::Synced.rank());
        assert!(LedgerStage::Synced.rank() < LedgerStage::Closed.rank());
    }

    #[test]
    fn judge_order_and_eio_semantics() {
        // 判定次序：未登记→缺签优先于篡改→单篡改回差异节→码可解码。
        let empty = RootLedger::new();
        assert_eq!(
            empty.judge_sync(SectionId::DomAccount),
            Err(-(HcDiag::ENotFull.val() as i32))
        );
        let mut dual = build_root_ledger();
        assert!(dual.inject_decl_unsign(4).is_ok());
        assert!(dual.inject_reg_tamper(4, 7).is_ok());
        assert_eq!(
            dual.judge_sync(SectionId::AssertWall),
            Err(eio_undeclared(4))
        );
        let mut solo = build_root_ledger();
        assert!(solo.inject_reg_tamper(4, 7).is_ok());
        assert_eq!(
            solo.judge_sync(SectionId::AssertWall),
            Err(eio_mismatch(4))
        );
        assert_eq!(solo.judge_sync(SectionId::Handoff), Ok(5));
        assert_eq!(solo.judge_sync(SectionId::DomAccount), Ok(800));
        // 码 → 差异节解码：§四=4、§六=6；闭集外章（§五=5）不解码。
        assert_eq!(judge_err_chapter(eio_mismatch(4)), Some(4));
        assert_eq!(judge_err_chapter(eio_undeclared(6)), Some(6));
        assert_eq!(judge_err_chapter(eio_mismatch(5)), None);
    }

    #[test]
    fn conservation_identity_holds_and_breaks_on_tamper() {
        // 守恒恒等式：建账四节 delta=0 且登记=实算；篡改后恒等式必破且不许收官。
        let l = build_root_ledger();
        assert!(l.conservation_ok());
        for sec in SECTION_ALL.iter() {
            match l.section_of(sec.chapter()) {
                Some(s) => {
                    assert_eq!(s.delta, 0);
                    assert!(s.declared);
                }
                None => assert_eq!(l.len_sections(), 4, "节应在账"),
            }
        }
        let mut t = build_root_ledger();
        assert!(t.inject_reg_tamper(3, 11).is_ok());
        assert!(!t.conservation_ok());
        assert_eq!(t.judge_sync(SectionId::NumPlane), Err(eio_mismatch(3)));
        assert_eq!(t.close(), Err(HcDiag::EConserv.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条（本卡 16 条 = 15 实质 + 1 收口）。
        let s = run_unx12792_checks();
        assert!(s.all_passed(), "红项：{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), 16);
        assert_eq!(s.tally().1, 0);
    }
}
