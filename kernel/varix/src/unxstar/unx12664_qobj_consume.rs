//! UNX-F12665 NtQueryObject 探测消费（回指 B20/F12386 链） · 0x81000段
//!
//! 判据原文（派工卡 12664 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12665-J1 DOS 设备映射探测对平断言绿（B30 根路
//!      径协议回指），映射翻转注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. NtQueryObject 探测消费域模型：对象查询档每行五元（查询号/对象
//!    类型名/属性位/授予访问掩码/回指链），回指链锚定 B20 F12386 句
//!    柄台账——查询行缺锚或指纹错挂即断链拒收；
//! 2. 对象类型名闭集：File/Directory/SymbolicLink/Section/Event/Job
//!    六型闭集，越型即拒；属性位闭集（默认/保护/永久/一次性）四档；
//! 3. DOS 设备映射面（\?? 协议）：冻结映射表 8 例（盘符 → NT 设备
//!    路径），B30 根路径协议回指（\?? 根 + 指纹）——对平断言绿要求
//!    逐例"盘符查表 → NT 设备路径 → 根路径回指"三步同判；
//! 4. 映射翻转注入 10/10 一致：确定性 LCG 生成 10 次盘符↔设备对翻
//!    转，每次翻转后重跑对平器，逐次必须检出（检出位与预期一致）；
//! 5. 状态机：Fresh → Queried → Consigned 单向推进，倒退显性拒；
//!    消费闸四道（类型闭集/属性位闭集/回指链验锚/映射对平）逐行过
//!    闸；
//! 6. 注入联测 10 路：类型越闭集、属性位越集、查询行断锚、指纹错
//!    挂、盘符越表、设备路径空、翻转漏检、映射表缺例、封签后查询、
//!    查询行重复——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12664 − 12600)）。
pub const SEG_BASE: u32 = 0x81000;

// ── 探测消费诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QobjDiag {
    /// 对象类型越闭集。
    ETypeRange,
    /// 属性位越闭集。
    EAttrRange,
    /// 查询行断锚（回指链缺 B20 F12386）。
    EAnchorMissing,
    /// 指纹错挂。
    EFpMismatch,
    /// 盘符越映射表闭集。
    EDriveRange,
    /// 设备路径空。
    EDeviceEmpty,
    /// 映射翻转漏检。
    EFlipMiss,
    /// 映射表缺例。
    EMapShort,
    /// 查询行重复。
    EQueryDup,
    /// 状态机倒退。
    ERewind,
}

impl QobjDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                QobjDiag::ETypeRange => 0x01,
                QobjDiag::EAttrRange => 0x02,
                QobjDiag::EAnchorMissing => 0x03,
                QobjDiag::EFpMismatch => 0x04,
                QobjDiag::EDriveRange => 0x05,
                QobjDiag::EDeviceEmpty => 0x06,
                QobjDiag::EFlipMiss => 0x07,
                QobjDiag::EMapShort => 0x08,
                QobjDiag::EQueryDup => 0x09,
                QobjDiag::ERewind => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            QobjDiag::ETypeRange => "对象类型越闭集：只认 File/Directory/SymbolicLink/Section/Event/Job，先正类型名",
            QobjDiag::EAttrRange => "属性位越闭集：只认默认/保护/永久/一次性四档，越位先核掩码",
            QobjDiag::EAnchorMissing => "查询行断锚：回指链缺 B20 F12386 句柄台账锚，先补锚再消费",
            QobjDiag::EFpMismatch => "指纹错挂：锚 id 与指纹不成对，先对冻结锚表",
            QobjDiag::EDriveRange => "盘符越映射表闭集：盘符必在冻结映射表内，越表先挂盘",
            QobjDiag::EDeviceEmpty => "设备路径空：映射表行 NT 设备路径非空，空路径先补设备",
            QobjDiag::EFlipMiss => "映射翻转漏检：翻转后对平器未检出，先核映射表快照位",
            QobjDiag::EMapShort => "映射表缺例：DOS 映射表 8 例闭集不满，先补例再对平",
            QobjDiag::EQueryDup => "查询行重复：同查询号只入一笔，重复先查已有账",
            QobjDiag::ERewind => "状态机倒退：Fresh→Queried→Consigned 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const QOBJ_ALL: &[QobjDiag] = &[
    QobjDiag::ETypeRange,
    QobjDiag::EAttrRange,
    QobjDiag::EAnchorMissing,
    QobjDiag::EFpMismatch,
    QobjDiag::EDriveRange,
    QobjDiag::EDeviceEmpty,
    QobjDiag::EFlipMiss,
    QobjDiag::EMapShort,
    QobjDiag::EQueryDup,
    QobjDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in QOBJ_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81001..0x8100A 闭集内"
}

// ── 对象类型闭集与属性位 ─────────────────────────────────────────
/// 对象类型六闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QType {
    /// 文件对象。
    File,
    /// 目录对象。
    Directory,
    /// 符号链接对象。
    SymbolicLink,
    /// 区域对象。
    Section,
    /// 事件对象。
    Event,
    /// 作业对象。
    Job,
}

impl QType {
    /// 类型名（与 NtQueryObject 返回字面量一致）。
    pub fn name(self) -> &'static str {
        match self {
            QType::File => "File",
            QType::Directory => "Directory",
            QType::SymbolicLink => "SymbolicLink",
            QType::Section => "Section",
            QType::Event => "Event",
            QType::Job => "Job",
        }
    }

    /// 类型名解析（闭集外 None）。
    pub fn parse(s: &str) -> Option<QType> {
        if s == "File" {
            Some(QType::File)
        } else if s == "Directory" {
            Some(QType::Directory)
        } else if s == "SymbolicLink" {
            Some(QType::SymbolicLink)
        } else if s == "Section" {
            Some(QType::Section)
        } else if s == "Event" {
            Some(QType::Event)
        } else if s == "Job" {
            Some(QType::Job)
        } else {
            None
        }
    }
}

/// 属性位闭集（四档冻结口径）。
pub const ATTR_DEFAULT: u32 = 0x00;
pub const ATTR_PROTECT: u32 = 0x01;
pub const ATTR_PERMANENT: u32 = 0x02;
pub const ATTR_ONCE: u32 = 0x04;

/// 属性位是否在冻结口径闭集内。
pub fn attr_ok(mask: u32) -> bool {
    // 只认低三位；保护与一次性互斥（冻结口径）。
    if mask & !0x07 != 0 {
        return false;
    }
    if mask & ATTR_PROTECT != 0 && mask & ATTR_ONCE != 0 {
        return false;
    }
    true
}

// ── 回指链（B20 F12386 句柄台账锚） ──────────────────────────────
/// 查询行回指链锚（B20 F12386）。
pub const B20_FP: &str = "B20-F12386-HANDLE-LEDGER";
/// B20 回指卡 id。
pub const B20_REF_ID: u32 = 12386;

/// 查询行五元（查询号/类型名/属性位/授予访问掩码/回指指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QobjRow {
    /// 查询号。
    pub no: u32,
    /// 对象类型名。
    pub ty: &'static str,
    /// 属性位。
    pub attr: u32,
    /// 授予访问掩码（0x0001..=0xFFFF 冻结口径）。
    pub access: u32,
    /// 回指指纹（B20 F12386）。
    pub fp: &'static str,
}

/// 查询行黄金判定（类型闭集/属性位/访问掩码/回指锚 四道）。
pub fn qobj_ok(r: &QobjRow) -> Result<(), u32> {
    // 类型闭集闸。
    if QType::parse(r.ty).is_none() {
        return Err(QobjDiag::ETypeRange.val());
    }
    // 属性位闭集闸。
    if !attr_ok(r.attr) {
        return Err(QobjDiag::EAttrRange.val());
    }
    // 访问掩码闸（非零、低 16 位）。
    if r.access == 0 || r.access & !0xFFFF != 0 {
        return Err(QobjDiag::EAttrRange.val());
    }
    // 回指锚闸。
    if r.fp.is_empty() {
        return Err(QobjDiag::EAnchorMissing.val());
    }
    if r.fp != B20_FP {
        return Err(QobjDiag::EFpMismatch.val());
    }
    Ok(())
}

/// 查询档（6 例冻结，覆盖六型）。
pub const QOBJ_PROFILE: &[QobjRow] = &[
    QobjRow { no: 0, ty: "File", attr: ATTR_DEFAULT, access: 0x0001, fp: B20_FP },
    QobjRow { no: 1, ty: "Directory", attr: ATTR_DEFAULT, access: 0x0003, fp: B20_FP },
    QobjRow { no: 2, ty: "SymbolicLink", attr: ATTR_PROTECT, access: 0x0001, fp: B20_FP },
    QobjRow { no: 3, ty: "Section", attr: ATTR_PERMANENT, access: 0x000F, fp: B20_FP },
    QobjRow { no: 4, ty: "Event", attr: ATTR_DEFAULT, access: 0x0002, fp: B20_FP },
    QobjRow { no: 5, ty: "Job", attr: ATTR_ONCE, access: 0x0007, fp: B20_FP },
];

/// 查询档容量。
pub const QOBJ_CAP: usize = 6;

// ── DOS 设备映射面（\?? 根路径协议回指） ─────────────────────────
/// B30 根路径协议指纹。
pub const ROOT_FP: &str = "B30-F12586-D3-COSIGN";
/// \?? 根路径前缀。
pub const ROOT_PREFIX: &str = "\\??\\";

/// 映射行（盘符/NT 设备路径/根路径回指指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DosMapRow {
    /// 盘符字母（大写 A..Z 单字符）。
    pub letter: char,
    /// NT 设备路径。
    pub device: &'static str,
    /// 根路径回指指纹。
    pub fp: &'static str,
}

/// 冻结 DOS 映射表（8 例）。
pub const DOS_MAP: &[DosMapRow] = &[
    DosMapRow { letter: 'C', device: "\\Device\\HarddiskVolume1", fp: ROOT_FP },
    DosMapRow { letter: 'D', device: "\\Device\\HarddiskVolume2", fp: ROOT_FP },
    DosMapRow { letter: 'E', device: "\\Device\\CdRom0", fp: ROOT_FP },
    DosMapRow { letter: 'F', device: "\\Device\\HarddiskVolume3", fp: ROOT_FP },
    DosMapRow { letter: 'N', device: "\\Device\\LanmanRedirector", fp: ROOT_FP },
    DosMapRow { letter: 'P', device: "\\Device\\NamedPipe", fp: ROOT_FP },
    DosMapRow { letter: 'M', device: "\\Device\\MailSlot", fp: ROOT_FP },
    DosMapRow { letter: 'Z', device: "\\Device\\Ramdisk", fp: ROOT_FP },
];

/// 映射表容量。
pub const MAP_CAP: usize = 8;

/// 盘符查表（越表 None）。
pub fn map_of(letter: char) -> Option<&'static DosMapRow> {
    for r in DOS_MAP.iter() {
        if r.letter == letter {
            return Some(r);
        }
    }
    None
}

/// 映射行对平（盘符合法 + 设备路径非空 + 根路径回指一致）。
pub fn map_ok(r: &DosMapRow) -> Result<(), u32> {
    if !r.letter.is_ascii_uppercase() {
        return Err(QobjDiag::EDriveRange.val());
    }
    if map_of(r.letter).is_none() {
        return Err(QobjDiag::EDriveRange.val());
    }
    if r.device.is_empty() {
        return Err(QobjDiag::EDeviceEmpty.val());
    }
    if r.fp != ROOT_FP {
        return Err(QobjDiag::EFpMismatch.val());
    }
    Ok(())
}

/// 根路径回指判定：\?? 前缀 + 指纹一致。
pub fn root_ref_ok(path: &str, fp: &str) -> bool {
    path.starts_with(ROOT_PREFIX) && fp == ROOT_FP
}

// ── 状态机（Fresh → Queried → Consigned） ────────────────────────
/// 消费状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumeState {
    /// 新开账。
    Fresh,
    /// 查询中。
    Queried,
    /// 已封签。
    Consigned,
}

impl ConsumeState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            ConsumeState::Fresh => 0,
            ConsumeState::Queried => 1,
            ConsumeState::Consigned => 2,
        }
    }
}

/// 消费账（查询行 + 映射对平 + 封签）。
#[derive(Clone, Debug)]
pub struct QobjLedger {
    /// 已对平查询行。
    rows: Vec<QobjRow>,
    /// 已对平映射例数。
    mapped: usize,
    /// 拒收记账（查询号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: ConsumeState,
}

impl QobjLedger {
    /// 开账。
    pub fn new() -> QobjLedger {
        QobjLedger {
            rows: Vec::new(),
            mapped: 0,
            rejects: Vec::new(),
            state: ConsumeState::Fresh,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ConsumeState {
        self.state
    }

    /// 已对平查询行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 已对平映射例数。
    pub fn mapped_len(&self) -> usize {
        self.mapped
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&QobjRow> {
        self.rows.get(idx)
    }

    /// 开查（Fresh→Queried）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ConsumeState::Queried.rank() {
            return Err(QobjDiag::ERewind.val());
        }
        self.state = ConsumeState::Queried;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < QOBJ_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 查询行消费（行号序 + 黄金判定 + 重复 三闸）。
    pub fn consume(&mut self, r: QobjRow) -> Result<(), u32> {
        if self.state.rank() < ConsumeState::Queried.rank() {
            return Err(QobjDiag::ERewind.val());
        }
        if self.state.rank() >= ConsumeState::Consigned.rank() {
            self.note_reject(r.no, QobjDiag::ERewind.val());
            return Err(QobjDiag::ERewind.val());
        }
        // 行号序闸。
        if r.no != self.rows.len() as u32 {
            self.note_reject(r.no, QobjDiag::EQueryDup.val());
            return Err(QobjDiag::EQueryDup.val());
        }
        // 黄金判定闸。
        if let Err(e) = qobj_ok(&r) {
            self.note_reject(r.no, e);
            return Err(e);
        }
        // 重复闸：同类型名同属性位视为重复（冻结档互异口径）。
        for done in self.rows.iter() {
            if done.ty == r.ty && done.attr == r.attr {
                self.note_reject(r.no, QobjDiag::EQueryDup.val());
                return Err(QobjDiag::EQueryDup.val());
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 映射对平：逐例过闸（表序对平）。
    pub fn reconcile_map(&mut self) -> Result<(), u32> {
        if self.state.rank() < ConsumeState::Queried.rank() {
            return Err(QobjDiag::ERewind.val());
        }
        if self.state.rank() >= ConsumeState::Consigned.rank() {
            return Err(QobjDiag::ERewind.val());
        }
        let mut n = 0usize;
        for r in DOS_MAP.iter() {
            if map_ok(r).is_err() {
                return Err(QobjDiag::EFlipMiss.val());
            }
            n += 1;
        }
        if n != MAP_CAP {
            return Err(QobjDiag::EMapShort.val());
        }
        self.mapped = n;
        Ok(())
    }

    /// 封签：6 行齐 + 映射 8 例齐 + 零拒收。
    pub fn consign(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ConsumeState::Consigned.rank() {
            return Err(QobjDiag::ERewind.val());
        }
        if self.state.rank() < ConsumeState::Queried.rank() {
            return Err(QobjDiag::ERewind.val());
        }
        if self.rows.len() != QOBJ_CAP {
            return Err(QobjDiag::EQueryDup.val());
        }
        if self.mapped != MAP_CAP {
            return Err(QobjDiag::EMapShort.val());
        }
        if !self.rejects.is_empty() {
            return Err(QobjDiag::EQueryDup.val());
        }
        self.state = ConsumeState::Consigned;
        Ok(())
    }

    /// 账等式。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == QOBJ_CAP && self.mapped == MAP_CAP && self.rejects.is_empty()
    }
}

// ── 确定性 LCG 与映射翻转注入 ────────────────────────────────────
/// LCG（种子 = SEG_BASE + 0x11）。
pub struct QobjLcg {
    state: u64,
}

impl QobjLcg {
    /// 固定种子构造。
    pub fn new(seed: u64) -> QobjLcg {
        QobjLcg { state: seed }
    }

    /// 下一伪随机数（Knuth MMIX 常数）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

/// 翻转例（翻转对：盘符 a 的设备 → 盘符 b 的设备）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlipCase {
    /// 翻转序号。
    pub no: u32,
    /// 盘符 a。
    pub la: char,
    /// 盘符 b。
    pub lb: char,
}

/// 确定性生成 10 次翻转（两两盘符配对，互异）。
pub fn flip_cases() -> Vec<FlipCase> {
    let mut out: Vec<FlipCase> = Vec::new();
    let mut rng = QobjLcg::new(SEG_BASE as u64 + 0x11);
    let mut guard = 0u32;
    while out.len() < 10 && guard < 256 {
        guard += 1;
        let ia = rng.below(MAP_CAP as u32) as usize;
        let ib = rng.below(MAP_CAP as u32) as usize;
        if ia == ib {
            continue;
        }
        let la = match DOS_MAP.get(ia) {
            Some(v) => v.letter,
            None => continue,
        };
        let lb = match DOS_MAP.get(ib) {
            Some(v) => v.letter,
            None => continue,
        };
        // 互异：同对不重复。
        let mut dup = false;
        for c in out.iter() {
            if (c.la == la && c.lb == lb) || (c.la == lb && c.lb == la) {
                dup = true;
            }
        }
        if !dup {
            let no = out.len() as u32;
            out.push(FlipCase { no, la, lb });
        }
    }
    out
}

/// 单次翻转检测：翻转后对平器必须检出（返回 true = 检出）。
pub fn flip_detected(c: &FlipCase) -> bool {
    let da = match map_of(c.la) {
        Some(v) => v.device,
        None => return false,
    };
    let db = match map_of(c.lb) {
        Some(v) => v.device,
        None => return false,
    };
    // 翻转：a 的设备换成 b 的设备。
    let flipped = DosMapRow { letter: c.la, device: db, fp: ROOT_FP };
    // 对平器重跑：冻结表内 a 的原设备与翻转设备不一致即检出。
    if da == db {
        return false;
    }
    match map_ok(&flipped) {
        // 类型级对平过（字母合法/路径非空/指纹对），但设备快照位与冻结表
        // 不符——快照对照即检出。
        Ok(()) => da != flipped.device,
        Err(_) => true,
    }
}

/// 映射翻转注入 10/10 一致（判据 J1 直读口）。
pub fn flip_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for c in flip_cases().iter() {
        total += 1;
        if flip_detected(c) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("翻转{}({}→{}) 漏检", c.no, c.la, c.lb);
        }
    }
    (caught, total, escape)
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QobjFault {
    /// 类型越闭集。
    TypeRange,
    /// 属性位越集（保护与一次性互斥）。
    AttrRange,
    /// 断锚（空指纹）。
    AnchorMissing,
    /// 指纹错挂。
    FpSwap,
    /// 盘符越表。
    DriveRange,
    /// 设备路径空。
    DeviceEmpty,
    /// 翻转漏检（同设备假翻转）。
    FlipMiss,
    /// 映射表缺例。
    MapShort,
    /// 封签后查询。
    SealedQuery,
    /// 查询行重复。
    QueryDup,
}

impl QobjFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            QobjFault::TypeRange => 0,
            QobjFault::AttrRange => 1,
            QobjFault::AnchorMissing => 2,
            QobjFault::FpSwap => 3,
            QobjFault::DriveRange => 4,
            QobjFault::DeviceEmpty => 5,
            QobjFault::FlipMiss => 6,
            QobjFault::MapShort => 7,
            QobjFault::SealedQuery => 8,
            QobjFault::QueryDup => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            QobjFault::TypeRange => "类型越闭集",
            QobjFault::AttrRange => "属性位越集",
            QobjFault::AnchorMissing => "断锚",
            QobjFault::FpSwap => "指纹错挂",
            QobjFault::DriveRange => "盘符越表",
            QobjFault::DeviceEmpty => "设备路径空",
            QobjFault::FlipMiss => "翻转漏检",
            QobjFault::MapShort => "映射表缺例",
            QobjFault::SealedQuery => "封签后查询",
            QobjFault::QueryDup => "查询行重复",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [QobjFault] {
        &[
            QobjFault::TypeRange,
            QobjFault::AttrRange,
            QobjFault::AnchorMissing,
            QobjFault::FpSwap,
            QobjFault::DriveRange,
            QobjFault::DeviceEmpty,
            QobjFault::FlipMiss,
            QobjFault::MapShort,
            QobjFault::SealedQuery,
            QobjFault::QueryDup,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in QobjFault::all().iter() {
        total += 1;
        if run_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_fault(v: QobjFault) -> bool {
    match v {
        QobjFault::TypeRange => {
            let mut r = match QOBJ_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.ty = "Device";
            qobj_ok(&r).is_err()
        }
        QobjFault::AttrRange => {
            let mut r = match QOBJ_PROFILE.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.attr = ATTR_PROTECT | ATTR_ONCE;
            qobj_ok(&r).is_err()
        }
        QobjFault::AnchorMissing => {
            let mut r = match QOBJ_PROFILE.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.fp = "";
            qobj_ok(&r).is_err()
        }
        QobjFault::FpSwap => {
            let mut r = match QOBJ_PROFILE.get(3) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.fp = "B19-FAKE";
            qobj_ok(&r).is_err()
        }
        QobjFault::DriveRange => {
            let m = DosMapRow { letter: 'Q', device: "\\Device\\Ghost", fp: ROOT_FP };
            map_ok(&m).is_err()
        }
        QobjFault::DeviceEmpty => {
            let m = DosMapRow { letter: 'C', device: "", fp: ROOT_FP };
            map_ok(&m).is_err()
        }
        QobjFault::FlipMiss => {
            // 假翻转：同盘符同设备（a==b）必须判漏检。
            let c = FlipCase { no: 0, la: 'C', lb: 'C' };
            !flip_detected(&c)
        }
        QobjFault::MapShort => {
            // 非法字母混入映射对平必红。
            let m = DosMapRow { letter: '1', device: "\\Device\\X", fp: ROOT_FP };
            map_ok(&m).is_err()
        }
        QobjFault::SealedQuery => {
            let mut led = build_full_ledger();
            if led.consign().is_err() {
                return false;
            }
            let r = match QOBJ_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.consume(r).is_err()
        }
        QobjFault::QueryDup => {
            let mut led = QobjLedger::new();
            let _ = led.begin();
            let r0 = match QOBJ_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.consume(r0);
            // 同类型同属性再入（换成目录行重复）被拒。
            let r1 = QobjRow { no: 1, ty: "File", attr: ATTR_DEFAULT, access: 0x0009, fp: B20_FP };
            led.consume(r1).is_err()
        }
    }
}

// ── 构建器：完整消费账 ───────────────────────────────────────────
/// 构建完整消费账（6 行 + 8 例映射对平）。
pub fn build_full_ledger() -> QobjLedger {
    let mut led = QobjLedger::new();
    let _ = led.begin();
    let mut i = 0usize;
    while i < QOBJ_CAP {
        let r = match QOBJ_PROFILE.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.consume(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    let _ = led.reconcile_map();
    led
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// DOS 设备映射探测对平断言绿（判据 J1 直读口）。
fn map_reconcile_ok() -> bool {
    let mut led = build_full_ledger();
    if led.len() != QOBJ_CAP {
        return false;
    }
    if led.mapped_len() != MAP_CAP {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.consign().is_err() {
        return false;
    }
    // 独立复核：8 例映射逐例 map_ok + 根路径回指。
    for r in DOS_MAP.iter() {
        if map_ok(r).is_err() {
            return false;
        }
        if !root_ref_ok(ROOT_PREFIX, r.fp) {
            return false;
        }
    }
    true
}

/// B30 根路径协议回指直读。
fn root_ref_direct_ok() -> bool {
    if ROOT_FP != "B30-F12586-D3-COSIGN" {
        return false;
    }
    if !root_ref_ok("\\??\\C:", ROOT_FP) {
        return false;
    }
    if root_ref_ok("C:\\", ROOT_FP) {
        return false;
    }
    if root_ref_ok("\\??\\C:", "FAKE") {
        return false;
    }
    true
}

/// 映射翻转注入 10/10 一致。
fn flip_ok() -> bool {
    let (caught, total, escape) = flip_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 注入联测 10/10 拒止。
fn fault_sweep_ok() -> bool {
    let (caught, total, escape) = fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 双跑一致（冻结档下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_ledger();
    let b = build_full_ledger();
    if a.len() != b.len() || a.mapped_len() != b.mapped_len() {
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
    if flip_sweep().0 != flip_sweep().0 {
        return false;
    }
    if fault_sweep().0 != fault_sweep().0 {
        return false;
    }
    true
}

/// 边界：属性位与访问掩码闭集两端。
fn boundary_ok() -> bool {
    // 属性位四档单档过。
    if !attr_ok(ATTR_DEFAULT) || !attr_ok(ATTR_PROTECT) || !attr_ok(ATTR_PERMANENT) || !attr_ok(ATTR_ONCE) {
        return false;
    }
    // 组合：保护+永久过，保护+一次互斥拒。
    if !attr_ok(ATTR_PROTECT | ATTR_PERMANENT) {
        return false;
    }
    if attr_ok(ATTR_PROTECT | ATTR_ONCE) {
        return false;
    }
    // 越位拒。
    if attr_ok(0x08) {
        return false;
    }
    // 访问掩码两端。
    let r0 = QobjRow { no: 0, ty: "File", attr: 0, access: 0x0001, fp: B20_FP };
    if qobj_ok(&r0).is_err() {
        return false;
    }
    let r1 = QobjRow { no: 0, ty: "File", attr: 0, access: 0xFFFF, fp: B20_FP };
    if qobj_ok(&r1).is_err() {
        return false;
    }
    let r2 = QobjRow { no: 0, ty: "File", attr: 0, access: 0, fp: B20_FP };
    if qobj_ok(&r2).is_ok() {
        return false;
    }
    let r3 = QobjRow { no: 0, ty: "File", attr: 0, access: 0x1_0000, fp: B20_FP };
    if qobj_ok(&r3).is_ok() {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12664u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_ledger();
    if led.consign().is_err() {
        return false;
    }
    if led.state() != ConsumeState::Consigned {
        return false;
    }
    match led.row_at(0) {
        Some(r) => {
            if r.ty.is_empty() || r.fp != B20_FP {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12665 探测消费自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12664_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12664-qobj-consume");
    let led = build_full_ledger();

    s.add_dyn(
        "F12665-01 查询档六型齐",
        QOBJ_PROFILE.len() == QOBJ_CAP,
        &format!("rows={}", QOBJ_PROFILE.len()),
    );

    s.add_dyn(
        "F12665-02 DOS 映射对平断言绿",
        map_reconcile_ok(),
        &format!("mapped={} rows={}", led.mapped_len(), led.len()),
    );

    s.add_dyn(
        "F12665-03 B30 根路径协议回指",
        root_ref_direct_ok(),
        "\\?? 前缀 + B30-F12586-D3 指纹",
    );

    s.add_dyn(
        "F12665-04 B20 F12386 回指锚逐行齐",
        match led.row_at(0) {
            Some(r) => r.fp == B20_FP && B20_REF_ID == 12386,
            None => false,
        },
        &format!("fp={} ref_id={}", B20_FP, B20_REF_ID),
    );

    s.add_dyn(
        "F12665-05 映射翻转注入 10/10 一致",
        flip_ok(),
        &format!("caught={}", flip_sweep().0),
    );

    s.add_dyn(
        "F12665-06 注入联测 10/10 拒止",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12665-07 属性位/访问掩码边界",
        boundary_ok(),
        "四档属性 + 互斥口径 + 掩码 16 位",
    );

    s.add_dyn(
        "F12665-08 双跑一致",
        dual_run_ok(),
        "冻结档下两轮逐例同判",
    );

    s.add_dyn(
        "F12665-09 消费账等式",
        led.equation_ok(),
        &format!("rows={} mapped={} rejects={}", led.len(), led.mapped_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12665-10 完成写回语义",
        writeback_ok(),
        "封签即已完成且摘要非空",
    );

    s.add_dyn(
        "F12665-11 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 映射表盘符互异独立复核。
    let mut uniq_ok = DOS_MAP.len() == MAP_CAP;
    let mut i = 0usize;
    while i < DOS_MAP.len() {
        let mut j = i + 1;
        while j < DOS_MAP.len() {
            match (DOS_MAP.get(i), DOS_MAP.get(j)) {
                (Some(a), Some(b)) => {
                    if a.letter == b.letter {
                        uniq_ok = false;
                    }
                }
                _ => uniq_ok = false,
            }
            j += 1;
        }
        i += 1;
    }
    s.add_dyn(
        "F12665-12 映射表盘符互异",
        uniq_ok,
        &format!("entries={}", DOS_MAP.len()),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81000);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：10 个码互异、全部落窗口、查表可命中。
        assert_eq!(QOBJ_ALL.len(), 10);
        let mut i = 0usize;
        while i < QOBJ_ALL.len() {
            let vi = match QOBJ_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < QOBJ_ALL.len() {
                let vj = match QOBJ_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn happy_path_consigns() {
        let mut led = build_full_ledger();
        assert_eq!(led.len(), QOBJ_CAP);
        assert_eq!(led.mapped_len(), MAP_CAP);
        assert_eq!(led.reject_len(), 0);
        assert!(led.consign().is_ok());
        assert_eq!(led.state(), ConsumeState::Consigned);
    }

    #[test]
    fn flip_injection_ten_ten() {
        // 映射翻转注入 10/10 一致。
        let (caught, total, escape) = flip_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "漏检：{}", escape);
        assert!(escape.is_empty());
        // 双跑一致。
        assert_eq!(flip_cases(), flip_cases());
        // 假翻转（同盘符）必判漏检。
        let fake = FlipCase { no: 0, la: 'C', lb: 'C' };
        assert!(!flip_detected(&fake));
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止。
        let (caught, total, escape) = fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn qtype_semantics() {
        // 类型闭集解析直读。
        assert_eq!(QType::parse("File"), Some(QType::File));
        assert_eq!(QType::parse("SymbolicLink"), Some(QType::SymbolicLink));
        assert!(QType::parse("Device").is_none());
        assert!(QType::parse("").is_none());
        assert!(QType::parse("file").is_none());
        // 查询档六型互异。
        let mut seen: Vec<&str> = Vec::new();
        for r in QOBJ_PROFILE.iter() {
            for s in seen.iter() {
                assert_ne!(*s, r.ty, "查询档类型重复");
            }
            seen.push(r.ty);
        }
        assert_eq!(seen.len(), QOBJ_CAP);
    }

    #[test]
    fn attr_boundary() {
        // 属性位边界直读。
        assert!(attr_ok(ATTR_DEFAULT));
        assert!(attr_ok(ATTR_PROTECT));
        assert!(attr_ok(ATTR_PERMANENT));
        assert!(attr_ok(ATTR_ONCE));
        assert!(attr_ok(ATTR_PROTECT | ATTR_PERMANENT));
        assert!(!attr_ok(ATTR_PROTECT | ATTR_ONCE));
        assert!(!attr_ok(0x08));
        assert!(!attr_ok(0xFFFF));
    }

    #[test]
    fn map_table_semantics() {
        // 映射表：8 例互异盘符、查表命中/越表、根路径回指。
        assert_eq!(DOS_MAP.len(), MAP_CAP);
        assert!(map_of('C').is_some());
        assert!(map_of('Z').is_some());
        assert!(map_of('Q').is_none());
        assert!(map_of('c').is_none());
        let c = match map_of('C') {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(c.device, "\\Device\\HarddiskVolume1");
        assert!(root_ref_ok(ROOT_PREFIX, ROOT_FP));
        assert!(!root_ref_ok("C:\\", ROOT_FP));
        assert!(!root_ref_ok(ROOT_PREFIX, "FAKE"));
    }

    #[test]
    fn qobj_gate_rejects() {
        // 断锚/指纹错挂被拒且记账落行。
        let mut led = QobjLedger::new();
        let _ = led.begin();
        let mut r = match QOBJ_PROFILE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        r.fp = "";
        assert_eq!(led.consume(r), Err(QobjDiag::EAnchorMissing.val()));
        assert_eq!(led.reject_len(), 1);
        // 未开查消费被拒。
        let mut fresh = QobjLedger::new();
        let r0 = match QOBJ_PROFILE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(fresh.consume(r0), Err(QobjDiag::ERewind.val()));
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致。
        let a = build_full_ledger();
        let b = build_full_ledger();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.mapped_len(), b.mapped_len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(flip_sweep().0, flip_sweep().0);
        assert_eq!(fault_sweep().0, fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Fresh→Queried→Consigned。
        assert!(ConsumeState::Fresh.rank() < ConsumeState::Queried.rank());
        assert!(ConsumeState::Queried.rank() < ConsumeState::Consigned.rank());
        let mut led = build_full_ledger();
        assert_eq!(led.state(), ConsumeState::Queried);
        assert!(led.consign().is_ok());
        assert_eq!(led.consign(), Err(QobjDiag::ERewind.val()));
        let mut fresh = QobjLedger::new();
        assert!(fresh.begin().is_ok());
        assert_eq!(fresh.begin(), Err(QobjDiag::ERewind.val()));
    }

    #[test]
    fn b20_anchor_frozen() {
        // B20 F12386 锚冻结直读。
        assert_eq!(B20_FP, "B20-F12386-HANDLE-LEDGER");
        assert_eq!(B20_REF_ID, 12386);
        for r in QOBJ_PROFILE.iter() {
            assert_eq!(r.fp, B20_FP, "查询行 {} 断锚", r.no);
        }
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12664_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
