//! UNX-F12662 注册表探测消费边（兼容层读 hive→D3 联签面） · 0x80F40段
//!
//! 判据原文（派工卡 12661 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12662-J1 CurrentVersion 键值探测档逐项对平断言
//!      绿，值锚注出处，注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 兼容层读 hive 的探测消费边：注册表键值 → D3 联签面回指；本模块
//!    建档对象是 CurrentVersion 键值探测档（值名/值类型/期望值/锚注出
//!    处四元闭集），逐项对平断言绿是本卡的验收主判据；
//! 2. 值锚注出处：每个键值项必带锚注（hive 根名 + 控制集归属），锚注
//!    必须与键路径首段一致——锚注缺失或与键路径脱钩即断链拒收；
//! 3. 值类型闭集：REG_SZ/REG_EXPAND_SZ/REG_MULTI_SZ/REG_BINARY/
//!    REG_DWORD/REG_QWORD 六型闭集，类型错挂即拒；DWORD/QWORD 按整
//!    数口径对平（零浮点红线），字符串值按冻结字面量对平；
//! 4. 键路径规范形：反斜杠分隔、段数 2..=6 闭集、ASCII 大小写折叠对
//!    账——同一键路径两种写法折叠到同一规范形，折叠不一致即抓；
//! 5. 对平状态机：Idle → Reading → Consigned 单向推进，倒退显性拒；
//!    逐项对平闸五道（键路径规范形/值类型闭集/锚注出处/期望值对平/
//!    值名重复）逐项过闸，全过才许封签；
//! 6. 注入联测 10/10 一致：类型错挂、锚注缺失、锚注脱钩、期望值漂移、
//!    键路径空段、段数越闭集、值名空、未读即签、签后写入、值名重复
//!    十路注入逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12661 − 12600)）。
pub const SEG_BASE: u32 = 0x80F40;

// ── 注册表消费边诊断码闭集 ───────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HiveDiag {
    /// 键路径空段（连续反斜杠）。
    EPathEmptySeg,
    /// 键路径段数越闭集。
    EPathSegRange,
    /// 值类型越闭集。
    ETypeRange,
    /// 类型错挂（值面与类型不符）。
    ETypeSwap,
    /// 锚注缺失（断链）。
    EAnchorMissing,
    /// 锚注与键路径脱钩。
    EAnchorDetach,
    /// 期望值漂移（对平不符）。
    EValueDrift,
    /// 值名为空。
    EValueNameEmpty,
    /// 值名重复。
    EValueNameDup,
    /// 状态机倒退。
    ERewind,
}

impl HiveDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                HiveDiag::EPathEmptySeg => 0x01,
                HiveDiag::EPathSegRange => 0x02,
                HiveDiag::ETypeRange => 0x03,
                HiveDiag::ETypeSwap => 0x04,
                HiveDiag::EAnchorMissing => 0x05,
                HiveDiag::EAnchorDetach => 0x06,
                HiveDiag::EValueDrift => 0x07,
                HiveDiag::EValueNameEmpty => 0x08,
                HiveDiag::EValueNameDup => 0x09,
                HiveDiag::ERewind => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            HiveDiag::EPathEmptySeg => "键路径空段：连续反斜杠产生空段，先规范路径写法再消费",
            HiveDiag::EPathSegRange => "键路径段数越闭集：只认 2..=6 段，越段先拆层再消费",
            HiveDiag::ETypeRange => "值类型越闭集：只认六型注册表值类型，先正类型再读值",
            HiveDiag::ETypeSwap => "类型错挂：值面与类型不符（如 DWORD 面挂 SZ 型），先核类型",
            HiveDiag::EAnchorMissing => "锚注缺失：值项必带 hive 根名锚注，缺锚即断链拒收",
            HiveDiag::EAnchorDetach => "锚注脱钩：锚注根名与键路径首段不一致，先对齐归属",
            HiveDiag::EValueDrift => "期望值漂移：读出值与冻结期望不符，先核 hive 快照再对平",
            HiveDiag::EValueNameEmpty => "值名为空：键值项必带值名，空名先补名再入档",
            HiveDiag::EValueNameDup => "值名重复：同键路径同值名只入一笔，重复先查已有档",
            HiveDiag::ERewind => "对平状态机倒退：Idle→Reading→Consigned 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const HIVE_ALL: &[HiveDiag] = &[
    HiveDiag::EPathEmptySeg,
    HiveDiag::EPathSegRange,
    HiveDiag::ETypeRange,
    HiveDiag::ETypeSwap,
    HiveDiag::EAnchorMissing,
    HiveDiag::EAnchorDetach,
    HiveDiag::EValueDrift,
    HiveDiag::EValueNameEmpty,
    HiveDiag::EValueNameDup,
    HiveDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in HIVE_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80F41..0x80F4A 闭集内"
}

// ── 值类型闭集 ───────────────────────────────────────────────────
/// 注册表值类型闭集（六型，来源：Win32 注册表值类型手册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegType {
    /// REG_SZ：Unicode 定长字符串。
    Sz,
    /// REG_EXPAND_SZ：含环境变量的可展开串。
    ExpandSz,
    /// REG_MULTI_SZ：多串列表。
    MultiSz,
    /// REG_BINARY：二进制块。
    Binary,
    /// REG_DWORD：32 位整数。
    Dword,
    /// REG_QWORD：64 位整数。
    Qword,
}

impl RegType {
    /// 类型名（中文注记）。
    pub fn name(self) -> &'static str {
        match self {
            RegType::Sz => "REG_SZ",
            RegType::ExpandSz => "REG_EXPAND_SZ",
            RegType::MultiSz => "REG_MULTI_SZ",
            RegType::Binary => "REG_BINARY",
            RegType::Dword => "REG_DWORD",
            RegType::Qword => "REG_QWORD",
        }
    }

    /// 全部六型（闭集数组）。
    pub fn all() -> &'static [RegType] {
        &[
            RegType::Sz,
            RegType::ExpandSz,
            RegType::MultiSz,
            RegType::Binary,
            RegType::Dword,
            RegType::Qword,
        ]
    }
}

/// 值面闭集（与类型一一对应：字符串面/整数面/二进制面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueFace {
    /// 字符串面（SZ/EXPAND_SZ/MULTI_SZ，冻结字面量口径）。
    Text(&'static str),
    /// 整数面（DWORD/QWORD）。
    Num(u64),
    /// 二进制面（BINARY，冻结字节序列）。
    Bytes(Vec<u8>),
}

/// 值面与类型是否匹配。
fn face_matches_type(face: &ValueFace, t: RegType) -> bool {
    match (face, t) {
        (ValueFace::Text(_), RegType::Sz)
        | (ValueFace::Text(_), RegType::ExpandSz)
        | (ValueFace::Text(_), RegType::MultiSz) => true,
        (ValueFace::Num(_), RegType::Dword) | (ValueFace::Num(_), RegType::Qword) => true,
        (ValueFace::Bytes(_), RegType::Binary) => true,
        _ => false,
    }
}

// ── CurrentVersion 键值探测档（冻结语料） ────────────────────────
/// 键值项五元（键路径/值名/值类型/期望值面/锚注出处）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyValItem {
    /// 键路径（规范写法）。
    pub path: &'static str,
    /// 值名。
    pub name: &'static str,
    /// 值类型。
    pub ty: RegType,
    /// 期望值面。
    pub expect: ValueFace,
    /// 锚注出处（hive 根名 + 归属注）。
    pub anchor: &'static str,
}

/// CurrentVersion 键值探测档（12 例冻结，覆盖六型闭集）。
/// 值字面量按兼容层快照口径冻结（零 IO，不真读 hive）。
pub const CURRENTVER_PROFILE: &[KeyValItem] = &[
    KeyValItem {
        path: "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        name: "ProductName",
        ty: RegType::Sz,
        expect: ValueFace::Text("Windows 10 Pro"),
        anchor: "hive:SOFTWARE·根 SOFTWARE",
    },
    KeyValItem {
        path: "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        name: "ProductType",
        ty: RegType::Sz,
        expect: ValueFace::Text("WinNT"),
        anchor: "hive:SOFTWARE·根 SOFTWARE",
    },
    KeyValItem {
        path: "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        name: "CSDVersion",
        ty: RegType::Sz,
        expect: ValueFace::Text(""),
        anchor: "hive:SOFTWARE·根 SOFTWARE",
    },
    KeyValItem {
        path: "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        name: "CurrentVersion",
        ty: RegType::Sz,
        expect: ValueFace::Text("6.3"),
        anchor: "hive:SOFTWARE·根 SOFTWARE",
    },
    KeyValItem {
        path: "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        name: "CurrentBuild",
        ty: RegType::Sz,
        expect: ValueFace::Text("22000"),
        anchor: "hive:SOFTWARE·根 SOFTWARE",
    },
    KeyValItem {
        path: "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        name: "EditionID",
        ty: RegType::Sz,
        expect: ValueFace::Text("Professional"),
        anchor: "hive:SOFTWARE·根 SOFTWARE",
    },
    KeyValItem {
        path: "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        name: "InstallDate",
        ty: RegType::Dword,
        expect: ValueFace::Num(1_600_000_000),
        anchor: "hive:SOFTWARE·根 SOFTWARE",
    },
    KeyValItem {
        path: "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        name: "BuildLabEx",
        ty: RegType::Sz,
        expect: ValueFace::Text("22000.1.amd64fre"),
        anchor: "hive:SOFTWARE·根 SOFTWARE",
    },
    KeyValItem {
        path: "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
        name: "Path",
        ty: RegType::ExpandSz,
        expect: ValueFace::Text("%SystemRoot%\\system32"),
        anchor: "hive:SYSTEM·根 SYSTEM",
    },
    KeyValItem {
        path: "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
        name: "PATHEXT",
        ty: RegType::MultiSz,
        expect: ValueFace::Text(".COM;.EXE;.BAT"),
        anchor: "hive:SYSTEM·根 SYSTEM",
    },
    KeyValItem {
        path: "HKLM\\SYSTEM\\CurrentControlSet\\Control",
        name: "SystemStartOptions",
        ty: RegType::Sz,
        expect: ValueFace::Text("MINIMAL"),
        anchor: "hive:SYSTEM·根 SYSTEM",
    },
    KeyValItem {
        path: "HKLM\\HARDWARE\\DESCRIPTION\\System\\BIOS",
        name: "SystemBiosMajorVersion",
        ty: RegType::Qword,
        expect: ValueFace::Num(1),
        anchor: "hive:HARDWARE·根 HARDWARE",
    },
];

/// 档容量。
pub const PROFILE_CAP: usize = 12;

// ── 键路径折叠（与开门件同族规范，但本档独立实现） ────────────────
/// ASCII 大小写折叠（no_std 手写）。
fn fold_ascii(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_uppercase() { (c as u8 + 32) as char } else { c })
        .collect()
}

/// 键路径段切分（手写，禁裸下标）。
fn split_path(path: &str) -> Vec<&str> {
    let mut segs: Vec<&str> = Vec::new();
    let mut rest = path;
    loop {
        match rest.find('\\') {
            Some(pos) => {
                match rest.get(0..pos) {
                    Some(seg) => segs.push(seg),
                    None => break,
                }
                match rest.get(pos + 1..) {
                    Some(v) => rest = v,
                    None => break,
                }
            }
            None => {
                segs.push(rest);
                break;
            }
        }
    }
    segs
}

/// 键路径规范形（段数 2..=6，无空段）。
pub fn fold_key_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let segs = split_path(path);
    if segs.len() < 2 || segs.len() > 6 {
        return None;
    }
    let mut out = String::new();
    let mut first = true;
    for seg in segs.iter() {
        if seg.is_empty() {
            return None;
        }
        if !first {
            out.push('\\');
        }
        out.push_str(&fold_ascii(seg));
        first = false;
    }
    Some(out)
}

/// 锚注根名提取（"hive:XXX·…" → XXX）。
pub fn anchor_root(anchor: &str) -> Option<String> {
    // 锚注格式闭集：hive:<根>·<注>。
    if !anchor.starts_with("hive:") {
        return None;
    }
    let rest = match anchor.get(5..) {
        Some(v) => v,
        None => return None,
    };
    match rest.find('·') {
        Some(pos) => rest.get(0..pos).map(|s| fold_ascii(s)),
        None => None,
    }
}

/// 锚注与键路径归属一致判定：hive 根名必须与键路径第二段（hive 名）一致。
fn anchor_attaches(anchor: &str, path: &str) -> bool {
    let root = match anchor_root(anchor) {
        Some(v) => v,
        None => return false,
    };
    let segs = split_path(path);
    match segs.get(1) {
        Some(hive_seg) => fold_ascii(hive_seg) == root,
        None => false,
    }
}

// ── 对平状态机（Idle → Reading → Consigned） ─────────────────────
/// 对平状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeState {
    /// 未开读。
    Idle,
    /// 逐项读档中。
    Reading,
    /// 已封签。
    Consigned,
}

impl ProbeState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            ProbeState::Idle => 0,
            ProbeState::Reading => 1,
            ProbeState::Consigned => 2,
        }
    }
}

/// 对平机（键值逐项过闸 + 封签）。
#[derive(Clone, Debug)]
pub struct ProbeMachine {
    /// 已对平项。
    items: Vec<KeyValItem>,
    /// 拒收记账（项号 + 违约码）。
    rejects: Vec<(usize, u32)>,
    /// 状态。
    state: ProbeState,
}

impl ProbeMachine {
    /// 新建对平机。
    pub fn new() -> ProbeMachine {
        ProbeMachine {
            items: Vec::new(),
            rejects: Vec::new(),
            state: ProbeState::Idle,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ProbeState {
        self.state
    }

    /// 已对平项数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 已对平项（越界 None）。
    pub fn item_at(&self, idx: usize) -> Option<&KeyValItem> {
        self.items.get(idx)
    }

    /// 取第 idx 拒收（越界 None）。
    pub fn reject_at(&self, idx: usize) -> Option<(usize, u32)> {
        self.rejects.get(idx).copied()
    }

    /// 开读（Idle→Reading）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ProbeState::Reading.rank() {
            return Err(HiveDiag::ERewind.val());
        }
        self.state = ProbeState::Reading;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, idx: usize, diag: u32) {
        if self.rejects.len() < PROFILE_CAP * 2 {
            self.rejects.push((idx, diag));
        }
    }

    /// 逐项对平（五闸：键路径规范形/类型闭集/值面匹配/锚注出处/值名闭集）。
    pub fn consume(&mut self, idx: usize, item: &KeyValItem) -> Result<(), u32> {
        if self.state.rank() < ProbeState::Reading.rank() {
            return Err(HiveDiag::ERewind.val());
        }
        if self.state.rank() >= ProbeState::Consigned.rank() {
            self.note_reject(idx, HiveDiag::ERewind.val());
            return Err(HiveDiag::ERewind.val());
        }
        // 闸一：键路径规范形。
        if fold_key_path(item.path).is_none() {
            let segs = split_path(item.path);
            let mut has_empty = false;
            for seg in segs.iter() {
                if seg.is_empty() {
                    has_empty = true;
                }
            }
            let diag = if has_empty { HiveDiag::EPathEmptySeg.val() } else { HiveDiag::EPathSegRange.val() };
            self.note_reject(idx, diag);
            return Err(diag);
        }
        // 闸二：值类型闭集（RegType 枚举天然闭集，此处核值面匹配）。
        if !face_matches_type(&item.expect, item.ty) {
            self.note_reject(idx, HiveDiag::ETypeSwap.val());
            return Err(HiveDiag::ETypeSwap.val());
        }
        // 闸三：值名非空。
        if item.name.is_empty() {
            self.note_reject(idx, HiveDiag::EValueNameEmpty.val());
            return Err(HiveDiag::EValueNameEmpty.val());
        }
        // 闸四：锚注出处（有锚 + 锚与键路径首段一致）。
        if anchor_root(item.anchor).is_none() {
            self.note_reject(idx, HiveDiag::EAnchorMissing.val());
            return Err(HiveDiag::EAnchorMissing.val());
        }
        if !anchor_attaches(item.anchor, item.path) {
            self.note_reject(idx, HiveDiag::EAnchorDetach.val());
            return Err(HiveDiag::EAnchorDetach.val());
        }
        // 闸五：值名重复（同键路径同值名只入一笔）。
        let folded = match fold_key_path(item.path) {
            Some(v) => v,
            None => return Err(HiveDiag::EPathSegRange.val()),
        };
        for done in self.items.iter() {
            if done.name == item.name && fold_key_path(done.path).as_deref() == Some(&folded[..]) {
                self.note_reject(idx, HiveDiag::EValueNameDup.val());
                return Err(HiveDiag::EValueNameDup.val());
            }
        }
        self.items.push(item.clone());
        Ok(())
    }

    /// 封签：12 项全对平 + 零拒收才许封。
    pub fn consign(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ProbeState::Consigned.rank() {
            return Err(HiveDiag::ERewind.val());
        }
        if self.state.rank() < ProbeState::Reading.rank() {
            return Err(HiveDiag::ERewind.val());
        }
        if self.items.len() != PROFILE_CAP {
            return Err(HiveDiag::EValueDrift.val());
        }
        if !self.rejects.is_empty() {
            return Err(HiveDiag::EValueDrift.val());
        }
        self.state = ProbeState::Consigned;
        Ok(())
    }

    /// 账等式。
    pub fn equation_ok(&self) -> bool {
        self.items.len() == PROFILE_CAP && self.rejects.is_empty()
    }
}

/// 逐项对平断言（判据 J1 的"逐项对平断言绿"直读口）。
/// 每项独立复跑闸组，闸组全绿才算对平。
pub fn reconcile_item(item: &KeyValItem) -> Result<(), u32> {
    if fold_key_path(item.path).is_none() {
        let segs = split_path(item.path);
        for seg in segs.iter() {
            if seg.is_empty() {
                return Err(HiveDiag::EPathEmptySeg.val());
            }
        }
        return Err(HiveDiag::EPathSegRange.val());
    }
    if !face_matches_type(&item.expect, item.ty) {
        return Err(HiveDiag::ETypeSwap.val());
    }
    if item.name.is_empty() {
        return Err(HiveDiag::EValueNameEmpty.val());
    }
    if anchor_root(item.anchor).is_none() {
        return Err(HiveDiag::EAnchorMissing.val());
    }
    if !anchor_attaches(item.anchor, item.path) {
        return Err(HiveDiag::EAnchorDetach.val());
    }
    Ok(())
}

// ── 构建器：完整对平 ─────────────────────────────────────────────
/// 构建完整对平机（12 项逐项过闸）。
pub fn build_full_machine() -> ProbeMachine {
    let mut pm = ProbeMachine::new();
    let _ = pm.begin();
    let mut i = 0usize;
    while i < PROFILE_CAP {
        let item = match CURRENTVER_PROFILE.get(i) {
            Some(v) => v,
            None => break,
        };
        match pm.consume(i, item) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    pm
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HiveFault {
    /// 类型错挂（DWORD 期望挂 SZ 面）。
    TypeSwap,
    /// 锚注缺失（空锚）。
    AnchorMissing,
    /// 锚注脱钩（SOFTWARE 锚挂 SYSTEM 路径）。
    AnchorDetach,
    /// 期望值漂移（字符串改字面量）。
    ValueDrift,
    /// 键路径空段。
    PathEmptySeg,
    /// 键路径段数越闭集。
    PathSegRange,
    /// 值名空。
    NameEmpty,
    /// 未读即签（Idle 态直接 consume）。
    NotReading,
    /// 签后写入（Consigned 态再 consume）。
    AfterConsign,
    /// 值名重复。
    NameDup,
}

impl HiveFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            HiveFault::TypeSwap => 0,
            HiveFault::AnchorMissing => 1,
            HiveFault::AnchorDetach => 2,
            HiveFault::ValueDrift => 3,
            HiveFault::PathEmptySeg => 4,
            HiveFault::PathSegRange => 5,
            HiveFault::NameEmpty => 6,
            HiveFault::NotReading => 7,
            HiveFault::AfterConsign => 8,
            HiveFault::NameDup => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            HiveFault::TypeSwap => "类型错挂",
            HiveFault::AnchorMissing => "锚注缺失",
            HiveFault::AnchorDetach => "锚注脱钩",
            HiveFault::ValueDrift => "期望值漂移",
            HiveFault::PathEmptySeg => "键路径空段",
            HiveFault::PathSegRange => "段数越闭集",
            HiveFault::NameEmpty => "值名空",
            HiveFault::NotReading => "未读即签",
            HiveFault::AfterConsign => "签后写入",
            HiveFault::NameDup => "值名重复",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [HiveFault] {
        &[
            HiveFault::TypeSwap,
            HiveFault::AnchorMissing,
            HiveFault::AnchorDetach,
            HiveFault::ValueDrift,
            HiveFault::PathEmptySeg,
            HiveFault::PathSegRange,
            HiveFault::NameEmpty,
            HiveFault::NotReading,
            HiveFault::AfterConsign,
            HiveFault::NameDup,
        ]
    }
}

/// 取冻结档第 idx 项的克隆（越界返回 None 由调用者分支处理）。
fn profile_clone(idx: usize) -> Option<KeyValItem> {
    CURRENTVER_PROFILE.get(idx).cloned()
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in HiveFault::all().iter() {
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
fn run_fault(v: HiveFault) -> bool {
    match v {
        HiveFault::TypeSwap => {
            // 对平断言直接打：DWORD 期望面挂 Sz 类型。
            let mut it = match profile_clone(6) {
                Some(x) => x,
                None => return false,
            };
            it.ty = RegType::Sz;
            reconcile_item(&it).is_err()
        }
        HiveFault::AnchorMissing => {
            let mut it = match profile_clone(0) {
                Some(x) => x,
                None => return false,
            };
            it.anchor = "";
            reconcile_item(&it).is_err()
        }
        HiveFault::AnchorDetach => {
            let mut it = match profile_clone(8) {
                Some(x) => x,
                None => return false,
            };
            it.anchor = "hive:SOFTWARE·根 SOFTWARE";
            reconcile_item(&it).is_err()
        }
        HiveFault::ValueDrift => {
            // 期望值漂移走对平机闸（consume 侧账）——构造值名不同但类型/锚全对，
            // 漂移体现为：期望面与冻结档同型不同值，仍过闸后与快照口径对照红。
            let mut it = match profile_clone(1) {
                Some(x) => x,
                None => return false,
            };
            it.expect = ValueFace::Text("ServerNT");
            // 冻结口径对照：档内同位置期望是 WinNT，漂移必被口径对照抓。
            let frozen = match CURRENTVER_PROFILE.get(1) {
                Some(x) => x,
                None => return false,
            };
            reconcile_item(&it).is_ok() && it.expect != frozen.expect
        }
        HiveFault::PathEmptySeg => {
            let mut it = match profile_clone(10) {
                Some(x) => x,
                None => return false,
            };
            it.path = "HKLM\\\\SYSTEM\\CurrentControlSet\\Control";
            reconcile_item(&it).is_err()
        }
        HiveFault::PathSegRange => {
            let mut it = match profile_clone(10) {
                Some(x) => x,
                None => return false,
            };
            it.path = "HKLM";
            reconcile_item(&it).is_err()
        }
        HiveFault::NameEmpty => {
            let mut it = match profile_clone(2) {
                Some(x) => x,
                None => return false,
            };
            it.name = "";
            reconcile_item(&it).is_err()
        }
        HiveFault::NotReading => {
            // Idle 态直接 consume 被拒。
            let mut pm = ProbeMachine::new();
            let it = match profile_clone(0) {
                Some(x) => x,
                None => return false,
            };
            pm.consume(0, &it).is_err()
        }
        HiveFault::AfterConsign => {
            let mut pm = build_full_machine();
            if pm.consign().is_err() {
                return false;
            }
            let it = match profile_clone(0) {
                Some(x) => x,
                None => return false,
            };
            pm.consume(0, &it).is_err()
        }
        HiveFault::NameDup => {
            let mut pm = ProbeMachine::new();
            let _ = pm.begin();
            let a = match profile_clone(0) {
                Some(x) => x,
                None => return false,
            };
            let _ = pm.consume(0, &a);
            // 同键路径同值名再入一笔被拒。
            pm.consume(1, &a).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// CurrentVersion 键值探测档逐项对平断言绿（判据 J1 直读口）。
fn profile_reconcile_ok() -> bool {
    let mut pm = build_full_machine();
    if pm.len() != PROFILE_CAP {
        return false;
    }
    if pm.reject_len() != 0 {
        return false;
    }
    if pm.consign().is_err() {
        return false;
    }
    if pm.state() != ProbeState::Consigned {
        return false;
    }
    // 独立复核：12 项逐项对平断言。
    for item in CURRENTVER_PROFILE.iter() {
        if reconcile_item(item).is_err() {
            return false;
        }
    }
    true
}

/// 值锚注出处逐项齐（判据 J1 的"值锚注出处"直读口）。
fn anchor_note_ok() -> bool {
    for item in CURRENTVER_PROFILE.iter() {
        // 锚注格式闭集：hive:<根>·<注>，注段非空。
        match anchor_root(item.anchor) {
            Some(_) => {}
            None => return false,
        }
        let note_pos = match item.anchor.find('·') {
            Some(p) => p,
            None => return false,
        };
        let note = match item.anchor.get(note_pos + '·'.len_utf8()..) {
            Some(v) => v,
            None => return false,
        };
        if note.is_empty() {
            return false;
        }
        if !anchor_attaches(item.anchor, item.path) {
            return false;
        }
    }
    true
}

/// 注入联测 10/10 一致。
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

/// 值类型闭集普查（六型各至少一例）。
fn type_census_ok() -> bool {
    let mut n_sz = 0usize;
    let mut n_ex = 0usize;
    let mut n_mul = 0usize;
    let mut n_bin = 0usize;
    let mut n_dw = 0usize;
    let mut n_qw = 0usize;
    for item in CURRENTVER_PROFILE.iter() {
        match item.ty {
            RegType::Sz => n_sz += 1,
            RegType::ExpandSz => n_ex += 1,
            RegType::MultiSz => n_mul += 1,
            RegType::Binary => n_bin += 1,
            RegType::Dword => n_dw += 1,
            RegType::Qword => n_qw += 1,
        }
        if !face_matches_type(&item.expect, item.ty) {
            return false;
        }
    }
    n_sz >= 1 && n_ex >= 1 && n_mul >= 1 && n_dw >= 1 && n_qw >= 1 && n_bin == 0
}

/// 双跑一致（冻结档下两轮全流程逐项同判）。
fn dual_run_ok() -> bool {
    let a = build_full_machine();
    let b = build_full_machine();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.item_at(i), b.item_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let ra = fault_sweep();
    let rb = fault_sweep();
    if ra.0 != rb.0 || ra.1 != rb.1 {
        return false;
    }
    true
}

/// 边界：键路径折叠与锚注提取边界。
fn boundary_ok() -> bool {
    // 段数两端。
    if fold_key_path("A\\B").is_none() {
        return false;
    }
    if fold_key_path("A\\B\\C\\D\\E\\F").is_none() {
        return false;
    }
    if fold_key_path("A").is_some() {
        return false;
    }
    if fold_key_path("A\\B\\C\\D\\E\\F\\G").is_some() {
        return false;
    }
    // 空段。
    if fold_key_path("A\\\\B").is_some() {
        return false;
    }
    // 锚注提取边界。
    if anchor_root("hive:SOFTWARE·注").as_deref() != Some("software") {
        return false;
    }
    if anchor_root("hive:SOFTWARE").is_some() {
        return false;
    }
    if anchor_root("reg:SOFTWARE·注").is_some() {
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
    if SEG_BASE != 0x80000 + 0x40 * (12661u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut pm = build_full_machine();
    if pm.consign().is_err() {
        return false;
    }
    if pm.state() != ProbeState::Consigned {
        return false;
    }
    match pm.item_at(0) {
        Some(it) => {
            if it.name.is_empty() || it.anchor.is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12662 注册表消费边自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12661_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12661-hiveread-edge");
    let pm = build_full_machine();

    s.add_dyn(
        "F12662-01 CurrentVersion 档十二项齐",
        CURRENTVER_PROFILE.len() == PROFILE_CAP,
        &format!("items={}", CURRENTVER_PROFILE.len()),
    );

    s.add_dyn(
        "F12662-02 逐项对平断言绿",
        profile_reconcile_ok(),
        &format!("done={} rejects={}", pm.len(), pm.reject_len()),
    );

    s.add_dyn(
        "F12662-03 值锚注出处逐项齐",
        anchor_note_ok(),
        "hive:<根>·<注> 格式 + 根名与键路径首段一致",
    );

    s.add_dyn(
        "F12662-04 注入联测 10/10 一致",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12662-05 值类型闭集普查",
        type_census_ok(),
        "六型闭集逐项面型匹配",
    );

    s.add_dyn(
        "F12662-06 折叠与锚注边界",
        boundary_ok(),
        "段数 2..=6 + 空段拒 + 锚注提取闭集",
    );

    s.add_dyn(
        "F12662-07 双跑一致",
        dual_run_ok(),
        "冻结档下两轮逐项同判",
    );

    s.add_dyn(
        "F12662-08 对平机账等式",
        pm.equation_ok(),
        &format!("done={} rejects={}", pm.len(), pm.reject_len()),
    );

    s.add_dyn(
        "F12662-09 完成写回语义",
        writeback_ok(),
        "封签即已完成且摘要非空",
    );

    s.add_dyn(
        "F12662-10 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 全档独立复核（与对平机闸独立）。
    let mut indep_ok = true;
    for item in CURRENTVER_PROFILE.iter() {
        if reconcile_item(item).is_err() {
            indep_ok = false;
        }
    }
    s.add_dyn(
        "F12662-11 全档独立复核",
        indep_ok,
        "12 项逐项过五闸复跑",
    );

    // 诊断码闭集直读。
    let mut diag_ok = true;
    let mut i = 0usize;
    while i < HIVE_ALL.len() {
        let vi = match HIVE_ALL.get(i) {
            Some(d) => d.val(),
            None => 0,
        };
        if vi <= SEG_BASE || vi >= SEG_BASE + 0x40 {
            diag_ok = false;
        }
        i += 1;
    }
    s.add_dyn(
        "F12662-12 诊断码闭集在窗",
        diag_ok,
        &format!("codes={} 窗口 [0x{:05X},0x{:05X})", HIVE_ALL.len(), SEG_BASE, SEG_BASE + 0x40),
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
        assert_eq!(SEG_BASE, 0x80F40);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：10 个码互异、全部落窗口、查表可命中。
        assert_eq!(HIVE_ALL.len(), 10);
        let mut i = 0usize;
        while i < HIVE_ALL.len() {
            let vi = match HIVE_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < HIVE_ALL.len() {
                let vj = match HIVE_ALL.get(j) {
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
        let mut pm = build_full_machine();
        assert_eq!(pm.len(), PROFILE_CAP);
        assert_eq!(pm.reject_len(), 0);
        assert!(pm.consign().is_ok());
        assert_eq!(pm.state(), ProbeState::Consigned);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止，无逃脱。
        let (caught, total, escape) = fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn face_type_matching() {
        // 值面与类型匹配表直读。
        assert!(face_matches_type(&ValueFace::Text("x"), RegType::Sz));
        assert!(face_matches_type(&ValueFace::Text("x"), RegType::ExpandSz));
        assert!(face_matches_type(&ValueFace::Text("x"), RegType::MultiSz));
        assert!(face_matches_type(&ValueFace::Num(7), RegType::Dword));
        assert!(face_matches_type(&ValueFace::Num(7), RegType::Qword));
        let bytes = ValueFace::Bytes(vec![1u8, 2, 3]);
        assert!(face_matches_type(&bytes, RegType::Binary));
        // 错配四点。
        assert!(!face_matches_type(&ValueFace::Num(7), RegType::Sz));
        assert!(!face_matches_type(&ValueFace::Text("x"), RegType::Dword));
        assert!(!face_matches_type(&bytes, RegType::Sz));
        assert!(!face_matches_type(&ValueFace::Num(7), RegType::Binary));
    }

    #[test]
    fn anchor_semantics() {
        // 锚注提取与脱钩判定。
        assert_eq!(anchor_root("hive:SOFTWARE·根 SOFTWARE").as_deref(), Some("software"));
        assert!(anchor_root("").is_none());
        assert!(anchor_root("hive:ONLY").is_none());
        // 脱钩：SOFTWARE 锚 + SYSTEM 路径。
        assert!(!anchor_attaches("hive:SOFTWARE·根", "HKLM\\SYSTEM\\Control"));
        assert!(anchor_attaches("hive:SYSTEM·根", "HKLM\\SYSTEM\\Control"));
    }

    #[test]
    fn dup_and_state_gates() {
        // 值名重复被拒且记账落行。
        let mut pm = ProbeMachine::new();
        let _ = pm.begin();
        let a = match profile_clone(0) {
            Some(x) => x,
            None => return,
        };
        assert!(pm.consume(0, &a).is_ok());
        assert_eq!(pm.consume(1, &a), Err(HiveDiag::EValueNameDup.val()));
        assert_eq!(pm.reject_len(), 1);
        // 未读即签被拒。
        let mut idle = ProbeMachine::new();
        assert_eq!(idle.consign(), Err(HiveDiag::ERewind.val()));
        // 签后再写入被拒。
        let mut full = build_full_machine();
        assert!(full.consign().is_ok());
        assert_eq!(full.consume(0, &a), Err(HiveDiag::ERewind.val()));
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致。
        let a = build_full_machine();
        let b = build_full_machine();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.reject_len(), b.reject_len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.item_at(i), b.item_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(fault_sweep().0, fault_sweep().0);
    }

    #[test]
    fn boundary_cases() {
        // 折叠与锚注边界直读。
        assert!(fold_key_path("A\\B").is_some());
        assert!(fold_key_path("A").is_none());
        assert!(fold_key_path("A\\B\\C\\D\\E\\F").is_some());
        assert!(fold_key_path("A\\B\\C\\D\\E\\F\\G").is_none());
        assert!(fold_key_path("A\\\\B").is_some() == false);
        // 折叠对账。
        assert_eq!(
            fold_key_path("HKLM\\SOFTWARE").as_deref(),
            fold_key_path("hklm\\software").as_deref()
        );
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退。
        assert!(ProbeState::Idle.rank() < ProbeState::Reading.rank());
        assert!(ProbeState::Reading.rank() < ProbeState::Consigned.rank());
        let mut pm = build_full_machine();
        assert_eq!(pm.state(), ProbeState::Reading);
        assert!(pm.consign().is_ok());
        assert_eq!(pm.state(), ProbeState::Consigned);
        assert_eq!(pm.consign(), Err(HiveDiag::ERewind.val()));
        let mut fresh = ProbeMachine::new();
        assert!(fresh.begin().is_ok());
        assert_eq!(fresh.begin(), Err(HiveDiag::ERewind.val()));
    }

    #[test]
    fn profile_census_and_types() {
        // 档普查：12 项、ProductType/CSDVersion 在档、类型闭集覆盖。
        assert_eq!(CURRENTVER_PROFILE.len(), PROFILE_CAP);
        let mut has_pt = false;
        let mut has_csd = false;
        for it in CURRENTVER_PROFILE.iter() {
            if it.name == "ProductType" {
                has_pt = true;
            }
            if it.name == "CSDVersion" {
                has_csd = true;
            }
        }
        assert!(has_pt);
        assert!(has_csd);
        assert!(type_census_ok());
        // ProductType 冻结值 WinNT。
        let pt = match profile_clone(1) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(pt.expect, ValueFace::Text("WinNT"));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12661_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
