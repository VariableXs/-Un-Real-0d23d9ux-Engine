//! UNX-F12666 文件对象探测（\?? 与 DOS 设备映射消费边） · 0x81040段
//!
//! 判据原文（派工卡 12665 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12666-J1 管道/邮槽对象类型探测档对平断言绿，
//!      类型错配注入 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 文件对象探测域模型：路径前缀协议三分（\Device\NamedPipe → 命名
//!    管道、\Device\MailSlot → 邮槽、\??\<盘符> → DOS 设备映射消费
//!    边）；每行五元（探测号/路径/期望类型/盘符位/映射指纹）；
//! 2. 路径分类引擎（冻结口径单一权威）：按前缀闭集分类——NamedPipe
//!    前缀 → 命名管道型、MailSlot 前缀 → 邮槽型、\??\<大写盘符> →
//!    文件型（DOS 映射边）、其余 → 未知拒；分类结果与表内期望类型
//!    逐行对平，对平断言绿是本卡验收主判据；
//! 3. DOS 设备映射消费边：\??\<盘符> 行必带冻结映射指纹（B30 根路径
//!    协议回指），盘符必在冻结映射表内，缺指纹或越表即拒；
//! 4. 类型错配注入 10/10 检出：确定性 LCG 生成 10 例类型错配（管道
//!    行挂邮槽型、文件行挂管道型、邮槽行挂文件型等），逐例必须被分
//!    类引擎检出，检出数 10/10 才判绿；
//! 5. 状态机：Idle → Probing → Charted 单向推进，倒退显性拒；探测
//!    闸四道（路径非空/分类闭集/类型对平/映射指纹）逐行过闸；
//! 6. 注入联测 10 路：类型错配漏检、路径空、前缀未知、盘符越表、缺
//!    映射指纹、小写盘符、探测号乱序、行重复、未探测即成图、成图后
//!    再探——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12665 − 12600)）。
pub const SEG_BASE: u32 = 0x81040;

// ── 文件对象探测诊断码闭集 ───────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FobjDiag {
    /// 路径为空。
    EPathEmpty,
    /// 前缀未知（不属三分闭集）。
    EPrefixUnknown,
    /// 类型错配（分类结果与期望不符）。
    ETypeMismatch,
    /// 盘符越映射表。
    EDriveRange,
    /// 盘符小写（\?? 边只认大写）。
    EDriveLower,
    /// 缺映射指纹。
    EMapFpMissing,
    /// 探测号乱序。
    EProbeOrder,
    /// 探测行重复。
    EProbeDup,
    /// 未探测即成图。
    ENotProbed,
    /// 成图后再探。
    EAfterChart,
    /// 状态机倒退。
    ERewind,
}

impl FobjDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                FobjDiag::EPathEmpty => 0x01,
                FobjDiag::EPrefixUnknown => 0x02,
                FobjDiag::ETypeMismatch => 0x03,
                FobjDiag::EDriveRange => 0x04,
                FobjDiag::EDriveLower => 0x05,
                FobjDiag::EMapFpMissing => 0x06,
                FobjDiag::EProbeOrder => 0x07,
                FobjDiag::EProbeDup => 0x08,
                FobjDiag::ENotProbed => 0x09,
                FobjDiag::EAfterChart => 0x0a,
                FobjDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            FobjDiag::EPathEmpty => "路径为空：探测行路径非空，空行先补路径再探测",
            FobjDiag::EPrefixUnknown => "前缀未知：只认 NamedPipe/MailSlot/\\?? 三分闭集，先正前缀",
            FobjDiag::ETypeMismatch => "类型错配：分类结果与期望类型不符，先核期望列或路径写法",
            FobjDiag::EDriveRange => "盘符越映射表：\\?? 边盘符必在冻结映射表内，越表先挂盘",
            FobjDiag::EDriveLower => "盘符小写：\\?? 边只认大写盘符，小写先转大写再消费",
            FobjDiag::EMapFpMissing => "缺映射指纹：\\?? 边必带 B30 根路径回指指纹，缺指纹即断链",
            FobjDiag::EProbeOrder => "探测号乱序：按探测号 0..N 逐行探测，跳号先回补上一行",
            FobjDiag::EProbeDup => "探测行重复：同路径只探一笔，重复先查已有账",
            FobjDiag::ENotProbed => "未探测即成图：逐行探测过才许成图，先探完再封",
            FobjDiag::EAfterChart => "成图后再探：成图后账面只读，改动走下一轮探测",
            FobjDiag::ERewind => "状态机倒退：Idle→Probing→Charted 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const FOBJ_ALL: &[FobjDiag] = &[
    FobjDiag::EPathEmpty,
    FobjDiag::EPrefixUnknown,
    FobjDiag::ETypeMismatch,
    FobjDiag::EDriveRange,
    FobjDiag::EDriveLower,
    FobjDiag::EMapFpMissing,
    FobjDiag::EProbeOrder,
    FobjDiag::EProbeDup,
    FobjDiag::ENotProbed,
    FobjDiag::EAfterChart,
    FobjDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in FOBJ_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81041..0x8104B 闭集内"
}

// ── 对象类型三分闭集 ─────────────────────────────────────────────
/// 探测对象类型三分闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FKind {
    /// 命名管道（\Device\NamedPipe 边）。
    NamedPipe,
    /// 邮槽（\Device\MailSlot 边）。
    MailSlot,
    /// 文件（\??\<盘符> DOS 设备映射边）。
    File,
}

impl FKind {
    /// 类型名。
    pub fn name(self) -> &'static str {
        match self {
            FKind::NamedPipe => "NamedPipe",
            FKind::MailSlot => "MailSlot",
            FKind::File => "File",
        }
    }

    /// 全部三分（闭集数组）。
    pub fn all() -> &'static [FKind] {
        &[FKind::NamedPipe, FKind::MailSlot, FKind::File]
    }
}

/// 前缀闭集（冻结口径）。
pub const PIPE_PREFIX: &str = "\\Device\\NamedPipe\\";
pub const MAILSLOT_PREFIX: &str = "\\Device\\MailSlot\\";
pub const DOS_PREFIX: &str = "\\??\\";

/// DOS 设备映射边冻结盘符表（5 例）。
pub const DOS_LETTERS: &[char] = &['C', 'D', 'E', 'F', 'Z'];

/// DOS 盘符是否在映射边闭集内。
pub fn letter_in_set(c: char) -> bool {
    for l in DOS_LETTERS.iter() {
        if *l == c {
            return true;
        }
    }
    false
}

/// 路径分类引擎（冻结口径单一权威）。
/// 返回 Some(类型) 或 None（未知前缀）。
pub fn classify(path: &str) -> Option<FKind> {
    if path.is_empty() {
        return None;
    }
    if path.starts_with(PIPE_PREFIX) {
        // 管道名非空。
        let name = match path.get(PIPE_PREFIX.len()..) {
            Some(v) => v,
            None => return None,
        };
        if name.is_empty() {
            return None;
        }
        return Some(FKind::NamedPipe);
    }
    if path.starts_with(MAILSLOT_PREFIX) {
        let name = match path.get(MAILSLOT_PREFIX.len()..) {
            Some(v) => v,
            None => return None,
        };
        if name.is_empty() {
            return None;
        }
        return Some(FKind::MailSlot);
    }
    if path.starts_with(DOS_PREFIX) {
        // \??\<盘符>：盘符大写单字符且在闭集内。
        let rest = match path.get(DOS_PREFIX.len()..) {
            Some(v) => v,
            None => return None,
        };
        let mut chars = rest.chars();
        let first = match chars.next() {
            Some(c) => c,
            None => return None,
        };
        if !first.is_ascii_uppercase() {
            return None;
        }
        if !letter_in_set(first) {
            return None;
        }
        return Some(FKind::File);
    }
    None
}

// ── 探测行五元 ───────────────────────────────────────────────────
/// 探测行五元（探测号/路径/期望类型/盘符位/映射指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FobjRow {
    /// 探测号。
    pub no: u32,
    /// 探测路径。
    pub path: &'static str,
    /// 期望类型。
    pub expect: FKind,
    /// 盘符位（\?? 边行带盘符，其余 '\0'）。
    pub drive: char,
    /// 映射指纹（\?? 边行带 B30 根路径回指）。
    pub fp: &'static str,
}

/// B30 根路径协议回指指纹。
pub const ROOT_FP: &str = "B30-F12586-D3-COSIGN";

/// 探测档（10 例冻结：管道 4 / 邮槽 3 / 文件 3）。
pub const FOBJ_PROFILE: &[FobjRow] = &[
    FobjRow { no: 0, path: "\\Device\\NamedPipe\\svc-alert", expect: FKind::NamedPipe, drive: '\0', fp: "" },
    FobjRow { no: 1, path: "\\Device\\NamedPipe\\log-stream", expect: FKind::NamedPipe, drive: '\0', fp: "" },
    FobjRow { no: 2, path: "\\Device\\NamedPipe\\rpc-epmapper", expect: FKind::NamedPipe, drive: '\0', fp: "" },
    FobjRow { no: 3, path: "\\Device\\NamedPipe\\wkssvc", expect: FKind::NamedPipe, drive: '\0', fp: "" },
    FobjRow { no: 4, path: "\\Device\\MailSlot\\slots\\bcast", expect: FKind::MailSlot, drive: '\0', fp: "" },
    FobjRow { no: 5, path: "\\Device\\MailSlot\\slots\\mesgr", expect: FKind::MailSlot, drive: '\0', fp: "" },
    FobjRow { no: 6, path: "\\Device\\MailSlot\\slots\\netsvcs", expect: FKind::MailSlot, drive: '\0', fp: "" },
    FobjRow { no: 7, path: "\\??\\C:\\Windows\\System32", expect: FKind::File, drive: 'C', fp: ROOT_FP },
    FobjRow { no: 8, path: "\\??\\D:\\data", expect: FKind::File, drive: 'D', fp: ROOT_FP },
    FobjRow { no: 9, path: "\\??\\Z:\\ram", expect: FKind::File, drive: 'Z', fp: ROOT_FP },
];

/// 探测档容量。
pub const PROBE_CAP: usize = 10;

/// 单行黄金判定（路径分类 + 类型对平 + 盘符/指纹附加闸）。
pub fn fobj_ok(r: &FobjRow) -> Result<(), u32> {
    // 路径非空闸。
    if r.path.is_empty() {
        return Err(FobjDiag::EPathEmpty.val());
    }
    // 分类闭集闸。
    let kind = match classify(r.path) {
        Some(k) => k,
        None => return Err(FobjDiag::EPrefixUnknown.val()),
    };
    // 类型对平闸。
    if kind != r.expect {
        return Err(FobjDiag::ETypeMismatch.val());
    }
    // \?? 边附加闸：盘符在闭集 + 指纹齐。
    if kind == FKind::File {
        if !letter_in_set(r.drive) {
            return Err(FobjDiag::EDriveRange.val());
        }
        if r.fp != ROOT_FP {
            return Err(FobjDiag::EMapFpMissing.val());
        }
    }
    Ok(())
}

// ── 状态机（Idle → Probing → Charted） ───────────────────────────
/// 探测状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FobjState {
    /// 未探测。
    Idle,
    /// 探测中。
    Probing,
    /// 已成图。
    Charted,
}

impl FobjState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            FobjState::Idle => 0,
            FobjState::Probing => 1,
            FobjState::Charted => 2,
        }
    }
}

/// 探测账。
#[derive(Clone, Debug)]
pub struct FobjChart {
    /// 已对平探测行。
    rows: Vec<FobjRow>,
    /// 拒收记账（探测号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: FobjState,
}

impl FobjChart {
    /// 开账。
    pub fn new() -> FobjChart {
        FobjChart {
            rows: Vec::new(),
            rejects: Vec::new(),
            state: FobjState::Idle,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> FobjState {
        self.state
    }

    /// 已对平行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&FobjRow> {
        self.rows.get(idx)
    }

    /// 开探（Idle→Probing）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state.rank() >= FobjState::Probing.rank() {
            return Err(FobjDiag::ERewind.val());
        }
        self.state = FobjState::Probing;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < PROBE_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐行探测（行号序 + 黄金判定 + 路径互异 三闸）。
    pub fn probe(&mut self, r: FobjRow) -> Result<(), u32> {
        if self.state.rank() < FobjState::Probing.rank() {
            return Err(FobjDiag::ENotProbed.val());
        }
        if self.state.rank() >= FobjState::Charted.rank() {
            self.note_reject(r.no, FobjDiag::EAfterChart.val());
            return Err(FobjDiag::EAfterChart.val());
        }
        // 行号序闸。
        if r.no != self.rows.len() as u32 {
            self.note_reject(r.no, FobjDiag::EProbeOrder.val());
            return Err(FobjDiag::EProbeOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = fobj_ok(&r) {
            self.note_reject(r.no, e);
            return Err(e);
        }
        // 路径互异闸。
        for done in self.rows.iter() {
            if done.path == r.path {
                self.note_reject(r.no, FobjDiag::EProbeDup.val());
                return Err(FobjDiag::EProbeDup.val());
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 成图：10 行齐 + 零拒收。
    pub fn chart(&mut self) -> Result<(), u32> {
        if self.state.rank() >= FobjState::Charted.rank() {
            return Err(FobjDiag::ERewind.val());
        }
        if self.state.rank() < FobjState::Probing.rank() {
            return Err(FobjDiag::ENotProbed.val());
        }
        if self.rows.len() != PROBE_CAP {
            return Err(FobjDiag::EProbeOrder.val());
        }
        if !self.rejects.is_empty() {
            return Err(FobjDiag::EProbeOrder.val());
        }
        self.state = FobjState::Charted;
        Ok(())
    }

    /// 账等式。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == PROBE_CAP && self.rejects.is_empty()
    }
}

// ── 类型错配注入（LCG 确定性 10 例） ─────────────────────────────
/// LCG（种子 = SEG_BASE + 0x13）。
pub struct FobjLcg {
    state: u64,
}

impl FobjLcg {
    /// 固定种子构造。
    pub fn new(seed: u64) -> FobjLcg {
        FobjLcg { state: seed }
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

/// 错配例（错配号/行位/错挂类型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MismatchCase {
    /// 错配号。
    pub no: u32,
    /// 错配目标行位（探测档内）。
    pub row: usize,
    /// 错挂类型。
    pub wrong: FKind,
}

/// 确定性生成 10 例类型错配（每例挂与分类结果不同的类型）。
pub fn mismatch_cases() -> Vec<MismatchCase> {
    let mut out: Vec<MismatchCase> = Vec::new();
    let mut rng = FobjLcg::new(SEG_BASE as u64 + 0x13);
    let kinds = [FKind::NamedPipe, FKind::MailSlot, FKind::File];
    let mut guard = 0u32;
    while out.len() < 10 && guard < 256 {
        guard += 1;
        let row_idx = rng.below(PROBE_CAP as u32) as usize;
        let kind_idx = rng.below(kinds.len() as u32) as usize;
        let r = match FOBJ_PROFILE.get(row_idx) {
            Some(v) => v,
            None => continue,
        };
        let wrong = match kinds.get(kind_idx) {
            Some(v) => *v,
            None => continue,
        };
        // 错配前提：错挂类型 ≠ 分类真型。
        if wrong != r.expect {
            let no = out.len() as u32;
            out.push(MismatchCase { no, row: row_idx, wrong });
        }
    }
    out
}

/// 类型错配注入 10/10 检出（判据 J1 直读口）。
pub fn mismatch_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for c in mismatch_cases().iter() {
        total += 1;
        // 取错配目标行，错挂 c.wrong 类型。
        let base = match FOBJ_PROFILE.get(c.row) {
            Some(v) => v.clone(),
            None => continue,
        };
        let bad = FobjRow {
            no: base.no,
            path: base.path,
            expect: c.wrong,
            drive: base.drive,
            fp: base.fp,
        };
        match fobj_ok(&bad) {
            Err(e) if e == FobjDiag::ETypeMismatch.val() => caught += 1,
            _ => {
                if escape.is_empty() {
                    escape = format!("错配{}({}) 漏检", c.no, c.wrong.name());
                }
            }
        }
    }
    (caught, total, escape)
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FobjFault {
    /// 类型错配漏检（错配例必须检出——本路验证 sweep 面）。
    MismatchMiss,
    /// 路径空。
    PathEmpty,
    /// 前缀未知。
    PrefixUnknown,
    /// 盘符越表。
    DriveRange,
    /// 盘符小写。
    DriveLower,
    /// 缺映射指纹。
    FpMissing,
    /// 探测号乱序。
    ProbeOrder,
    /// 探测行重复。
    ProbeDup,
    /// 未探测即成图。
    NotProbed,
    /// 成图后再探。
    AfterChart,
}

impl FobjFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            FobjFault::MismatchMiss => 0,
            FobjFault::PathEmpty => 1,
            FobjFault::PrefixUnknown => 2,
            FobjFault::DriveRange => 3,
            FobjFault::DriveLower => 4,
            FobjFault::FpMissing => 5,
            FobjFault::ProbeOrder => 6,
            FobjFault::ProbeDup => 7,
            FobjFault::NotProbed => 8,
            FobjFault::AfterChart => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            FobjFault::MismatchMiss => "错配漏检",
            FobjFault::PathEmpty => "路径空",
            FobjFault::PrefixUnknown => "前缀未知",
            FobjFault::DriveRange => "盘符越表",
            FobjFault::DriveLower => "盘符小写",
            FobjFault::FpMissing => "缺映射指纹",
            FobjFault::ProbeOrder => "探测号乱序",
            FobjFault::ProbeDup => "探测行重复",
            FobjFault::NotProbed => "未探测即成图",
            FobjFault::AfterChart => "成图后再探",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [FobjFault] {
        &[
            FobjFault::MismatchMiss,
            FobjFault::PathEmpty,
            FobjFault::PrefixUnknown,
            FobjFault::DriveRange,
            FobjFault::DriveLower,
            FobjFault::FpMissing,
            FobjFault::ProbeOrder,
            FobjFault::ProbeDup,
            FobjFault::NotProbed,
            FobjFault::AfterChart,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in FobjFault::all().iter() {
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
fn run_fault(v: FobjFault) -> bool {
    match v {
        FobjFault::MismatchMiss => {
            // 错配 sweep 面 10/10 检出（漏检即本路逃脱）。
            let (caught, total, escape) = mismatch_sweep();
            total == 10 && caught == 10 && escape.is_empty()
        }
        FobjFault::PathEmpty => {
            let r = FobjRow { no: 0, path: "", expect: FKind::File, drive: '\0', fp: "" };
            fobj_ok(&r).is_err()
        }
        FobjFault::PrefixUnknown => {
            let r = FobjRow { no: 0, path: "\\Device\\Ghost\\x", expect: FKind::File, drive: '\0', fp: "" };
            fobj_ok(&r).is_err()
        }
        FobjFault::DriveRange => {
            let r = FobjRow { no: 0, path: "\\??\\Q:\\x", expect: FKind::File, drive: 'Q', fp: ROOT_FP };
            fobj_ok(&r).is_err()
        }
        FobjFault::DriveLower => {
            let r = FobjRow { no: 0, path: "\\??\\c:\\x", expect: FKind::File, drive: 'c', fp: ROOT_FP };
            fobj_ok(&r).is_err()
        }
        FobjFault::FpMissing => {
            let r = FobjRow { no: 0, path: "\\??\\C:\\x", expect: FKind::File, drive: 'C', fp: "" };
            fobj_ok(&r).is_err()
        }
        FobjFault::ProbeOrder => {
            let mut ch = FobjChart::new();
            let _ = ch.begin();
            let r = match FOBJ_PROFILE.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            ch.probe(r).is_err()
        }
        FobjFault::ProbeDup => {
            let mut ch = FobjChart::new();
            let _ = ch.begin();
            let r0 = match FOBJ_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let path0 = r0.path;
            let _ = ch.probe(r0);
            // 同路径换探测号再入被拒。
            let r1 = FobjRow { no: 1, path: path0, expect: FKind::NamedPipe, drive: '\0', fp: "" };
            ch.probe(r1).is_err()
        }
        FobjFault::NotProbed => {
            let mut ch = FobjChart::new();
            ch.chart().is_err()
        }
        FobjFault::AfterChart => {
            let mut ch = build_full_chart();
            if ch.chart().is_err() {
                return false;
            }
            let r = match FOBJ_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            ch.probe(r).is_err()
        }
    }
}

// ── 构建器：完整探测图 ───────────────────────────────────────────
/// 构建完整探测图（10 行逐行探测）。
pub fn build_full_chart() -> FobjChart {
    let mut ch = FobjChart::new();
    let _ = ch.begin();
    let mut i = 0usize;
    while i < PROBE_CAP {
        let r = match FOBJ_PROFILE.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match ch.probe(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    ch
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 探测档对平断言绿（判据 J1 直读口）。
fn probe_profile_ok() -> bool {
    let mut ch = build_full_chart();
    if ch.len() != PROBE_CAP {
        return false;
    }
    if ch.reject_len() != 0 {
        return false;
    }
    if ch.chart().is_err() {
        return false;
    }
    // 独立复核：10 行逐行 fobj_ok。
    for r in FOBJ_PROFILE.iter() {
        if fobj_ok(r).is_err() {
            return false;
        }
    }
    true
}

/// 类型错配注入 10/10 检出。
fn mismatch_ok() -> bool {
    let (caught, total, escape) = mismatch_sweep();
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

/// 三分分类引擎边界直读。
fn classify_boundary_ok() -> bool {
    // 三分正例。
    if classify("\\Device\\NamedPipe\\x") != Some(FKind::NamedPipe) {
        return false;
    }
    if classify("\\Device\\MailSlot\\x") != Some(FKind::MailSlot) {
        return false;
    }
    if classify("\\??\\C:\\x") != Some(FKind::File) {
        return false;
    }
    // 负例：空路径/空管道名/未知前缀/小写盘符/越表盘符。
    if classify("").is_some() {
        return false;
    }
    if classify("\\Device\\NamedPipe\\").is_some() {
        return false;
    }
    if classify("\\Device\\Ghost\\x").is_some() {
        return false;
    }
    if classify("\\??\\c:\\x").is_some() {
        return false;
    }
    if classify("\\??\\Q:\\x").is_some() {
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

/// 类型普查（管道 4 / 邮槽 3 / 文件 3）。
fn kind_census_ok() -> bool {
    let mut n_pipe = 0usize;
    let mut n_slot = 0usize;
    let mut n_file = 0usize;
    for r in FOBJ_PROFILE.iter() {
        match classify(r.path) {
            Some(FKind::NamedPipe) => n_pipe += 1,
            Some(FKind::MailSlot) => n_slot += 1,
            Some(FKind::File) => n_file += 1,
            None => return false,
        }
        if fobj_ok(r).is_err() {
            return false;
        }
    }
    n_pipe == 4 && n_slot == 3 && n_file == 3
}

/// 双跑一致（冻结档下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_chart();
    let b = build_full_chart();
    if a.len() != b.len() {
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
    if mismatch_sweep().0 != mismatch_sweep().0 {
        return false;
    }
    if fault_sweep().0 != fault_sweep().0 {
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
    if SEG_BASE != 0x80000 + 0x40 * (12665u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut ch = build_full_chart();
    if ch.chart().is_err() {
        return false;
    }
    if ch.state() != FobjState::Charted {
        return false;
    }
    match ch.row_at(0) {
        Some(r) => {
            if r.path.is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12666 文件对象探测自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12665_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12665-fileobj-dosdev");
    let ch = build_full_chart();

    s.add_dyn(
        "F12666-01 探测档十行齐",
        FOBJ_PROFILE.len() == PROBE_CAP,
        &format!("rows={}", FOBJ_PROFILE.len()),
    );

    s.add_dyn(
        "F12666-02 管道/邮槽探测档对平断言绿",
        probe_profile_ok(),
        &format!("rows={} rejects={}", ch.len(), ch.reject_len()),
    );

    s.add_dyn(
        "F12666-03 类型错配注入 10/10 检出",
        mismatch_ok(),
        &format!("caught={}", mismatch_sweep().0),
    );

    s.add_dyn(
        "F12666-04 注入联测 10/10 拒止",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12666-05 三分分类引擎边界",
        classify_boundary_ok(),
        "三分正例 + 空名/未知前缀/小写/越表拒",
    );

    s.add_dyn(
        "F12666-06 类型普查",
        kind_census_ok(),
        "管道 4 / 邮槽 3 / 文件 3",
    );

    s.add_dyn(
        "F12666-07 双跑一致",
        dual_run_ok(),
        "冻结档下两轮逐行同判",
    );

    s.add_dyn(
        "F12666-08 探测账等式",
        ch.equation_ok(),
        &format!("rows={} rejects={}", ch.len(), ch.reject_len()),
    );

    s.add_dyn(
        "F12666-09 DOS 映射边盘符闭集",
        DOS_LETTERS.len() == 5,
        &format!("letters={}", DOS_LETTERS.len()),
    );

    s.add_dyn(
        "F12666-10 完成写回语义",
        writeback_ok(),
        "成图即已完成且摘要非空",
    );

    s.add_dyn(
        "F12666-11 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // \?? 边行指纹齐备独立复核。
    let mut fp_ok = true;
    for r in FOBJ_PROFILE.iter() {
        if r.path.starts_with(DOS_PREFIX) && r.fp != ROOT_FP {
            fp_ok = false;
        }
    }
    s.add_dyn(
        "F12666-12 \\?? 边指纹齐备",
        fp_ok,
        "DOS 设备映射边逐行回指 B30 根路径协议",
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
        assert_eq!(SEG_BASE, 0x81040);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：11 个码互异、全部落窗口、查表可命中。
        assert_eq!(FOBJ_ALL.len(), 11);
        let mut i = 0usize;
        while i < FOBJ_ALL.len() {
            let vi = match FOBJ_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < FOBJ_ALL.len() {
                let vj = match FOBJ_ALL.get(j) {
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
    fn happy_path_charts() {
        let mut ch = build_full_chart();
        assert_eq!(ch.len(), PROBE_CAP);
        assert_eq!(ch.reject_len(), 0);
        assert!(ch.chart().is_ok());
        assert_eq!(ch.state(), FobjState::Charted);
    }

    #[test]
    fn mismatch_injection_ten_ten() {
        // 类型错配注入 10/10 检出。
        let (caught, total, escape) = mismatch_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "漏检：{}", escape);
        assert!(escape.is_empty());
        // 双跑一致。
        assert_eq!(mismatch_cases(), mismatch_cases());
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
    fn classify_engine_cases() {
        // 分类引擎边界直读。
        assert_eq!(classify("\\Device\\NamedPipe\\a"), Some(FKind::NamedPipe));
        assert_eq!(classify("\\Device\\MailSlot\\a"), Some(FKind::MailSlot));
        assert_eq!(classify("\\??\\C:\\"), Some(FKind::File));
        assert!(classify("").is_none());
        assert!(classify("\\Device\\NamedPipe\\").is_none());
        assert!(classify("\\Device\\MailSlot\\").is_none());
        assert!(classify("\\??\\q:\\").is_none());
        assert!(classify("\\??\\Q:\\").is_none());
        assert!(classify("C:\\Windows").is_none());
        // 类型名直读。
        assert_eq!(FKind::NamedPipe.name(), "NamedPipe");
        assert_eq!(FKind::MailSlot.name(), "MailSlot");
        assert_eq!(FKind::File.name(), "File");
        assert_eq!(FKind::all().len(), 3);
    }

    #[test]
    fn letter_set_boundary() {
        // 盘符闭集边界。
        assert!(letter_in_set('C'));
        assert!(letter_in_set('D'));
        assert!(letter_in_set('E'));
        assert!(letter_in_set('F'));
        assert!(letter_in_set('Z'));
        assert!(!letter_in_set('Q'));
        assert!(!letter_in_set('c'));
        assert!(!letter_in_set('0'));
    }

    #[test]
    fn probe_gates_reject() {
        // 探测闸直读：乱序/重复/断指纹。
        let mut ch = FobjChart::new();
        let _ = ch.begin();
        let r1 = match FOBJ_PROFILE.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(ch.probe(r1), Err(FobjDiag::EProbeOrder.val()));
        let r0 = match FOBJ_PROFILE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(ch.probe(r0).is_ok());
        let dup = FobjRow { no: 1, path: "\\Device\\NamedPipe\\svc-alert", expect: FKind::NamedPipe, drive: '\0', fp: "" };
        assert_eq!(ch.probe(dup), Err(FobjDiag::EProbeDup.val()));
        // \?? 行缺指纹被拒。
        let mut ch2 = FobjChart::new();
        let _ = ch2.begin();
        let bad = FobjRow { no: 0, path: "\\??\\C:\\x", expect: FKind::File, drive: 'C', fp: "" };
        assert_eq!(ch2.probe(bad), Err(FobjDiag::EMapFpMissing.val()));
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致。
        let a = build_full_chart();
        let b = build_full_chart();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(mismatch_sweep().0, mismatch_sweep().0);
        assert_eq!(fault_sweep().0, fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Idle→Probing→Charted。
        assert!(FobjState::Idle.rank() < FobjState::Probing.rank());
        assert!(FobjState::Probing.rank() < FobjState::Charted.rank());
        let mut ch = build_full_chart();
        assert_eq!(ch.state(), FobjState::Probing);
        assert!(ch.chart().is_ok());
        assert_eq!(ch.chart(), Err(FobjDiag::ERewind.val()));
        let mut fresh = FobjChart::new();
        assert!(fresh.begin().is_ok());
        assert_eq!(fresh.begin(), Err(FobjDiag::ERewind.val()));
    }

    #[test]
    fn census_four_three_three() {
        // 类型普查：管道 4 / 邮槽 3 / 文件 3。
        assert!(kind_census_ok());
        let mut n0 = 0usize;
        let mut n1 = 0usize;
        let mut n2 = 0usize;
        for r in FOBJ_PROFILE.iter() {
            match r.expect {
                FKind::NamedPipe => n0 += 1,
                FKind::MailSlot => n1 += 1,
                FKind::File => n2 += 1,
            }
        }
        assert_eq!((n0, n1, n2), (4, 3, 3));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12665_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
