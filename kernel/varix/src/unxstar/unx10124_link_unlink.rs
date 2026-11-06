//! UNX-F10124 link/unlink 硬链正反双判据（链接计数达成与跨设备误用受控）· 0x92F00 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10125-J1 符号链达成（symlink→readlink 回读一致 10/10 次）；
//!       UNX-F10125-J2 链环路误用受控（预期 ELOOP 精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92F00（本卡独占 0x40 窗口，下一卡 0x92F40）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_link_call`："op|old|new" 三成分（op ∈ {link, unlink,
//!      stat}），op 闭集/缺成分逐档拒绝；
//!   ② 双表模型 `InoTable`+`NameTable`：ino→(nlink, dev, body) 与
//!      name→ino，link 同 ino 加名、unlink 减名减 nlink，nlink 归零释放；
//!   ③ 状态机 `InoState`：New→Linked→Unlinked→Released 单向（禁倒退）；
//!   ④ 注入核 `BadLink`：跨设备 link / 目录 link / 缺失源 / 目标重名
//!      四类坏样本逐类 10/10 次精确复现 EXDEV/EPERM/ENOENT/EEXIST 档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10124 独占 0x40 窗口，下一卡 0x92F40）。
pub const SEG_BASE: u32 = 0x92F00;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x92F40;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// inode 表容量。
pub const INO_CAP: usize = 16;

/// 名表容量。
pub const NAME_CAP: usize = 32;

/// nlink 饱和上限。
pub const NLINK_MAX: u32 = 8;

/// 本册设备号。
pub const DEV_A: u32 = 1;
/// 外来设备号。
pub const DEV_B: u32 = 2;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LkCode {
    /// 跨设备 link（EXDEV 族）
    CrossDev,
    /// 目录 link（EPERM 族）
    DirLink,
    /// 缺失源（ENOENT 族）
    MissingOld,
    /// 目标重名（EEXIST 族）
    DupNew,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// nlink 饱和（EMLINK 族）
    NlinkFull,
    /// 状态倒退
    StateRegress,
    /// 名表满（ENOSPC 档）
    NameFull,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 守恒失平
    CountDrift,
}

impl LkCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            LkCode::CrossDev => "跨设备 link 被拒：old 与 new 不同设备；EXDEV 精确归档",
            LkCode::DirLink => "目录 link 被拒：link 不吃目录；EPERM 精确归档",
            LkCode::MissingOld => "缺失源被拒：old 不在名表；ENOENT 精确归档",
            LkCode::DupNew => "目标重名被拒：new 已被占用；EEXIST 精确归档",
            LkCode::BadOp => "非法 op 被拒：op 闭集 {link, unlink, stat}；请核对调用行",
            LkCode::MissingPart => "成分缺失被拒：调用行必须三段 op|old|new；请补成分",
            LkCode::NlinkFull => "nlink 饱和被拒：已达 EMLINK 上限；请先减链",
            LkCode::StateRegress => "状态倒退被拒：New→Linked→Unlinked→Released 单向",
            LkCode::NameFull => "名表满被拒：请先 unlink 释放名槽",
            LkCode::EmptySummary => "空成文被拒：硬链小结不得为空串；请先 link 再成文",
            LkCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            LkCode::CountDrift => "守恒失平被拒：Σ名表计数 == 逐 ino nlink；请回查",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [LkCode; 12] = [
        LkCode::CrossDev,
        LkCode::DirLink,
        LkCode::MissingOld,
        LkCode::DupNew,
        LkCode::BadOp,
        LkCode::MissingPart,
        LkCode::NlinkFull,
        LkCode::StateRegress,
        LkCode::NameFull,
        LkCode::EmptySummary,
        LkCode::SegmentViolation,
        LkCode::CountDrift,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的硬链调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkCall {
    /// 动词
    pub op: String,
    /// 源路径
    pub old: String,
    /// 目标路径
    pub new: String,
}

/// 解析 "op|old|new" 三段调用行。
pub fn parse_link_call(line: &str) -> Result<LinkCall, LkCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
    }
    let _ = parts.push(cur);
    if parts.len() != 3 {
        return Err(LkCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(LkCode::MissingPart),
    };
    if op != "link" && op != "unlink" && op != "stat" {
        return Err(LkCode::BadOp);
    }
    let old = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(LkCode::MissingPart),
    };
    let new = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(LkCode::MissingPart),
    };
    Ok(LinkCall { op, old, new })
}

// ── inode 状态机 ──────────────────────────────────────────────────────
/// inode 生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InoState {
    /// 新建
    New,
    /// 已加链
    Linked,
    /// 已减链
    Unlinked,
    /// 已释放
    Released,
}

impl InoState {
    /// 单调推进判定（允许跳级、禁倒退）。
    pub fn can_advance(self, next: InoState) -> bool {
        let rank = |s: InoState| match s {
            InoState::New => 0u8,
            InoState::Linked => 1,
            InoState::Unlinked => 2,
            InoState::Released => 3,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            InoState::New => "新建",
            InoState::Linked => "已加链",
            InoState::Unlinked => "已减链",
            InoState::Released => "已释放",
        }
    }
}

// ── 双表模型 ──────────────────────────────────────────────────────────
/// inode 记录。
#[derive(Clone, Debug)]
pub struct InoEnt {
    /// inode 号
    pub ino: u32,
    /// 链接计数
    pub nlink: u32,
    /// 设备号
    pub dev: u32,
    /// 目录标记
    pub is_dir: bool,
    /// 文件体
    pub body: Vec<u8>,
    /// 生命周期状态
    pub state: InoState,
}

/// 名表记录。
#[derive(Clone, Debug)]
pub struct NameEnt {
    /// 名
    pub name: String,
    /// 指向 inode
    pub ino: u32,
}

/// 硬链面：ino 表 + 名表。
#[derive(Clone, Debug)]
pub struct InoLedger {
    inos: Vec<InoEnt>,
    names: Vec<NameEnt>,
    next_ino: u32,
    /// 累计 link 成功次数
    pub links: u32,
    /// 累计 unlink 成功次数
    pub unlinks: u32,
    /// 累计释放数
    pub releases: u32,
}

impl InoLedger {
    /// 空账构造。
    pub fn new() -> InoLedger {
        InoLedger {
            inos: Vec::new(),
            names: Vec::new(),
            next_ino: 1000,
            links: 0,
            unlinks: 0,
            releases: 0,
        }
    }

    /// 建文件：新 ino（nlink=1）+ 首名。
    pub fn create(&mut self, name: &str, dev: u32, is_dir: bool, body: &[u8]) -> Result<u32, LkCode> {
        if self.names.len() >= NAME_CAP {
            return Err(LkCode::NameFull);
        }
        if self.name_ino(name).is_some() {
            return Err(LkCode::DupNew);
        }
        if self.inos.len() >= INO_CAP {
            return Err(LkCode::NameFull);
        }
        let ino = self.next_ino;
        self.next_ino = self.next_ino.saturating_add(1);
        let mut b: Vec<u8> = Vec::new();
        let _ = b.extend_from_slice(body);
        let _ = self.inos.push(InoEnt {
            ino,
            nlink: 1,
            dev,
            is_dir,
            body: b,
            state: InoState::New,
        });
        let _ = self.names.push(NameEnt { name: String::from(name), ino });
        Ok(ino)
    }

    /// 名→ino。
    pub fn name_ino(&self, name: &str) -> Option<u32> {
        let mut i = 0usize;
        while i < self.names.len() {
            if let Some(n) = self.names.get(i) {
                if n.name == name {
                    return Some(n.ino);
                }
            }
            i += 1;
        }
        None
    }

    /// ino→下标。
    fn ino_idx(&self, ino: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.inos.len() {
            if let Some(e) = self.inos.get(i) {
                if e.ino == ino {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// ino 是否目录样本。
    pub fn is_dir(&self, ino: u32) -> Option<bool> {
        match self.ino_idx(ino) {
            Some(i) => match self.inos.get(i) {
                Some(e) => Some(e.is_dir),
                None => None,
            },
            None => None,
        }
    }

    /// link：同 ino 加名 +1；跨设备/目录/缺源/重名/饱和逐闸。
    pub fn link(&mut self, old: &str, new: &str) -> Result<u32, LkCode> {
        let ino = match self.name_ino(old) {
            Some(x) => x,
            None => return Err(LkCode::MissingOld),
        };
        if self.name_ino(new).is_some() {
            return Err(LkCode::DupNew);
        }
        let idx = match self.ino_idx(ino) {
            Some(i) => i,
            None => return Err(LkCode::MissingOld),
        };
        let (dev, nlink, isdir) = match self.inos.get(idx) {
            Some(e) => (e.dev, e.nlink, e.is_dir),
            None => return Err(LkCode::MissingOld),
        };
        if dev != DEV_A {
            return Err(LkCode::CrossDev);
        }
        if isdir {
            return Err(LkCode::DirLink);
        }
        if nlink >= NLINK_MAX {
            return Err(LkCode::NlinkFull);
        }
        if self.names.len() >= NAME_CAP {
            return Err(LkCode::NameFull);
        }
        if let Some(e) = self.inos.get_mut(idx) {
            e.nlink = e.nlink.saturating_add(1);
            if e.state.can_advance(InoState::Linked) {
                e.state = InoState::Linked;
            }
        }
        let _ = self.names.push(NameEnt { name: String::from(new), ino });
        self.links = self.links.saturating_add(1);
        Ok(ino)
    }

    /// unlink：减名减 nlink；归零释放 inode（Released 出账）。
    pub fn unlink(&mut self, name: &str) -> Result<u32, LkCode> {
        let ino = match self.name_ino(name) {
            Some(x) => x,
            None => return Err(LkCode::MissingOld),
        };
        // 减名。
        let mut slot: Option<usize> = None;
        let mut i = 0usize;
        while i < self.names.len() {
            if let Some(n) = self.names.get(i) {
                if n.name == name {
                    slot = Some(i);
                    break;
                }
            }
            i += 1;
        }
        if let Some(s) = slot {
            let _ = self.names.remove(s);
        }
        let idx = match self.ino_idx(ino) {
            Some(x) => x,
            None => return Err(LkCode::MissingOld),
        };
        let cur = match self.inos.get(idx) {
            Some(e) => e.nlink,
            None => return Err(LkCode::MissingOld),
        };
        let after = cur.saturating_sub(1);
        if after == 0 {
            if let Some(e) = self.inos.get_mut(idx) {
                e.nlink = 0;
                e.state = InoState::Released;
            }
            let _ = self.inos.remove(idx);
            self.releases = self.releases.saturating_add(1);
        } else {
            if let Some(e) = self.inos.get_mut(idx) {
                e.nlink = after;
                if e.state.can_advance(InoState::Unlinked) {
                    e.state = InoState::Unlinked;
                }
            }
        }
        self.unlinks = self.unlinks.saturating_add(1);
        Ok(after)
    }

    /// nlink 读取。
    pub fn nlink_of(&self, name: &str) -> Option<u32> {
        let ino = self.name_ino(name)?;
        match self.ino_idx(ino) {
            Some(i) => match self.inos.get(i) {
                Some(e) => Some(e.nlink),
                None => None,
            },
            None => None,
        }
    }

    /// 内容读取（双名同体验证用）。
    pub fn body_of(&self, name: &str) -> Option<Vec<u8>> {
        let ino = self.name_ino(name)?;
        match self.ino_idx(ino) {
            Some(i) => match self.inos.get(i) {
                Some(e) => Some(e.body.clone()),
                None => None,
            },
            None => None,
        }
    }

    /// 守恒式：名表在账数 == Σ活跃 ino 的 nlink。
    pub fn conserved(&self) -> bool {
        let mut sum: u32 = 0;
        let mut i = 0usize;
        while i < self.inos.len() {
            if let Some(e) = self.inos.get(i) {
                sum = sum.saturating_add(e.nlink);
            }
            i += 1;
        }
        sum as usize == self.names.len()
    }
}

// ── J1 达成核：link01/link03 面 10/10 轮 ──────────────────────────────
/// 逐轮：建→link(nlink2,双名同体)→unlink(nlink1)→归零释放→守恒。
pub fn run_link_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = InoLedger::new();
        let mut all_ok = true;
        // 面1：建文件。
        let ino = match l.create("a", DEV_A, false, b"DATA") {
            Ok(x) => x,
            Err(_) => {
                round += 1;
                continue;
            }
        };
        all_ok = all_ok && l.nlink_of("a") == Some(1);
        probes += 1;
        // 面2：link 后 nlink=2。
        all_ok = all_ok && l.link("a", "b") == Ok(ino) && l.nlink_of("a") == Some(2);
        probes += 1;
        // 面3：双名同体。
        let ba = l.body_of("a").unwrap_or_default();
        let bb = l.body_of("b").unwrap_or_default();
        all_ok = all_ok && ba == bb && !ba.is_empty();
        probes += 1;
        // 面4：unlink b 后 nlink 回 1。
        all_ok = all_ok && l.unlink("b") == Ok(1) && l.nlink_of("a") == Some(1);
        probes += 1;
        // 面5：unlink a 归零释放。
        all_ok = all_ok && l.unlink("a") == Ok(0) && l.name_ino("a").is_none()
            && l.releases == 1;
        probes += 1;
        // 面6：全程守恒。
        all_ok = all_ok && l.conserved();
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏硬链矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadLink {
    /// 跨设备 link（EXDEV 族）
    CrossDev,
    /// 目录 link（EPERM 族）
    DirLink,
    /// 缺失源（ENOENT 族）
    MissingOld,
    /// 目标重名（EEXIST 族）
    DupNew,
}

impl BadLink {
    /// 闭集全量。
    pub const ALL: [BadLink; 4] = [
        BadLink::CrossDev,
        BadLink::DirLink,
        BadLink::MissingOld,
        BadLink::DupNew,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadLink::CrossDev => "跨设备 link",
            BadLink::DirLink => "目录 link",
            BadLink::MissingOld => "缺失源 link",
            BadLink::DupNew => "目标重名 link",
        }
    }
}

/// 注入账。
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

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), LkCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(LkCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏硬链注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_link_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadLink::ALL.len() {
        let case = match BadLink::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = InoLedger::new();
            let _ = l.create("a", DEV_A, false, b"x");
            let caught = match case {
                BadLink::CrossDev => {
                    let _ = l.create("x", DEV_B, false, b"y");
                    l.link("x", "y2").err() == Some(LkCode::CrossDev)
                }
                BadLink::DirLink => {
                    let _ = l.create("d", DEV_A, true, b"D");
                    l.link("d", "d2").err() == Some(LkCode::DirLink)
                }
                BadLink::MissingOld => {
                    l.link("ghost", "g").err() == Some(LkCode::MissingOld)
                }
                BadLink::DupNew => {
                    let _ = l.create("b", DEV_A, false, b"z");
                    l.link("a", "b").err() == Some(LkCode::DupNew)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 硬链小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「name=<s> ino=<n> nlink=<k>」；账外先拒。
pub fn compose_link_summary(l: &InoLedger, name: &str) -> Result<String, LkCode> {
    let ino = match l.name_ino(name) {
        Some(x) => x,
        None => return Err(LkCode::EmptySummary),
    };
    let nlink = match l.nlink_of(name) {
        Some(x) => x,
        None => return Err(LkCode::EmptySummary),
    };
    let out = format!("name={} ino={} nlink={}", name, ino, nlink);
    if out.is_empty() {
        return Err(LkCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EXDEV", -18, 17, "Windows ERROR_NOT_SAME_DEVICE；跨设备 link 族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；目录 link 族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失源族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；目标重名族"),
    ("EMLINK", -31, 232, "Windows ERROR_TOO_MANY_LINKS；nlink 饱和族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料指纹：建→link→双名同体→unlink→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = InoLedger::new();
    let _ = l.create("f", DEV_A, false, b"BODY");
    let _ = l.link("f", "g");
    let _ = l.unlink("g");
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_link_summary(&l, "f") {
        bytes.extend_from_slice(s.as_bytes());
    }
    if let Some(b) = l.body_of("f") {
        bytes.extend_from_slice(&b);
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10124 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10124_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10124");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92F00..=0x92F3F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < LkCode::ALL.len() {
        let mut j = i + 1;
        while j < LkCode::ALL.len() {
            if let (Some(a), Some(b)) = (LkCode::ALL.get(i), LkCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = LkCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_link_call("link|a|b").is_ok()
        && parse_link_call("ln|a|b").err() == Some(LkCode::BadOp)
        && parse_link_call("link|a").err() == Some(LkCode::MissingPart);
    s.add("调用行解析器：op 闭集放行且坏成分逐档拒绝", parse_ok,
          "op 闭集 {link, unlink, stat}；出闭集逐档拒");

    // 4. J1 达成核
    let (ok, probes) = run_link_rounds();
    s.add("J1 硬链达成：nlink 递增/递减守恒 LTP link01/link03 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（建/加链/同体/减链/释放/守恒）全对");

    // 5. J2 跨设备 link
    let mut l = InoLedger::new();
    let _ = l.create("a", DEV_A, false, b"x");
    let _ = l.create("x", DEV_B, false, b"y");
    let xdev_ok = l.link("x", "y2").err() == Some(LkCode::CrossDev)
        && l.nlink_of("x") == Some(1);
    s.add("J2 跨设备 link 受控：EXDEV 精确复现且账面不动", xdev_ok,
          "跨设备必须拒；被拒 link 不得改 nlink");

    // 6. 目录 link 档
    let _ = l.create("d", DEV_A, true, b"D");
    let dir_ok = l.link("d", "d2").err() == Some(LkCode::DirLink);
    s.add("目录 link：EPERM 档独立精确归因", dir_ok,
          "link 不吃目录；目录样本以 D 前缀标记");

    // 7. 双名同体守恒
    let _ = l.link("a", "b");
    let ba = l.body_of("a").unwrap_or_default();
    let bb = l.body_of("b").unwrap_or_default();
    let same_ok = ba == bb && l.nlink_of("a") == Some(2) && l.conserved();
    s.add("双名同体：两名读回逐字节一致且账守恒", same_ok,
          "硬链共享同一 inode；内容与计数必须同步");

    // 8. nlink 饱和档
    let mut l2 = InoLedger::new();
    let _ = l2.create("n", DEV_A, false, b"x");
    let mut sat = true;
    let mut k = 1usize;
    while k < NLINK_MAX as usize {
        sat = sat && l2.link("n", &format!("n{}", k)).is_ok();
        k += 1;
    }
    sat = sat && l2.link("n", "nover").err() == Some(LkCode::NlinkFull);
    s.add("nlink 饱和：EMLINK 档精确复现", sat,
          "NLINK_MAX 之外必须拒；已加链数不回退");

    // 9. 注入核
    let (inj, per_class) = run_bad_link_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "跨设备/目录/缺源/重名必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadLink::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = InoState::New.can_advance(InoState::Linked)
        && InoState::Linked.can_advance(InoState::Released)
        && !InoState::Released.can_advance(InoState::New)
        && InoState::Unlinked.label() == "已减链";
    s.add("状态机：New→Linked→Unlinked→Released 单向", sm_ok,
          "inode 生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(18) && agrees_with_linux(1) && agrees_with_linux(2)
        && agrees_with_linux(17) && agrees_with_linux(31)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_link_summary(&l, "a");
    let text_ok = match &sum {
        Ok(x) => x.contains("name=a") && x.contains("nlink=2"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结字段齐备且双跑哈希逐位一致",
          text_ok && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("硬链达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_link_call("link|a|b").is_ok());
        assert_eq!(parse_link_call("ln|a|b").err(), Some(LkCode::BadOp));
        assert_eq!(parse_link_call("link|a").err(), Some(LkCode::MissingPart));
    }

    #[test]
    fn create_and_link() {
        let mut l = InoLedger::new();
        let ino = l.create("a", DEV_A, false, b"x").unwrap_or(0);
        assert_eq!(l.link("a", "b"), Ok(ino));
        assert_eq!(l.nlink_of("a"), Some(2));
        assert_eq!(l.name_ino("b"), Some(ino));
    }

    #[test]
    fn cross_dev_blocked() {
        let mut l = InoLedger::new();
        let _ = l.create("x", DEV_B, false, b"y");
        assert_eq!(l.link("x", "z").err(), Some(LkCode::CrossDev));
    }

    #[test]
    fn dir_link_blocked() {
        let mut l = InoLedger::new();
        let _ = l.create("d", DEV_A, true, b"D");
        assert_eq!(l.link("d", "d2").err(), Some(LkCode::DirLink));
    }

    #[test]
    fn missing_old_blocked() {
        let mut l = InoLedger::new();
        assert_eq!(l.link("ghost", "g").err(), Some(LkCode::MissingOld));
    }

    #[test]
    fn dup_new_blocked() {
        let mut l = InoLedger::new();
        let _ = l.create("a", DEV_A, false, b"x");
        let _ = l.create("b", DEV_A, false, b"y");
        assert_eq!(l.link("a", "b").err(), Some(LkCode::DupNew));
    }

    #[test]
    fn shared_body() {
        let mut l = InoLedger::new();
        let _ = l.create("a", DEV_A, false, b"SAMEDATA");
        let _ = l.link("a", "b");
        assert_eq!(l.body_of("a"), l.body_of("b"));
    }

    #[test]
    fn unlink_releases_at_zero() {
        let mut l = InoLedger::new();
        let _ = l.create("a", DEV_A, false, b"x");
        let _ = l.link("a", "b");
        let _ = l.unlink("a");
        assert_eq!(l.nlink_of("b"), Some(1));
        let _ = l.unlink("b");
        assert_eq!(l.name_ino("a"), None);
        assert_eq!(l.releases, 1);
    }

    #[test]
    fn nlink_saturation() {
        let mut l = InoLedger::new();
        let _ = l.create("n", DEV_A, false, b"x");
        let mut k = 1usize;
        while k < NLINK_MAX as usize {
            assert!(l.link("n", &format!("n{}", k)).is_ok());
            k += 1;
        }
        assert_eq!(l.link("n", "nover").err(), Some(LkCode::NlinkFull));
    }

    #[test]
    fn conservation_holds() {
        let mut l = InoLedger::new();
        let _ = l.create("a", DEV_A, false, b"x");
        let _ = l.link("a", "b");
        let _ = l.link("a", "c");
        let _ = l.unlink("b");
        assert!(l.conserved());
        assert_eq!(l.nlink_of("a"), Some(2));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_link_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_link_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(InoState::New.can_advance(InoState::Linked));
        assert!(!InoState::Released.can_advance(InoState::New));
        assert_eq!(InoState::Released.label(), "已释放");
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10124_checks_green() {
        let set = run_unx10124_checks();
        assert!(set.all_passed(), "UNX-F10124 red: {}", set.red_items_text());
    }
}
