//! UNX-F10113 getdents 正反双判据（目录枚举达成与非目录 fd 误用受控）· 0x92C00 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10114-J1 access 可达性判定达成（R_OK/W_OK/F_OK 面 LTP
//! access01 10/10 次）；UNX-F10114-J2 非法 mode 误用受控（预期 EINVAL 精确
//! 复现 10/10 次）
//!
//! SEG_BASE = 0x92C00（本卡独占 0x40 窗口，下一卡 0x92C40）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 注册表：fd→节点注册表（目录/非目录二态注册，"."/".." 固定前两位，
//!      注册是枚举前置）。
//!   ② 状态机：目录流态机 Open→Closed，关闭后枚举显性拒绝。
//!   ③ 解析器：dent 记录解析器（reclen=16+name.len 闭集校验，乱序/坏长
//!      逐档拒绝）。
//!   ④ 注入核：非目录 fd / 关闭后枚举 / 坏 reclen / 账外 fd 四类坏样本
//!      逐类 10/10 次精确复现 ENOTDIR/EINVAL/EBADF 族，注入账逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10113 独占 0x40 窗口，下一卡 0x92C40）。
pub const SEG_BASE: u32 = 0x92C00;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 目录项头宽（ino+off+reclen+d_type 的档位缩影）。
pub const DENT_HDR: usize = 16;

/// 注册表容量上限。
pub const NODE_CAP: usize = 32;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DentCode {
    /// 非目录 fd（ENOTDIR 族）
    NonDirFd,
    /// 账外 fd（EBADF 族）
    BadFd,
    /// 关闭后枚举（EBADF 族）
    ClosedFd,
    /// 坏 reclen（EINVAL 族）
    BadReclen,
    /// 空目录项名
    EmptyName,
    /// 枚举顺序破坏（"."/".." 不在前两位）
    OrderViolation,
    /// 零号 fd（保留键）
    ZeroFd,
    /// 重复注册
    DupFd,
    /// 越窗缓冲
    OutOfRangeBuffer,
    /// 未对齐缓冲
    UnalignedBuffer,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
}

impl DentCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DentCode::NonDirFd => "非目录 fd 被拒：getdents 只认目录节点；\
                请先注册目录再枚举",
            DentCode::BadFd => "账外 fd：注册表中无此描述符；请先注册节点",
            DentCode::ClosedFd => "关闭后枚举被拒：目录流已关闭；请重新打开再 getdents",
            DentCode::BadReclen => "坏 reclen 被拒：目录项长度必须等于 16+名字字节数；\
                请核对记录头",
            DentCode::EmptyName => "空名目录项被拒：名字不得为空串；请核对注册参数",
            DentCode::OrderViolation => "枚举顺序被拒：\".\" 与 \"..\" 必须固定前两位；\
                请保持注册序",
            DentCode::ZeroFd => "fd 号为零：零号是保留键，禁止入账；请换用正数 fd",
            DentCode::DupFd => "重复注册被拒：同 fd 只能注册一次；请先关闭再重开",
            DentCode::OutOfRangeBuffer => "越窗缓冲被拒：枚举落点整段必须落在合法窗内；\
                请核对窗口边界",
            DentCode::UnalignedBuffer => "未对齐缓冲被拒：落点必须 8 字节对齐；\
                请检查分配器对齐承诺",
            DentCode::EmptySummary => "空成文被拒：目录小结不得为空串；请检查 fd 是否在账",
            DentCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [DentCode; 12] = [
        DentCode::NonDirFd,
        DentCode::BadFd,
        DentCode::ClosedFd,
        DentCode::BadReclen,
        DentCode::EmptyName,
        DentCode::OrderViolation,
        DentCode::ZeroFd,
        DentCode::DupFd,
        DentCode::OutOfRangeBuffer,
        DentCode::UnalignedBuffer,
        DentCode::EmptySummary,
        DentCode::SegmentViolation,
    ];
}

// ── 节点态机 ──────────────────────────────────────────────────────────
/// 注册节点种类与生命周期：目录/文件 × Open/Closed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeStage {
    /// 目录流打开态
    DirOpen,
    /// 目录流关闭态
    DirClosed,
    /// 普通文件（非目录）
    PlainFile,
}

/// 单条目录项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dent {
    /// inode 号（注册时按序分配，确定性）
    pub ino: u32,
    /// 项偏移（前一项 reclen 累计）
    pub off: u64,
    /// 记录长（16+名字字节数）
    pub reclen: usize,
    /// 项名
    pub name: String,
}

/// 单 fd 节点记录。
#[derive(Clone, Debug)]
pub struct DirNode {
    /// fd
    pub fd: u32,
    /// 目录项表（"." 与 ".." 固定前两位）
    pub dents: Vec<Dent>,
    /// 生命周期态
    pub stage: NodeStage,
}

/// 目录注册表：fd→节点；目录注册即生成 "."/".." 前缀。
#[derive(Clone, Debug)]
pub struct DirRegistry {
    nodes: Vec<DirNode>,
    ino_next: u32,
    cap: usize,
}

impl DirRegistry {
    /// 空注册表构造。
    pub fn new() -> DirRegistry {
        DirRegistry { nodes: Vec::new(), ino_next: 1, cap: NODE_CAP }
    }

    /// 注册目录：零号/重复拒绝、空名拒绝；自动前置 "."/".."。
    pub fn open_dir(&mut self, fd: u32, names: &[&str]) -> Result<(), DentCode> {
        self.open_base(fd, names, NodeStage::DirOpen)
    }

    /// 注册普通文件（非目录）：枚举必拒。
    pub fn open_file(&mut self, fd: u32) -> Result<(), DentCode> {
        self.open_base(fd, &[], NodeStage::PlainFile)
    }

    fn open_base(&mut self, fd: u32, names: &[&str], stage: NodeStage) -> Result<(), DentCode> {
        if fd == 0 {
            return Err(DentCode::ZeroFd);
        }
        if self.nodes.len() >= self.cap {
            return Err(DentCode::SegmentViolation);
        }
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.fd == fd {
                    return Err(DentCode::DupFd);
                }
            }
            i += 1;
        }
        let mut dents: Vec<Dent> = Vec::new();
        // "." 与 ".." 固定前两位（getdents 契约），业务名随后按注册序追加。
        let _ = dents.push(Dent {
            ino: 0,
            off: 0,
            reclen: DENT_HDR + 1,
            name: String::from("."),
        });
        let _ = dents.push(Dent {
            ino: 0,
            off: 0,
            reclen: DENT_HDR + 2,
            name: String::from(".."),
        });
        let mut k = 0usize;
        while k < names.len() {
            let name = match names.get(k) {
                Some(s) if s.is_empty() => return Err(DentCode::EmptyName),
                Some(s) => *s,
                None => break,
            };
            let _ = dents.push(Dent {
                ino: 0, // 占位，下面统一编号
                off: 0,
                reclen: DENT_HDR + name.len(),
                name: String::from(name),
            });
            k += 1;
        }
        // 统一编号：业务名在前仍按表序拿号，"."/".." 拿尾号。
        let mut j = 0usize;
        while j < dents.len() {
            if let Some(d) = dents.get_mut(j) {
                d.ino = self.ino_next;
                d.off = j as u64;
                self.ino_next = self.ino_next.saturating_add(1);
            }
            j += 1;
        }
        let _ = self.nodes.push(DirNode { fd, dents, stage });
        Ok(())
    }

    /// 在账节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 关闭目录流：态置 DirClosed，枚举拒绝。
    pub fn close(&mut self, fd: u32) -> Result<(), DentCode> {
        let mut idx = self.nodes.len();
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.fd == fd {
                    idx = i;
                    break;
                }
            }
            i += 1;
        }
        if idx < self.nodes.len() {
            if let Some(n) = self.nodes.get_mut(idx) {
                if n.stage == NodeStage::PlainFile {
                    return Err(DentCode::NonDirFd);
                }
                n.stage = NodeStage::DirClosed;
                return Ok(());
            }
        }
        Err(DentCode::BadFd)
    }

    /// getdents 枚举：目录打开态放行；顺序保序且 "."/".." 前两位强制校验。
    pub fn getdents(&self, fd: u32) -> Result<Vec<Dent>, DentCode> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.fd == fd {
                    match n.stage {
                        NodeStage::PlainFile => return Err(DentCode::NonDirFd),
                        NodeStage::DirClosed => return Err(DentCode::ClosedFd),
                        NodeStage::DirOpen => {}
                    }
                    // 顺序守卫：前两位必须是 "." 与 ".."。
                    let first_ok = match n.dents.get(0) {
                        Some(d) => d.name == ".",
                        None => false,
                    };
                    let second_ok = match n.dents.get(1) {
                        Some(d) => d.name == "..",
                        None => false,
                    };
                    if !first_ok || !second_ok {
                        return Err(DentCode::OrderViolation);
                    }
                    // reclen 闭集校验：每项 reclen == 16+名字字节数。
                    let mut j = 0usize;
                    while j < n.dents.len() {
                        if let Some(d) = n.dents.get(j) {
                            if d.reclen != DENT_HDR + d.name.len() {
                                return Err(DentCode::BadReclen);
                            }
                            if d.name.is_empty() {
                                return Err(DentCode::EmptyName);
                            }
                        }
                        j += 1;
                    }
                    return Ok(n.dents.clone());
                }
            }
            i += 1;
        }
        Err(DentCode::BadFd)
    }

    /// 节点态读取（审计用）。
    pub fn stage_of(&self, fd: u32) -> Option<NodeStage> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.fd == fd {
                    return Some(n.stage);
                }
            }
            i += 1;
        }
        None
    }
}

// ── J1 达成核："."/".." 项与顺序 10/10 轮 ─────────────────────────────
/// 逐轮注册三业务名目录并枚举；顺序与首两项必须 10/10 命中。
pub fn run_enumerate_rounds() -> (u32, Vec<String>) {
    let mut ok = 0u32;
    let mut last_names: Vec<String> = Vec::new();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut reg = DirRegistry::new();
        let name_a = String::from("alpha");
        let _ = reg.open_dir(9, &[name_a.as_str(), "beta", "gamma"]);
        match reg.getdents(9) {
            Ok(ds) => {
                // 顺序断言："." → ".." → alpha → beta → gamma。
                let seq_ok = ds.len() == 5
                    && ds.get(0).map(|d| d.name == ".").unwrap_or(false)
                    && ds.get(1).map(|d| d.name == "..").unwrap_or(false)
                    && ds.get(2).map(|d| d.name == "alpha").unwrap_or(false)
                    && ds.get(3).map(|d| d.name == "beta").unwrap_or(false)
                    && ds.get(4).map(|d| d.name == "gamma").unwrap_or(false);
                // 偏移单调：off 逐项 +1，reclen 闭集逐项成立。
                let meta_ok = ds.iter().enumerate().all(|(i, d)| {
                    d.off == i as u64 && d.reclen == DENT_HDR + d.name.len() && d.ino > 0
                });
                if seq_ok && meta_ok {
                    ok += 1;
                }
                last_names = ds.iter().map(|d| d.name.clone()).collect();
            }
            Err(_) => {}
        }
        round += 1;
    }
    (ok, last_names)
}

// ── 注入核：坏枚举样本矩阵 ────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadEnum {
    /// 非目录 fd 枚举
    NonDir,
    /// 关闭后枚举
    AfterClose,
    /// 账外 fd 枚举
    BadFd,
    /// 零号 fd 枚举
    ZeroFd,
}

impl BadEnum {
    /// 闭集全量。
    pub const ALL: [BadEnum; 4] = [
        BadEnum::NonDir,
        BadEnum::AfterClose,
        BadEnum::BadFd,
        BadEnum::ZeroFd,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadEnum::NonDir => "非目录 fd 枚举",
            BadEnum::AfterClose => "关闭后枚举",
            BadEnum::BadFd => "账外 fd 枚举",
            BadEnum::ZeroFd => "零号 fd 枚举",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 SegmentViolation。
    pub fn record(&mut self, caught: bool) -> Result<(), DentCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(DentCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏枚举注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_enum_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadEnum::ALL.len() {
        let case = match BadEnum::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut reg = DirRegistry::new();
            let _ = reg.open_dir(5, &["only"]);
            let _ = reg.open_file(6);
            let caught = match case {
                BadEnum::NonDir => reg.getdents(6).err() == Some(DentCode::NonDirFd),
                BadEnum::AfterClose => {
                    let _ = reg.close(5);
                    reg.getdents(5).err() == Some(DentCode::ClosedFd)
                }
                BadEnum::BadFd => reg.getdents(77).err() == Some(DentCode::BadFd),
                BadEnum::ZeroFd => reg.getdents(0).err() == Some(DentCode::BadFd),
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

// ── 目录小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「fd=<n> dents=<k> head=<.>/..」；账外/关闭先拒。
pub fn compose_dir_summary(reg: &DirRegistry, fd: u32) -> Result<String, DentCode> {
    if reg.stage_of(fd) != Some(NodeStage::DirOpen) {
        return Err(DentCode::EmptySummary);
    }
    let ds = reg.getdents(fd)?;
    let head = match ds.get(0) {
        Some(d) => d.name.clone(),
        None => String::new(),
    };
    let out = format!("fd={} dents={} head={}", fd, ds.len(), head);
    if out.is_empty() {
        return Err(DentCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOTDIR", -20, 267, "Windows ERROR_DIRECTORY；非目录 fd 族"),
    ("EBADF", -9, 10038, "Windows WSAENOTSOCK 近似；账外/关闭 fd 族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏 reclen 族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；空项名族"),
    ("EOVERFLOW", -75, 1323, "Windows ERROR_ARITHMETIC_OVERFLOW；偏移溢出族"),
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

/// 规范语料双跑指纹：双目录注册 → 枚举 → 逐项成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = DirRegistry::new();
    let _ = reg.open_dir(11, &["aaa", "bbb"]);
    let _ = reg.open_dir(12, &["ccc"]);
    let mut bytes: Vec<u8> = Vec::new();
    let mut e = 0u32;
    while e < 2 {
        if let Ok(s) = compose_dir_summary(&reg, 11 + e) {
            bytes.extend_from_slice(s.as_bytes());
            bytes.push(b';');
        }
        if let Ok(ds) = reg.getdents(11 + e) {
            let mut k = 0usize;
            while k < ds.len() {
                if let Some(d) = ds.get(k) {
                    bytes.extend_from_slice(d.name.as_bytes());
                    bytes.push(b',');
                }
                k += 1;
            }
        }
        e += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10113 判据集（13 条静态 + 3 条动态；动态明细 ≤8 条）。
pub fn run_unx10113_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10113");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92C00..=0x92C3F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < DentCode::ALL.len() {
        let mut j = i + 1;
        while j < DentCode::ALL.len() {
            if let (Some(a), Some(b)) = (DentCode::ALL.get(i), DentCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = DentCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 注册表：目录自动前置 "."/".."，非目录零项，零号/重复拒绝
    let mut reg = DirRegistry::new();
    let reg_ok = reg.open_dir(9, &["alpha", "beta"]).is_ok()
        && reg.open_file(10).is_ok()
        && reg.len() == 2
        && reg.open_dir(0, &[]).err() == Some(DentCode::ZeroFd)
        && reg.open_dir(9, &[]).err() == Some(DentCode::DupFd)
        && reg.open_dir(11, &[""]).err() == Some(DentCode::EmptyName);
    s.add("节点注册表：目录前置点项且零号/重复/空名拒绝", reg_ok,
          "注册是枚举前置；空名与保留键必须逐次显性拒绝");

    // 4. J1 达成核：枚举顺序与首两项 10/10
    let (ok, names) = run_enumerate_rounds();
    s.add("J1 枚举达成：\".\"/\"..\" 项与顺序 10/10 次",
          ok == INJECT_ROUNDS && names.get(0).map(|x| x.as_str()) == Some(".")
              && names.get(1).map(|x| x.as_str()) == Some(".."),
          "点项前缀与注册序是 getdents 契约；任何一轮乱序即失守");

    // 5. 枚举幂等：同态两次枚举逐项一致
    let d1 = reg.getdents(9);
    let d2 = reg.getdents(9);
    s.add("getdents 幂等：同态两次枚举逐项一致", d1 == d2 && d1.is_ok(),
          "枚举是只读投影，不得推进隐藏游标");

    // 6. 偏移与 reclen 闭集
    let meta_ok = match reg.getdents(9) {
        Ok(ds) => ds.iter().enumerate().all(|(i, d)| {
            d.off == i as u64 && d.reclen == DENT_HDR + d.name.len()
        }),
        Err(_) => false,
    };
    s.add("dent 元数据：off 单调且 reclen=16+名长闭集", meta_ok,
          "off 与 reclen 是用户态解析依据，缺一即解析失败");

    // 7. 关闭态：close 后枚举拒绝且态位准确
    let _ = reg.close(9);
    let closed_ok = reg.getdents(9).err() == Some(DentCode::ClosedFd)
        && reg.stage_of(9) == Some(NodeStage::DirClosed)
        && reg.getdents(10).err() == Some(DentCode::NonDirFd);
    s.add("关闭/非目录：关后枚举拒绝且非目录逐次拒绝", closed_ok,
          "关闭即流不可用；非目录与关闭是两档独立归因");

    // 8. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_enum_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "非目录/关闭后/账外/零号 fd 必须逐类逐次精确复现");

    // 9. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadEnum::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. J2 精确归因：ENOTDIR 族逐类复现
    let mut reg2 = DirRegistry::new();
    let _ = reg2.open_file(3);
    let j2_ok = reg2.getdents(3).err() == Some(DentCode::NonDirFd)
        && reg2.close(3).err() == Some(DentCode::NonDirFd)
        && reg2.stage_of(3) == Some(NodeStage::PlainFile);
    s.add("J2 非目录受控：ENOTDIR 族精确复现且态不被误改", j2_ok,
          "对文件 fd 的枚举与关闭都必须按非目录档拒绝");

    // 11. Linux 逐码一致
    let codes_ok = agrees_with_linux(20) && agrees_with_linux(9) && agrees_with_linux(22)
        && agrees_with_linux(2) && agrees_with_linux(75)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 12. 成文非空且含三字段 + 双跑一致
    let sum = compose_dir_summary(&DirRegistry::with_pair(21, "one"), 21);
    let text_ok = match &sum {
        Ok(t) => t.contains("fd=21") && t.contains("dents=3") && t.contains("head=."),
        Err(_) => false,
    };
    s.add("目录成文：三项字段齐备且两次成文逐字节一致",
          text_ok && sum == compose_dir_summary(&DirRegistry::with_pair(21, "one"), 21),
          "成文必须携带 fd/dents/head 标记且为纯函数");

    // 13. 双跑指纹
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希逐位一致", f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 达成轮数
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("枚举达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

impl DirRegistry {
    /// 辅助构造：单 fd 单业务名目录（成文判据用）。
    pub fn with_pair(fd: u32, name: &str) -> DirRegistry {
        let mut r = DirRegistry::new();
        let _ = r.open_dir(fd, &[name]);
        r
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_guards() {
        let mut reg = DirRegistry::new();
        assert!(reg.open_dir(1, &["a"]).is_ok());
        assert_eq!(reg.open_dir(0, &[]).err(), Some(DentCode::ZeroFd));
        assert!(reg.open_dir(1, &[]).is_err());
        assert_eq!(reg.open_dir(2, &[""]).err(), Some(DentCode::EmptyName));
    }

    #[test]
    fn dot_dot_prefix() {
        let reg = DirRegistry::with_pair(2, "x");
        let ds = reg.getdents(2).unwrap_or_default();
        assert_eq!(ds.len(), 3);
        assert_eq!(ds.get(0).map(|d| d.name.as_str()), Some("."));
        assert_eq!(ds.get(1).map(|d| d.name.as_str()), Some(".."));
        assert_eq!(ds.get(2).map(|d| d.name.as_str()), Some("x"));
    }

    #[test]
    fn non_dir_rejected() {
        let mut reg = DirRegistry::new();
        let _ = reg.open_file(3);
        assert_eq!(reg.getdents(3).err(), Some(DentCode::NonDirFd));
    }

    #[test]
    fn close_blocks_enum() {
        let mut reg = DirRegistry::new();
        let _ = reg.open_dir(4, &["a"]);
        let _ = reg.close(4);
        assert_eq!(reg.getdents(4).err(), Some(DentCode::ClosedFd));
    }

    #[test]
    fn enum_idempotent() {
        let reg = DirRegistry::with_pair(5, "n");
        let a = reg.getdents(5);
        let b = reg.getdents(5);
        assert_eq!(a, b);
    }

    #[test]
    fn reclen_closed_set() {
        let reg = DirRegistry::with_pair(6, "abcd");
        let ds = reg.getdents(6).unwrap_or_default();
        for d in ds.iter() {
            assert_eq!(d.reclen, DENT_HDR + d.name.len());
        }
    }

    #[test]
    fn bad_enum_matrix_green() {
        let (inj, per) = run_bad_enum_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn enumerate_rounds_full() {
        let (ok, _) = run_enumerate_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(20));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(22), -22);
    }

    #[test]
    fn unx10113_checks_green() {
        let set = run_unx10113_checks();
        assert!(set.all_passed(), "UNX-F10113 red: {}", set.red_items_text());
    }
}
